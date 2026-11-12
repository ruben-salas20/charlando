"""Cerebro de Charlando: abre una sesión de Claude Code en una carpeta y la habla con el servicio de voz."""
import asyncio
import datetime
import json
import logging
import os
import re
import time
import traceback
import uuid
from collections import deque

from claude_agent_sdk import (ClaudeAgentOptions, ClaudeSDKClient, PermissionResultAllow,
                              PermissionResultDeny, get_session_info)
from claude_agent_sdk.types import StreamEvent
from dotenv import load_dotenv
from websockets.asyncio.client import connect
from websockets.asyncio.server import serve
from websockets.exceptions import ConnectionClosed

load_dotenv(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".env"))
HOST = os.environ.get("CHARLANDO_HOST", "127.0.0.1")
PORT = int(os.environ.get("CHARLANDO_PORT", 8765))
VOZ_URL = os.environ.get("CHARLANDO_VOZ_URL", "ws://127.0.0.1:8766/voz")
CHARLAS = os.path.abspath(os.path.expanduser(os.environ.get("CHARLANDO_CHARLAS", "~/charlando")))
DEPURAR = os.environ.get("CHARLANDO_DEPURAR") == "1"
ESPERA_PERMISO = float(os.environ.get("CHARLANDO_ESPERA_PERMISO", 60))
# Si Claude usa herramientas sin decir nada, el aviso de reserva solo suena pasado este tiempo.
ESPERA_AVISO = float(os.environ.get("CHARLANDO_ESPERA_AVISO", 3))
CONFIG = os.path.join(os.environ.get("XDG_CONFIG_HOME") or os.path.expanduser("~/.config"), "charlando")
RECIENTES = os.path.join(CONFIG, "recientes.json")
MAX_RECIENTES = 8
CIERRE_OTRO_CLIENTE = 4001  # PROTOCOLO: la app que lo recibe no reintenta sola

MODELOS = {"haiku", "sonnet", "opus"}
REGLAS_VOZ = ("Hablas con la persona por voz, en español: tus respuestas se leen en voz alta. "
              "Responde en dos o tres frases cortas y naturales, como en una charla; "
              "si el tema da para más, ofrece seguir en vez de alargarte. "
              "Sin markdown, sin listas, sin tablas, sin bloques de código y sin emojis. "
              "No leas en voz alta rutas largas, URLs, comandos ni código: si algo necesita verse, "
              "déjalo en un archivo de la carpeta de trabajo y di cómo se llama. "
              "Antes de usar herramientas, di en una frase corta y concreta qué vas a hacer, "
              "por ejemplo «Miro tus notas.» o «Lo apunto en el diario.». Dilo una sola vez aunque "
              "uses varias herramientas seguidas, y no repitas la misma fórmula en cada turno. "
              "Cuando una herramienta necesita permiso, la persona lo acepta o lo rechaza en pantalla: "
              "no le pidas que lo diga. "
              "Al contar lo que encontraste, resume con tus palabras. No añadas lista de fuentes ni enlaces.")
# Necesitan una interfaz que Charlando no tiene.
DESACTIVADAS = ["AskUserQuestion", "EnterPlanMode", "ExitPlanMode"]
LIBRES = {"Read", "Grep", "Glob", "WebSearch", "WebFetch", "TodoWrite"}
ESCRITURA = {"Write": "file_path", "Edit": "file_path", "NotebookEdit": "notebook_path"}
AVISO_HERRAMIENTA = "Dame un momento."
AVISO_PERMISO = "Necesito tu permiso. Míralo en pantalla."
RECORDATORIO_PERMISO = "Sigo esperando tu permiso. Míralo en pantalla."
FIN_FRASE = ".?!:;\n"
CIERRE = "\"'»)]”’"  # comillas y paréntesis que cierran la frase junto al punto
FUENTES = re.compile(r"^(fuentes|sources)\s*:?$", re.IGNORECASE)

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(message)s", datefmt="%H:%M:%S")
log = logging.getLogger("cerebro")
logging.getLogger("websockets").setLevel(logging.WARNING)


