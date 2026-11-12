//! La bola de metal líquido: un `El::effect` que este compositor pinta con rune-fx.
//! Los valores de cada estado son los de `target()` en maqueta.html; aquí se suavizan igual
//! (`k = dt · 7`) y la fase de las ondas se integra con `speed`, para que cambiar de estado no
//! dé saltos.

use rune::compose::{Compositor, Frame};
use rune::render::{EffectPhase, EffectSite, KIND_EFFECT};
use rune_fx::{Fx, FxFrame, FxLayer, FxOptions, FxRenderer, FxSlot, LiquidMetalParams, Rgb, Shape, rgb};

use crate::state::{Phase, State};

pub const ORB: u32 = 1;

/// Lo que la maqueta anima de la bola.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub flow: f32,
    pub speed: f32,
    /// Frecuencia de las ondas en radios de la bola (la `scale` del shader de la maqueta).
    pub scale: f32,
    /// Radio respecto al hueco.
    pub size: f32,
    pub angle: f32,
    pub irid: f32,
    pub contrast: f32,
    pub light: Rgb,
}

/// `target(st, now, base, "barrido")` de la maqueta, con `now` en segundos.
pub fn target(phase: Phase, connected: bool, now: f64, level: f32) -> Look {
    let base = LiquidMetalParams::gunmetal();
    let ms = now * 1000.0;
    let b = Look { flow: 0.0, speed: 0.0, scale: 1.6, size: 1.0, angle: 0.0, irid: base.iridescence, contrast: base.contrast, light: base.light };
    if !connected {
        // En rune-fx más contraste no oscurece la cara iluminada como el `pow` de la maqueta;
        // bajar la luz da el «apagada» que pide DISEÑO.md.
        let dim = Rgb(base.light.0.map(|c| c * 0.3));
        return Look { scale: 1.6, size: 0.88, irid: 0.0, contrast: base.contrast * 1.9, light: dim, ..b };
    }
    let lv = level.clamp(0.0, 1.0);
    match phase {
        Phase::Idle | Phase::Permission => Look { flow: 0.08, speed: 0.15, size: 1.0 + 0.025 * (ms / 650.0).sin() as f32, ..b },
        Phase::Recording => Look { flow: 0.25 + 0.75 * lv, speed: -0.9, scale: 2.4, size: 0.97 - 0.05 * lv, light: rgb(0xff5a5a), ..b },
        Phase::Thinking => Look { flow: 0.15, speed: 0.2, size: 0.94, angle: 1.1 * (ms / 700.0).sin() as f32, ..b },
        Phase::Speaking => Look { flow: 0.2 + 0.8 * lv, speed: 1.0, scale: 1.4, size: 0.96 + 0.07 * lv, ..b },
    }
}

fn lerp(a: f32, b: f32, k: f32) -> f32 {
    a + (b - a) * k
}

#[derive(Default)]
pub struct Orb {
    fx: Option<(rune::compose::DeviceKey, FxRenderer)>,
    cur: Option<Look>,
    /// Tiempo de las ondas: avanza `speed` por segundo, y retrocede con `speed` negativa.
    phase_t: f32,
    /// Hubo bola en el último fotograma: mientras la haya, el compositor pide fotogramas.
    shown: bool,
}

impl Orb {
    fn step(&mut self, s: &State, now: f64, dt: f32) -> Look {
        let g = target(s.phase, s.connected, now, s.level);
        let k = (dt * 7.0).min(1.0);
        let cur = match self.cur {
            None => g,
            Some(c) => Look {
                flow: lerp(c.flow, g.flow, k),
                speed: lerp(c.speed, g.speed, k),
                scale: lerp(c.scale, g.scale, k),
                size: lerp(c.size, g.size, k),
                // La maqueta no suaviza el ángulo en el barrido: lo fija cada fotograma.
                angle: if s.connected && s.phase == Phase::Thinking { g.angle } else { lerp(c.angle, g.angle, k) },
                irid: lerp(c.irid, g.irid, k),
                contrast: lerp(c.contrast, g.contrast, k),
                light: Rgb([0, 1, 2].map(|i| lerp(c.light.0[i], g.light.0[i], k))),
            },
        };
        self.cur = Some(cur);
        self.phase_t += dt * cur.speed;
        cur
    }
}

/// La bola en un hueco de `w` pt de lado centrado en (cx, cy).
pub fn layer(look: Look, cx: f32, cy: f32, w: f32, t: f32) -> FxLayer {
    let r = w * 0.5 * look.size;
    let base = LiquidMetalParams::gunmetal();
    let p = LiquidMetalParams {
        flow: look.flow,
        // La maqueta repite las ondas `scale` veces por radio; rune-fx mide su tamaño en pt.
        scale: (w * 0.5 / look.scale * 2.0).max(16.0),
        speed: 1.0,
        angle: base.angle + look.angle,
        iridescence: look.irid,
        contrast: look.contrast,
        light: look.light,
        bevel: (r * 0.16).max(2.0),
        ..base
    };
    // 1000 de margen para que el tiempo no pase por debajo de cero al grabar.
    Fx::fill(p).on(Shape::circle(cx, cy, r)).at(1000.0 + t)
}

impl Compositor<State> for Orb {
    fn animating(&self) -> bool {
        self.shown
    }

    fn render(&mut self, f: &mut Frame<'_, State>) {
        let (device, queue, size) = (f.device, f.queue, f.size);
        let dt = (f.dt as f32).clamp(0.0, 0.05);
        let mut at: Vec<u32> = Vec::new();
        let mut layers: Vec<FxLayer> = Vec::new();
        let mut slots: Vec<FxSlot> = Vec::new();
        let mut look = None;
        for (n, i) in f.rt.instances.iter().enumerate() {
            if i.params[1] != KIND_EFFECT || f.rt.marks.get(n).copied().unwrap_or(0) != ORB {
                continue;
            }
            let look = *look.get_or_insert_with(|| self.step(&f.rt.state, f.rt.t, dt));
            let k = i.params[3];
            let [x, y, w, h] = i.rect.map(|v| v / k);
            at.push(n as u32);
            layers.push(layer(look, x + w * 0.5, y + h * 0.5, w.min(h), self.phase_t));
            slots.push(FxSlot::default().scale(k).clip(i.clip, i.fade));
        }
        self.shown = !layers.is_empty();
        if !self.shown {
            self.cur = None;
        }
        let key = f.device_key();
        if self.fx.as_ref().is_none_or(|(k, _)| *k != key) {
            self.fx = Some((key, FxRenderer::new(device, f.format, FxOptions { dither: true })));
        }
        let (_, fx) = self.fx.as_mut().unwrap();
        fx.set_srgb_blend(f.srgb_blend());
        fx.add_format(device, wgpu::TextureFormat::Rgba16Float);
        fx.prepare_with_slots(device, queue, &FxFrame { viewport: [size.0, size.1], scale: 1.0, layers: &layers, backdrop: None, sources: &[] }, &slots);
        let clear = f.rt.clear_color();
        f.ui.prepare(device, queue, &mut f.rt.text.atlas, &f.rt.instances, size);
        f.ui.draw_with_backdrop(f.encoder, f.target, clear, &mut |phase, s: &EffectSite<'_>| {
            let Some(k) = at.iter().position(|n| *n == s.index) else { return };
            if let EffectPhase::Draw(pass) = phase {
                fx.draw_layer_in(pass, s.format, s.origin, k);
            }
        });
    }
}
