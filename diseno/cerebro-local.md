# Charlando en tus carpetas: plan

Estado: propuesta, sin implementar. Escrito el 2026-11-12.

Hoy Charlando es una conversación aislada: Claude solo trabaja en una carpeta temporal y no carga tus ajustes. Este plan lo convierte en una sesión de Claude Code por voz, en la carpeta que elijas. Sirve para lo que se piensa mejor hablando: planear, ordenar tareas, anotar ideas. Lo que hay que leer (tablas, código, comandos) se sigue en la terminal, en la misma sesión.

## Cómo se usa

1. En la terminal: `cd ~/mi-proyecto && charlando`. También se puede abrir desde el lanzador y elegir la carpeta.
2. En la pantalla de inicio aparece la carpeta, el modelo y «Razonar». Pulsas «Empezar».
3. Hablas con Espacio, como hoy. Claude lee los archivos del proyecto y sus instrucciones (`CLAUDE.md`), igual que en una sesión normal.
4. Si Claude quiere hacer algo que necesita permiso, la app muestra una tarjeta. Aceptas o rechazas con el ratón o el dedo.
5. Para ver la conversación en texto, en la terminal de esa carpeta: `claude --resume <id>`.

## Arquitectura

El servidor actual se parte en dos procesos:

```
App ──► Cerebro (Agent SDK, en la máquina de tus carpetas)
             │
             └──► Voz (equipo con GPU): voz a texto y texto a voz
```

| Proceso | Dónde corre | Qué hace |
|---|---|---|
| Voz | El equipo con GPU | Solo ASR (voz a texto) y TTS (texto a voz) con LFM3-Audio. No sabe nada de Claude |
| Cerebro | La máquina donde están tus carpetas | Abre la sesión del Agent SDK en la carpeta elegida y guarda las sesiones ahí |

Las dos piezas pueden ir en la misma máquina. Las sesiones se guardan donde corre el cerebro, que es donde `claude --resume` las busca.

La app sigue hablando con un solo servidor: el cerebro. El protocolo entre el cerebro y la voz va en un archivo aparte, como `PROTOCOLO.md`.

## La sesión de Claude

| Opción del Agent SDK | Hoy | Con este plan |
|---|---|---|
| `cwd` | `CHARLANDO_CARPETA` | La carpeta elegida |
| `setting_sources` | `[]` | `["user", "project"]`: ajustes, `CLAUDE.md` y subagentes |
| `system_prompt` | Uno propio que reemplaza al de Claude Code | El de Claude Code (`preset`) más las reglas de voz, añadidas al final |
| Herramientas | Lectura, escritura y web | Todas, incluidos los servidores MCP del usuario |
| Permisos | `dontAsk`: rechaza en silencio | `can_use_tool`: pregunta con una tarjeta (ver abajo) |

Las reglas de voz piden respuestas cortas y habladas: sin tablas, rutas largas ni bloques de código. Si algo necesita verse, Claude lo dice («te lo dejo en el archivo») en vez de leerlo.

## Permisos

| Acción | Qué pasa |
|---|---|
| Leer y buscar | Libre |
| Crear y editar archivos dentro de la carpeta | Libre |
| Comandos de terminal, archivos fuera de la carpeta, borrar, subir cambios | Tarjeta de permiso |

La tarjeta de permiso:

- Muestra la herramienta y lo que va a hacer: el comando completo o la ruta del archivo.
- Tiene dos botones: «Aceptar» y «Rechazar».
- **No se responde con la voz.** El ASR puede entender mal un «sí» o un «no», y aquí un error tiene consecuencias.
- Mientras está abierta, Espacio no hace nada, para no aceptar ni grabar por accidente.
- Claude avisa de voz: «Necesito tu permiso. Míralo en pantalla».
- Si la conexión se cae con una tarjeta abierta, la acción se rechaza.

## Elegir la carpeta

- **Desde la terminal:** la carpeta actual.
- **Desde el lanzador:** la lista de carpetas recientes.
- **«Charla nueva»:** crea una carpeta con la fecha dentro de `CHARLANDO_CHARLAS` (por defecto `~/charlando`). Sirve para ideas sueltas que no son de ningún proyecto.

Las carpetas recientes se guardan en `~/.config/charlando/`, nunca en el repo.

## Cambios en la app

El estilo no cambia. Hay que actualizar `maqueta.html` y `DISEÑO.md` antes de tocar el código.

1. **Inicio:** una sección «Carpeta» encima de «Modelo», con la carpeta actual, las recientes y «Charla nueva».
2. **Barra superior:** el nombre de la carpeta junto a «Charlando».
3. **Tarjeta de permiso:** en Chat, dentro de la conversación; en Voz, encima de la bola. Estado nuevo en la bola: «Esperando tu permiso».
4. **Opcional:** botón «Copiar comando para la terminal», que copia `claude --resume <id>`.

El cliente de Android se adapta cuando el de escritorio funcione.

## Cambios en el protocolo app–cerebro

- `start` lleva la carpeta: `{"type":"start","folder":"/ruta","model":"sonnet",...}`. Si falta, se usa la carpeta desde la que se lanzó `charlando`.
- Mensaje nuevo del cerebro: `{"type":"permission","id":"...","tool":"Bash","detail":"git push"}`.
- Respuesta nueva de la app: `{"type":"permission_reply","id":"...","allow":true}`.
- `state` admite un valor nuevo: `waiting_permission`.

## Registros

| Qué | Hoy | Con este plan |
|---|---|---|
| Transcripciones y frases de Claude en el registro del servicio | Siempre | Solo con `CHARLANDO_DEPURAR=1` |
| Tiempos y errores | Siempre | Siempre |
| Sesiones del Agent SDK | En la máquina del servidor | En la máquina del cerebro, como cualquier sesión de Claude Code |
| Audio | No se guarda | No se guarda |

## Seguridad

- Quien pueda conectarse al cerebro maneja un Claude Code con acceso a tus carpetas. Por eso el cerebro escucha solo en `127.0.0.1` por defecto.
- Si lo abres a la red (por ejemplo, para el móvil), hazlo solo en una red privada como Tailscale. Una contraseña para el WebSocket queda como trabajo futuro.
- Lo que dices, ya en texto, va a Claude (Anthropic), como en cualquier sesión de Claude Code. La voz no sale del equipo con GPU.
- El repo no lleva rutas, direcciones ni nombres de máquinas de nadie: todo eso va en `.env` y en `~/.config/charlando/`.

## Orden de trabajo

1. Separar el servicio de voz del cerebro, sin cambiar lo que hace hoy.
2. Abrir la sesión: carpeta, ajustes, herramientas y reglas de voz.
3. Maquetas: carpeta en inicio, barra superior y tarjeta de permiso.
4. Permisos con tarjeta en la app y en el cerebro.
5. Carpetas recientes y «Charla nueva».
6. Registros sin contenido por defecto.
7. Actualizar `README.md` y `PROTOCOLO.md`.