def depurar(texto, *args):
    if DEPURAR:
        log.info(texto, *args)


class VozCaida(Exception):
    pass


class ErrorVoz(Exception):
    pass


class Cancelado(ErrorVoz):
    pass


# --- Texto para la voz -------------------------------------------------------------------------

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


def para_voz(frase):
    """Quita lo que no se puede decir en voz alta: enlaces, URLs y marcas de markdown."""
    frase = re.sub(r"\[([^\]]+)\]\([^)]*\)", r"\1", frase)
    frase = re.sub(r"https?://\S+", "", frase)
    frase = re.sub(r"^\s*(?:[-*•]|\d+[.)])\s+", "", frase)
    frase = re.sub(r"[*_#`|]+", "", frase)
    frase = re.sub(r"\s+", " ", frase).strip()
    return frase if re.search(r"\w", frase) else ""


# --- Carpetas ------------------------------------------------------------------------------------

def nombre(carpeta):
    return os.path.basename(carpeta.rstrip("/")) or carpeta


def leer_recientes():
    try:
        with open(RECIENTES, encoding="utf-8") as f:
            rutas = json.load(f)
    except (OSError, ValueError):
        return []
    return [r for r in rutas if isinstance(r, str)][:MAX_RECIENTES]


def apuntar_reciente(carpeta):
    rutas = [carpeta] + [r for r in leer_recientes() if r != carpeta]
    os.makedirs(CONFIG, exist_ok=True)
    temporal = RECIENTES + ".tmp"
    with open(temporal, "w", encoding="utf-8") as f:
        json.dump(rutas[:MAX_RECIENTES], f, ensure_ascii=False, indent=1)
    os.replace(temporal, RECIENTES)


def carpeta_nueva():
    base = os.path.join(CHARLAS, f"{datetime.date.today().isoformat()}-charla")
    os.makedirs(CHARLAS, exist_ok=True)
    n = 1
    while True:
        ruta = base if n == 1 else f"{base}-{n}"
        try:
            os.mkdir(ruta)
            return ruta
        except FileExistsError:
            n += 1


def dentro(ruta, carpeta):
    """La ruta, con los enlaces simbólicos resueltos, queda dentro de la carpeta."""
    if not isinstance(ruta, str) or not ruta:
        return False
    real = os.path.realpath(os.path.join(carpeta, os.path.expanduser(ruta)))
    base = os.path.realpath(carpeta)
    return real == base or real.startswith(base.rstrip(os.sep) + os.sep)


def sesion_existe(sesion, carpeta):
    """La conversación de Claude que se quiere reanudar sigue guardada en disco."""
    try:
        uuid.UUID(sesion)
    except (TypeError, ValueError, AttributeError):
        return False
    return get_session_info(sesion, directory=carpeta) is not None


# --- Cliente del servicio de voz ---------------------------------------------------------------

class Frase:
    """Audio de una frase pedida a la voz: trozos PCM y, al final, None o una excepción."""

    def __init__(self, id_):
        self.id = id_
        self.cola = asyncio.Queue()

    def cerrar(self, fin=None):
        self.cola.put_nowait(fin)

    async def trozos(self):
        while True:
            trozo = await self.cola.get()
            if trozo is None:
                return
            if isinstance(trozo, Exception):
                raise trozo
            yield trozo


