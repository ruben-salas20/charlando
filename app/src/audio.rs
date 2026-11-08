//! Micrófono y altavoz con cpal: captura a 16 kHz para el servidor, reproduce lo que llega a
//! 24 kHz y mide el nivel RMS de ambos para la bola y el medidor.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};

use crate::net::{Net, Out};

pub const SEND_RATE: u32 = 16_000;
pub const REPLY_RATE: u32 = 24_000;
/// 40 ms a 16 kHz: dentro de los 20–100 ms que pide el protocolo.
const CHUNK: usize = 640;

/// Remuestreo lineal con estado entre bloques. Al bajar de frecuencia promedia antes, para que
/// lo que pasa de la nueva mitad de banda no se pliegue tanto sobre la voz.
pub struct Resampler {
    step: f64,
    pos: f64,
    prev: f32,
    taps: usize,
    hist: VecDeque<f32>,
    sum: f32,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Resampler {
        let step = from as f64 / to as f64;
        let taps = if step > 1.0 { step.round() as usize } else { 1 };
        Resampler { step, pos: 0.0, prev: 0.0, taps, hist: VecDeque::new(), sum: 0.0 }
    }

    fn smooth(&mut self, x: f32) -> f32 {
        if self.taps <= 1 {
            return x;
        }
        self.hist.push_back(x);
        self.sum += x;
        if self.hist.len() > self.taps {
            self.sum -= self.hist.pop_front().unwrap_or(0.0);
        }
        self.sum / self.hist.len() as f32
    }

    pub fn process(&mut self, input: &[f32], out: &mut impl Extend<f32>) {
        if input.is_empty() {
            return;
        }
        let smoothed: Vec<f32> = input.iter().map(|&x| self.smooth(x)).collect();
        let n = smoothed.len();
        // Índice 0 es la última muestra del bloque anterior; 1..=n, las de este.
        let at = |i: usize| if i == 0 { self.prev } else { smoothed[i - 1] };
        let mut produced = Vec::with_capacity((n as f64 / self.step) as usize + 2);
        while self.pos + 1.0 <= n as f64 {
            let i = self.pos.floor() as usize;
            let f = (self.pos - i as f64) as f32;
            produced.push(at(i) * (1.0 - f) + at(i + 1) * f);
            self.pos += self.step;
        }
        self.pos -= n as f64;
        self.prev = smoothed[n - 1];
        out.extend(produced);
    }
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// RMS a 0..1: -50 dBFS es 0 y -10 dBFS es 1, el rango de una voz normal al micrófono.
pub fn meter(rms: f32) -> f32 {
    let db = 20.0 * rms.max(1e-6).log10();
    ((db + 50.0) / 40.0).clamp(0.0, 1.0)
}

/// Sube rápido y baja despacio, como un vúmetro.
fn publish_level(cell: &AtomicU32, value: f32, block_secs: f32) {
    let old = f32::from_bits(cell.load(Ordering::Relaxed));
    let fall = (-block_secs / 0.25).exp();
    let next = if value > old { old + (value - old) * 0.6 } else { value + (old - value) * fall };
    cell.store(next.to_bits(), Ordering::Relaxed);
}

#[derive(Default)]
struct MicShared {
    recording: bool,
    net: Option<Net>,
    buf: Vec<i16>,
}

impl MicShared {
    fn flush(&mut self) {
        if self.buf.is_empty() {
            return;
        }
        let bytes: Vec<u8> = self.buf.iter().flat_map(|s| s.to_le_bytes()).collect();
        self.buf.clear();
        if let Some(net) = &self.net {
            net.send(Out::Audio(bytes));
        }
    }
}

#[derive(Clone, Default)]
pub struct Mic {
    shared: Arc<Mutex<MicShared>>,
    level: Arc<AtomicU32>,
}

impl Mic {
    /// Envía `ptt_start` y empieza a mandar audio, con el cerrojo tomado para que ningún trozo
    /// se adelante al mensaje.
    pub fn begin(&self, net: &Net, start: Out) {
        let mut m = self.shared.lock().unwrap();
        net.send(start);
        m.net = Some(net.clone());
        m.buf.clear();
        m.recording = true;
    }

    /// Manda lo que quede en el búfer y después `ptt_end`.
    pub fn end(&self, net: &Net, end: Out) {
        let mut m = self.shared.lock().unwrap();
        m.recording = false;
        m.flush();
        net.send(end);
    }

