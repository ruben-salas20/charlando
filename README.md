<p align="center"><img src="diseno/logo/charlando-1024.png" width="128" alt="Logo de Charlando: una bola de metal líquido"></p>

# Charlando

Habla con Claude Code por voz, en la carpeta que elijas. Mantienes Espacio, hablas, sueltas, y Claude te responde en voz alta.

Sirve para lo que se piensa mejor hablando: planear, ordenar tareas, anotar ideas. Lo que hay que leer (tablas, código, comandos) se sigue en la terminal, en la misma conversación.

Es una cascada de voz: **ASR → LLM → TTS**. Un modelo local, [LFM3-Audio-2B](https://huggingface.co/LiquidAI/LFM3-Audio-2B), pasa tu voz a texto y lee las respuestas. Claude responde a través del [Agent SDK](https://code.claude.com/docs/en/agent-sdk/overview), con tu sesión de Claude Code.

<p align="center">
  <img src="app/capturas/voz-pensando.png" width="45%" alt="Vista Voz: una bola de metal líquido mientras Claude piensa">
  <img src="app/capturas/chat-hablando.png" width="45%" alt="Vista Chat: la respuesta de Claude se escribe mientras suena">
</p>

## Cómo funciona

| Pieza | Qué hace | Tecnología |
|---|---|---|
| `app/` | Graba el micrófono, reproduce la voz y muestra la conversación | Rust, [Rune](https://github.com/anomalyco/rune), cpal |
| `android/` | Lo mismo en el teléfono | Kotlin, Jetpack Compose |
| `cerebro/` | Abre una sesión de Claude Code en la carpeta elegida, corta la respuesta en frases y pide permisos | Python, Agent SDK |
| `voz/` | Pasa la voz a texto y las frases a voz, en la GPU | Python, Transformers |
| `despliegue/` | El comando `charlando` y los servicios de systemd | Bash, systemd |

```
App ──► Cerebro (en la máquina de tus carpetas)
             │
             └──► Voz (en el equipo con GPU)
```

El cerebro y la voz pueden ir en la misma máquina o en dos, unidas por una red privada como Tailscale. El cerebro debe correr donde están tus carpetas: ahí trabaja Claude y ahí se guardan las conversaciones.

Los contratos entre las piezas están en [PROTOCOLO.md](PROTOCOLO.md) (app ↔ cerebro) y [PROTOCOLO-VOZ.md](PROTOCOLO-VOZ.md) (cerebro ↔ voz). El diseño de la interfaz está en [diseno/](diseno/).

### Claude en tu carpeta

Claude trabaja como en una sesión normal de Claude Code: carga tus ajustes, el `CLAUDE.md` de la carpeta, tus memorias y tus servidores MCP. Antes de usar una herramienta, Claude dice con sus palabras qué va a hacer. Si calla y la herramienta tarda más de 3 segundos, el cerebro dice «Dame un momento», como mucho una vez por respuesta.

| Acción | Permiso |
|---|---|
| Leer y buscar archivos en cualquier carpeta, buscar en la web | Libre |
| Crear y editar archivos dentro de la carpeta | Libre |
| Comandos de terminal, escribir fuera de la carpeta, conectores, subagentes | Tarjeta de permiso |

La tarjeta muestra el comando o la ruta completa, con dos botones: «Aceptar» y «Rechazar». **Solo se responde con el ratón o el dedo, nunca con la voz**: el ASR puede entender mal y aquí un error tiene consecuencias. Antes de mostrarla, Claude dice «Necesito tu permiso. Míralo en pantalla.», y lo repite una vez si pasa un minuto.

Los comandos de solo lectura (`ls`, `git status`…) y los permisos que ya tengas en tus ajustes de Claude Code no muestran tarjeta.

Medido con una RTX 3050 de 6 GB y Claude Haiku: la voz de Claude empieza a sonar entre 2,5 y 5 s después de que sueltas Espacio.

## Requisitos

**Voz** (el equipo con GPU):
- Linux con una GPU NVIDIA y unos 4 GB de VRAM libres.
- [uv](https://docs.astral.sh/uv/).

**Cerebro** (donde están tus carpetas):
- [uv](https://docs.astral.sh/uv/).
- [Claude Code](https://code.claude.com) instalado y con sesión iniciada (`claude`).

**App:**
- Linux con Wayland o X11. Probada en Arch con Hyprland.
- Rust y las cabeceras de ALSA (`libasound2-dev` en Debian y Ubuntu, `alsa-lib` en Arch). En Arch con PipeWire también hace falta `pipewire-alsa`.

> Usar tu suscripción de Claude con el Agent SDK está pensado para uso personal. Si construyes un producto para otros, usa una clave de API.

## Instalación

Clona el repo en cada máquina que vaya a correr una pieza:

```sh
git clone https://github.com/ruben-salas20/charlando
cd charlando
mkdir -p ~/.config/systemd/user
```

### Voz

```sh
cd voz
cp .env.example .env        # ajusta CHARLANDO_VOZ_HOST si el cerebro está en otra máquina
uv sync
sed "s|RUTA_DEL_REPO|$(dirname "$PWD")|" ../despliegue/charlando-voz.service > ~/.config/systemd/user/charlando-voz.service
systemctl --user daemon-reload
```

La primera vez que hables, la voz descarga el modelo de Hugging Face (unos 4 GB).

### Cerebro

```sh
cd cerebro
cp .env.example .env        # ajusta CHARLANDO_VOZ_URL si la voz está en otra máquina
uv sync
sed "s|RUTA_DEL_REPO|$(dirname "$PWD")|" ../despliegue/charlando-cerebro.service > ~/.config/systemd/user/charlando-cerebro.service
systemctl --user daemon-reload
```

### App

```sh
cd app
cargo build --release
install -Dm755 target/release/charlando ~/.local/lib/charlando/charlando-app
install -Dm755 ../despliegue/charlando ~/.local/bin/charlando
mkdir -p ~/.config/charlando
cp ../despliegue/charlando.env.example ~/.config/charlando/charlando.env
```

La app y el cerebro van en la misma máquina. Si la voz está en otra, edita `~/.config/charlando/charlando.env` y pon en `CHARLANDO_SSH` su host de ssh: el comando `charlando` arranca la voz por ssh.

Para tener la app en el lanzador de apps:

```sh
for s in 48 64 128 256 512; do
  mkdir -p ~/.local/share/icons/hicolor/${s}x${s}/apps
  magick ../diseno/logo/charlando-1024.png -resize ${s}x${s} ~/.local/share/icons/hicolor/${s}x${s}/apps/charlando.png
done
sed "s|RUTA_DEL_COMANDO|$HOME/.local/bin/charlando|" ../despliegue/charlando.desktop > ~/.local/share/applications/charlando.desktop
```

### Android

Cliente para el teléfono en `android/`, con Kotlin y Jetpack Compose. Tocas para hablar y vuelves a tocar para enviar.

Requisitos:
- Android 13 o superior.
- Para compilar: JDK 17 o superior (con `javac`) y el SDK de Android con la plataforma 37. Gradle se descarga solo.

```sh
cd charlando/android
echo "sdk.dir=/ruta/al/android-sdk" > local.properties
./gradlew assembleDebug
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

La app no trae ninguna dirección. La primera vez abre Ajustes: escribe ahí la dirección del cerebro (por ejemplo, `ws://<IP de Tailscale>:8765/ws`). Para eso el cerebro tiene que escuchar en esa IP (`CHARLANDO_HOST` en `cerebro/.env`). Hazlo solo en una red privada: quien se conecte al cerebro maneja un Claude Code con acceso a tus carpetas.

## Uso

| Comando | Qué hace |
|---|---|
| `charlando` | Arranca la voz y el cerebro, abre la app y, al cerrarla, los apaga. Desde una terminal, propone la carpeta actual |
| `charlando <carpeta>` | Lo mismo, proponiendo esa carpeta |
| `charlando servidor iniciar` | Arranca la voz y el cerebro |
| `charlando servidor parar` | Apaga los dos y libera la GPU |
| `charlando servidor reiniciar` | Reinicia los dos |
| `charlando servidor estado` | Dice si están en marcha y cuánta GPU se usa |
| `charlando servidor registros` | Muestra los registros de los dos en vivo |

En la app:
1. Elige la carpeta: la actual, una reciente o «Charla nueva», que crea una carpeta con la fecha para una idea suelta.
2. Elige el modelo (Haiku, Sonnet u Opus) y si Claude puede razonar antes de responder.
3. Pulsa «Empezar».
4. Mantén **Espacio** mientras hablas y suéltalo al terminar.

Para ver la conversación en texto, pulsa el icono de copiar de la barra y pega el comando en una terminal: `cd '<carpeta>' && claude --resume <id>`.

La voz carga el modelo al empezar y lo descarga tras 3 minutos sin uso (`CHARLANDO_INACTIVIDAD`).

## Configuración

Cada pieza lee su `.env`, con todas las variables explicadas en su `.env.example`: [voz/.env.example](voz/.env.example), [cerebro/.env.example](cerebro/.env.example) y [despliegue/charlando.env.example](despliegue/charlando.env.example).

| Variable | Pieza | Por defecto | Para qué |
|---|---|---|---|
| `CHARLANDO_HOST`, `CHARLANDO_PORT` | Cerebro | `127.0.0.1`, `8765` | Dónde escucha a la app |
| `CHARLANDO_VOZ_URL` | Cerebro | `ws://127.0.0.1:8766/voz` | Dónde está la voz |
| `CHARLANDO_CHARLAS` | Cerebro | `~/charlando` | Dónde crea «Charla nueva» sus carpetas |
| `CHARLANDO_ESPERA_PERMISO` | Cerebro | `60` | Segundos antes de repetir el aviso de permiso |
| `CHARLANDO_ESPERA_AVISO` | Cerebro | `3` | Segundos de silencio con herramientas antes de decir «Dame un momento» |
| `CHARLANDO_VOZ_HOST`, `CHARLANDO_VOZ_PORT` | Voz | `127.0.0.1`, `8766` | Dónde escucha al cerebro |
| `CHARLANDO_MODELO` | Voz | `LiquidAI/LFM3-Audio-2B` | Id de Hugging Face o carpeta local |
| `CHARLANDO_INACTIVIDAD` | Voz | `180` | Segundos sin uso antes de descargar el modelo |
| `CHARLANDO_LIBERAR_GPU_URL` | Voz | vacía | URL que libera la GPU de otro servidor de modelos, como llama-swap |
| `CHARLANDO_DEPURAR` | Voz y cerebro | apagada | Con `1`, el registro guarda lo que dices y lo que responde Claude |

### Qué se guarda

- **Las conversaciones**, en la máquina del cerebro, como cualquier sesión de Claude Code (`~/.claude/projects/`). Hacen falta para retomarlas.
- **Las carpetas recientes**, en `~/.config/charlando/recientes.json`.
- **Los registros** de los servicios (`journalctl`), solo con tiempos y errores, salvo con `CHARLANDO_DEPURAR=1`.
- **El audio no se guarda.** Lo que dices, ya en texto, va a Claude, como en cualquier sesión de Claude Code.

## Licencia

MIT. Las fuentes Inter e IBM Plex Mono de `app/assets/` tienen su propia licencia (OFL), junto a cada fuente. El modelo LFM3-Audio-2B se distribuye con la [LFM Open License v1.0](https://huggingface.co/LiquidAI/LICENSE) y no se incluye en este repo.
