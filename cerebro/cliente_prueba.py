"""Hace de app sin ventana: abre una conversación, manda turnos, mide la latencia y detecta cortes de audio.

Uso: python cliente_prueba.py (--carpeta RUTA | --nueva) [opciones] turno1 [turno2 ...]
Cada turno es un WAV mono a 16 kHz o una duración como «2s», que manda ese tiempo de silencio
(sirve con voz_falsa.py, que no escucha el audio).
La URL sale de CHARLANDO_URL (por defecto ws://127.0.0.1:8765/ws).
Sale con código 1 si llega un error; imprime su `code` si lo trae.
"""
import argparse
import asyncio
import json
import os
import sys
import time
import wave

import websockets

URL = os.environ.get("CHARLANDO_URL", "ws://127.0.0.1:8765/ws")
TROZO_MS = 40


class Fallo(Exception):
    pass


def leer_turno(turno):
    if turno.endswith("s") and turno[:-1].replace(".", "", 1).isdigit():
        return bytes(2 * int(16000 * float(turno[:-1])))
    with wave.open(turno) as w:
        assert w.getframerate() == 16000 and w.getnchannels() == 1, f"{turno}: se espera WAV mono a 16 kHz"
        return w.readframes(w.getnframes())


async def turno(ws, nombre, a):
    pcm = leer_turno(nombre)
    print(f"\n--- {nombre} ---")
    await ws.send(json.dumps({"type": "ptt_start"}))
    paso = 16000 * 2 * TROZO_MS // 1000
    for i in range(0, len(pcm), paso):
        await ws.send(pcm[i:i + paso])
        await asyncio.sleep(TROZO_MS / 1000 / a.rapidez)
    t0 = time.perf_counter()
    await ws.send(json.dumps({"type": "ptt_end"}))

    # Simula la reproducción: si un trozo llega cuando el anterior ya terminó de sonar, hay un corte.
    # La espera de un permiso no cuenta como corte.
    fin_audio = primer = None
    cortes, silencio = 0, 0.0
    hay_frase = False
    while True:
        msg = await ws.recv()
        t = time.perf_counter() - t0
        if isinstance(msg, bytes):
            if not hay_frase:
                raise Fallo("audio sin su frase antes")
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
        elif tipo == "state":
            print(f"  {t:5.2f} s · estado: {datos['state']}")
        elif tipo == "sentence":
            hay_frase = True
            print(f"  {t:5.2f} s · Claude: {datos['text']}")
        elif tipo == "permission":
            print(f"  {t:5.2f} s · PERMISO {datos['tool']}: {datos['detail']}")
            if a.permisos == "colgar":
                await asyncio.sleep(a.esperar_permiso)
                print("  cuelgo con el permiso pendiente")
                await ws.close()
                return
            asyncio.create_task(responder_permiso(ws, datos["id"], a))
            # El tiempo hasta la respuesta no es un corte de audio.
            primer = fin_audio = None
        elif tipo == "error":
            print(f"  error: {datos['message']}" + (f" (code {datos['code']})" if "code" in datos else ""))
            raise Fallo(datos.get("code") or datos["message"])
        elif tipo == "reply_end":
            if primer is None:
                print("  sin audio")
            else:
                print(f"  audio {fin_audio - primer - silencio:.1f} s · "
                      f"cortes {cortes} ({silencio:.2f} s de silencio)")
            return


async def responder_permiso(ws, id_, a):
    await asyncio.sleep(a.esperar_permiso)
    permitir = a.permisos == "aceptar"
    print(f"  → {'acepto' if permitir else 'rechazo'} el permiso")
    await ws.send(json.dumps({"type": "permission_reply", "id": id_, "allow": permitir}))


async def main():
    args = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    donde = args.add_mutually_exclusive_group(required=True)
    donde.add_argument("--carpeta")
    donde.add_argument("--nueva", action="store_true", help="charla nueva en CHARLANDO_CHARLAS")
    args.add_argument("--modelo", default="haiku", choices=["haiku", "sonnet", "opus"])
    args.add_argument("--razonar", action="store_true")
    args.add_argument("--reanudar", metavar="SESION")
    args.add_argument("--permisos", default="rechazar", choices=["aceptar", "rechazar", "colgar"])
    args.add_argument("--esperar-permiso", type=float, default=0.5, metavar="SEG",
                      help="segundos antes de responder a la tarjeta")
    args.add_argument("--rapidez", type=float, default=1, help="manda el micrófono N veces más rápido")
    args.add_argument("turnos", nargs="*")
    a = args.parse_args()
    async with websockets.connect(URL, max_size=None) as ws:
        hola = json.loads(await ws.recv())
        print(f"hello: charlas={hola['charlas']} · {[c['path'] for c in hola['folders']]}")
        inicio = {"type": "start", "model": a.modelo, "thinking": a.razonar}
        if a.nueva:
            inicio["new_folder"] = True
        else:
            inicio["folder"] = os.path.abspath(a.carpeta)
        if a.reanudar:
            inicio["resume"] = a.reanudar
        await ws.send(json.dumps(inicio))
        listo = json.loads(await ws.recv())
        if listo["type"] == "error":
            print(f"error: {listo['message']}" + (f" (code {listo['code']})" if "code" in listo else ""))
            sys.exit(1)
        print(f"ready: {listo}")
        hola = json.loads(await ws.recv())
        assert hola["type"] == "hello", hola
        print(f"hello tras ready: {[c['path'] for c in hola['folders']]}")
        try:
            for t in a.turnos:
                await turno(ws, t, a)
        except Fallo as e:
            print(f"FALLO: {e}")
            sys.exit(1)


asyncio.run(main())