class Voz:
    """Conexión permanente con el servicio de voz (PROTOCOLO-VOZ.md)."""

    def __init__(self, url):
        self.url = url
        self.ws = None
        self.salida = asyncio.Queue()  # un solo escritor: los mensajes salen en el orden pedido
        self.frases = deque()  # el binario es de la frase más antigua sin tts_end
        self.asr = {}
        self.descartar = False  # tras cancel, hasta cancelled

    @property
    def conectada(self):
        return self.ws is not None

    def _enviar(self, mensaje):
        if self.ws is None:
            raise VozCaida("no hay conexión con el servicio de voz")
        self.salida.put_nowait(mensaje)

    def warmup(self):
        if self.ws is not None:
            self.salida.put_nowait(json.dumps({"type": "warmup"}))

    def asr_start(self):
        id_ = uuid.uuid4().hex
        self._enviar(json.dumps({"type": "asr_start", "id": id_}))
        futuro = asyncio.get_running_loop().create_future()
        futuro.add_done_callback(lambda f: f.cancelled() or f.exception())  # nadie lo espera si se cancela
        self.asr[id_] = futuro
        return id_

    def audio(self, pcm):
        if self.ws is not None:
            self.salida.put_nowait(pcm)

    def asr_end(self, id_):
        self._enviar(json.dumps({"type": "asr_end", "id": id_}))
        return self.asr[id_]

    def tts(self, texto):
        frase = Frase(uuid.uuid4().hex)
        self._enviar(json.dumps({"type": "tts", "id": frase.id, "text": texto}))
        self.frases.append(frase)
        return frase

    def cancel(self):
        """Corta todo lo pedido. El audio que aún llegue de antes de `cancelled` se tira."""
        self._fallar(Cancelado("cancelado"))
        if self.ws is not None:
            self.descartar = True
            self.salida.put_nowait(json.dumps({"type": "cancel"}))

    def _fallar(self, error):
        while self.frases:
            self.frases.popleft().cerrar(error)
        for futuro in self.asr.values():
            if not futuro.done():
                futuro.set_exception(error)
        self.asr.clear()

    async def correr(self):
        avisado = False
        while True:
            try:
                async with connect(self.url, max_size=None, ping_interval=20, open_timeout=5) as ws:
                    self.ws, self.descartar = ws, False
                    self.salida = asyncio.Queue()
                    log.info("Conectado con la voz")
                    avisado = False
                    escritor = asyncio.create_task(self._escribir(ws))
                    try:
                        async for msg in ws:
                            self._recibir(msg)
                    finally:
                        escritor.cancel()
            except (OSError, ConnectionClosed, asyncio.TimeoutError) as e:
                if self.ws is not None:
                    log.error("Se perdió la conexión con la voz: %s", e)
                elif not avisado:
                    log.error("No llego a la voz (%s): reintento cada 2 s", e)
                    avisado = True
            except Exception:
                log.error("Fallo en la conexión con la voz:\n%s", traceback.format_exc())
            if self.ws is not None:
                self.ws = None
                self._fallar(VozCaida("se perdió la conexión con el servicio de voz"))
            await asyncio.sleep(2)

    async def _escribir(self, ws):
        cola = self.salida
        while True:
            await ws.send(await cola.get())

    def _recibir(self, msg):
        if isinstance(msg, bytes):
            if not self.descartar and self.frases:
                self.frases[0].cola.put_nowait(msg)
            return
        datos = json.loads(msg)
        tipo, id_ = datos.get("type"), datos.get("id")
        if tipo == "asr_result":
            futuro = self.asr.pop(id_, None)
            if futuro is not None and not futuro.done():
                futuro.set_result(datos.get("text") or "")
        elif tipo == "tts_end":
            self._terminar_frase(id_, None)
        elif tipo == "cancelled":
            self.descartar = False
        elif tipo == "error":
            log.error("Error de la voz: %s", datos.get("message"))
            error = ErrorVoz(datos.get("message") or "error de la voz")
            if id_ in self.asr:
                futuro = self.asr.pop(id_)
                if not futuro.done():
                    futuro.set_exception(error)
            else:
                self._terminar_frase(id_, error)

    def _terminar_frase(self, id_, fin):
        for frase in self.frases:
            if frase.id == id_:
                self.frases.remove(frase)
                frase.cerrar(fin)
                return


VOZ = Voz(VOZ_URL)


# --- Sesión con la app -------------------------------------------------------------------------

PENSAR = object()        # Claude va a usar una herramienta: pasa a thinking si estaba hablando
TRAS_PERMISO = object()  # la persona respondió a la tarjeta: siempre thinking


