//! Charlando: cliente de escritorio de una cascada de voz (ASR → Claude → TTS).
//!
//! ```sh
//! charlando                                  # la ventana; servidor en CHARLANDO_URL
//! charlando --snap out.png --scene voz-hablando --at 1.0   # sin ventana (ver --list-scenes)
//! charlando --probar                         # prueba el protocolo contra el servidor y sale
//! ```

mod audio;
mod config;
mod net;
mod orb;
mod probe;
mod state;
mod ui;

use std::time::Duration;

use rune::{App, AppSpec, Key, KeyEvent, Tx};

use crate::audio::Audio;
use crate::net::{Folder, Where};
use crate::state::{Message, Permission, Phase, Screen, State, View, Who};

const FONTS: &[&[u8]] = &[
    include_bytes!("../assets/Inter-Regular.ttf"),
    include_bytes!("../assets/Inter-Medium.ttf"),
    include_bytes!("../assets/Inter-SemiBold.ttf"),
    include_bytes!("../assets/IBMPlexMono-Regular.ttf"),
];

fn is_space(ev: &KeyEvent) -> bool {
    matches!(&ev.key, Key::Text(t) if t == " ") && !ev.mods.cmd && !ev.mods.ctrl && !ev.mods.alt && !ev.mods.logo
}

pub fn on_key(s: &mut State, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if !is_space(ev) || s.screen != Screen::Conversation {
        return false;
    }
    if !tx.key_repeat() {
        s.ptt_down();
    }
    true
}

pub fn on_key_up(s: &mut State, ev: &KeyEvent) -> bool {
    if !is_space(ev) || s.screen != Screen::Conversation {
        return false;
    }
    s.ptt_up();
    true
}

fn poll(s: &mut State, t: f64, tx: &mut Tx) -> bool {
    let changed = s.tick(t);
    let card_waiting = s.permission.is_some() && s.phase != Phase::Permission;
    if matches!(s.phase, Phase::Recording | Phase::Speaking) || s.reply_ended || card_waiting {
        tx.wake_after(Duration::from_millis(33));
    }
    changed
}

fn init() -> State {
    State::new(None, Audio::null())
}

// ---- escenas para las capturas sin ventana (los textos de ejemplo de la maqueta)

const REPLY: &str = "Mira el span de cada etapa: el más largo es tu cuello de botella. Aquí suele ser el ASR.";

/// Las carpetas de la maqueta.
fn folders(s: &mut State) {
    let f = |rel: &str, name: &str| Folder { path: format!("/home/persona/{rel}"), name: name.to_owned() };
    s.connected = true;
    s.charlas = "/home/persona/charlando".to_owned();
    s.folders = vec![f("proyectos/mi-proyecto", "mi-proyecto"), f("notas", "notas"), f("trabajo/cliente", "trabajo")];
    s.place = Some(Where::Folder(s.folders[0].path.clone()));
}

fn conversation(s: &mut State, view: View, phase: Phase) {
    let m = |who, text: &str| Message { who, text: text.to_owned() };
    folders(s);
    s.folder = Some(s.folders[0].clone());
    s.session = Some("3f2a9c1e".to_owned());
    s.screen = Screen::Conversation;
    s.view = view;
    s.phase = phase;
    s.demo_level = true;
    s.messages = vec![
        m(Who::You, "¿Qué es un span?"),
        m(Who::Claude, "Es un tramo medido de una traza: tiene inicio, fin y atributos. Cada etapa de la cascada sería un span."),
        m(Who::You, "¿Y cómo veo cuál etapa es la más lenta?"),
    ];
    match phase {
        Phase::Idle | Phase::Permission => s.messages.push(m(Who::Claude, REPLY)),
        Phase::Speaking => s.messages.push(m(Who::Claude, &REPLY.chars().take(70).collect::<String>())),
        Phase::Recording | Phase::Thinking => {}
    }
}

const LONG_QUESTION: &str = "Hola, quiero que me ayudes a pensar en cómo escribir un post para X, donde la idea es comentar un poco que estoy aprendiendo sobre un agente de voz desarrollado en tipo cascada, o sea que es un modelo ASR que pasa a un LLM, que es el que piensa y luego pasa a un text-to-speech, que es el que genera la voz.";
const LONG_REPLY: &str = "Sí, se entiende bien, y la idea tiene mucho sentido para un post. Lo que describes es un pipeline en cascada: el audio entra por un ASR, el texto va a un LLM que razona y responde, y esa respuesta pasa a un TTS que la convierte en voz. Construirlo tú mismo en Rust, con un LLM intercambiable, es un buen ángulo porque muestra que no hace falta magia: son tres piezas conectadas y el reto está en la latencia y en cómo se pasan el turno.";

