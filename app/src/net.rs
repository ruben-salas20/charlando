//! El WebSocket con el servidor, en su propio hilo con un runtime de tokio.
//! Reintenta cada 2 s mientras no hay conexión (PROTOCOLO.md).

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use rune::AppHandle;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::audio::Player;
use crate::state::State;

pub const DEFAULT_URL: &str = "ws://127.0.0.1:8765/ws";
const RETRY: Duration = Duration::from_secs(2);

/// Dónde abrir la conversación.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Where {
    /// Una carpeta que ya existe, con su ruta absoluta.
    Folder(String),
    /// Una carpeta nueva que crea el servidor.
    New,
}

/// De la app al servidor.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    Start { place: Where, model: &'static str, thinking: bool },
    /// Reanuda la conversación `session` de `folder` tras perder la conexión.
    Resume { folder: String, model: &'static str, thinking: bool, session: String },
    PttStart,
    PttEnd,
    PermissionReply { id: String, allow: bool },
    /// PCM 16 bits LE, mono, 16 kHz.
    Audio(Vec<u8>),
}

impl Out {
    fn into_message(self) -> Message {
        let v = match self {
            Out::Start { place: Where::Folder(folder), model, thinking } => {
                json!({"type": "start", "folder": folder, "model": model, "thinking": thinking})
            }
            Out::Start { place: Where::New, model, thinking } => {
                json!({"type": "start", "new_folder": true, "model": model, "thinking": thinking})
            }
            Out::Resume { folder, model, thinking, session } => {
                json!({"type": "start", "folder": folder, "model": model, "thinking": thinking, "resume": session})
            }
            Out::PttStart => json!({"type": "ptt_start"}),
            Out::PttEnd => json!({"type": "ptt_end"}),
            Out::PermissionReply { id, allow } => json!({"type": "permission_reply", "id": id, "allow": allow}),
            Out::Audio(pcm) => return Message::binary(pcm),
        };
        Message::text(v.to_string())
    }
}

/// Una carpeta reciente de `hello`, o la de `ready`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub path: String,
    pub name: String,
}

/// Del servidor a la app (los mensajes de texto; el audio va directo al reproductor).
#[derive(Debug, Clone, PartialEq)]
pub enum In {
    /// Las carpetas recientes, la más reciente primero, y dónde se crean las charlas nuevas.
    Hello { folders: Vec<Folder>, charlas: String },
    /// El id de la conversación, para reanudarla, y la carpeta en la que trabaja Claude.
    Ready { session: String, folder: Folder },
    State(String),
    Transcript(String),
    Sentence(String),
    ReplyEnd,
    Error(String),
    /// La conversación que se quería reanudar ya no existe.
    SessionGone,
    /// La carpeta de `start` no existe. Lleva el mensaje para la pantalla de inicio.
    FolderMissing(String),
    Permission { id: String, tool: String, detail: String },
}

pub fn parse(text: &str) -> Option<In> {
    let v: Value = serde_json::from_str(text).ok()?;
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
    Some(match v.get("type")?.as_str()? {
        "hello" => In::Hello {
            charlas: s("charlas"),
            folders: v
                .get("folders")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|f| {
                    let path = f.get("path")?.as_str()?.to_owned();
                    let name = f.get("name").and_then(Value::as_str).unwrap_or_default().to_owned();
                    Some(Folder { path, name })
                })
                .collect(),
        },
        "ready" => In::Ready { session: s("session"), folder: Folder { path: s("folder"), name: s("name") } },
        "state" => In::State(s("state")),
        "transcript" => In::Transcript(s("text")),
        "sentence" => In::Sentence(s("text")),
        "reply_end" => In::ReplyEnd,
        "permission" => In::Permission { id: s("id"), tool: s("tool"), detail: s("detail") },
        "error" if s("code") == "session_gone" => In::SessionGone,
        "error" if s("code") == "folder_missing" => In::FolderMissing(s("message")),
        "error" => In::Error(s("message")),
        _ => return None,
    })
}

#[derive(Clone)]
pub struct Net {
    tx: mpsc::UnboundedSender<Out>,
}