class Permiso:
    def __init__(self, herramienta, detalle):
        self.id = uuid.uuid4().hex
        self.herramienta, self.detalle = herramienta, detalle
        self.respuesta = asyncio.get_running_loop().create_future()
        self.mostrado = asyncio.Event()


class Sesion:
    def __init__(self, ws):
        self.ws = ws
        self.cliente = None
        self.modelo = self.carpeta = None
        self.asr_id = None
        self.tarea = None
        self.cola = None  # frases del turno en curso, en orden
        self.hablando = False
        self.permiso = None
        self.turno_permisos = asyncio.Lock()
        self.vistas = set()  # tool_use ya avisados en la cola
        self.aviso = None  # tarea del aviso de reserva, si Claude calla mientras usa herramientas
        self.avisado = False  # el aviso de reserva suena como mucho una vez por turno
        self.vista = asyncio.Event()
        self.t_fin = self.t_audio = None

    async def enviar(self, mensaje):
        await self.ws.send(json.dumps(mensaje, ensure_ascii=False))

    async def error(self, texto, codigo=None):
        log.error("error: %s", texto)
        mensaje = {"type": "error", "message": texto}
        if codigo:
            mensaje["code"] = codigo
        await self.enviar(mensaje)

    def ocupada(self):
        return self.tarea is not None and not self.tarea.done()

    def rechazar_permiso(self):
        if self.permiso is not None and not self.permiso.respuesta.done():
            self.permiso.respuesta.set_result(False)

    async def cerrar_turno(self):
        self.rechazar_permiso()
        if self.tarea is not None:
            self.tarea.cancel()
            try:
                await self.tarea
            except (asyncio.CancelledError, Exception):
                pass
        self.tarea = None
        if self.asr_id is not None:
            VOZ.cancel()
            self.asr_id = None

    async def cerrar_cliente(self):
        if self.cliente is not None:
            try:
                await self.cliente.disconnect()
            except Exception:
                log.warning("No se pudo cerrar la sesión de Claude limpiamente")
        self.cliente = None

    async def nueva_conversacion(self, carpeta, modelo, razonar, reanudar=None):
        await self.cerrar_turno()
        await self.cerrar_cliente()
        sesion = reanudar or str(uuid.uuid4())
        id_sesion = {"resume": sesion} if reanudar else {"session_id": sesion}
        opciones = ClaudeAgentOptions(
            model=modelo, cwd=carpeta,
            system_prompt={"type": "preset", "preset": "claude_code", "append": REGLAS_VOZ},
            tools={"type": "preset", "preset": "claude_code"}, disallowed_tools=DESACTIVADAS,
            setting_sources=["user", "project", "local"],
            # Explícito: si no, vale el defaultMode de los ajustes del usuario (p. ej. "auto").
            permission_mode="default", can_use_tool=self.puede_usar,
            include_partial_messages=True,
            thinking={"type": "adaptive" if razonar else "disabled"},
            **id_sesion)
        t0 = time.perf_counter()
        self.cliente = ClaudeSDKClient(opciones)
        await self.cliente.connect()
        self.modelo, self.carpeta = modelo, carpeta
        log.info("Conversación %s con %s en %.0f ms", "reanudada" if reanudar else "nueva", modelo,
                 (time.perf_counter() - t0) * 1000)
        depurar("  carpeta: %s", carpeta)
        apuntar_reciente(carpeta)
        await self.enviar({"type": "ready", "model": modelo, "session": sesion,
                           "folder": carpeta, "name": nombre(carpeta)})
        await self.hola()

    async def hola(self):
        await self.enviar({"type": "hello", "charlas": CHARLAS,
                           "folders": [{"path": r, "name": nombre(r)}
                                       for r in leer_recientes() if os.path.isdir(r)]})

    # --- Permisos --------------------------------------------------------------------------------

    async def puede_usar(self, herramienta, entrada, contexto):
        if herramienta in LIBRES or (herramienta in ESCRITURA
                                     and dentro(entrada.get(ESCRITURA[herramienta]), self.carpeta)):
            depurar("  libre: %s", herramienta)
            return PermissionResultAllow()
        if self.cola is None:
            return PermissionResultDeny(message="No hay nadie para dar el permiso.")
        try:
            async with self.turno_permisos:
                return await self.pedir_permiso(herramienta, entrada, contexto)
        except Exception as e:
            log.error("No se pudo pedir permiso: %s", e)
            return PermissionResultDeny(message="No se pudo pedir el permiso.", interrupt=True)

    async def pedir_permiso(self, herramienta, entrada, contexto):
        # El aviso de permiso sustituye al de reserva, y la tarjeta va después de lo ya dicho.
        self.cancelar_aviso()
        self.avisado = True
        id_uso = contexto.tool_use_id
        try:
            await asyncio.wait_for(self._esperar_vista(id_uso), 1)
        except asyncio.TimeoutError:
            pass
        permiso = Permiso(herramienta, detalle(herramienta, entrada, self.carpeta))
        self.permiso = permiso
        t0 = time.perf_counter()
        depurar("  permiso: %s · %s", herramienta, permiso.detalle)
        self.decir_ya(AVISO_PERMISO)
        self.cola.put_nowait(permiso)
        mostrado = asyncio.create_task(permiso.mostrado.wait())
        await asyncio.wait({mostrado, permiso.respuesta}, return_when=asyncio.FIRST_COMPLETED)
        mostrado.cancel()
        try:
            permitido = await asyncio.wait_for(asyncio.shield(permiso.respuesta), ESPERA_PERMISO)
        except asyncio.TimeoutError:
            if self.cola is not None:
                self.decir_ya(RECORDATORIO_PERMISO, recordatorio=True)
            permitido = await permiso.respuesta
        self.permiso = None
        log.info("permiso %s a los %.1f s", "aceptado" if permitido else "rechazado",
                 time.perf_counter() - t0)
        if self.cola is not None:
            self.cola.put_nowait(TRAS_PERMISO)
        if permitido:
            return PermissionResultAllow()
        return PermissionResultDeny(message="El usuario rechazó el permiso en pantalla.")

    async def _esperar_vista(self, id_uso):
        while id_uso not in self.vistas:
            self.vista.clear()
            await self.vista.wait()

    def programar_aviso(self):
        if self.avisado or self.aviso is not None:
            return

        async def esperar():
            await asyncio.sleep(ESPERA_AVISO)
            self.aviso = None
            if self.cola is not None and not self.avisado:
                self.avisado = True
                self.decir_ya(AVISO_HERRAMIENTA)
                self.cola.put_nowait(PENSAR)

        self.aviso = asyncio.create_task(esperar())

    def cancelar_aviso(self):
        if self.aviso is not None:
            self.aviso.cancel()
            self.aviso = None

    def decir_ya(self, texto, recordatorio=False):
        """Pide el audio a la voz ahora, para que se sintetice mientras suena lo anterior."""
        self.cola.put_nowait((texto, VOZ.tts(texto), recordatorio))

    # --- Turno ----------------------------------------------------------------------------------

    async def locutor(self):
        """Envía a la app, en orden, cada frase seguida de su audio, los cambios de estado y las tarjetas."""
        while (item := await self.cola.get()) is not None:
            if item is PENSAR or item is TRAS_PERMISO:
                if self.hablando or item is TRAS_PERMISO:
                    self.hablando = False
                    await self.enviar({"type": "state", "state": "thinking"})
                continue
            if isinstance(item, Permiso):
                await self.enviar({"type": "permission", "id": item.id, "tool": item.herramienta,
                                   "detail": item.detalle})
                item.mostrado.set()
                continue
            texto, frase, recordatorio = item
            if not self.hablando and not recordatorio:
                self.hablando = True
                await self.enviar({"type": "state", "state": "speaking"})
            await self.enviar({"type": "sentence", "text": texto})
            depurar("  frase: %s", texto)
            try:
                async for pcm in frase.trozos():
                    if self.t_audio is None:
                        self.t_audio = time.perf_counter()
                        log.info("  primer audio: %.0f ms desde ptt_end", (self.t_audio - self.t_fin) * 1000)
                    await self.ws.send(pcm)
            except Cancelado:
                raise
            except ErrorVoz as e:
                log.error("La voz no pudo decir una frase: %s", e)

    async def turno(self, futuro_asr):
        self.t_fin = time.perf_counter()
        self.t_audio = None
        self.hablando = False
        await self.enviar({"type": "state", "state": "thinking"})
        texto = await futuro_asr
        self.asr_id = None
        log.info("transcript a los %.0f ms", (time.perf_counter() - self.t_fin) * 1000)
        depurar("  tú: %s", texto)
        await self.enviar({"type": "transcript", "text": texto})
        if texto:
            await self.responder(texto)
        await self.enviar({"type": "reply_end"})
        log.info("reply_end a los %.0f ms", (time.perf_counter() - self.t_fin) * 1000)

    async def responder(self, texto):
        self.cola = asyncio.Queue()
        self.vistas.clear()
        self.avisado = False
        locutor = asyncio.create_task(self.locutor())
        pendiente = ""
        primera = True
        en_herramientas = False  # varias herramientas seguidas pasan una sola vez a thinking
        callar = False  # tras «Fuentes:» solo vienen enlaces: no se dicen

        def decir(frase):
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
            self.decir_ya(frase)

        try:
            await self.cliente.query(texto)
            async for msg in self.cliente.receive_response():
                if locutor.done():
                    locutor.result()  # si el locutor falló, que falle el turno
                if not isinstance(msg, StreamEvent):
                    continue
                ev = msg.event
                bloque = ev.get("content_block") or {}
                if ev.get("type") == "content_block_start" and bloque.get("type") == "tool_use":
                    depurar("  herramienta: %s", bloque.get("name"))
                    if pendiente.strip():
                        decir(pendiente)
                        pendiente = ""
                    elif not en_herramientas:
                        self.programar_aviso()
                    if not en_herramientas:
                        self.cola.put_nowait(PENSAR)
                    en_herramientas = True
                    self.vistas.add(bloque.get("id"))
                    self.vista.set()
                    continue
                if ev.get("type") == "content_block_delta" and ev["delta"].get("type") == "text_delta":
                    en_herramientas = False
                    self.cancelar_aviso()
                    pendiente += ev["delta"]["text"]
                    frases, pendiente = cortar(pendiente)
                    for frase in frases:
                        decir(frase)
            if pendiente.strip():
                decir(pendiente)
            self.cola.put_nowait(None)
            await locutor
        except BaseException:
            locutor.cancel()
            raise
        finally:
            self.cancelar_aviso()
            self.cola = None

    async def ejecutar_turno(self, futuro_asr):
        try:
            await self.turno(futuro_asr)
        except asyncio.CancelledError:
            VOZ.cancel()
            raise
        except Exception as e:
            log.error("Fallo en el turno:\n%s", traceback.format_exc())
            VOZ.cancel()
            await self.calmar_claude()
            mensaje = str(e) if isinstance(e, (VozCaida, ErrorVoz)) else f"{type(e).__name__}: {e}"
            try:
                await self.error(mensaje)
            except ConnectionClosed:
                pass

    async def calmar_claude(self):
        """Tras un fallo a mitad de respuesta, para a Claude y descarta lo que quede del turno."""
        if self.cliente is None:
            return
        try:
            await asyncio.wait_for(self._interrumpir(), 15)
        except Exception:
            log.warning("No se pudo interrumpir a Claude: se cierra la sesión")
            await self.cerrar_cliente()

    async def _interrumpir(self):
        await self.cliente.interrupt()
        async for _ in self.cliente.receive_response():
            pass

    # --- Mensajes de la app ---------------------------------------------------------------------

    async def mensaje(self, datos):
        tipo = datos.get("type")
        if tipo == "start":
            await self.empezar(datos)
        elif tipo == "ptt_start":
            if self.cliente is None:
                return await self.error("no hay conversación: envía start primero")
            if self.ocupada() or self.asr_id is not None:
                return await self.error("turno en curso")
            if not VOZ.conectada:
                return await self.error("no hay conexión con el servicio de voz")
            VOZ.warmup()
            self.asr_id = VOZ.asr_start()
        elif tipo == "ptt_end":
            if self.asr_id is None:
                return await self.error("ptt_end sin ptt_start")
            try:
                futuro = VOZ.asr_end(self.asr_id)
            except VozCaida as e:
                self.asr_id = None
                return await self.error(str(e))
            self.tarea = asyncio.create_task(self.ejecutar_turno(futuro))
        elif tipo == "permission_reply":
            p = self.permiso
            if p is not None and datos.get("id") == p.id and not p.respuesta.done():
                p.respuesta.set_result(datos.get("allow") is True)
        else:
            await self.error(f"tipo desconocido: {tipo}")

    async def empezar(self, datos):
        modelo = datos.get("model")
        if modelo not in MODELOS:
            return await self.error(f"modelo desconocido: {modelo}")
        if datos.get("new_folder") is True:
            carpeta = carpeta_nueva()
        elif isinstance(datos.get("folder"), str) and datos["folder"]:
            carpeta = os.path.abspath(os.path.expanduser(datos["folder"]))
            if not os.path.isdir(carpeta):
                return await self.error(f"la carpeta no existe: {carpeta}", "folder_missing")
        else:
            return await self.error("falta la carpeta: envía folder o new_folder")
        reanudar = datos.get("resume")
        if reanudar is not None and not sesion_existe(reanudar, carpeta):
            return await self.error("la conversación ya no existe", "session_gone")
        VOZ.warmup()
        await self.nueva_conversacion(carpeta, modelo, datos.get("thinking") is True, reanudar)

    async def correr(self):
        await self.hola()
        try:
            async for msg in self.ws:
                if isinstance(msg, bytes):
                    if self.asr_id is not None and not self.ocupada():
                        VOZ.audio(msg)
                    continue
                try:
                    datos = json.loads(msg)
                except json.JSONDecodeError:
                    await self.error("JSON no válido")
                    continue
                try:
                    await self.mensaje(datos)
                except Exception as e:
                    log.error("Fallo al atender %s:\n%s", datos.get("type"), traceback.format_exc())
                    await self.error(f"{type(e).__name__}: {e}")
        finally:
            await self.cerrar_turno()
            await self.cerrar_cliente()


