"""Servidor de Charlando: ASR (LFM3) -> Claude (Agent SDK) -> TTS (LFM3) por WebSocket."""
import asyncio
import gc
import json
import logging
import os
import re
import subprocess
import sys
import threading
import time
import traceback
import uuid
from concurrent.futures import ThreadPoolExecutor

import librosa
import numpy as np
import torch
from claude_agent_sdk import ClaudeAgentOptions, ClaudeSDKClient, get_session_info
from dotenv import load_dotenv
from claude_agent_sdk.types import StreamEvent
from transformers import AutoModelForMultimodalLM, AutoProcessor
from transformers.generation.streamers import BaseStreamer
from websockets.asyncio.server import serve

load_dotenv(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".env"))
HOST = os.environ.get("CHARLANDO_HOST", "127.0.0.1")
PORT = int(os.environ.get("CHARLANDO_PORT", 8765))
# Un id de Hugging Face o una carpeta local. La revisión fija el código remoto que se ejecuta.
MODELO = os.environ.get("CHARLANDO_MODELO", "LiquidAI/LFM3-Audio-2B")
REVISION = os.environ.get("CHARLANDO_MODELO_REVISION", "2d6c1f9") or None
# Opcional: URL que libera la GPU de otro servidor de modelos (p. ej. llama-swap) antes de cargar.
LIBERAR_GPU_URL = os.environ.get("CHARLANDO_LIBERAR_GPU_URL", "")
AUDIO_PARAMS = dict(audio_temperature=1.0, audio_top_k=128,
                    audio_repetition_penalty=1.1, audio_repetition_window=4096)
MODELOS = {"haiku", "sonnet", "opus"}
SISTEMA = ("Eres un asistente de voz en español. Tus respuestas se leen en voz alta. "
           "Responde en dos o tres frases cortas y naturales, como en una charla; "
           "si el tema da para más, ofrece seguir en vez de alargarte. "
           "Sin markdown, sin listas y sin emojis. "
           "Puedes leer, buscar, crear y editar archivos dentro de tu carpeta de trabajo, y buscar en la web. "
           "Úsalo solo cuando haga falta. Antes de usar una herramienta no anuncies nada: "
           "el sistema ya avisa en voz alta. Al contar lo que encontraste, resume con tus palabras "
           "y no leas URLs ni rutas. No añadas lista de fuentes ni enlaces.")
# Leer y escribir solo en la carpeta de trabajo; lo que no esté aquí se rechaza solo (permission_mode="dontAsk").
CARPETA = os.environ.get("CHARLANDO_CARPETA", "/tmp/charlando")
os.makedirs(CARPETA, exist_ok=True)  # /tmp se vacía al reiniciar
HERRAMIENTAS = ["Read", "Grep", "Glob", "Write", "Edit", "WebSearch", "WebFetch"]
PERMITIDAS = [f"{h}(/{CARPETA}/**)" for h in ("Read", "Grep", "Glob", "Write", "Edit")] + ["WebSearch", "WebFetch"]
AVISO_HERRAMIENTA = "Déjame mirarlo."
PENSAR = object()  # marca en la cola de frases: Claude está usando una herramienta
FRAMES_POR_TROZO = 4  # 4 × 80 ms: decodificar frame a frame deja el TTS por debajo del tiempo real
FIN_FRASE = ".?!:;\n"
CIERRE = "\"'»)]”’"  # comillas y paréntesis que cierran la frase junto al punto
FUENTES = re.compile(r"^(fuentes|sources)\s*:?$", re.IGNORECASE)
MUESTRAS_MINIMAS = 16000 // 3  # menos de 0,33 s de audio se considera silencio
INACTIVIDAD = int(os.environ.get("CHARLANDO_INACTIVIDAD", 180))  # segundos sin turnos antes de descargar LFM3
# Sin memorias y sin los conectores de la cuenta de claude.ai (Canva, Notion…): Claude solo ve
# las herramientas de HERRAMIENTAS.
ENTORNO_CLAUDE = {"CLAUDE_CODE_DISABLE_AUTO_MEMORY": "1", "ENABLE_CLAUDEAI_MCP_SERVERS": "false"}


logging.basicConfig(level=logging.INFO, format="%(asctime)s %(message)s", datefmt="%H:%M:%S")
log = logging.getLogger("charlando")