    pub fn cancel(&self) {
        let mut m = self.shared.lock().unwrap();
        m.recording = false;
        m.buf.clear();
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    /// Lo que hace el callback de captura con cada bloque mono a 16 kHz.
    fn feed(&self, mono16k: &[f32]) {
        let mut m = self.shared.lock().unwrap();
        if !m.recording {
            return;
        }
        for &s in mono16k {
            m.buf.push((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
            if m.buf.len() >= CHUNK {
                m.flush();
            }
        }
    }
}

struct PlayerShared {
    active: bool,
    queue: VecDeque<f32>,
    resampler: Resampler,
    /// Un byte suelto si un trozo binario cortó una muestra por la mitad.
    odd: Option<u8>,
}

#[derive(Clone)]
pub struct Player {
    shared: Arc<Mutex<PlayerShared>>,
    level: Arc<AtomicU32>,
}

impl Player {
    fn new(active: bool, out_rate: u32) -> Player {
        Player {
            shared: Arc::new(Mutex::new(PlayerShared {
                active,
                queue: VecDeque::new(),
                resampler: Resampler::new(REPLY_RATE, out_rate),
                odd: None,
            })),
            level: Arc::default(),
        }
    }

    /// PCM 16 bits LE mono a 24 kHz, en trozos de cualquier tamaño.
    pub fn push_pcm16(&self, bytes: &[u8]) {
        let mut p = self.shared.lock().unwrap();
        if !p.active {
            return;
        }
        let mut data = Vec::with_capacity(bytes.len() + 1);
        data.extend(p.odd.take());
        data.extend_from_slice(bytes);
        if data.len() % 2 == 1 {
            p.odd = data.pop();
        }
        let samples: Vec<f32> = data.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
        let PlayerShared { resampler, queue, .. } = &mut *p;
        resampler.process(&samples, queue);
    }

    pub fn clear(&self) {
        let mut p = self.shared.lock().unwrap();
        p.queue.clear();
        p.odd = None;
        self.level.store(0, Ordering::Relaxed);
    }

    pub fn is_empty(&self) -> bool {
        self.shared.lock().unwrap().queue.is_empty()
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    fn pull(&self, frames: usize, rate: u32, out: &mut Vec<f32>) {
        out.clear();
        let mut p = self.shared.lock().unwrap();
        let n = frames.min(p.queue.len());
        out.extend(p.queue.drain(..n));
        drop(p);
        let level = if out.is_empty() { 0.0 } else { meter(rms(out)) };
        publish_level(&self.level, level, frames as f32 / rate as f32);
        out.resize(frames, 0.0);
    }
}

pub struct Audio {
    pub mic: Mic,
    pub player: Player,
}

impl Audio {
    /// Sin dispositivos: para las capturas sin ventana y los tests.
    pub fn null() -> Audio {
        Audio { mic: Mic::default(), player: Player::new(false, REPLY_RATE) }
    }

    /// Abre el micrófono y la salida por defecto. Los streams de cpal no son `Send`, así que
    /// viven en un hilo propio que no termina nunca.
    pub fn start() -> Audio {
        let mic = Mic::default();
        let (tx, rx) = std::sync::mpsc::channel();
        let mic_thread = mic.clone();
        std::thread::Builder::new()
            .name("charlando-audio".into())
            .spawn(move || {
                let host = cpal::default_host();
                let input = open_input(&host, mic_thread).map_err(|e| eprintln!("charlando: micrófono: {e}")).ok();
                let (player, output) = match open_output(&host) {
                    Ok((p, s)) => (p, Some(s)),
                    Err(e) => {
                        eprintln!("charlando: altavoz: {e}");
                        (Player::new(false, REPLY_RATE), None)
                    }
                };
                let _ = tx.send(player);
                let _keep = (input, output);
                loop {
                    std::thread::park();
                }
            })
            .expect("hilo de audio");
        let player = rx.recv().unwrap_or_else(|_| Player::new(false, REPLY_RATE));
        Audio { mic, player }
    }
}

fn open_input(host: &cpal::Host, mic: Mic) -> Result<cpal::Stream, String> {
    let device = host.default_input_device().ok_or("no hay micrófono por defecto")?;
    let supported = device.default_input_config().map_err(|e| e.to_string())?;
    let config = supported.config();
    eprintln!("charlando: micrófono a {} Hz, {} canales, {:?}", config.sample_rate, config.channels, supported.sample_format());
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build_input::<f32>(&device, config, mic),
        cpal::SampleFormat::I16 => build_input::<i16>(&device, config, mic),
        cpal::SampleFormat::I32 => build_input::<i32>(&device, config, mic),
        cpal::SampleFormat::U16 => build_input::<u16>(&device, config, mic),
        other => return Err(format!("formato de muestra no soportado: {other:?}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

fn build_input<T>(device: &cpal::Device, config: cpal::StreamConfig, mic: Mic) -> Result<cpal::Stream, String>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = config.channels.max(1) as usize;
    let rate = config.sample_rate;
    let mut resampler = Resampler::new(rate, SEND_RATE);
    let mut mono = Vec::new();
    let mut out = Vec::new();
    device
        .build_input_stream::<T, _, _>(
            config,
            move |data: &[T], _| {
                mono.clear();
                mono.extend(data.chunks(channels).map(|f| f.iter().map(|s| s.to_sample::<f32>()).sum::<f32>() / channels as f32));
                publish_level(&mic.level, meter(rms(&mono)), mono.len() as f32 / rate as f32);
                out.clear();
                resampler.process(&mono, &mut out);
                mic.feed(&out);
            },
            |e| eprintln!("charlando: error del micrófono: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

fn open_output(host: &cpal::Host) -> Result<(Player, cpal::Stream), String> {
    let device = host.default_output_device().ok_or("no hay salida de audio por defecto")?;
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let config = supported.config();
    eprintln!("charlando: altavoz a {} Hz, {} canales, {:?}", config.sample_rate, config.channels, supported.sample_format());
    let player = Player::new(true, config.sample_rate);
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build_output::<f32>(&device, config, player.clone()),
        cpal::SampleFormat::I16 => build_output::<i16>(&device, config, player.clone()),
        cpal::SampleFormat::I32 => build_output::<i32>(&device, config, player.clone()),
        cpal::SampleFormat::U16 => build_output::<u16>(&device, config, player.clone()),
        other => return Err(format!("formato de muestra no soportado: {other:?}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((player, stream))
}

fn build_output<T>(device: &cpal::Device, config: cpal::StreamConfig, player: Player) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels.max(1) as usize;
    let rate = config.sample_rate;
    let mut mono = Vec::new();
    device
        .build_output_stream::<T, _, _>(
            config,
            move |data: &mut [T], _| {
                player.pull(data.len() / channels, rate, &mut mono);
                for (frame, s) in data.chunks_mut(channels).zip(&mono) {
                    frame.fill(T::from_sample(*s));
                }
            },
            |e| eprintln!("charlando: error del altavoz: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampling_keeps_the_duration_across_blocks() {
        let mut r = Resampler::new(48_000, 16_000);
        let mut out = Vec::new();
        for _ in 0..100 {
            r.process(&[0.5; 480], &mut out);
        }
        assert!((out.len() as i64 - 16_000).abs() <= 1, "{}", out.len());
        assert!((out[out.len() - 1] - 0.5).abs() < 1e-4);

        let mut up = Resampler::new(24_000, 48_000);
        let mut out = Vec::new();
        for _ in 0..10 {
            up.process(&[0.25; 2400], &mut out);
        }
        assert!((out.len() as i64 - 48_000).abs() <= 2, "{}", out.len());
    }

    #[test]
    fn a_sample_split_between_chunks_is_kept() {
        let p = Player::new(true, REPLY_RATE);
        let bytes = 1000i16.to_le_bytes();
        p.push_pcm16(&[bytes[0]]);
        p.push_pcm16(&[bytes[1], 0, 0]);
        let q: Vec<f32> = p.shared.lock().unwrap().queue.iter().copied().collect();
        assert_eq!(q.len(), 2);
        assert!((q[1] - 1000.0 / 32768.0).abs() < 1e-3 || (q[0] - 1000.0 / 32768.0).abs() < 1e-3, "{q:?}");
        assert!(!p.is_empty());
        let mut out = Vec::new();
        p.pull(4, REPLY_RATE, &mut out);
        assert!(p.is_empty());
        assert_eq!(out.len(), 4);
    }

    #[test]
    fn meter_maps_speech_levels() {
        assert_eq!(meter(0.0), 0.0);
        assert_eq!(meter(1.0), 1.0);
        let m = meter(0.03);
        assert!(m > 0.3 && m < 0.9, "{m}");
    }
}
