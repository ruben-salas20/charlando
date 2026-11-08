"""Hace de app sin ventana: manda grabaciones al servidor, mide la latencia y detecta cortes de audio.

Uso: python cliente_prueba.py [--modelo haiku|sonnet|opus] [--razonar] audio1.wav [audio2.wav ...]
Cada archivo es un turno de la misma conversación; deben ser WAV mono a 16 kHz.
La URL sale de CHARLANDO_URL (por defecto ws://127.0.0.1:8765/ws).
"""
import argparse
import asyncio
import json
import os
import time
import wave

import websockets

URL = os.environ.get("CHARLANDO_URL", "ws://127.0.0.1:8765/ws")
TROZO_MS = 40


async def turno(ws, ruta):
    with wave.open(ruta) as w:
        assert w.getframerate() == 16000 and w.getnchannels() == 1, f"{ruta}: se espera WAV mono a 16 kHz"
        pcm = w.readframes(w.getnframes())
    print(f"\n--- {ruta} ---")
    await ws.send(json.dumps({"type": "ptt_start"}))
    paso = 16000 * 2 * TROZO_MS // 1000
    for i in range(0, len(pcm), paso):
        await ws.send(pcm[i:i + paso])
        await asyncio.sleep(TROZO_MS / 1000)
    t0 = time.perf_counter()
    await ws.send(json.dumps({"type": "ptt_end"}))

    # Simula la reproducción: si un trozo llega cuando el anterior ya terminó de sonar, hay un corte.
    fin_audio = primer = None
    cortes, silencio = 0, 0.0
    while True:
        msg = await ws.recv()
        t = time.perf_counter() - t0
        if isinstance(msg, bytes):
            if primer is None:
                primer = fin_audio = t
            if t > fin_audio + 0.001:
                cortes += 1
                silencio += t - fin_audio
                fin_audio = t
            fin_audio += len(msg) / 2 / 24000
            continue
        datos = json.loads(msg)
        tipo = datos["type"]
        if tipo == "transcript":
            print(f"  {t:5.2f} s · tú: {datos['text']}")
        elif tipo == "sentence":
            print(f"  {t:5.2f} s · Claude: {datos['text']}")
        elif tipo == "error":
            print(f"  error: {datos['message']}")
            return
        elif tipo == "reply_end":
            if primer is None:
                print("  sin audio")
            else:
                print(f"  primer audio {primer:.2f} s · audio {fin_audio - primer - silencio:.1f} s · "
                      f"cortes {cortes} ({silencio:.2f} s de silencio)")
            return


async def main():
    args = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    args.add_argument("--modelo", default="haiku", choices=["haiku", "sonnet", "opus"])
    args.add_argument("--razonar", action="store_true")
    args.add_argument("audios", nargs="+")
    a = args.parse_args()
    async with websockets.connect(URL, max_size=None) as ws:
        await ws.send(json.dumps({"type": "start", "model": a.modelo, "thinking": a.razonar}))
        print(json.loads(await ws.recv()))
        for ruta in a.audios:
            await turno(ws, ruta)


asyncio.run(main())
