"""Servicio de voz de Charlando: ASR y TTS con LFM3-Audio por WebSocket (contrato en PROTOCOLO-VOZ.md)."""
import asyncio
import gc
import json
import logging
import os
import subprocess
import sys
import threading
import time
import traceback
from concurrent.futures import ThreadPoolExecutor

import librosa
import numpy as np
import torch
from dotenv import load_dotenv
from transformers import AutoModelForMultimodalLM, AutoProcessor
from transformers.generation.streamers import BaseStreamer
from websockets.asyncio.server import serve
from websockets.exceptions import ConnectionClosed

load_dotenv(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".env"))
HOST = os.environ.get("CHARLANDO_VOZ_HOST", "127.0.0.1")
PORT = int(os.environ.get("CHARLANDO_VOZ_PORT", 8766))
RUTA = "/voz"
# Un id de Hugging Face o una carpeta local. La revisión fija el código remoto que se ejecuta.
MODELO = os.environ.get("CHARLANDO_MODELO", "LiquidAI/LFM3-Audio-2B")
REVISION = os.environ.get("CHARLANDO_MODELO_REVISION", "2d6c1f9") or None
# Opcional: URL que libera la GPU de otro servidor de modelos (p. ej. llama-swap) antes de cargar.
LIBERAR_GPU_URL = os.environ.get("CHARLANDO_LIBERAR_GPU_URL", "")
INACTIVIDAD = int(os.environ.get("CHARLANDO_INACTIVIDAD", 180))  # segundos sin peticiones antes de descargar
DEPURAR = os.environ.get("CHARLANDO_DEPURAR") == "1"
AUDIO_PARAMS = dict(audio_temperature=1.0, audio_top_k=128,
                    audio_repetition_penalty=1.1, audio_repetition_window=4096)
FRAMES_POR_TROZO = 4  # 4 × 80 ms: decodificar frame a frame deja el TTS por debajo del tiempo real
MUESTRAS_MINIMAS = 16000 // 3  # menos de 0,33 s de audio se considera silencio

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(message)s", datefmt="%H:%M:%S")
log = logging.getLogger("charlando-voz")

# Un único hilo: el ASR y el TTS usan la GPU en serie.
GPU = ThreadPoolExecutor(max_workers=1, thread_name_prefix="gpu")
model = processor = codec = None
ACTIVA = None  # conexión del único cliente
cargado = calentado = False
ultimo_uso = time.monotonic()
CARGA = asyncio.Lock()
TAREAS = set()  # referencias a las tareas sueltas, para que no las recoja el recolector


class Cancelado(Exception):
    pass


def contenido(texto):
    """El texto solo va al registro con CHARLANDO_DEPURAR=1."""
    return f"«{texto}»" if DEPURAR else f"{len(texto)} caracteres"


def cargar_modelo():
    if LIBERAR_GPU_URL:
        subprocess.run(["curl", "-s", "-X", "POST", LIBERAR_GPU_URL], check=False, stdout=subprocess.DEVNULL)
    global model, processor, codec
    t0 = time.perf_counter()
    revision = None if os.path.isdir(MODELO) else REVISION
    model = AutoModelForMultimodalLM.from_pretrained(MODELO, revision=revision, dtype=torch.bfloat16,
                                                     device_map="cuda", trust_remote_code=True).eval()
    processor = AutoProcessor.from_pretrained(MODELO, revision=revision, trust_remote_code=True)
    codec = processor.audio_tokenizer.to(device="cuda").eval()
    log.info("Modelo cargado en %.1f s · VRAM %.2f GiB", time.perf_counter() - t0,
             torch.cuda.memory_allocated() / 2**30)


def asr(audio):
    conv = [{"role": "system", "content": "Perform ASR in Spanish."},
            {"role": "user", "content": [{"type": "audio", "audio": audio}]}]
    inputs = processor.apply_chat_template(conv, add_generation_prompt=True, tokenize=True,
                                           return_dict=True, return_tensors="pt").to("cuda")
    with torch.no_grad():
        out = model.generate(**inputs, max_new_tokens=256, suppress_mixed=True,
                             suppress_audio_out=True, return_dict_in_generate=True)
    return processor.decode(out.sequences[0, inputs["input_ids"].shape[1]:],
                            skip_special_tokens=True).strip()


