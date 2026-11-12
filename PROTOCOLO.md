# Charlando: protocolo entre la app y el cerebro

La app (Rust en `app/`, Kotlin en `android/`) y el cerebro (Python en `cerebro/`) se hablan por **un WebSocket**.
Este archivo es el contrato: ninguno de los dos lo cambia sin cambiar el otro.

El cerebro no hace ASR ni TTS: se los pide al servicio de voz (`voz/`). Ese contrato está en [PROTOCOLO-VOZ.md](PROTOCOLO-VOZ.md). Para la app, el cerebro es el único servidor.

## Conexión

- URL: `ws://<host>:<puerto>/ws`. La app la lee de la variable `CHARLANDO_URL` (por defecto `ws://127.0.0.1:8765/ws`); el cerebro escucha donde digan `CHARLANDO_HOST` y `CHARLANDO_PORT` en su `.env`.
- El cerebro atiende **un cliente a la vez**: si llega otro, cierra la conexión anterior.
- Al abrirse la conexión, el cerebro envía `hello` con las carpetas recientes. Lo vuelve a enviar justo después de cada `ready`, con la lista actualizada.
- La app reintenta cada 2 s mientras no hay conexión. El punto de la barra está verde con el socket abierto y rojo si no.

## Mensajes

Hay dos tipos de mensaje:
- **Texto:** JSON con un campo `type`.
- **Binario:** audio PCM con signo de 16 bits, little-endian, mono, sin cabecera.

### De la app al cerebro

| Mensaje | Cuándo | Efecto |
|---|---|---|
| `{"type":"start","folder":"/ruta/absoluta","model":"haiku","thinking":false}` | Al pulsar «Empezar». `model` es `haiku`, `sonnet` u `opus`. `thinking` es el interruptor «Razonar»: `true` deja que Claude decida cuándo razonar (`adaptive`), `false` lo desactiva. Si falta, vale `false` | Borra la conversación anterior y abre una nueva en esa carpeta. Responde `ready`, o el error `folder_missing` si la carpeta no existe |
| `{"type":"start","new_folder":true,"model":"haiku","thinking":false}` | Al elegir «Charla nueva» | Crea una carpeta nueva en `CHARLANDO_CHARLAS` y abre la conversación en ella. Responde `ready` con la ruta creada |
| `{"type":"start","folder":"/ruta","model":"haiku","thinking":false,"resume":"<id>"}` | Al reconectar dentro de una conversación. `folder` y `resume` son los del último `ready` | Reanuda esa conversación: Claude recuerda lo hablado. Responde `ready`, o el error `session_gone` si ya no existe |
| `{"type":"ptt_start"}` | Al pulsar Espacio, solo en estado `idle` | Empieza un turno |
| binario, **16 kHz** | Mientras Espacio sigue pulsado, en trozos de 20–100 ms | Audio del micrófono |
| `{"type":"ptt_end"}` | Al soltar Espacio | Cierra el audio del turno y arranca ASR → Claude → TTS |
| `{"type":"permission_reply","id":"<id>","allow":true}` | Al pulsar «Aceptar» (`true`) o «Rechazar» (`false`) en la tarjeta de permiso | Claude sigue con la herramienta o recibe el rechazo. El turno continúa |

En cada `start`, `folder` o `new_folder` es obligatorio. Si faltan los dos, el cerebro responde un error.

El botón «+» vuelve a la pantalla de inicio en la app. El cerebro no se entera hasta el siguiente `start`.

### Del cerebro a la app

| Mensaje | Significado |
|---|---|
| `{"type":"hello","folders":[{"path":"/ruta","name":"proyecto"}],"charlas":"/ruta/de/charlas"}` | Llega al conectar y tras cada `ready`. Carpetas recientes, la más reciente primero, como mucho 8. `name` es el último tramo de la ruta. Puede ser una lista vacía. `charlas` es la carpeta donde se crean las de «Charla nueva». La app actualiza la lista sin cambiar una carpeta que la persona ya eligió |
| `{"type":"ready","model":"haiku","session":"<id>","folder":"/ruta","name":"proyecto"}` | Conversación lista. `session` identifica la conversación para reanudarla. `folder` es la carpeta en la que trabaja Claude |
| `{"type":"state","state":"thinking"}` | Tras `ptt_end`: transcribe y espera a Claude |
| `{"type":"transcript","text":"..."}` | Lo que entendió el ASR. Va en la burbuja «Tú» |
| `{"type":"state","state":"speaking"}` | Justo antes del primer audio de la respuesta |
| `{"type":"state","state":"thinking"}` (otra vez) | Claude va a usar una herramienta. Lo normal es que antes Claude diga qué va a hacer, con sus palabras. Si no dice nada y pasan `CHARLANDO_ESPERA_AVISO` segundos (3 por defecto), el cerebro envía la frase «Dame un momento.» con su audio y vuelve a enviar `thinking`; como mucho una vez por turno. Después vuelve `speaking` y siguen las frases |
| `{"type":"permission","id":"<id>","tool":"Bash","detail":"git push origin main"}` | Claude necesita permiso para una herramienta. `detail` es lo que la app muestra tal cual: el comando completo, la ruta del archivo o, para otras herramientas, su entrada en JSON. Antes llega la frase «Necesito tu permiso. Míralo en pantalla.» con su audio |
| `{"type":"sentence","text":"..."}` | Una frase de la respuesta. Se envía **justo antes** de su audio. La app la va añadiendo a la burbuja «Claude» |
| binario, **24 kHz** | Audio de la respuesta, en trozos de cualquier tamaño, en orden |
| `{"type":"reply_end"}` | No llega más audio de este turno |
| `{"type":"error","message":"..."}` | Algo falló en el turno. El cerebro vuelve a aceptar `ptt_start` |
| `{"type":"error","message":"...","code":"session_gone"}` | La conversación pedida con `resume` ya no existe. La app vuelve a la pantalla de inicio |
| `{"type":"error","message":"...","code":"folder_missing"}` | La carpeta de `start` no existe o no es una carpeta. La app vuelve a la pantalla de inicio y muestra el mensaje |

