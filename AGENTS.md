# Charlando

Cascada de voz: `app/` (Rust + Rune) graba y reproduce, y `android/` (Kotlin + Compose) es su equivalente para el teléfono; `cerebro/` (Python) abre una sesión de Claude Code con el Agent SDK en la carpeta que elige la persona, y `voz/` (Python) hace el ASR y el TTS con LFM3 en la GPU. Contexto general en `README.md`; el plan de esta arquitectura, en `diseno/cerebro-local.md`.

## Reglas

- **Hay dos contratos.** `PROTOCOLO.md` (app ↔ cerebro): un cambio va en `PROTOCOLO.md`, `app/src/net.rs`, el cliente de `android/` y `cerebro/` en el mismo commit. `PROTOCOLO-VOZ.md` (cerebro ↔ voz): un cambio va en `PROTOCOLO-VOZ.md`, `cerebro/` y `voz/` en el mismo commit.
- **Repo público: nada de la máquina en el código.** IPs, rutas y hosts van al `.env` de cada servicio (documentado en su `.env.example`) o a `~/.config/charlando/charlando.env` (`despliegue/charlando.env.example`).
- **Rune es prerelease y no lo conoces.** Antes de usar su API, búscala en el commit fijado en `app/Cargo.toml` (`~/.cargo/git/checkouts/rune-*/`); empieza por `crates/rune/README.md`.
- **La GPU se usa desde un solo hilo** (`GPU` en `voz/`): todo `generate` y `codec.decode` pasa por ese executor.
- **Velocidad frente a calidad lo decide la persona** (razonamiento, longitud de respuesta, tamaño de los trozos de audio): propón con datos medidos y espera respuesta.
- Textos de la interfaz en español, según `diseno/DISEÑO.md` y `diseno/maqueta.html` (`diseno/maqueta-android.html` en Android).
- **`android/` se comporta igual que `app/`.** `Estado.kt` traduce `app/src/state.rs` y `EstadoTest.kt` sus tests: un cambio en uno va al otro.

## Comprobar

- **App:** `cargo build --release && cargo test --release` en `app/`. La interfaz se revisa sin ventana: `target/release/charlando --snap salida.png --scene <escena> --scale 2`, con las escenas de `SCENES` en `app/src/main.rs`. Un estado nuevo lleva su escena. Hecho cuando la captura se ha mirado, no solo generado.
- **Voz:** `uv run python prueba_voz.py` desde `voz/`, con la voz en marcha. Hecho cuando termina con «Todo OK». Necesita la GPU libre: si `nvidia-smi` muestra otro modelo cargado, para antes el servicio que lo tenga.
- **Cerebro:** sin GPU, con `uv run python voz_falsa.py` en lugar de la voz. Luego `uv run python cliente_prueba.py --carpeta <carpeta de prueba> <wav mono 16 kHz o «2s»>…` desde `cerebro/`, con `CHARLANDO_URL` apuntando al cerebro (`--permisos aceptar|rechazar|colgar` para la tarjeta). Hecho cuando el turno termina sin errores. Usa una carpeta temporal y comandos inofensivos: Claude ejecuta de verdad lo que aceptes.
- **Conversación completa:** la voz real, el cerebro y `cliente_prueba.py`. La voz admite un solo cliente: si otro programa se conecta, echa al cerebro. Registros: `journalctl --user -u charlando-voz` y `-u charlando-cerebro`.
- **Android:** `./gradlew assembleDebug test` en `android/`, con `JAVA_HOME` en un JDK 17 o superior que traiga `javac` y el SDK indicado en `local.properties`. El teléfono va por USB al portátil: copia el APK y `adb install -r`. Para revisar pantallas sin voz: `adb shell am start -n io.github.rubensalas.charlando/.MainActivity --es escena voz-hablando` (escenas en `Escenas.kt`).
- El cerebro admite un cliente: si la app está abierta, la prueba la desconecta. Para no molestar a una app en uso, prueba con otros puertos (`CHARLANDO_PORT`, `CHARLANDO_VOZ_PORT`).
