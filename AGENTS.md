# Charlando

Cascada de voz: `app/` (Rust + Rune) graba y reproduce; `servidor/` (Python) hace ASR con LFM3, llama a Claude por el Agent SDK y sintetiza con LFM3. Contexto general en `README.md`.

## Reglas

- **`PROTOCOLO.md` es el contrato.** Un cambio en los mensajes se hace en `PROTOCOLO.md`, `app/src/net.rs` y `servidor/servidor.py` en el mismo commit.
- **Repo público: nada de la máquina en el código.** IPs, rutas y hosts van a `servidor/.env` (documentado en `.env.example`) o a `~/.config/charlando/charlando.env` (`despliegue/charlando.env.example`).
- **Rune es prerelease y no lo conoces.** Antes de usar su API, búscala en el commit fijado en `app/Cargo.toml` (`~/.cargo/git/checkouts/rune-*/`); empieza por `crates/rune/README.md`.
- **La GPU se usa desde un solo hilo** (`GPU` en `servidor.py`): todo `generate` y `codec.decode` pasa por ese executor.
- **Velocidad frente a calidad lo decide la persona** (razonamiento, longitud de respuesta, tamaño de los trozos de audio): propón con datos medidos y espera respuesta.
- Textos de la interfaz en español, según `diseno/DISEÑO.md` y `diseno/maqueta.html`.

## Comprobar

- **App:** `cargo build --release && cargo test --release` en `app/`. La interfaz se revisa sin ventana: `target/release/charlando --snap salida.png --scene <escena> --scale 2`, con las escenas de `SCENES` en `app/src/main.rs`. Un estado nuevo lleva su escena. Hecho cuando la captura se ha mirado, no solo generado.
- **Servidor:** `systemctl --user restart charlando`, luego `uv run python cliente_prueba.py <wav mono 16 kHz>…` desde `servidor/` con `CHARLANDO_URL` apuntando al servidor. Hecho cuando el turno termina con `cortes 0`. Registros: `journalctl --user -u charlando`.
- El servidor admite un cliente: si la app está abierta, la prueba la desconecta.
