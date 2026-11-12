"""Prueba el servicio de voz contra PROTOCOLO-VOZ.md: TTS en cola, ASR, cancel y tiempos.

Uso: python prueba_voz.py [carpeta de salida]
Guarda el audio sintetizado como WAV en la carpeta (por defecto, la actual).
La URL sale de CHARLANDO_VOZ_URL (por defecto ws://127.0.0.1:8766/voz).
"""
import asyncio
import json
import os
import sys
import time
import wave

import librosa
import numpy as np
import websockets

URL = os.environ.get("CHARLANDO_VOZ_URL", "ws://127.0.0.1:8766/voz")
FRASES = ["Hola, soy la voz de Charlando.",
          "Esta es la segunda frase, pedida sin esperar a la primera.",
          "Y esta es la tercera, para comprobar que llegan en orden."]
LARGA = ("Esta es una frase muy larga para probar la cancelación: el servicio debería cortarla a medias, "
         "vaciar la cola de frases pendientes y avisar de que ya no va a llegar más audio de nada de lo "
         "que se pidió antes, aunque esta frase siga y siga durante bastantes segundos más.")
ESPERA = 300  # la primera petición puede tener que cargar el modelo
SILENCIO_TRAS_CANCEL = 3.0
TROZO_ASR = 16000 * 2 * 40 // 1000  # 40 ms a 16 kHz, como la app


class Fallo(Exception):
    pass


