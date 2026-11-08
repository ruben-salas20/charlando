# Charlando: protocolo entre la app y el servidor

La app (Rust) y el servidor (Python) se hablan por **un WebSocket**.
Este archivo es el contrato: ninguno de los dos lo cambia sin cambiar el otro.

## Conexión

- URL: `ws://<host>:<puerto>/ws`. La app la lee de la variable `CHARLANDO_URL` (por defecto `ws://127.0.0.1:8765/ws`); el servidor escucha donde digan `CHARLANDO_HOST` y `CHARLANDO_PORT` en su `.env`.
- El servidor atiende **un cliente a la vez**: si llega otro, cierra la conexión anterior.
- La app reintenta cada 2 s mientras no hay conexión. El punto de la barra está verde con el socket abierto y rojo si no.

## Mensajes

Hay dos tipos de mensaje:
- **Texto:** JSON con un campo `type`.
- **Binario:** audio PCM con signo de 16 bits, little-endian, mono, sin cabecera.

### De la app al servidor

| Mensaje | Cuándo | Efecto |
|---|---|---|
| `{"type":"start","model":"haiku","thinking":false}` | Al pulsar «Empezar». `model` es `haiku`, `sonnet` u `opus`. `thinking` es el interruptor «Razonar»: `true` deja que Claude decida cuándo razonar (`adaptive`), `false` lo desactiva. Si falta, vale `false` | Borra la conversación anterior y abre una nueva con ese modelo. Responde `ready` |
| `{"type":"ptt_start"}` | Al pulsar Espacio, solo en estado `idle` | Empieza un turno |
| binario, **16 kHz** | Mientras Espacio sigue pulsado, en trozos de 20–100 ms | Audio del micrófono |
| `{"type":"ptt_end"}` | Al soltar Espacio | Cierra el audio del turno y arranca ASR → Claude → TTS |

El botón «+» vuelve a la pantalla de inicio en la app. El servidor no se entera hasta el siguiente `start`.

### Del servidor a la app

| Mensaje | Significado |
|---|---|
| `{"type":"ready","model":"haiku"}` | Conversación lista |
| `{"type":"state","state":"thinking"}` | Tras `ptt_end`: transcribe y espera a Claude |
| `{"type":"transcript","text":"..."}` | Lo que entendió el ASR. Va en la burbuja «Tú» |
| `{"type":"state","state":"speaking"}` | Justo antes del primer audio de la respuesta |
| `{"type":"state","state":"thinking"}` (otra vez) | Claude va a usar una herramienta. Antes llega la frase «Déjame mirarlo.» con su audio. Después vuelve `speaking` y siguen las frases |
| `{"type":"sentence","text":"..."}` | Una frase de la respuesta. Se envía **justo antes** de su audio. La app la va añadiendo a la burbuja «Claude» |
| binario, **24 kHz** | Audio de la respuesta, en trozos de cualquier tamaño, en orden |
| `{"type":"reply_end"}` | No llega más audio de este turno |
| `{"type":"error","message":"..."}` | Algo falló en el turno. El servidor vuelve a aceptar `ptt_start` |

Si el ASR no saca texto (silencio), el servidor envía `transcript` con `""` y después `reply_end`, sin llamar a Claude.

## Estados en la app

`idle` → (pulsa Espacio) `recording` → (suelta) `thinking` → (`state: speaking`) `speaking` → `idle`.

- `recording` lo decide la app; el servidor no lo envía.
- La app pasa a `idle` cuando llega `reply_end` **y** ha terminado de reproducir el audio que tenía en cola.
- Un turno puede alternar `speaking` → `thinking` → `speaking` cuando Claude usa herramientas. La app pasa a `thinking` cuando termina de sonar el audio en cola, y todas las frases del turno van a la misma burbuja de Claude.
- En `thinking` y `speaking`, la app ignora Espacio: en esta versión no se interrumpe a Claude.
- Si se cae la conexión, la app pasa a `disconnected`. Al reconectar, vuelve a la pantalla de inicio.

## Nivel de audio para la bola

La app lo calcula ella misma (RMS de las muestras): del micrófono en `recording` y del audio que reproduce en `speaking`. El servidor no envía niveles.