class Emisor(BaseStreamer):
    """Recibe cada frame de códigos del modelo y devuelve PCM16 a 24 kHz según se genera."""

    def __init__(self, emitir, cancelar, guardar_codigos=False):
        clase_cache = sys.modules[type(codec).__module__].LiquidCodecStreamingCache
        self.emitir_pcm, self.cancelar = emitir, cancelar
        self.cache = clase_cache(codec.config)
        self.n_frames = 0
        self.pendientes = []
        self.guardar = [] if guardar_codigos else None

    def _emitir(self, onda):
        pcm = (onda.float().clamp(-1, 1) * 32767).round().to(torch.int16).cpu().numpy().tobytes()
        self.emitir_pcm(pcm)

    def put(self, codes):
        if self.cancelar.is_set():
            raise Cancelado()
        codes = codes.reshape(1, codec.config.n_q, 1)
        if self.guardar is not None:
            self.guardar.append(codes)
        self.n_frames += 1
        self.pendientes.append(codes)
        if len(self.pendientes) >= FRAMES_POR_TROZO:
            self._decodificar(finalize=False)

    def _decodificar(self, finalize):
        if self.pendientes:
            codes = torch.cat(self.pendientes, dim=2)
        else:
            codes = torch.zeros((1, codec.config.n_q, 0), dtype=torch.long, device="cuda")
        self.pendientes = []
        with torch.inference_mode():
            out = codec.decode(codes, streaming_cache=self.cache, finalize=finalize)
        if out.audio_values.shape[-1]:
            self._emitir(out.audio_values[0])

    def end(self):
        # generate() llama a end() al terminar; sin cache.finished se repetiría el cierre.
        if self.cancelar.is_set() or self.cache.finished or not (self.cache.started or self.pendientes):
            return
        self._decodificar(finalize=True)


def sintetizar(texto, emisor):
    conv = [{"role": "system", "content": "Perform TTS in Spanish using a male voice."},
            {"role": "user", "content": texto},
            {"role": "assistant", "content": "<|audio_start|>"}]
    inputs = processor.apply_chat_template(conv, continue_final_message=True, tokenize=True,
                                           return_dict=True, return_tensors="pt").to("cuda")
    with torch.no_grad():
        model.generate(**inputs, max_new_tokens=1024, audio_streamer=emisor,
                       return_dict_in_generate=True, **AUDIO_PARAMS)


def calentar():
    """Primer uso de CUDA: el modelo dice una frase y luego la transcribe."""
    trozos = []
    emisor = Emisor(trozos.append, threading.Event(), guardar_codigos=True)
    t0 = time.perf_counter()
    sintetizar("Hola, ¿qué tal? Esto es una prueba de calentamiento.", emisor)
    log.info("Calentamiento TTS: %d frames en %.2f s", emisor.n_frames, time.perf_counter() - t0)

    voz = np.frombuffer(b"".join(trozos), dtype=np.int16).astype(np.float32) / 32767
    t0 = time.perf_counter()
    texto = asr(librosa.resample(voz, orig_sr=24000, target_sr=16000))
    log.info("Calentamiento ASR: %.2f s · %s", time.perf_counter() - t0, contenido(texto))

    # Comprobación: el streaming por frames debe dar el mismo audio que decodificar todo de una vez.
    with torch.inference_mode():
        completo = codec.decode(torch.cat(emisor.guardar, dim=2)).audio_values[0]
    ref = completo.float().clamp(-1, 1).cpu().numpy()
    n = min(len(ref), len(voz))
    log.info("Streaming del códec: %d muestras (completo: %d), diferencia máxima %.4f",
             len(voz), len(ref), float(np.abs(ref[:n] - voz[:n]).max()))


def cargar_y_calentar():
    global calentado
    cargar_modelo()
    # Calentar solo la primera vez: tras una recarga en el mismo proceso, CUDA ya está caliente.
    if not calentado:
        calentar()
        calentado = True


def descargar_modelo():
    global model, processor, codec
    model = processor = codec = None
    gc.collect()
    torch.cuda.empty_cache()


def tocar():
    global ultimo_uso
    ultimo_uso = time.monotonic()


async def _cargar_si_hace_falta():
    global cargado
    async with CARGA:
        if not cargado:
            await asyncio.get_running_loop().run_in_executor(GPU, cargar_y_calentar)
            cargado = True


async def asegurar_modelo():
    tocar()
    # Si se cancela quien espera (se cae el cliente), la carga sigue y `cargado` queda bien puesto.
    await asyncio.shield(_cargar_si_hace_falta())