# Un único hilo: el ASR y el TTS usan la GPU en serie.
GPU = ThreadPoolExecutor(max_workers=1, thread_name_prefix="gpu")
model = processor = codec = None
ACTIVA = None  # sesión del único cliente conectado
cargado = calentado = False
ultimo_uso = time.monotonic()
CARGA = asyncio.Lock()
TAREAS = set()  # referencias a las tareas sueltas, para que no las recoja el recolector


class Cancelado(Exception):
    pass


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


def cortar(texto):
    """Devuelve las frases ya cerradas y el resto pendiente.

    Solo corta si tras la puntuación (y sus comillas de cierre) viene un espacio, así que no parte
    «3.5», «10:30», «nota.txt» ni URLs. Si la puntuación es lo último recibido, espera al siguiente
    carácter.
    """
    frases, inicio = [], 0
    for i, c in enumerate(texto):
        if c not in FIN_FRASE or i < inicio:
            continue
        fin = i + 1
        while fin < len(texto) and texto[fin] in CIERRE:
            fin += 1
        if c != "\n":
            if fin >= len(texto):
                break
            if not texto[fin].isspace():
                continue
        frase = texto[inicio:fin].strip()
        if frase:
            frases.append(frase)
        inicio = fin
    resto = texto[inicio:]
    if len(resto) > 200 and " " in resto[:200]:
        k = resto.rfind(" ", 0, 200)
        frases.append(resto[:k].strip())
        resto = resto[k + 1:]
    return frases, resto


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
    log.info("Calentamiento ASR: %.2f s · «%s»", time.perf_counter() - t0, texto)

    # Comprobación: el streaming por frames debe dar el mismo audio que decodificar todo de una vez.
    with torch.inference_mode():
        completo = codec.decode(torch.cat(emisor.guardar, dim=2)).audio_values[0]
    por_trozos = voz
    ref = completo.float().clamp(-1, 1).cpu().numpy()
    n = min(len(ref), len(por_trozos))
    log.info("Streaming del códec: %d muestras (completo: %d), diferencia máxima %.4f",
             len(por_trozos), len(ref), float(np.abs(ref[:n] - por_trozos[:n]).max()))


def para_voz(frase):
    """Quita lo que no se puede decir en voz alta: enlaces, URLs y marcas de markdown."""
    frase = re.sub(r"\[([^\]]+)\]\([^)]*\)", r"\1", frase)
    frase = re.sub(r"https?://\S+", "", frase)
    frase = re.sub(r"^\s*(?:[-*•]|\d+[.)])\s+", "", frase)
    frase = re.sub(r"[*_#`]+", "", frase)
    frase = re.sub(r"\s+", " ", frase).strip()
    return frase if re.search(r"\w", frase) else ""


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


async def asegurar_modelo():
    global cargado
    tocar()
    async with CARGA:
        if not cargado:
            await asyncio.get_running_loop().run_in_executor(GPU, cargar_y_calentar)
            cargado = True


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
        en_uso = ACTIVA is not None and (ACTIVA.grabando or ACTIVA.ocupada())
        if not cargado or en_uso or time.monotonic() - ultimo_uso < INACTIVIDAD:
            continue
        async with CARGA:
            await asyncio.get_running_loop().run_in_executor(GPU, descargar_modelo)
            cargado = False
        log.info("Modelo descargado tras %d s sin uso", INACTIVIDAD)


async def turno_tts(sesion, cola_frases):
    """Lee frases de la cola y manda su audio, una tras otra."""
    loop = asyncio.get_running_loop()
    while (frase := await cola_frases.get()) is not None:
        if sesion.cancelar.is_set():
            break
        if frase is PENSAR:
            if sesion.hablando:
                sesion.hablando = False
                await sesion.enviar({"type": "state", "state": "thinking"})
            continue
        if not sesion.hablando:
            sesion.hablando = True
            await sesion.enviar({"type": "state", "state": "speaking"})
        await sesion.enviar({"type": "sentence", "text": frase})
        log.info("  frase: %s", frase)
        cola = asyncio.Queue()
        emisor = Emisor(lambda pcm: loop.call_soon_threadsafe(cola.put_nowait, pcm), sesion.cancelar)

        def trabajo(frase=frase, emisor=emisor):
            try:
                sintetizar(frase, emisor)
            except Cancelado:
                pass
            finally:
                loop.call_soon_threadsafe(cola.put_nowait, None)

        futuro = loop.run_in_executor(GPU, trabajo)
        while (pcm := await cola.get()) is not None:
            if sesion.t_audio is None:
                sesion.t_audio = time.perf_counter()
                log.info("  primer audio: %.0f ms desde ptt_end", (sesion.t_audio - sesion.t_fin) * 1000)
            await sesion.ws.send(pcm)
        await futuro


