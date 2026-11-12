# Charlando: protocolo entre el cerebro y la voz

El cerebro (`cerebro/`) y el servicio de voz (`voz/`) se hablan por **un WebSocket**.
Este archivo es el contrato: ninguno de los dos lo cambia sin cambiar el otro.

La voz solo pasa voz a texto (ASR) y texto a voz (TTS) con LFM3-Audio en la GPU. No sabe nada de Claude, de carpetas ni de la app.

## Conexión

- URL: `ws://<host>:<puerto>/voz`. El cerebro la lee de `CHARLANDO_VOZ_URL` (por defecto `ws://127.0.0.1:8766/voz`); la voz escucha donde digan `CHARLANDO_VOZ_HOST` y `CHARLANDO_VOZ_PORT` en su `.env`.
- La voz atiende **un cliente a la vez**: si llega otro, cierra la conexión anterior y cancela lo que tuviera en curso.
- El cerebro abre la conexión al arrancar y la reabre cada 2 s si se cae. Si un turno necesita la voz y no hay conexión, el cerebro envía un `error` a la app.

## Mensajes

- **Texto:** JSON con un campo `type`.
- **Binario:** audio PCM con signo de 16 bits, little-endian, mono, sin cabecera.

Cada petición lleva un `id` que pone el cerebro. La respuesta repite ese `id`.

### Del cerebro a la voz

| Mensaje | Cuándo | Efecto |
|---|---|---|
| `{"type":"warmup"}` | Al recibir `start` o `ptt_start` de la app | Carga el modelo en segundo plano si no está cargado. No responde |
| `{"type":"asr_start","id":"<id>"}` | Al recibir `ptt_start` | Empieza a acumular audio para una transcripción |
| binario, **16 kHz** | Según llega de la app | Audio para la transcripción abierta. Sin transcripción abierta, se descarta |
| `{"type":"asr_end","id":"<id>"}` | Al recibir `ptt_end` | Cierra el audio y transcribe. Responde `asr_result` |
| `{"type":"tts","id":"<id>","text":"..."}` | Por cada frase lista para decir | Pone la frase en cola. Responde con su audio y `tts_end` |
| `{"type":"cancel"}` | Cuando el turno termina antes de tiempo (la app se desconecta o hay un error) | Para la síntesis en curso, vacía la cola de frases y responde `cancelled` |

### De la voz al cerebro

| Mensaje | Significado |
|---|---|
| `{"type":"asr_result","id":"<id>","text":"..."}` | Transcripción. `""` si el audio dura menos de 0,33 s o no hay voz |
| binario, **24 kHz** | Audio de la frase que se está sintetizando, en trozos de cualquier tamaño, en orden |
| `{"type":"tts_end","id":"<id>"}` | No llega más audio de esa frase |
| `{"type":"cancelled"}` | Tras `cancel`: ya no llega más audio de nada pedido antes |
| `{"type":"error","id":"<id>","message":"..."}` | Falló esa petición. `id` falta si el error no es de una petición concreta |

## Orden

- La GPU trabaja en serie. Las frases se sintetizan en el orden en que llegaron sus `tts`.
- El audio binario siempre pertenece a la frase más antigua que aún no ha recibido su `tts_end`. Por eso el binario no lleva `id`.
- El cerebro puede enviar varios `tts` seguidos sin esperar: así la frase siguiente se sintetiza mientras suena la anterior.
- Un `asr_end` se atiende después de las frases que ya estaban en cola. En la práctica no hay cola en ese momento: la app solo deja hablar en `idle`.

## Modelo y memoria

- La voz carga el modelo con el primer `warmup`, `asr_start` o `tts`, y lo descarga tras `CHARLANDO_INACTIVIDAD` segundos sin peticiones. Una transcripción abierta o una frase en cola cuentan como uso.
- `CHARLANDO_LIBERAR_GPU_URL` se llama antes de cargar el modelo, como hasta ahora.

## Registros

La voz no escribe en el registro el texto transcrito ni el de las frases, salvo con `CHARLANDO_DEPURAR=1`. Siempre registra tiempos y errores.