impl Net {
    pub fn send(&self, msg: Out) {
        let _ = self.tx.send(msg);
    }
}

#[cfg(test)]
impl Net {
    /// Un `Net` sin red: lo enviado se lee del receptor.
    pub fn capture() -> (Net, mpsc::UnboundedReceiver<Out>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Net { tx }, rx)
    }
}

pub fn url() -> String {
    std::env::var("CHARLANDO_URL").ok().filter(|u| !u.is_empty()).unwrap_or_else(|| DEFAULT_URL.to_owned())
}

/// Arranca el hilo de red. `on_event` recibe lo que la interfaz tiene que saber.
pub fn spawn(url: String, handle: AppHandle<State>, player: Player) -> Net {
    let (tx, rx) = mpsc::unbounded_channel();
    let events = move |e: Event| match e {
        Event::Connected(c) => handle.update(move |s, _| s.on_connected(c)),
        Event::Message(m) => handle.update(move |s, _| s.on_message(m)),
    };
    std::thread::Builder::new()
        .name("charlando-red".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime de tokio");
            rt.block_on(run(url, rx, events, move |pcm| player.push_pcm16(pcm)));
        })
        .expect("hilo de red");
    Net { tx }
}

pub enum Event {
    Connected(bool),
    Message(In),
}