class Sesion:
    def __init__(self, ws):
        self.ws = ws
        self.cliente = None
        self.modelo = None
        self.grabando = False
        self.audio = bytearray()
        self.tarea = None
        self.cancelar = threading.Event()
        self.hablando = False
        self.t_fin = self.t_audio = None

    async def enviar(self, mensaje):
        await self.ws.send(json.dumps(mensaje))

    async def error(self, texto, codigo=None):
        log.error("error: %s", texto)
        mensaje = {"type": "error", "message": texto}
        if codigo:
            mensaje["code"] = codigo
        await self.enviar(mensaje)

    def ocupada(self):
        return self.tarea is not None and not self.tarea.done()

    async def cerrar_turno(self):
        self.cancelar.set()
        if self.tarea is not None:
            self.tarea.cancel()
            try:
                await self.tarea
            except (asyncio.CancelledError, Exception):
                pass
        self.tarea = None

    async def cerrar_cliente(self):
        if self.cliente is not None:
            try:
                await self.cliente.disconnect()
            except Exception:
                log.warning("No se pudo cerrar la sesión de Claude limpiamente")
        self.cliente = None

    async def nueva_conversacion(self, modelo, razonar, reanudar=None):
        await self.cerrar_turno()
        await self.cerrar_cliente()
        sesion = reanudar or str(uuid.uuid4())
        id_sesion = {"resume": sesion} if reanudar else {"session_id": sesion}
        opciones = ClaudeAgentOptions(model=modelo, system_prompt=SISTEMA, tools=HERRAMIENTAS,
                                      allowed_tools=PERMITIDAS, permission_mode="dontAsk",
                                      setting_sources=[], mcp_servers={}, include_partial_messages=True,
                                      cwd=CARPETA, env=ENTORNO_CLAUDE,
                                      thinking={"type": "adaptive" if razonar else "disabled"},
                                      **id_sesion)
        self.cliente = ClaudeSDKClient(opciones)
        await self.cliente.connect()
        self.modelo = modelo
        log.info("Conversación %s con %s", "reanudada" if reanudar else "nueva", modelo)
        await self.enviar({"type": "ready", "model": modelo, "session": sesion})

    async def turno(self, audio_bytes):
        self.t_fin = time.perf_counter()
        self.t_audio = None
        self.hablando = False
        self.cancelar = threading.Event()
        log.info("ptt_end: %.1f s de audio", len(audio_bytes) / 32000)
        await self.enviar({"type": "state", "state": "thinking"})

        audio = np.frombuffer(audio_bytes, dtype="<i2").astype(np.float32) / 32768
        if len(audio) < MUESTRAS_MINIMAS:
            texto = ""
        else:
            texto = await asyncio.get_running_loop().run_in_executor(GPU, asr, audio)
        log.info("transcript a los %.0f ms: «%s»", (time.perf_counter() - self.t_fin) * 1000, texto)
        await self.enviar({"type": "transcript", "text": texto})
        if texto:
            await self.responder(texto)
        await self.enviar({"type": "reply_end"})
        log.info("reply_end a los %.0f ms", (time.perf_counter() - self.t_fin) * 1000)

    async def responder(self, texto):
        await self.cliente.query(texto)
        cola_frases = asyncio.Queue()
        locutor = asyncio.create_task(turno_tts(self, cola_frases))
        pendiente = ""
        primera = True
        en_herramientas = False  # varias herramientas seguidas llevan un solo aviso
        callar = False  # tras «Fuentes:» solo vienen enlaces: no se dicen

        async def decir(frase):
            nonlocal primera, callar
            if callar or FUENTES.match(frase.strip()):
                callar = True
                return
            frase = para_voz(frase)
            if not frase:
                return
            if primera:
                log.info("primera frase de Claude a los %.0f ms", (time.perf_counter() - self.t_fin) * 1000)
                primera = False
            await cola_frases.put(frase)

        try:
            async for msg in self.cliente.receive_response():
                if not isinstance(msg, StreamEvent):
                    continue
                ev = msg.event
                bloque = ev.get("content_block") or {}
                if ev.get("type") == "content_block_start" and bloque.get("type") == "tool_use":
                    log.info("  herramienta: %s", bloque.get("name"))
                    if pendiente.strip():
                        await decir(pendiente)
                        pendiente = ""
                    elif not en_herramientas:
                        await cola_frases.put(AVISO_HERRAMIENTA)
                    if not en_herramientas:
                        await cola_frases.put(PENSAR)
                    en_herramientas = True
                    continue
                if ev.get("type") == "content_block_delta" and ev["delta"].get("type") == "text_delta":
                    en_herramientas = False
                    pendiente += ev["delta"]["text"]
                    frases, pendiente = cortar(pendiente)
                    for frase in frases:
                        await decir(frase)
            if pendiente.strip():
                await decir(pendiente)
            await cola_frases.put(None)
            await locutor
        except BaseException:
            locutor.cancel()
            raise

    async def mensaje(self, datos):
        tipo = datos.get("type")
        if tipo == "start":
            modelo = datos.get("model")
            if modelo not in MODELOS:
                return await self.error(f"modelo desconocido: {modelo}")
            reanudar = datos.get("resume")
            if reanudar is not None and not sesion_existe(reanudar):
                return await self.error("la conversación ya no existe", "session_gone")
            en_segundo_plano(asegurar_modelo())
            await self.nueva_conversacion(modelo, datos.get("thinking") is True, reanudar)
        elif tipo == "ptt_start":
            if self.cliente is None:
                return await self.error("no hay conversación: envía start primero")
            if self.ocupada():
                return await self.error("turno en curso")
            self.grabando = True
            self.audio = bytearray()
            en_segundo_plano(asegurar_modelo())
        elif tipo == "ptt_end":
            if not self.grabando:
                return await self.error("ptt_end sin ptt_start")
            self.grabando = False
            self.tarea = asyncio.create_task(self.ejecutar_turno(bytes(self.audio)))
        else:
            await self.error(f"tipo desconocido: {tipo}")

    async def ejecutar_turno(self, audio):
        try:
            await asegurar_modelo()
            await self.turno(audio)
        except asyncio.CancelledError:
            raise
        except Exception as e:
            log.error("Fallo en el turno:\n%s", traceback.format_exc())
            await self.enviar({"type": "error", "message": f"{type(e).__name__}: {e}"})
        finally:
            tocar()

    async def correr(self):
        try:
            async for msg in self.ws:
                if isinstance(msg, bytes):
                    if self.grabando:
                        self.audio.extend(msg)
                    continue
                try:
                    datos = json.loads(msg)
                except json.JSONDecodeError:
                    await self.error("JSON no válido")
                    continue
                await self.mensaje(datos)
        finally:
            await self.cerrar_turno()
            await self.cerrar_cliente()


def sesion_existe(sesion):
    """La conversación de Claude que se quiere reanudar sigue guardada en disco."""
    try:
        uuid.UUID(sesion)
    except (TypeError, ValueError, AttributeError):
        return False
    return get_session_info(sesion, directory=CARPETA) is not None


async def atender(ws):
    global ACTIVA
    if ws.request.path != "/ws":
        await ws.close(1008, "ruta no válida")
        return
    if ACTIVA is not None:
        log.info("Llega otro cliente: se cierra la conexión anterior")
        await ACTIVA.ws.close(1000, "otro cliente")
    sesion = Sesion(ws)
    ACTIVA = sesion
    log.info("Cliente conectado")
    try:
        await sesion.correr()
    finally:
        if ACTIVA is sesion:
            ACTIVA = None
        log.info("Cliente desconectado")


async def main():
    en_segundo_plano(vigilar_inactividad())
    async with serve(atender, HOST, PORT, max_size=None, ping_interval=20) as servidor:
        log.info("Escuchando en ws://%s:%d/ws", HOST, PORT)
        await servidor.serve_forever()


if __name__ == "__main__":
    asyncio.run(main())
