//! Preferencias que se recuerdan, en `~/.config/charlando/`: la última vista (`vista`) y el
//! interruptor de razonamiento (`razonar`).

use std::path::PathBuf;

use crate::state::View;

fn path(name: &str) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("charlando").join(name))
}

pub fn load_view() -> View {
    match path("vista").and_then(|p| std::fs::read_to_string(p).ok()).as_deref().map(str::trim) {
        Some("voz") => View::Voice,
        _ => View::Chat,
    }
}

pub fn save_view(view: View) {
    save("vista", if view == View::Voice { "voz\n" } else { "chat\n" });
}

pub fn load_thinking() -> bool {
    path("razonar").and_then(|p| std::fs::read_to_string(p).ok()).as_deref().map(str::trim) == Some("si")
}

pub fn save_thinking(on: bool) {
    save("razonar", if on { "si\n" } else { "no\n" });
}

fn save(name: &str, contents: &str) {
    if rune::tooling::headless_run().is_some() {
        return;
    }
    let Some(p) = path(name) else { return };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(p, contents);
}