/// El bucle de conexión, separado de Rune para poder probarlo.
pub async fn run(url: String, mut rx: mpsc::UnboundedReceiver<Out>, events: impl Fn(Event), audio: impl Fn(&[u8])) {
    let mut last_error = String::new();
    loop {
        match tokio_tungstenite::connect_async(url.as_str()).await {
            Ok((ws, _)) => {
                eprintln!("charlando: conectado a {url}");
                last_error.clear();
                // Lo que se pidió sin conexión ya no vale.
                while rx.try_recv().is_ok() {}
                events(Event::Connected(true));
                let (mut sink, mut stream) = ws.split();
                loop {
                    tokio::select! {
                        msg = stream.next() => match msg {
                            Some(Ok(Message::Text(t))) => {
                                if let Some(m) = parse(&t) { events(Event::Message(m)); }
                            }
                            Some(Ok(Message::Binary(b))) => audio(&b),
                            Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                            Some(Ok(_)) => {}
                        },
                        out = rx.recv() => match out {
                            Some(o) => if sink.send(o.into_message()).await.is_err() { break },
                            None => return,
                        },
                    }
                }
                eprintln!("charlando: conexión perdida");
                events(Event::Connected(false));
            }
            Err(e) => {
                let e = e.to_string();
                if e != last_error {
                    eprintln!("charlando: no conecta con {url}: {e}; reintento cada 2 s");
                    last_error = e;
                }
            }
        }
        tokio::time::sleep(RETRY).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_server_message() {
        assert_eq!(
            parse(r#"{"type":"hello","folders":[{"path":"/a/proyecto","name":"proyecto"},{"path":"/b"}],"charlas":"/c"}"#),
            Some(In::Hello {
                folders: vec![
                    Folder { path: "/a/proyecto".into(), name: "proyecto".into() },
                    Folder { path: "/b".into(), name: String::new() },
                ],
                charlas: "/c".into(),
            })
        );
        assert_eq!(
            parse(r#"{"type":"hello","folders":[],"charlas":"/c"}"#),
            Some(In::Hello { folders: vec![], charlas: "/c".into() })
        );
        assert_eq!(
            parse(r#"{"type":"ready","model":"haiku","session":"s1","folder":"/a/proyecto","name":"proyecto"}"#),
            Some(In::Ready { session: "s1".into(), folder: Folder { path: "/a/proyecto".into(), name: "proyecto".into() } })
        );
        assert_eq!(
            parse(r#"{"type":"permission","id":"p1","tool":"Bash","detail":"git push origin main"}"#),
            Some(In::Permission { id: "p1".into(), tool: "Bash".into(), detail: "git push origin main".into() })
        );
        assert_eq!(
            parse(r#"{"type":"error","message":"No existe","code":"folder_missing"}"#),
            Some(In::FolderMissing("No existe".into()))
        );
        assert_eq!(parse(r#"{"type":"state","state":"speaking"}"#), Some(In::State("speaking".into())));
        assert_eq!(parse(r#"{"type":"transcript","text":""}"#), Some(In::Transcript(String::new())));
        assert_eq!(parse(r#"{"type":"sentence","text":"Hola."}"#), Some(In::Sentence("Hola.".into())));
        assert_eq!(parse(r#"{"type":"reply_end"}"#), Some(In::ReplyEnd));
        assert_eq!(parse(r#"{"type":"error","message":"x"}"#), Some(In::Error("x".into())));
        assert_eq!(parse(r#"{"type":"error","message":"x","code":"session_gone"}"#), Some(In::SessionGone));
        assert_eq!(parse(r#"{"type":"otro"}"#), None);
    }

    #[test]
    fn messages_follow_the_protocol() {
        let text = |o: Out| match o.into_message() {
            Message::Text(t) => serde_json::from_str::<Value>(&t).unwrap(),
            _ => panic!(),
        };
        assert_eq!(
            text(Out::Start { place: Where::Folder("/a".into()), model: "opus", thinking: true }),
            json!({"type":"start","folder":"/a","model":"opus","thinking":true})
        );
        assert_eq!(
            text(Out::Start { place: Where::New, model: "haiku", thinking: false }),
            json!({"type":"start","new_folder":true,"model":"haiku","thinking":false})
        );
        assert_eq!(
            text(Out::Resume { folder: "/a".into(), model: "opus", thinking: false, session: "s1".into() }),
            json!({"type":"start","folder":"/a","model":"opus","thinking":false,"resume":"s1"})
        );
        assert_eq!(
            text(Out::PermissionReply { id: "p1".into(), allow: false }),
            json!({"type":"permission_reply","id":"p1","allow":false})
        );
        assert_eq!(text(Out::PttStart), json!({"type":"ptt_start"}));
        assert_eq!(text(Out::PttEnd), json!({"type":"ptt_end"}));
    }

    /// Un turno entero contra el servidor de verdad, con el bucle de red y el estado de la app.
    /// `cargo test --release -- --ignored real_server`
    #[test]
    #[ignore]
    fn real_server_turn() {
        use crate::audio::Audio;
        use crate::state::{Phase, Screen, State};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{Arc, mpsc as smpsc};

        let (tx, rx) = mpsc::unbounded_channel();
        let (ev_tx, ev_rx) = smpsc::channel();
        let bytes = Arc::new(AtomicUsize::new(0));
        let b = bytes.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            rt.block_on(run(url(), rx, move |e| ev_tx.send(e).unwrap(), move |pcm| {
                b.fetch_add(pcm.len(), Ordering::Relaxed);
            }));
        });
        let net = Net { tx };
        let mut s = State::new(Some(net.clone()), Audio::null());
        let deadline = std::time::Instant::now() + Duration::from_secs(120);
        let mut talked = false;
        while std::time::Instant::now() < deadline {
            if let Ok(e) = ev_rx.recv_timeout(Duration::from_millis(50)) {
                match e {
                    Event::Connected(c) => s.on_connected(c),
                    Event::Message(m) => {
                        eprintln!("← {m:?}");
                        let hello = matches!(m, In::Hello { .. });
                        s.on_message(m);
                        if hello {
                            s.start();
                        }
                    }
                }
            }
            if s.screen == Screen::Conversation && !talked {
                talked = true;
                s.ptt_down();
                assert_eq!(s.phase, Phase::Recording);
                for _ in 0..10 {
                    net.send(Out::Audio(vec![0; 1280]));
                }
                s.ptt_up();
            }
            s.tick(0.0);
            if talked && s.phase == Phase::Idle {
                eprintln!("mensajes: {:?}\naudio: {} bytes", s.messages, bytes.load(Ordering::Relaxed));
                return;
            }
        }
        panic!("el turno no terminó: fase {:?}", s.phase);
    }
}