fn permission(s: &mut State, view: View) {
    conversation(s, view, Phase::Permission);
    s.permission = Some(Permission { id: "p1".into(), tool: "Bash".into(), detail: "git push origin main".into() });
}

const LONG_DETAIL: &str = "for f in $(git ls-files '*.rs'); do\n  rustfmt --edition 2024 \"$f\"\ndone\ncargo build --release\ncargo test --release\ngit add -A\ngit commit -m 'Formatea todo el código con rustfmt antes de publicar'\ngit push origin main\ngh pr create --fill\necho hecho";

fn taken(s: &mut State) {
    s.connected = false;
    s.taken = true;
}

const SCENES: &[(&str, fn(&mut State))] = &[
    ("inicio", folders),
    ("inicio-carpetas", folders),
    ("inicio-selector", |s| {
        folders(s);
        s.folder_menu = true;
    }),
    ("inicio-carpeta-perdida", |s| {
        folders(s);
        s.error = Some("La carpeta ya no existe.".to_owned());
    }),
    ("inicio-desconectado", |s| s.connected = false),
    ("inicio-razonar", |s| {
        folders(s);
        s.thinking = true;
    }),
    ("chat-espera", |s| conversation(s, View::Chat, Phase::Idle)),
    ("chat-grabando", |s| conversation(s, View::Chat, Phase::Recording)),
    ("chat-pensando", |s| conversation(s, View::Chat, Phase::Thinking)),
    ("chat-hablando", |s| conversation(s, View::Chat, Phase::Speaking)),
    ("voz-espera", |s| conversation(s, View::Voice, Phase::Idle)),
    ("voz-grabando", |s| conversation(s, View::Voice, Phase::Recording)),
    ("voz-pensando", |s| conversation(s, View::Voice, Phase::Thinking)),
    ("voz-hablando", |s| conversation(s, View::Voice, Phase::Speaking)),
    ("chat-permiso", |s| permission(s, View::Chat)),
    ("voz-permiso", |s| permission(s, View::Voice)),
    // Nombre de carpeta y `detail` que no caben: «…» en la barra y el recuadro se desplaza.
    ("chat-permiso-largo", |s| {
        permission(s, View::Chat);
        s.folder = Some(Folder { path: "/home/persona/un-proyecto-con-un-nombre-muy-largo".into(), name: "un-proyecto-con-un-nombre-muy-largo".into() });
        s.permission = Some(Permission { id: "p1".into(), tool: "Bash".into(), detail: LONG_DETAIL.into() });
    }),
    ("chat-desconectado", |s| {
        conversation(s, View::Chat, Phase::Idle);
        s.connected = false;
    }),
    ("voz-larga", |s| {
        conversation(s, View::Voice, Phase::Speaking);
        let n = s.messages.len();
        s.messages[n - 2].text = LONG_QUESTION.to_owned();
        s.messages[n - 1].text = LONG_REPLY.to_owned();
    }),
    ("voz-desconectado", |s| {
        conversation(s, View::Voice, Phase::Idle);
        s.connected = false;
    }),
    ("inicio-otro", |s| {
        folders(s);
        taken(s);
    }),
    ("chat-otro", |s| {
        conversation(s, View::Chat, Phase::Idle);
        taken(s);
    }),
    ("voz-otro", |s| {
        conversation(s, View::Voice, Phase::Idle);
        taken(s);
    }),
    // Sin datos de ejemplo: para probar con guiones de teclas.
    ("conversacion-vacia", |s| {
        s.connected = true;
        s.screen = Screen::Conversation;
        s.view = View::Chat;
    }),
];