def detalle(herramienta, entrada, carpeta):
    """Lo que la tarjeta de permiso enseña tal cual."""
    if herramienta == "Bash" and isinstance(entrada.get("command"), str):
        return entrada["command"]
    if herramienta in ESCRITURA and isinstance(entrada.get(ESCRITURA[herramienta]), str):
        return os.path.normpath(os.path.join(carpeta, os.path.expanduser(entrada[ESCRITURA[herramienta]])))
    return json.dumps(entrada, ensure_ascii=False, indent=1)


ACTIVA = None  # sesión del único cliente conectado


async def atender(ws):
    global ACTIVA
    if ws.request.path != "/ws":
        await ws.close(1008, "ruta no válida")
        return
    if ACTIVA is not None:
        log.info("Llega otro cliente: se cierra la conexión anterior")
        await ACTIVA.ws.close(CIERRE_OTRO_CLIENTE, "otro cliente")
    sesion = Sesion(ws)
    ACTIVA = sesion
    log.info("Cliente conectado")
    try:
        await sesion.correr()
    except ConnectionClosed:
        pass
    finally:
        if ACTIVA is sesion:
            ACTIVA = None
        log.info("Cliente desconectado")


async def main():
    voz = asyncio.create_task(VOZ.correr())
    async with serve(atender, HOST, PORT, max_size=None, ping_interval=20) as servidor:
        log.info("Escuchando en ws://%s:%d/ws · voz en %s", HOST, PORT, VOZ_URL)
        await servidor.serve_forever()
    voz.cancel()


if __name__ == "__main__":
    asyncio.run(main())
