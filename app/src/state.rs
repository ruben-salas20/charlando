//! El estado de la app y cómo cambia con lo que llega del servidor.

use crate::audio::Audio;
use crate::net::{Folder, In, Net, Out, Where};

/// La carpeta con la que se abre la app (`charlando` desde una terminal pasa la actual).
pub const INITIAL_FOLDER_VAR: &str = "CHARLANDO_CARPETA_INICIAL";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Model {
    Haiku,
    Sonnet,
    Opus,
}

impl Model {
    pub const ALL: [Model; 3] = [Model::Haiku, Model::Sonnet, Model::Opus];

    pub fn id(self) -> &'static str {
        match self {
            Model::Haiku => "haiku",
            Model::Sonnet => "sonnet",
            Model::Opus => "opus",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Model::Haiku => "Claude Haiku 5.5",
            Model::Sonnet => "Claude Sonnet 5.5",
            Model::Opus => "Claude Opus 5.5",
        }
    }
    pub fn blurb(self) -> &'static str {
        match self {
            Model::Haiku => "El más rápido",
            Model::Sonnet => "Equilibrado",
            Model::Opus => "El más capaz, más lento",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Start,
    Conversation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Chat,
    Voice,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Recording,
    Thinking,
    Speaking,
    /// La tarjeta de permiso está en pantalla.
    Permission,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Permission {
    pub id: String,
    pub tool: String,
    pub detail: String,
}

impl Permission {
    /// Lo que quiere hacer Claude, debajo del título de la tarjeta.
    pub fn action(&self) -> String {
        match self.tool.as_str() {
            "Bash" => "Ejecutar un comando".to_owned(),
            "Write" | "Edit" | "NotebookEdit" => "Escribir un archivo".to_owned(),
            "Task" | "Agent" => "Lanzar un subagente".to_owned(),
            tool if tool.starts_with("mcp__") => "Usar un conector".to_owned(),
            tool => format!("Usar {tool}"),
        }
    }
}

/// El último tramo de la ruta, para una carpeta que no trae nombre.
pub fn folder_name(path: &str) -> String {
    path.trim_end_matches('/').rsplit('/').next().filter(|n| !n.is_empty()).unwrap_or(path).to_owned()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Who {
    You,
    Claude,
}

#[derive(Clone, Debug)]
pub struct Message {
    pub who: Who,
    pub text: String,
}

pub struct State {
    pub screen: Screen,
    pub view: View,
    pub model: Model,
    /// Interruptor «Razonar»: Claude decide cuándo razonar. Se recuerda entre sesiones.
    pub thinking: bool,
    /// Carpetas recientes de `hello`, más la elegida si no venía en la lista.
    pub folders: Vec<Folder>,
    /// Dónde crea el servidor las charlas nuevas (`charlas` de `hello`).
    pub charlas: String,
    /// La carpeta de `CHARLANDO_CARPETA_INICIAL`.
    pub initial: Option<String>,
    /// Lo elegido en el selector «Carpeta». `None` hasta el primer `hello`.
    pub place: Option<Where>,
    /// La persona eligió la carpeta en el selector: un `hello` nuevo no la cambia.
    pub place_by_hand: bool,
    /// El desplegable de «Carpeta» está abierto.
    pub folder_menu: bool,
    /// La carpeta de la conversación abierta, del último `ready`.
    pub folder: Option<Folder>,
    pub connected: bool,
    /// Se pulsó «Empezar» y esperamos `ready`.
    pub starting: bool,
    /// Id de la conversación abierta, para reanudarla si se cae la conexión.
    pub session: Option<String>,
    /// Se pidió reanudar y esperamos `ready`.
    pub resuming: bool,
    pub phase: Phase,
    pub messages: Vec<Message>,
    /// Ya llegó `reply_end`: falta que se vacíe la cola de reproducción.
    pub reply_ended: bool,
    /// Ya hay burbuja de Claude en este turno: lo que llegue tras una herramienta va a la misma.
    pub reply_started: bool,
    /// Llegó `thinking` mientras sonaba la voz: se pasa a Pensando cuando termine de sonar.
    pub pending_thinking: bool,
    /// Permiso pedido. La tarjeta sale cuando termina de sonar el audio en cola.
    pub permission: Option<Permission>,
    pub error: Option<String>,
    /// Nivel de audio 0..1 que se dibuja (micrófono o voz de Claude).
    pub level: f32,
    /// En las capturas sin ventana el nivel sale de la misma función que la maqueta.
    pub demo_level: bool,
    pub net: Option<Net>,
    pub audio: Audio,
}

impl State {
    pub fn new(net: Option<Net>, audio: Audio) -> State {
        let initial = std::env::var(INITIAL_FOLDER_VAR).ok().filter(|p| !p.is_empty());
        State {
            screen: Screen::Start,
            view: crate::config::load_view(),
            model: Model::Haiku,
            thinking: crate::config::load_thinking(),
            folders: Vec::new(),
            charlas: String::new(),
            place: initial.clone().map(Where::Folder),
            initial,
            place_by_hand: false,
            folder_menu: false,
            folder: None,
            connected: false,
            starting: false,
            session: None,
            resuming: false,
            phase: Phase::Idle,
            messages: Vec::new(),
            reply_ended: false,
            reply_started: false,
            pending_thinking: false,
            permission: None,
            error: None,
            level: 0.0,
            demo_level: false,
            net,
            audio,
        }
    }

    fn send(&self, msg: Out) {
        if let Some(net) = &self.net {
            net.send(msg);
        }
    }

    pub fn start(&mut self) {
        let Some(place) = self.place.clone() else { return };
        if !self.connected || self.starting {
            return;
        }
        self.starting = true;
        self.folder_menu = false;
        self.error = None;
        self.send(Out::Start { place, model: self.model.id(), thinking: self.thinking });
    }

    pub fn choose(&mut self, place: Where) {
        self.place = Some(place);
        self.place_by_hand = true;
        self.folder_menu = false;
    }

    /// Nombre y ruta de lo elegido en el selector. La ruta va vacía en «Charla nueva».
    pub fn chosen(&self) -> Option<Folder> {
        match self.place.as_ref()? {
            Where::New => Some(Folder { path: String::new(), name: "Charla nueva".to_owned() }),
            Where::Folder(path) => Some(
                self.folders
                    .iter()
                    .find(|f| &f.path == path)
                    .cloned()
                    .unwrap_or_else(|| Folder { path: path.clone(), name: folder_name(path) }),
            ),
        }
    }

    /// El comando que sigue esta conversación en la terminal.
    pub fn resume_command(&self) -> Option<String> {
        let (folder, session) = (self.folder.as_ref()?, self.session.as_ref()?);
        Some(format!("cd '{}' && claude --resume {session}", folder.path.replace('\'', r"'\''")))
    }

    /// `hello` llega al conectar y tras cada `ready`: rehace la lista sin tocar lo elegido a mano.
    fn on_hello(&mut self, mut folders: Vec<Folder>, charlas: String) {
        for f in &mut folders {
            if f.name.is_empty() {
                f.name = folder_name(&f.path);
            }
        }
        if !self.place_by_hand {
            self.place = Some(match &self.initial {
                Some(path) => Where::Folder(path.clone()),
                None => folders.first().map_or(Where::New, |f| Where::Folder(f.path.clone())),
            });
        }
        if let Some(Where::Folder(path)) = &self.place
            && !folders.iter().any(|f| &f.path == path)
        {
            folders.insert(0, Folder { path: path.clone(), name: folder_name(path) });
        }
        self.folders = folders;
        self.charlas = charlas;
    }

    /// «Rechazar» o «Aceptar» en la tarjeta de permiso.
    pub fn answer_permission(&mut self, allow: bool) {
        if self.phase != Phase::Permission {
            return;
        }
        let Some(p) = self.permission.take() else { return };
        self.phase = Phase::Thinking;
        self.send(Out::PermissionReply { id: p.id, allow });
    }

    pub fn toggle_thinking(&mut self) {
        self.thinking = !self.thinking;
        crate::config::save_thinking(self.thinking);
    }

    pub fn new_conversation(&mut self) {
        self.stop_recording_silently();
        self.audio.player.clear();
        self.screen = Screen::Start;
        self.starting = false;
        self.session = None;
        self.folder = None;
        self.permission = None;
        self.resuming = false;
        self.phase = Phase::Idle;
        self.messages.clear();
        self.reply_ended = false;
        self.error = None;
    }

    pub fn set_view(&mut self, view: View) {
        if self.view != view {
            self.view = view;
            crate::config::save_view(view);
        }
    }

    pub fn can_talk(&self) -> bool {
        self.connected && !self.resuming && !self.starting && self.screen == Screen::Conversation && self.phase == Phase::Idle
    }

    pub fn ptt_down(&mut self) {
        if !self.can_talk() {
            return;
        }
        self.error = None;
        self.phase = Phase::Recording;
        if let Some(net) = &self.net {
            self.audio.mic.begin(net, Out::PttStart);
        }
    }

    pub fn ptt_up(&mut self) {
        if self.phase != Phase::Recording {
            return;
        }
        self.phase = Phase::Thinking;
        self.reply_ended = false;
        self.reply_started = false;
        self.pending_thinking = false;
        if let Some(net) = &self.net {
            self.audio.mic.end(net, Out::PttEnd);
        }
    }

    fn stop_recording_silently(&mut self) {
        self.audio.mic.cancel();
    }

    pub fn on_connected(&mut self, connected: bool) {
        self.connected = connected;
        self.stop_recording_silently();
        self.audio.player.clear();
        self.starting = false;
        self.resuming = false;
        self.phase = Phase::Idle;
        self.reply_ended = false;
        self.permission = None;
        if !connected || self.screen != Screen::Conversation {
            return;
        }
        // PROTOCOLO: al reconectar en una conversación, se reanuda. Sin nada dicho aún no hay
        // sesión que reanudar: se abre otra con el mismo modelo.
        self.error = None;
        let (model, thinking) = (self.model.id(), self.thinking);
        let Some(folder) = self.folder.as_ref().map(|f| f.path.clone()) else {
            // Se cayó antes de `ready`: no hay carpeta a la que volver.
            self.new_conversation();
            return;
        };
        match self.session.clone() {
            Some(session) if !self.messages.is_empty() => {
                self.resuming = true;
                self.send(Out::Resume { folder, model, thinking, session });
            }
            _ => {
                self.starting = true;
                self.send(Out::Start { place: Where::Folder(folder), model, thinking });
            }
        }
    }

    pub fn on_message(&mut self, msg: In) {
        match msg {
            In::Hello { folders, charlas } => self.on_hello(folders, charlas),
            In::Ready { session, folder } => {
                self.session = Some(session);
                self.folder = Some(folder);
                if self.resuming {
                    self.resuming = false;
                } else if self.starting {
                    self.starting = false;
                    self.screen = Screen::Conversation;
                    self.phase = Phase::Idle;
                    self.messages.clear();
                    self.error = None;
                }
            }
            // La tarjeta manda: lo que llegue mientras está abierta no cambia de estado.
            In::State(_) if self.permission.is_some() => {}
            In::State(s) => match s.as_str() {
                "thinking" => match self.phase {
                    Phase::Speaking => self.pending_thinking = true,
                    Phase::Recording => {}
                    _ => self.phase = Phase::Thinking,
                },
                "speaking" => {
                    self.phase = Phase::Speaking;
                    self.pending_thinking = false;
                    if !self.reply_started {
                        self.reply_started = true;
                        self.messages.push(Message { who: Who::Claude, text: String::new() });
                    }
                }
                _ => {}
            },
            In::Transcript(text) => {
                if !text.trim().is_empty() {
                    self.messages.push(Message { who: Who::You, text });
                }
            }
            In::Sentence(text) => {
                match self.messages.last_mut() {
                    Some(m) if m.who == Who::Claude && self.reply_started => {
                        if !m.text.is_empty() {
                            m.text.push(' ');
                        }
                        m.text.push_str(text.trim());
                    }
                    _ => self.messages.push(Message { who: Who::Claude, text: text.trim().to_owned() }),
                }
            }
            In::ReplyEnd => {
                self.reply_ended = true;
                self.drop_permission();
            }
            In::SessionGone => self.new_conversation(),
            In::FolderMissing(message) => {
                self.new_conversation();
                self.error = Some(message);
            }
            // PROTOCOLO: solo dentro de un turno que Claude ya está contestando.
            In::Permission { .. } if !matches!(self.phase, Phase::Thinking | Phase::Speaking) => {}
            In::Permission { id, tool, detail } => {
                self.permission = Some(Permission { id, tool, detail });
                self.pending_thinking = false;
                self.show_permission_when_quiet();
            }
            In::Error(message) => {
                self.error = Some(message);
                self.starting = false;
                self.reply_ended = true;
                self.drop_permission();
            }
        }
    }

    /// Cada fotograma: nivel de audio y fin de la reproducción. True si cambió algo visible.
    pub fn tick(&mut self, t: f64) -> bool {
        let mut changed = false;
        if self.show_permission_when_quiet() {
            changed = true;
        } else if self.reply_ended && matches!(self.phase, Phase::Thinking | Phase::Speaking) && self.audio.player.is_empty() {
            self.phase = Phase::Idle;
            self.reply_ended = false;
            self.pending_thinking = false;
            changed = true;
        } else if self.pending_thinking && self.phase == Phase::Speaking && self.audio.player.is_empty() {
            self.phase = Phase::Thinking;
            self.pending_thinking = false;
            changed = true;
        }
        let level = match self.phase {
            _ if self.demo_level => demo_level(t),
            Phase::Recording => self.audio.mic.level(),
            Phase::Speaking => self.audio.player.level(),
            _ => 0.0,
        };
        if (level - self.level).abs() > 0.01 {
            self.level = level;
            changed = true;
        }
        changed
    }

    /// El turno terminó sin respuesta a la tarjeta: se quita y se vuelve a la espera.
    fn drop_permission(&mut self) {
        if self.permission.take().is_some() && self.phase == Phase::Permission {
            self.phase = Phase::Thinking;
        }
    }

    fn show_permission_when_quiet(&mut self) -> bool {
        if self.permission.is_none() || matches!(self.phase, Phase::Permission | Phase::Recording) || !self.audio.player.is_empty() {
            return false;
        }
        self.phase = Phase::Permission;
        true
    }

    pub fn last_question(&self) -> Option<&str> {
        self.messages.iter().rev().find(|m| m.who == Who::You).map(|m| m.text.as_str())
    }

    /// La respuesta que va con la última pregunta (o la última si no hubo pregunta).
    pub fn last_reply(&self) -> Option<&str> {
        match self.messages.last() {
            Some(m) if m.who == Who::Claude => Some(m.text.as_str()),
            _ => None,
        }
    }
}

/// La función `level()` de la maqueta, con `t` en segundos.
pub fn demo_level(t: f64) -> f32 {
    let now = t * 1000.0;
    ((now / 170.0).sin() * (now / 410.0).sin()).abs().min(1.0 / 1.4) as f32 * 1.4
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc::UnboundedReceiver;

    fn folder(path: &str) -> Folder {
        Folder { path: path.to_owned(), name: folder_name(path) }
    }

    fn hello(folders: Vec<Folder>) -> In {
        In::Hello { folders, charlas: "/charlas".into() }
    }

    fn ready(session: &str, path: &str) -> In {
        In::Ready { session: session.to_owned(), folder: folder(path) }
    }

    /// Conectada, con dos carpetas recientes y sin conversación.
    fn at_start(audio: Audio) -> (State, UnboundedReceiver<Out>) {
        let (net, rx) = Net::capture();
        let mut s = State::new(Some(net), audio);
        s.initial = None;
        s.place = None;
        s.on_connected(true);
        s.on_message(hello(vec![folder("/p/uno"), folder("/p/dos")]));
        (s, rx)
    }

    fn sent(rx: &mut UnboundedReceiver<Out>) -> Vec<Out> {
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    }

    /// En una conversación en `/p/uno` con un turno a medias: Claude ya habla.
    fn speaking(audio: Audio) -> (State, UnboundedReceiver<Out>) {
        let (mut s, mut rx) = at_start(audio);
        s.start();
        s.on_message(ready("s1", "/p/uno"));
        s.ptt_down();
        s.ptt_up();
        s.on_message(In::Transcript("Sube los cambios".into()));
        s.on_message(In::State("speaking".into()));
        s.on_message(In::Sentence("Necesito tu permiso. Míralo en pantalla.".into()));
        sent(&mut rx);
        (s, rx)
    }

    fn ask(s: &mut State) {
        s.on_message(In::Permission { id: "p1".into(), tool: "Bash".into(), detail: "git push origin main".into() });
    }

    #[test]
    fn the_most_recent_folder_is_chosen_first() {
        let (s, _) = at_start(Audio::null());
        assert_eq!(s.place, Some(Where::Folder("/p/uno".into())));
        assert_eq!(s.chosen().unwrap().name, "uno");
    }

    #[test]
    fn without_recent_folders_it_is_a_new_chat() {
        let mut s = State::new(None, Audio::null());
        s.place = None;
        s.on_message(hello(vec![]));
        assert_eq!(s.place, Some(Where::New));
        assert_eq!(s.chosen().unwrap().name, "Charla nueva");
    }

    #[test]
    fn the_launch_folder_wins_and_goes_first_if_it_is_not_recent() {
        let mut s = State::new(None, Audio::null());
        s.initial = Some("/p/tres/".into());
        s.on_message(hello(vec![folder("/p/uno"), folder("/p/tres/")]));
        assert_eq!(s.place, Some(Where::Folder("/p/tres/".into())));
        assert_eq!(s.folders.len(), 2, "si ya está en la lista no se repite");
        s.initial = Some("/p/cuatro".into());
        s.on_message(hello(vec![folder("/p/uno")]));
        assert_eq!(s.folders[0], Folder { path: "/p/cuatro".into(), name: "cuatro".into() });
        assert_eq!(s.place, Some(Where::Folder("/p/cuatro".into())));
    }

    #[test]
    fn a_later_hello_keeps_a_folder_chosen_by_hand() {
        let (mut s, _) = at_start(Audio::null());
        s.on_message(hello(vec![folder("/p/dos"), folder("/p/uno")]));
        assert_eq!(s.place, Some(Where::Folder("/p/dos".into())), "sin elección a mano sigue a la más reciente");
        s.choose(Where::Folder("/p/uno".into()));
        s.on_message(hello(vec![folder("/p/tres")]));
        assert_eq!(s.place, Some(Where::Folder("/p/uno".into())));
        assert_eq!(s.folders[0].path, "/p/uno", "lo elegido sigue en la lista");
        s.choose(Where::New);
        s.on_message(In::Hello { folders: vec![folder("/p/tres")], charlas: "/otras".into() });
        assert_eq!(s.place, Some(Where::New));
        assert_eq!(s.charlas, "/otras");
    }

    #[test]
    fn a_permission_outside_a_reply_is_ignored() {
        let (mut s, _) = at_start(Audio::null());
        s.start();
        s.on_message(ready("s1", "/p/uno"));
        ask(&mut s);
        assert_eq!(s.permission, None, "en idle");
        s.ptt_down();
        ask(&mut s);
        assert_eq!(s.permission, None, "grabando");
        assert_eq!(s.phase, Phase::Recording);
    }

    #[test]
    fn plus_loses_the_card() {
        let (mut s, _) = speaking(Audio::null());
        ask(&mut s);
        s.new_conversation();
        assert_eq!(s.permission, None);
        assert_eq!(s.phase, Phase::Idle);
    }

    #[test]
    fn hello_fills_in_a_missing_name() {
        let mut s = State::new(None, Audio::null());
        s.on_message(hello(vec![Folder { path: "/p/sin-nombre/".into(), name: String::new() }]));
        assert_eq!(s.folders[0].name, "sin-nombre");
    }

    #[test]
    fn start_sends_the_chosen_folder_or_asks_for_a_new_one() {
        let (mut s, mut rx) = at_start(Audio::null());
        s.folder_menu = true;
        s.choose(Where::Folder("/p/dos".into()));
        assert!(!s.folder_menu, "elegir cierra el desplegable");
        s.start();
        assert_eq!(sent(&mut rx), vec![Out::Start { place: Where::Folder("/p/dos".into()), model: "haiku", thinking: s.thinking }]);
        s.new_conversation();
        s.choose(Where::New);
        s.start();
        assert_eq!(sent(&mut rx), vec![Out::Start { place: Where::New, model: "haiku", thinking: s.thinking }]);
    }

    #[test]
    fn ready_names_the_conversation_folder() {
        let (mut s, _) = at_start(Audio::null());
        s.choose(Where::New);
        s.start();
        s.on_message(ready("s1", "/charlas/2026-11-12"));
        assert_eq!(s.screen, Screen::Conversation);
        assert_eq!(s.folder.as_ref().unwrap().name, "2026-11-12");
        assert_eq!(s.resume_command().as_deref(), Some("cd '/charlas/2026-11-12' && claude --resume s1"));
    }

    #[test]
    fn the_resume_command_quotes_the_folder() {
        let (mut s, _) = at_start(Audio::null());
        s.start();
        s.on_message(ready("s1", "/p/l'uno"));
        assert_eq!(s.resume_command().as_deref(), Some(r"cd '/p/l'\''uno' && claude --resume s1"));
        s.new_conversation();
        assert_eq!(s.resume_command(), None);
    }

    #[test]
    fn a_missing_folder_goes_back_to_start_with_the_message() {
        let (mut s, _) = at_start(Audio::null());
        s.start();
        s.on_message(In::FolderMissing("La carpeta /p/uno no existe".into()));
        assert_eq!(s.screen, Screen::Start);
        assert!(!s.starting, "se puede volver a pulsar «Empezar»");
        assert_eq!(s.error.as_deref(), Some("La carpeta /p/uno no existe"));
        s.start();
        assert_eq!(s.error, None);
    }

    #[test]
    fn reconnecting_resumes_in_the_same_folder() {
        let (mut s, mut rx) = speaking(Audio::null());
        s.on_connected(false);
        s.on_connected(true);
        assert_eq!(
            sent(&mut rx),
            vec![Out::Resume { folder: "/p/uno".into(), model: "haiku", thinking: s.thinking, session: "s1".into() }]
        );
    }

    #[test]
    fn reconnecting_before_speaking_starts_again_in_the_same_folder() {
        let (mut s, mut rx) = at_start(Audio::null());
        s.choose(Where::New);
        s.start();
        s.on_message(ready("s1", "/charlas/nueva"));
        sent(&mut rx);
        s.on_connected(false);
        s.on_connected(true);
        assert_eq!(
            sent(&mut rx),
            vec![Out::Start { place: Where::Folder("/charlas/nueva".into()), model: "haiku", thinking: s.thinking }],
            "no crea otra carpeta nueva"
        );
    }

    #[test]
    fn the_card_waits_for_the_queued_audio() {
        let (mut s, _) = speaking(Audio::queued());
        s.audio.player.push_pcm16(&[0; 4800]);
        ask(&mut s);
        assert_eq!(s.phase, Phase::Speaking, "primero suena «Necesito tu permiso»");
        s.tick(0.0);
        assert_eq!(s.phase, Phase::Speaking);
        s.audio.player.clear();
        assert!(s.tick(0.0));
        assert_eq!(s.phase, Phase::Permission);
    }

    #[test]
    fn the_card_shows_at_once_if_nothing_is_playing() {
        let (mut s, _) = speaking(Audio::null());
        s.on_message(In::State("thinking".into()));
        ask(&mut s);
        assert_eq!(s.phase, Phase::Permission);
        assert_eq!(s.permission.as_ref().unwrap().action(), "Ejecutar un comando");
    }

    #[test]
    fn space_does_nothing_while_the_card_is_open() {
        let (mut s, _) = speaking(Audio::null());
        ask(&mut s);
        assert!(!s.can_talk());
        s.ptt_down();
        assert_eq!(s.phase, Phase::Permission);
    }

    #[test]
    fn a_reminder_plays_without_closing_the_card() {
        let (mut s, _) = speaking(Audio::queued());
        ask(&mut s);
        s.on_message(In::State("speaking".into()));
        s.on_message(In::Sentence("Sigo esperando tu permiso. Míralo en pantalla.".into()));
        s.audio.player.push_pcm16(&[0; 4800]);
        s.tick(0.0);
        assert_eq!(s.phase, Phase::Permission);
        assert!(s.permission.is_some());
        assert_eq!(
            s.last_reply(),
            Some("Necesito tu permiso. Míralo en pantalla. Sigo esperando tu permiso. Míralo en pantalla.")
        );
    }

    #[test]
    fn answering_sends_the_reply_and_thinks() {
        let (mut s, mut rx) = speaking(Audio::null());
        ask(&mut s);
        s.answer_permission(true);
        assert_eq!(sent(&mut rx), vec![Out::PermissionReply { id: "p1".into(), allow: true }]);
        assert_eq!(s.phase, Phase::Thinking);
        assert_eq!(s.permission, None);
        s.answer_permission(false);
        assert!(sent(&mut rx).is_empty(), "una sola respuesta por permiso");
        s.on_message(In::State("speaking".into()));
        s.on_message(In::Sentence("Hecho.".into()));
        assert_eq!(s.messages.iter().filter(|m| m.who == Who::Claude).count(), 1, "el turno sigue en la misma burbuja");
    }

    #[test]
    fn the_card_cannot_be_answered_before_it_shows() {
        let (mut s, mut rx) = speaking(Audio::queued());
        s.audio.player.push_pcm16(&[0; 4800]);
        ask(&mut s);
        s.answer_permission(true);
        assert!(sent(&mut rx).is_empty());
        assert!(s.permission.is_some());
    }

    #[test]
    fn losing_the_connection_loses_the_card() {
        let (mut s, _) = speaking(Audio::null());
        ask(&mut s);
        s.on_connected(false);
        assert_eq!(s.phase, Phase::Idle);
        assert_eq!(s.permission, None);
        assert_eq!(s.screen, Screen::Conversation);
    }

    #[test]
    fn an_error_on_start_lets_you_start_again() {
        let (mut s, _) = at_start(Audio::null());
        s.start();
        s.on_message(In::Error("falló".into()));
        assert!(!s.starting, "se puede volver a pulsar «Empezar»");
    }

    #[test]
    fn an_error_closes_the_card() {
        let (mut s, _) = speaking(Audio::null());
        ask(&mut s);
        s.on_message(In::Error("falló".into()));
        assert_eq!(s.permission, None);
        s.tick(0.0);
        assert_eq!(s.phase, Phase::Idle);
    }

    #[test]
    fn tools_read_as_actions() {
        let p = |tool: &str| Permission { id: String::new(), tool: tool.into(), detail: String::new() };
        assert_eq!(p("Write").action(), "Escribir un archivo");
        assert_eq!(p("Edit").action(), "Escribir un archivo");
        assert_eq!(p("NotebookEdit").action(), "Escribir un archivo");
        assert_eq!(p("Task").action(), "Lanzar un subagente");
        assert_eq!(p("Agent").action(), "Lanzar un subagente");
        assert_eq!(p("mcp__github__push").action(), "Usar un conector");
        assert_eq!(p("WebFetch").action(), "Usar WebFetch");
    }
}