fn main() {
    if std::env::args().any(|a| a == "--probar") {
        std::process::exit(probe::run(net::url()));
    }
    rune::set_app_id("charlando");
    let app = App::new(AppSpec {
        min_size: Some((360.0, 460.0)),
        fonts: FONTS,
        families: ("Inter", "IBM Plex Mono"),
        scenes: SCENES,
        poll: Some(poll),
        on_key: Some(on_key),
        ..AppSpec::new("Charlando", (440.0, 620.0), init, ui::view)
    });
    let handle = app.handle();
    app.init(move || {
        if rune::tooling::headless_run().is_some() {
            return init();
        }
        let audio = Audio::start();
        let net = net::spawn(net::url(), handle, audio.player.clone());
        State::new(Some(net), audio)
    })
    .compositor(orb::Orb::default())
    .run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::In;

    fn ready(session: &str) -> In {
        In::Ready { session: session.into(), folder: Folder { path: "/p/uno".into(), name: "uno".into() } }
    }

    fn connected() -> State {
        let mut s = init();
        s.on_connected(true);
        s.on_message(In::Hello { folders: vec![], charlas: "/charlas".into() });
        s.start();
        s.on_message(ready("haiku"));
        s
    }

    #[test]
    fn a_turn_walks_the_protocol_states() {
        let mut s = connected();
        assert_eq!(s.screen, Screen::Conversation);
        s.ptt_down();
        assert_eq!(s.phase, Phase::Recording);
        s.ptt_up();
        assert_eq!(s.phase, Phase::Thinking);
        s.on_message(In::State("thinking".into()));
        s.on_message(In::Transcript("Hola".into()));
        s.on_message(In::State("speaking".into()));
        s.on_message(In::Sentence("Hola.".into()));
        s.on_message(In::Sentence("¿Qué tal?".into()));
        assert_eq!(s.phase, Phase::Speaking);
        assert_eq!(s.last_reply(), Some("Hola. ¿Qué tal?"));
        s.on_message(In::ReplyEnd);
        assert!(s.tick(0.0));
        assert_eq!(s.phase, Phase::Idle);
    }

    #[test]
    fn a_tool_call_thinks_mid_turn_and_keeps_one_bubble() {
        let mut s = connected();
        s.ptt_down();
        s.ptt_up();
        s.on_message(In::Transcript("Busca algo".into()));
        s.on_message(In::State("speaking".into()));
        s.on_message(In::Sentence("Déjame mirarlo.".into()));
        s.on_message(In::State("thinking".into()));
        assert_eq!(s.phase, Phase::Speaking, "sigue hablando hasta vaciar la cola");
        s.tick(0.0);
        assert_eq!(s.phase, Phase::Thinking);
        s.on_message(In::State("speaking".into()));
        s.on_message(In::Sentence("Ya lo tengo.".into()));
        assert_eq!(s.phase, Phase::Speaking);
        assert_eq!(s.last_reply(), Some("Déjame mirarlo. Ya lo tengo."));
        assert_eq!(s.messages.iter().filter(|m| m.who == Who::Claude).count(), 1);
    }

    #[test]
    fn silence_goes_back_to_idle_without_a_reply() {
        let mut s = connected();
        s.ptt_down();
        s.ptt_up();
        s.on_message(In::Transcript(String::new()));
        s.on_message(In::ReplyEnd);
        s.tick(0.0);
        assert_eq!(s.phase, Phase::Idle);
        assert!(s.messages.is_empty());
    }

    #[test]
    fn space_is_ignored_while_claude_thinks_or_is_disconnected() {
        let mut s = connected();
        s.ptt_down();
        s.ptt_up();
        s.ptt_down();
        assert_eq!(s.phase, Phase::Thinking);
        s.on_connected(false);
        s.ptt_down();
        assert_eq!(s.phase, Phase::Idle);
        assert_eq!(s.screen, Screen::Conversation);
    }

    #[test]
    fn reconnecting_resumes_the_conversation() {
        let mut s = connected();
        s.on_message(ready("s1"));
        s.ptt_down();
        s.ptt_up();
        s.on_message(In::Transcript("Hola".into()));
        s.on_message(In::State("speaking".into()));
        s.on_message(In::Sentence("Hola, ¿qué tal?".into()));
        s.on_connected(false);
        s.on_connected(true);
        assert_eq!(s.screen, Screen::Conversation);
        assert!(s.resuming);
        assert!(!s.can_talk(), "no se habla hasta que llegue ready");
        s.on_message(ready("s1"));
        assert!(s.can_talk());
        assert_eq!(s.messages.len(), 2, "la conversación sigue ahí");
    }

    #[test]
    fn a_lost_session_goes_back_to_start() {
        let mut s = connected();
        s.on_message(ready("s1"));
        s.messages.push(Message { who: Who::You, text: "Hola".into() });
        s.on_connected(false);
        s.on_connected(true);
        s.on_message(In::SessionGone);
        assert_eq!(s.screen, Screen::Start);
        assert!(s.messages.is_empty());
        assert_eq!(s.session, None);
    }

    #[test]
    fn reconnecting_before_speaking_opens_a_new_session() {
        let mut s = connected();
        s.on_connected(false);
        s.on_connected(true);
        assert!(s.starting && !s.resuming);
        s.on_message(ready("s2"));
        assert_eq!(s.screen, Screen::Conversation);
        assert_eq!(s.session.as_deref(), Some("s2"));
    }
}
