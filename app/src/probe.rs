//! `charlando --probar [--turno]`: se conecta, envía `start` y espera `ready`. Con `--turno`
//! manda además medio segundo de silencio por pulsar para hablar y espera `reply_end`.
//! Sin ventana ni audio: sirve para comprobar el contrato con el servidor.

use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

use crate::net::{In, parse};

pub fn run(url: String) -> i32 {
    let turn = std::env::args().any(|a| a == "--turno");
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime de tokio");
    match rt.block_on(probe(&url, turn)) {
        Ok(()) => {
            println!("OK");
            0
        }
        Err(e) => {
            println!("FALLO: {e}");
            1
        }
    }
}

async fn probe(url: &str, turn: bool) -> Result<(), String> {
    let t0 = Instant::now();
    let (mut ws, _) = tokio::time::timeout(Duration::from_secs(5), tokio_tungstenite::connect_async(url))
        .await
        .map_err(|_| "tiempo agotado al conectar".to_string())?
        .map_err(|e| e.to_string())?;
    println!("conectado a {url} en {} ms", t0.elapsed().as_millis());
    ws.send(Message::text(r#"{"type":"start","model":"haiku"}"#)).await.map_err(|e| e.to_string())?;
    println!("→ start haiku");
    wait(&mut ws, Duration::from_secs(15), |m| matches!(m, In::Ready(_))).await?;
    if !turn {
        return Ok(());
    }
    ws.send(Message::text(r#"{"type":"ptt_start"}"#)).await.map_err(|e| e.to_string())?;
    for _ in 0..12 {
        ws.send(Message::binary(vec![0u8; 1280])).await.map_err(|e| e.to_string())?;
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    ws.send(Message::text(r#"{"type":"ptt_end"}"#)).await.map_err(|e| e.to_string())?;
    println!("→ ptt_start, 480 ms de silencio a 16 kHz, ptt_end");
    wait(&mut ws, Duration::from_secs(90), |m| matches!(m, In::ReplyEnd | In::Error(_))).await
}

async fn wait<S>(ws: &mut S, limit: Duration, done: impl Fn(&In) -> bool) -> Result<(), String>
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let t0 = Instant::now();
    let mut audio = 0usize;
    loop {
        let left = limit.checked_sub(t0.elapsed()).ok_or("tiempo agotado")?;
        let msg = tokio::time::timeout(left, ws.next()).await.map_err(|_| "tiempo agotado".to_string())?;
        match msg {
            Some(Ok(Message::Text(t))) => {
                println!("← {t}  ({} ms)", t0.elapsed().as_millis());
                match parse(&t) {
                    Some(m) if done(&m) => {
                        if audio > 0 {
                            println!("  audio recibido: {audio} bytes ({:.2} s a 24 kHz)", audio as f64 / 48_000.0);
                        }
                        return if let In::Error(e) = m { Err(format!("el servidor dice: {e}")) } else { Ok(()) };
                    }
                    Some(_) => {}
                    None => return Err(format!("mensaje fuera del protocolo: {t}")),
                }
            }
            Some(Ok(Message::Binary(b))) => audio += b.len(),
            Some(Ok(Message::Close(_))) | None => return Err("el servidor cerró la conexión".into()),
            Some(Ok(_)) => {}
            Some(Err(e)) => return Err(e.to_string()),
        }
    }
}