Si el ASR no saca texto (silencio), el cerebro envía `transcript` con `""` y después `reply_end`, sin llamar a Claude.

## Permisos

El cerebro decide qué herramientas piden permiso:

| Herramienta | Permiso |
|---|---|
| Leer y buscar en cualquier carpeta (`Read`, `Grep`, `Glob`), la web (`WebSearch`, `WebFetch`) y la lista de tareas (`TodoWrite`) | Libre |
| Crear y editar archivos dentro de la carpeta (`Write`, `Edit`, `NotebookEdit`) | Libre |
| Todo lo demás: comandos (`Bash`), escribir fuera de la carpeta, servidores MCP, subagentes | Tarjeta de permiso |

Las herramientas que necesitan una interfaz que Charlando no tiene (`AskUserQuestion`, `EnterPlanMode`, `ExitPlanMode`) están desactivadas.

Los permisos que el usuario ya tenga en sus ajustes de Claude Code (`permissions.allow` y `permissions.deny`) se aplican antes que esta tabla. Además, Claude Code aprueba por su cuenta los comandos de solo lectura (`ls`, `cat`, `git status`…): esos no muestran tarjeta, igual que leer un archivo.

Reglas de la tarjeta:
- **Solo se responde con el ratón o el dedo.** La voz no acepta ni rechaza nada.
- Hay un permiso pendiente como mucho. El turno no avanza hasta que llega `permission_reply` con su `id`.
- Si pasa 1 minuto sin respuesta, el cerebro repite el aviso una sola vez: envía la frase «Sigo esperando tu permiso. Míralo en pantalla.» con su audio. Para la app es una `sentence` más, aunque esté en el estado `permission`: la muestra y la reproduce sin quitar la tarjeta.
- Si se cae la conexión con un permiso pendiente, el cerebro lo rechaza y termina el turno.
- Un `permission_reply` con un `id` que no está pendiente se ignora.

## Estados en la app

`idle` → (pulsa Espacio) `recording` → (suelta) `thinking` → (`state: speaking`) `speaking` → `idle`.

- `recording` lo decide la app; el cerebro no lo envía.
- La app pasa a `idle` cuando llega `reply_end` **y** ha terminado de reproducir el audio que tenía en cola.
- Un turno puede alternar `speaking` → `thinking` → `speaking` cuando Claude usa herramientas. La app pasa a `thinking` cuando termina de sonar el audio en cola, y todas las frases del turno van a la misma burbuja de Claude.
- **`permission`** es un estado de la app. Un mensaje `permission` solo se acepta en `thinking` o `speaking`; en otro estado se ignora. El estado entra cuando termina de sonar el audio en cola, y sale al pulsar un botón (pasa a `thinking`). En este estado la app muestra la tarjeta e ignora Espacio.
- En `thinking`, `speaking` y `permission`, la app ignora Espacio: en esta versión no se interrumpe a Claude.
- Si se cae la conexión, la app pasa a `disconnected` y conserva la conversación en pantalla. El turno en curso y la tarjeta de permiso se pierden. La tarjeta también se pierde con «+» o con un `error`, que además termina el turno.
- Al reconectar dentro de una conversación, la app envía `start` con `folder` y `resume`, y no deja hablar hasta recibir `ready`. Si aún no se había dicho nada, no hay nada que reanudar: envía un `start` normal con la misma carpeta y el mismo modelo.

## Nivel de audio para la bola

La app lo calcula ella misma (RMS de las muestras): del micrófono en `recording` y del audio que reproduce en `speaking`. El cerebro no envía niveles.
