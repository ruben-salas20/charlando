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

/// De la app al servidor.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    Start(&'static str, bool),
    /// Reanuda la conversación con ese id tras perder la conexión.
    Resume(&'static str, bool, String),
    PttStart,
    PttEnd,
    /// PCM 16 bits LE, mono, 16 kHz.
    Audio(Vec<u8>),
}

impl Out {
    fn into_message(self) -> Message {
        match self {
            Out::Start(model, thinking) => {
                Message::text(json!({"type": "start", "model": model, "thinking": thinking}).to_string())
            }
            Out::Resume(model, thinking, session) => Message::text(
                json!({"type": "start", "model": model, "thinking": thinking, "resume": session}).to_string(),
            ),
            Out::PttStart => Message::text(json!({"type": "ptt_start"}).to_string()),
            Out::PttEnd => Message::text(json!({"type": "ptt_end"}).to_string()),
            Out::Audio(pcm) => Message::binary(pcm),
        }
    }
}

/// Del servidor a la app (los mensajes de texto; el audio va directo al reproductor).
#[derive(Debug, Clone, PartialEq)]
pub enum In {
    /// El id de la conversación, para reanudarla.
    Ready(String),
    State(String),
    Transcript(String),
    Sentence(String),
    ReplyEnd,
    Error(String),
    /// La conversación que se quería reanudar ya no existe.
    SessionGone,
}

pub fn parse(text: &str) -> Option<In> {
    let v: Value = serde_json::from_str(text).ok()?;
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
    Some(match v.get("type")?.as_str()? {
        "ready" => In::Ready(s("session")),
        "state" => In::State(s("state")),
        "transcript" => In::Transcript(s("text")),
        "sentence" => In::Sentence(s("text")),
        "reply_end" => In::ReplyEnd,
        "error" if s("code") == "session_gone" => In::SessionGone,
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
        assert_eq!(parse(r#"{"type":"ready","model":"haiku","session":"s1"}"#), Some(In::Ready("s1".into())));
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
        assert_eq!(text(Out::Start("opus", true)), json!({"type":"start","model":"opus","thinking":true}));
        assert_eq!(
            text(Out::Resume("opus", false, "s1".into())),
            json!({"type":"start","model":"opus","thinking":false,"resume":"s1"})
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
                    Event::Connected(c) => {
                        s.on_connected(c);
                        if c {
                            s.start();
                        }
                    }
                    Event::Message(m) => {
                        eprintln!("← {m:?}");
                        s.on_message(m);
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
