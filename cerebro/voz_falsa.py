"""Voz falsa para probar el cerebro sin GPU: habla PROTOCOLO-VOZ.md.

Uso: python voz_falsa.py [texto1 texto2 ...]
Cada transcripción devuelve el siguiente texto de la lista; el último se repite. Sin textos, usa
CHARLANDO_VOZ_FALSA_TEXTO. Un audio de menos de 0,33 s se transcribe como "".
El TTS devuelve silencio a 24 kHz: CHARLANDO_VOZ_FALSA_SEG_LETRA segundos por letra (0,06).
Escucha en CHARLANDO_VOZ_HOST:CHARLANDO_VOZ_PORT (por defecto 127.0.0.1:8766).
"""
import asyncio
import json
import os
import sys

from websockets.asyncio.server import serve

HOST = os.environ.get("CHARLANDO_VOZ_HOST", "127.0.0.1")
PORT = int(os.environ.get("CHARLANDO_VOZ_PORT", 8766))
TEXTOS = sys.argv[1:] or [os.environ.get("CHARLANDO_VOZ_FALSA_TEXTO", "Hola, ¿qué tal?")]
SEG_LETRA = float(os.environ.get("CHARLANDO_VOZ_FALSA_SEG_LETRA", 0.06))
TROZO = 2400  # 100 ms a 24 kHz
RAPIDEZ = 4  # el silencio se genera cuatro veces más rápido que el tiempo real
MUESTRAS_MINIMAS = 16000 // 3

activa = None
turnos = 0


def registrar(texto):
    print(texto, flush=True)


class Cliente:
    def __init__(self, ws):
        self.ws = ws
        self.audio = {}
        self.abierta = None
        self.cola = asyncio.Queue()
        self.sintesis = asyncio.create_task(self.sintetizar())

    async def sintetizar(self):
        # Una sola tarea: como la GPU, atiende las peticiones en serie y en orden.
        while True:
            tipo, id_, dato = await self.cola.get()
            if tipo == "asr":
                await self.ws.send(json.dumps({"type": "asr_result", "id": id_, "text": dato}))
                continue
            muestras = int(len(dato) * SEG_LETRA * 24000)
            for i in range(0, muestras, TROZO):
                await self.ws.send(bytes(2 * min(TROZO, muestras - i)))
                await asyncio.sleep(TROZO / 24000 / RAPIDEZ)
            await self.ws.send(json.dumps({"type": "tts_end", "id": id_}))

    async def mensaje(self, datos):
        global turnos
        tipo, id_ = datos.get("type"), datos.get("id")
        if tipo != "tts":
            registrar(f"voz falsa: {tipo}")
        if tipo == "warmup":
            return
        if tipo == "asr_start":
            self.abierta = id_
            self.audio[id_] = bytearray()
        elif tipo == "asr_end":
            audio = self.audio.pop(id_, bytearray())
            self.abierta = None
            if len(audio) // 2 < MUESTRAS_MINIMAS:
                texto = ""
            else:
                texto = TEXTOS[min(turnos, len(TEXTOS) - 1)]
                turnos += 1
            self.cola.put_nowait(("asr", id_, texto))
        elif tipo == "tts":
            registrar(f"voz falsa: tts «{datos.get('text')}»")
            self.cola.put_nowait(("tts", id_, datos.get("text", "")))
        elif tipo == "cancel":
            self.sintesis.cancel()
            while not self.cola.empty():
                self.cola.get_nowait()
            self.sintesis = asyncio.create_task(self.sintetizar())
            await self.ws.send(json.dumps({"type": "cancelled"}))
        else:
            await self.ws.send(json.dumps({"type": "error", "message": f"tipo desconocido: {tipo}"}))

    async def correr(self):
        try:
            async for msg in self.ws:
                if isinstance(msg, bytes):
                    if self.abierta is not None:
                        self.audio[self.abierta].extend(msg)
                    continue
                await self.mensaje(json.loads(msg))
        finally:
            self.sintesis.cancel()


async def atender(ws):
    global activa
    if ws.request.path != "/voz":
        await ws.close(1008, "ruta no válida")
        return
    if activa is not None:
        await activa.ws.close(1000, "otro cliente")
    cliente = Cliente(ws)
    activa = cliente
    registrar("voz falsa: cliente conectado")
    try:
        await cliente.correr()
    finally:
        if activa is cliente:
            activa = None
        registrar("voz falsa: cliente desconectado")


async def main():
    async with serve(atender, HOST, PORT, max_size=None) as servidor:
        registrar(f"voz falsa en ws://{HOST}:{PORT}/voz")
        await servidor.serve_forever()


if __name__ == "__main__":
    asyncio.run(main())
