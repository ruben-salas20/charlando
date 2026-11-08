//! El estado de la app y cómo cambia con lo que llega del servidor.

use crate::audio::Audio;
use crate::net::{In, Net, Out};

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
    pub connected: bool,
    /// Se pulsó «Empezar» y esperamos `ready`.
    pub starting: bool,
    pub phase: Phase,
    pub messages: Vec<Message>,
    /// Ya llegó `reply_end`: falta que se vacíe la cola de reproducción.
    pub reply_ended: bool,
    /// Ya hay burbuja de Claude en este turno: lo que llegue tras una herramienta va a la misma.
    pub reply_started: bool,
    /// Llegó `thinking` mientras sonaba la voz: se pasa a Pensando cuando termine de sonar.
    pub pending_thinking: bool,
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
        State {
            screen: Screen::Start,
            view: crate::config::load_view(),
            model: Model::Haiku,
            thinking: crate::config::load_thinking(),
            connected: false,
            starting: false,
            phase: Phase::Idle,
            messages: Vec::new(),
            reply_ended: false,
            reply_started: false,
            pending_thinking: false,
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
        if !self.connected || self.starting {
            return;
        }
        self.starting = true;
        self.send(Out::Start(self.model.id(), self.thinking));
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
        self.connected && self.screen == Screen::Conversation && self.phase == Phase::Idle
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
        self.phase = Phase::Idle;
        self.reply_ended = false;
        if connected {
            // PROTOCOLO: al reconectar, vuelve a la pantalla de inicio.
            self.screen = Screen::Start;
            self.messages.clear();
            self.error = None;
        }
    }

    pub fn on_message(&mut self, msg: In) {
        match msg {
            In::Ready(_) => {
                if self.starting {
                    self.starting = false;
                    self.screen = Screen::Conversation;
                    self.phase = Phase::Idle;
                    self.messages.clear();
                    self.error = None;
                }
            }
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
            In::ReplyEnd => self.reply_ended = true,
            In::Error(message) => {
                self.error = Some(message);
                self.reply_ended = true;
            }
        }
    }

    /// Cada fotograma: nivel de audio y fin de la reproducción. True si cambió algo visible.
    pub fn tick(&mut self, t: f64) -> bool {
        let mut changed = false;
        if self.reply_ended && matches!(self.phase, Phase::Thinking | Phase::Speaking) && self.audio.player.is_empty() {
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