def guardar(ruta, pcm, frecuencia):
    with wave.open(ruta, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(frecuencia)
        w.writeframes(pcm)


async def recibir(ws, espera=ESPERA):
    try:
        return await asyncio.wait_for(ws.recv(), espera)
    except asyncio.TimeoutError:
        raise Fallo(f"no llega nada en {espera} s")


async def recibir_tts(ws, ids, t_envio):
    """Reparte el binario a la frase más antigua sin tts_end, hasta cerrar todas."""
    pendientes = list(ids)
    audio = {i: bytearray() for i in ids}
    primer, fin = {}, {}
    while pendientes:
        msg = await recibir(ws)
        t = time.perf_counter() - t_envio
        if isinstance(msg, bytes):
            primer.setdefault(pendientes[0], t)
            audio[pendientes[0]].extend(msg)
            continue
        datos = json.loads(msg)
        if datos["type"] == "tts_end":
            if datos.get("id") != pendientes[0]:
                raise Fallo(f"tts_end de {datos.get('id')} cuando tocaba {pendientes[0]}")
            fin[pendientes.pop(0)] = t
        elif datos["type"] == "error":
            raise Fallo(f"error del servicio: {datos}")
        else:
            raise Fallo(f"mensaje inesperado: {datos}")
    return audio, primer, fin


async def calentamiento(ws):
    await ws.send(json.dumps({"type": "warmup"}))
    t0 = time.perf_counter()
    await ws.send(json.dumps({"type": "tts", "id": "calentar", "text": "Hola."}))
    await recibir_tts(ws, ["calentar"], t0)
    print(f"Modelo listo en {time.perf_counter() - t0:.1f} s")


async def prueba_cola(ws, salida):
    print("\n1. TTS de 3 frases seguidas, sin esperar")
    ids = [f"f{i}" for i in range(1, len(FRASES) + 1)]
    t0 = time.perf_counter()
    for i, texto in zip(ids, FRASES):
        await ws.send(json.dumps({"type": "tts", "id": i, "text": texto}))
    audio, primer, fin = await recibir_tts(ws, ids, t0)
    tiempos = []
    for i in ids:
        dur = len(audio[i]) / 2 / 24000
        if dur < 0.5:
            raise Fallo(f"{i}: solo {dur:.2f} s de audio")
        guardar(os.path.join(salida, f"tts_{i}.wav"), bytes(audio[i]), 24000)
        print(f"   {i}: primer audio {primer[i] * 1000:5.0f} ms · tts_end {fin[i]:5.2f} s · {dur:.1f} s de audio")
        tiempos.append((i, primer[i], fin[i], dur))
    todo = b"".join(bytes(audio[i]) for i in ids)
    guardar(os.path.join(salida, "tts_todo.wav"), todo, 24000)
    print(f"   OK: 3 tts_end en orden; audio en {salida}/tts_*.wav")
    return todo, tiempos


async def prueba_asr(ws, pcm24):
    print("\n2. ASR del audio anterior, remuestreado a 16 kHz")
    voz = np.frombuffer(pcm24, dtype="<i2").astype(np.float32) / 32768
    pcm16 = (np.clip(librosa.resample(voz, orig_sr=24000, target_sr=16000), -1, 1) * 32767).astype("<i2").tobytes()
    await ws.send(json.dumps({"type": "asr_start", "id": "a1"}))
    for i in range(0, len(pcm16), TROZO_ASR):
        await ws.send(pcm16[i:i + TROZO_ASR])
    t0 = time.perf_counter()
    await ws.send(json.dumps({"type": "asr_end", "id": "a1"}))
    datos = json.loads(await recibir(ws))
    t_asr = time.perf_counter() - t0
    if datos.get("type") != "asr_result" or datos.get("id") != "a1":
        raise Fallo(f"se esperaba asr_result de a1: {datos}")
    print(f"   {len(pcm16) / 32000:.1f} s de audio · asr_result en {t_asr * 1000:.0f} ms")
    print(f"   texto: «{datos['text']}»")
    if not datos["text"]:
        raise Fallo("la transcripción salió vacía")

    await ws.send(json.dumps({"type": "asr_start", "id": "a2"}))
    await ws.send(pcm16[:16000 * 2 // 5])  # 0,2 s
    await ws.send(json.dumps({"type": "asr_end", "id": "a2"}))
    datos = json.loads(await recibir(ws))
    if datos != {"type": "asr_result", "id": "a2", "text": ""}:
        raise Fallo(f"audio de 0,2 s: se esperaba texto vacío: {datos}")
    print("   OK; con 0,2 s de audio devuelve \"\"")
    return t_asr, len(pcm16) / 32000


async def prueba_cancel(ws):
    print("\n3. cancel en mitad de una frase larga, con otras dos en cola")
    t0 = time.perf_counter()
    for i, texto in (("larga", LARGA), ("cola1", FRASES[1]), ("cola2", FRASES[2])):
        await ws.send(json.dumps({"type": "tts", "id": i, "text": texto}))
    recibido = 0
    while recibido < 24000 * 2:  # 1 s de audio, para cortar con la frase ya sonando
        msg = await recibir(ws)
        if not isinstance(msg, bytes):
            raise Fallo(f"mensaje inesperado antes de cancelar: {msg}")
        recibido += len(msg)
    t_cancel = time.perf_counter()
    await ws.send(json.dumps({"type": "cancel"}))
    tras_cancel = 0
    while True:
        msg = await recibir(ws, 30)
        if isinstance(msg, bytes):
            tras_cancel += len(msg)
            continue
        datos = json.loads(msg)
        if datos["type"] == "cancelled":
            break
        raise Fallo(f"mensaje inesperado antes de cancelled: {datos}")
    t_cancelled = time.perf_counter() - t_cancel
    print(f"   cancel a los {t_cancel - t0:.2f} s · cancelled en {t_cancelled * 1000:.0f} ms · "
          f"{tras_cancel / 48000:.2f} s de audio entre cancel y cancelled")
    try:
        msg = await asyncio.wait_for(ws.recv(), SILENCIO_TRAS_CANCEL)
        raise Fallo(f"llega algo tras cancelled: {msg if isinstance(msg, str) else f'{len(msg)} bytes de audio'}")
    except asyncio.TimeoutError:
        pass
    print(f"   OK: nada en {SILENCIO_TRAS_CANCEL:.0f} s tras cancelled")

    t0 = time.perf_counter()
    await ws.send(json.dumps({"type": "tts", "id": "despues", "text": "Sigo aquí."}))
    audio, primer, _ = await recibir_tts(ws, ["despues"], t0)
    print(f"   OK: tras cancel sigue sintetizando (primer audio {primer['despues'] * 1000:.0f} ms)")
    return t_cancelled


async def main():
    salida = sys.argv[1] if len(sys.argv) > 1 else "."
    os.makedirs(salida, exist_ok=True)
    try:
        async with websockets.connect(URL, max_size=None) as ws:
            await calentamiento(ws)
            pcm, tiempos = await prueba_cola(ws, salida)
            t_asr, dur_asr = await prueba_asr(ws, pcm)
            t_cancelled = await prueba_cancel(ws)
    except Fallo as e:
        print(f"\nFALLO: {e}")
        sys.exit(1)

    print("\n4. Tiempos")
    for i, primer, fin, dur in tiempos:
        print(f"   {i}: primer audio {primer * 1000:5.0f} ms desde el envío · {dur:.1f} s de audio · fin {fin:.2f} s")
    total = tiempos[-1][2]
    audio = sum(t[3] for t in tiempos)
    print(f"   TTS: {audio:.1f} s de audio en {total:.2f} s (×{audio / total:.2f} del tiempo real)")
    print(f"   ASR: {dur_asr:.1f} s de audio en {t_asr * 1000:.0f} ms")
    print(f"   cancel → cancelled: {t_cancelled * 1000:.0f} ms")
    print("\nTodo OK")


asyncio.run(main())