def en_segundo_plano(corrutina):
    tarea = asyncio.create_task(corrutina)
    TAREAS.add(tarea)
    tarea.add_done_callback(TAREAS.discard)
    tarea.add_done_callback(lambda t: t.cancelled() or t.exception() is None
                            or log.error("Fallo en segundo plano: %r", t.exception()))


async def vigilar_inactividad():
    global cargado
    while True:
        await asyncio.sleep(15)
        en_uso = ACTIVA is not None and ACTIVA.en_uso()
        if not cargado or en_uso or time.monotonic() - ultimo_uso < INACTIVIDAD:
            continue
        async with CARGA:
            await asyncio.get_running_loop().run_in_executor(GPU, descargar_modelo)
            cargado = False
        log.info("Modelo descargado tras %d s sin uso", INACTIVIDAD)


class Peticion:
    def __init__(self, tipo, id, datos, generacion):
        self.tipo, self.id, self.datos, self.generacion = tipo, id, datos, generacion
        self.t_llegada = time.perf_counter()


class Conexion:
    def __init__(self, ws):
        self.ws = ws
        self.cola = asyncio.Queue()  # FIFO de frases y transcripciones: la GPU las atiende en orden
        self.en_curso = None
        self.asr_id = None  # transcripción abierta
        self.asr_audio = bytearray()
        # Sube con cada cancel: lo pedido con una generación anterior ya no se envía.
        self.generacion = 0
        self.cancelar_tts = None  # threading.Event de la frase que se sintetiza ahora
        self.envio = asyncio.Lock()  # cancel espera al envío de audio que esté a medias
        self.trabajador = None

    def en_uso(self):
        return self.asr_id is not None or self.en_curso is not None or not self.cola.empty()

    async def enviar(self, mensaje):
        await self.ws.send(json.dumps(mensaje, ensure_ascii=False))

    async def error(self, texto, id=None):
        log.error("error%s: %s", f" ({id})" if id else "", texto)
        mensaje = {"type": "error", "message": texto}
        if id is not None:
            mensaje["id"] = id
        await self.enviar(mensaje)

    def descartar_frases(self):
        """Corta la frase en curso y quita de la cola las frases pendientes. Devuelve cuántas quitó."""
        self.generacion += 1
        if self.cancelar_tts is not None:
            self.cancelar_tts.set()
        quedan, quitadas = [], 0
        while not self.cola.empty():
            p = self.cola.get_nowait()
            if p.tipo == "tts":
                quitadas += 1
            else:
                quedan.append(p)
        for p in quedan:
            self.cola.put_nowait(p)
        return quitadas

    async def cancelar(self):
        en_curso = self.en_curso is not None and self.en_curso.tipo == "tts"
        quitadas = self.descartar_frases()
        async with self.envio:
            await self.enviar({"type": "cancelled"})
        log.info("cancel: %s, %d frases quitadas de la cola",
                 "frase en curso cortada" if en_curso else "sin frase en curso", quitadas)

    async def trabajar(self):
        while True:
            p = await self.cola.get()
            self.en_curso = p
            try:
                if p.tipo == "tts":
                    await self.tts(p)
                else:
                    await self.transcribir(p)
            except ConnectionClosed:
                return
            except Exception as e:
                log.error("Fallo en %s %s:\n%s", p.tipo, p.id, traceback.format_exc())
                try:
                    await self.error(f"{type(e).__name__}: {e}", p.id)
                except ConnectionClosed:
                    return
            finally:
                self.en_curso = None
                tocar()

    async def tts(self, p):
        if p.generacion != self.generacion:
            return
        await asegurar_modelo()
        if p.generacion != self.generacion:
            return
        loop = asyncio.get_running_loop()
        cancelar = self.cancelar_tts = threading.Event()
        trozos = asyncio.Queue()
        emisor = Emisor(lambda pcm: loop.call_soon_threadsafe(trozos.put_nowait, pcm), cancelar)

        def trabajo():
            try:
                sintetizar(p.datos, emisor)
            except Cancelado:
                pass
            finally:
                loop.call_soon_threadsafe(trozos.put_nowait, None)

        t_inicio = time.perf_counter()
        t_primer = None
        n_bytes = 0
        futuro = loop.run_in_executor(GPU, trabajo)
        try:
            while (pcm := await trozos.get()) is not None:
                async with self.envio:
                    if p.generacion != self.generacion:
                        continue
                    if t_primer is None:
                        t_primer = time.perf_counter()
                    await self.ws.send(pcm)
                    n_bytes += len(pcm)
            await futuro
        except BaseException:
            cancelar.set()
            raise
        finally:
            if self.cancelar_tts is cancelar:
                self.cancelar_tts = None
        t_fin = time.perf_counter()
        if p.generacion != self.generacion:
            log.info("tts %s cancelada tras %.1f s de audio", p.id, n_bytes / 48000)
            return
        await self.enviar({"type": "tts_end", "id": p.id})
        primer = f"{(t_primer - p.t_llegada) * 1000:.0f} ms" if t_primer else "sin audio"
        log.info("tts %s: espera %.0f ms · primer audio %s desde la petición · %.1f s de audio en %.2f s · %s",
                 p.id, (t_inicio - p.t_llegada) * 1000, primer, n_bytes / 48000, t_fin - t_inicio,
                 contenido(p.datos))

    async def transcribir(self, p):
        audio = np.frombuffer(p.datos[:len(p.datos) // 2 * 2], dtype="<i2").astype(np.float32) / 32768
        t_inicio = time.perf_counter()
        if len(audio) < MUESTRAS_MINIMAS:
            texto = ""
        else:
            await asegurar_modelo()
            t_inicio = time.perf_counter()
            texto = await asyncio.get_running_loop().run_in_executor(GPU, asr, audio)
        t_fin = time.perf_counter()
        await self.enviar({"type": "asr_result", "id": p.id, "text": texto})
        log.info("asr %s: %.1f s de audio · transcrito en %.0f ms (%.0f ms desde asr_end) · %s",
                 p.id, len(audio) / 16000, (t_fin - t_inicio) * 1000, (t_fin - p.t_llegada) * 1000,
                 contenido(texto))

    async def mensaje(self, datos):
        tipo = datos.get("type")
        id = datos.get("id")
        tocar()
        if tipo == "warmup":
            en_segundo_plano(asegurar_modelo())
        elif tipo == "asr_start":
            if not isinstance(id, str):
                return await self.error("asr_start sin id")
            if self.asr_id is not None:
                log.warning("asr_start %s con la transcripción %s abierta: se descarta la anterior", id, self.asr_id)
            self.asr_id, self.asr_audio = id, bytearray()
            en_segundo_plano(asegurar_modelo())
        elif tipo == "asr_end":
            if self.asr_id is None or id != self.asr_id:
                return await self.error("asr_end sin su asr_start", id)
            audio, self.asr_id, self.asr_audio = bytes(self.asr_audio), None, bytearray()
            self.cola.put_nowait(Peticion("asr", id, audio, self.generacion))
        elif tipo == "tts":
            texto = datos.get("text")
            if not isinstance(id, str) or not isinstance(texto, str):
                return await self.error("tts necesita id y text", id)
            self.cola.put_nowait(Peticion("tts", id, texto, self.generacion))
        elif tipo == "cancel":
            await self.cancelar()
        else:
            await self.error(f"tipo desconocido: {tipo}", id)

    async def correr(self):
        self.trabajador = asyncio.create_task(self.trabajar())
        try:
            async for msg in self.ws:
                if isinstance(msg, bytes):
                    if self.asr_id is not None:
                        self.asr_audio.extend(msg)
                    continue
                try:
                    datos = json.loads(msg)
                except json.JSONDecodeError:
                    await self.error("JSON no válido")
                    continue
                if not isinstance(datos, dict):
                    await self.error("se espera un objeto JSON")
                    continue
                await self.mensaje(datos)
        except ConnectionClosed:
            pass
        finally:
            self.terminar()

    def terminar(self):
        self.descartar_frases()
        self.asr_id = None
        while not self.cola.empty():
            self.cola.get_nowait()
        if self.trabajador is not None:
            self.trabajador.cancel()


async def atender(ws):
    global ACTIVA
    if ws.request.path != RUTA:
        await ws.close(1008, "ruta no válida")
        return
    if ACTIVA is not None:
        log.info("Llega otro cliente: se cierra la conexión anterior")
        ACTIVA.terminar()
        await ACTIVA.ws.close(1000, "otro cliente")
    conexion = Conexion(ws)
    ACTIVA = conexion
    log.info("Cliente conectado")
    try:
        await conexion.correr()
    finally:
        if ACTIVA is conexion:
            ACTIVA = None
        log.info("Cliente desconectado")


async def main():
    en_segundo_plano(vigilar_inactividad())
    async with serve(atender, HOST, PORT, max_size=None, ping_interval=20) as servidor:
        log.info("Escuchando en ws://%s:%d%s", HOST, PORT, RUTA)
        await servidor.serve_forever()


if __name__ == "__main__":
    asyncio.run(main())
