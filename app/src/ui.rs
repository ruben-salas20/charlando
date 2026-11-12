//! Las pantallas, con los colores, tamaños y textos de maqueta.html.

use rune::{Align, Color, Cursor, Cx, El, Family, Icon, ScrollSpec, Side, Span, TextStyle, col, effect, el, icon, rich, row, text};

use crate::net::{Folder, Where};
use crate::orb::ORB;
use crate::state::{Model, Permission, Phase, Screen, State, View, Who};

/// El icono de copiar de maqueta.html.
static COPY: Icon = Icon {
    name: "charlando-copiar",
    view: 14.0,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="1.3"><rect x="4.5" y="4.5" width="8" height="8" rx="1.5"/><path d="M9.5 2.5V2A1.5 1.5 0 0 0 8 .5H2A1.5 1.5 0 0 0 .5 2v6A1.5 1.5 0 0 0 2 9.5h.5"/></g>"#,
};

fn c(hex: &str) -> Color {
    rune::color::hex(hex).unwrap_or(Color::BLACK)
}

struct Pal {
    bg: Color,
    l1: Color,
    l2: Color,
    l3: Color,
    line: Color,
    fg: Color,
    fg2: Color,
    fg3: Color,
    accent: Color,
    rec: Color,
    ok: Color,
}

fn pal() -> Pal {
    Pal {
        bg: c("#141414"),
        l1: c("#1c1c1c"),
        l2: c("#232323"),
        l3: c("#2c2c2c"),
        line: Color::WHITE.alpha(0.08),
        fg: c("#ededed"),
        fg2: c("#a3a3a3"),
        fg3: c("#6e6e6e"),
        accent: c("#6f8fff"),
        rec: c("#e5484d"),
        ok: c("#46a758"),
    }
}

fn style(size: f32, weight: f32) -> TextStyle {
    TextStyle { line_height: (size * 1.45).round(), ..TextStyle::new(size, weight) }
}

fn mono(size: f32) -> TextStyle {
    TextStyle { family: Family::Mono, ..style(size, 400.0) }
}


const BODY: f32 = 14.0;

pub fn view(s: &State, _: &mut Cx<State>) -> El {
    let p = pal();
    let body = match s.screen {
        Screen::Start => start(s, &p),
        Screen::Conversation => conversation(s, &p),
    };
    let mut root = col().key("root").w_full().h_full().fill(p.bg).child(title_bar(s, &p)).child(body);
    if s.screen == Screen::Start && s.folder_menu {
        root = root
            .child(el().key("carpetas-fuera").role("none").absolute(0.0, 0.0).w_full().h_full().on_press(|s: &mut State| s.folder_menu = false))
            .child(folder_menu(s, &p));
    }
    // Con el foco en un control, Espacio lo pulsaría: aquí se queda para hablar.
    root.on_key(|s: &mut State, ev, tx| crate::on_key(s, ev, tx)).on_key_up(|s: &mut State, ev, _| crate::on_key_up(s, ev))
}

fn title_bar(s: &State, p: &Pal) -> El {
    let dot_color = if s.connected {
        p.ok
    } else if s.taken {
        p.fg3
    } else {
        p.rec
    };
    let dot = el().key("dot").size(7.0, 7.0).radius(3.5).fill(dot_color);
    let here = s.folder.as_ref().filter(|_| s.screen == Screen::Conversation);
    let label = here.map_or("Charlando".to_owned(), |f| f.name.clone());
    let mut name = row()
        .key("name")
        .shrink()
        .min_w(0.0)
        .items_center()
        .gap(8.0)
        .pr(10.0)
        .child(dot)
        .child(text(label).key("carpeta-actual").font(style(13.0, 600.0)).color(p.fg).truncate());
    if let Some(f) = here {
        name = name.help(f.path.clone());
    }
    // Ancho base 0 en los dos lados: el selector queda centrado aunque el nombre sea largo.
    let left = row().w(0.0).grow(1.0).min_w(0.0).items_center().child(name);
    let mut right = row().w(0.0).grow(1.0).items_center().justify_end().gap(10.0);
    let mut middle = row();
    if s.screen == Screen::Conversation {
        middle = segment(s, p);
        if s.resume_command().is_some() {
            right = right.child(
                row()
                    .key("copiar")
                    .role("button")
                    .label("Copiar el comando para seguir en la terminal")
                    .help("Copiar el comando para seguir en la terminal")
                    .px(8.0)
                    .py(5.0)
                    .radius(5.0)
                    .hover(p.l2)
                    .cursor(Cursor::Pointer)
                    .on_press_tx(|s: &mut State, tx| {
                        if let Some(cmd) = s.resume_command() {
                            tx.copy(cmd);
                        }
                    })
                    .child(icon(&COPY, 14.0).color(p.fg2)),
            );
        }
        right = right.child(
            row()
                .key("plus")
                .role("button")
                .label("Nueva conversación")
                .help("Nueva conversación")
                .px(8.0)
                .py(2.0)
                .radius(5.0)
                .hover(p.l2)
                .cursor(Cursor::Pointer)
                .on_press(|s: &mut State| s.new_conversation())
                .child(text("+").font(TextStyle { line_height: 18.0, ..TextStyle::new(18.0, 400.0) }).color(p.fg2)),
        );
    }
    row()
        .key("title")
        .w_full()
        .h(40.0)
        .pl(14.0)
        .pr(10.0)
        .items_center()
        .border_sides([0.0, 0.0, 1.0, 0.0], p.line)
        .child(left)
        .child(middle)
        .child(right)
}

fn segment(s: &State, p: &Pal) -> El {
    let button = |key: &'static str, label: &'static str, v: View| {
        let on = s.view == v;
        row()
            .key(key)
            .role("radio")
            .label(label)
            .checked(on)
            .px(10.0)
            .py(2.0)
            .radius(5.0)
            .fill(if on { p.l3 } else { Color::TRANSPARENT })
            .cursor(Cursor::Pointer)
            .on_press(move |s: &mut State| s.set_view(v))
            .child(text(label).font(style(12.0, 400.0)).color(if on { p.fg } else { p.fg2 }))
    };
    row()
        .key("vista")
        .role("radiogroup")
        .padding(2.0)
        .radius(6.0)
        .fill(p.l2)
        .child(button("chat", "Chat", View::Chat))
        .child(button("voz", "Voz", View::Voice))
}

fn start(s: &State, p: &Pal) -> El {
    let heading = col()
        .child(text("Charlando").key("h1").font(style(22.0, 600.0)).color(p.fg))
        .child(text("Elige dónde y con quién hablar.").font(style(BODY, 400.0)).color(p.fg2).mt(4.0));
    let mut opts = col().key("modelos").role("radiogroup").gap(6.0);
    for m in Model::ALL {
        let on = s.model == m;
        let radio = el()
            .size(14.0, 14.0)
            .radius(7.0)
            .border(if on { 4.0 } else { 1.5 }, if on { p.accent } else { p.fg3 });
        opts = opts.child(
            row()
                .key(m.id())
                .role("radio")
                .label(m.name())
                .checked(on)
                .items_center()
                .gap(12.0)
                .px(12.0)
                .py(10.0)
                .radius(8.0)
                .fill(if on { p.l2 } else { p.l1 })
                .hover(if on { Color::TRANSPARENT } else { p.l2 })
                .border(1.0, if on { p.accent } else { p.line })
                .cursor(Cursor::Pointer)
                .on_press(move |s: &mut State| s.model = m)
                .child(radio)
                .child(
                    col()
                        .child(text(m.name()).font(style(BODY, 500.0)).color(p.fg))
                        .child(text(m.blurb()).font(style(12.5, 400.0)).color(p.fg3)),
                ),
        );
    }
    let models = col().child(text("Modelo").font(style(12.0, 400.0)).color(p.fg3).mb(8.0)).child(opts);
    let knob = el().key("knob").size(18.0, 18.0).radius(9.0).fill(p.fg).absolute(if s.thinking { 18.0 } else { 2.0 }, 2.0);
    let switch = el()
        .key("razonar-switch")
        .size(38.0, 22.0)
        .radius(11.0)
        .fill(if s.thinking { p.accent } else { p.l3 })
        .child(knob);
    let thinking = row()
        .key("razonar")
        .role("switch")
        .label("Razonar")
        .checked(s.thinking)
        .focusable(true)
        .items_center()
        .gap(12.0)
        .cursor(Cursor::Pointer)
        .on_press(|s: &mut State| s.toggle_thinking())
        .child(
            col()
                .grow(1.0)
                .child(text("Razonar").font(style(BODY, 500.0)).color(p.fg))
                .child(text("Claude piensa cuando lo necesita. Más lento.").font(style(12.5, 400.0)).color(p.fg3)),
        )
        .child(switch);
    let go = if s.taken {
        col()
            .gap(10.0)
            .child(text(TAKEN_START).key("otro").font(style(13.0, 400.0)).color(p.fg2).wrap())
            .child(use_here().w_full().py(10.0))
    } else {
        let ready = s.connected && !s.starting && s.place.is_some();
        light_button("empezar", "Empezar", ready).w_full().py(10.0).on_press(|s: &mut State| s.start())
    };
    col()
        .key("inicio")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .px(32.0)
        .gap(22.0)
        .justify_center()
        .child(heading)
        .child(folder_picker(s, p))
        .child(models)
        .child(thinking)
        .child(go)
}

const TAKEN_START: &str = "Charlando está abierto en otro dispositivo.";
const TAKEN: &str = "Abierto en otro dispositivo";

/// El estilo de «Empezar».
fn light_button(key: &'static str, label: &'static str, enabled: bool) -> El {
    row()
        .key(key)
        .role("button")
        .label(label)
        .center()
        .radius(8.0)
        .fill(c("#ededed"))
        .hover(if enabled { Color::WHITE } else { Color::TRANSPARENT })
        .opacity(if enabled { 1.0 } else { 0.5 })
        .cursor(if enabled { Cursor::Pointer } else { Cursor::Default })
        .child(text(label).font(style(BODY, 600.0)).color(c("#141414")))
}

fn use_here() -> El {
    light_button("usar-aqui", "Usar aquí", true).on_press(|s: &mut State| s.use_here())
}

fn folder_line(f: &Folder, size: f32, weight: f32, color: Color, p: &Pal) -> El {
    let mut line = row().min_w(0.0).grow(1.0).items_center().gap(10.0).child(text(f.name.clone()).font(style(size, weight)).color(color));
    if !f.path.is_empty() {
        line = line.child(text(f.path.clone()).font(mono(11.5)).color(p.fg3).truncate_head());
    }
    line
}

fn folder_picker(s: &State, p: &Pal) -> El {
    let open = s.folder_menu;
    let mut trigger = row()
        .key("carpeta")
        .role("button")
        .label("Carpeta")
        .expanded(open)
        .items_center()
        .gap(10.0)
        .px(12.0)
        .py(10.0)
        .radius(8.0)
        .fill(p.l1)
        .hover(p.l2)
        .border(1.0, p.line)
        .cursor(Cursor::Pointer)
        .on_press(|s: &mut State| s.folder_menu = !s.folder_menu);
    if let Some(f) = s.chosen() {
        trigger = trigger.child(folder_line(&f, BODY, 500.0, p.fg, p));
    } else {
        trigger = trigger.child(text("Sin conexión").font(style(BODY, 500.0)).color(p.fg3).grow(1.0));
    }
    trigger = trigger.child(text(if open { "▴" } else { "▾" }).font(style(BODY, 400.0)).color(p.fg3));
    let mut block = col().child(text("Carpeta").font(style(12.0, 400.0)).color(p.fg3).mb(8.0)).child(trigger);
    if let Some(e) = &s.error {
        block = block.child(text(e.clone()).key("error").font(style(13.0, 400.0)).color(p.rec).wrap().mt(8.0));
    }
    block
}

fn folder_menu(s: &State, p: &Pal) -> El {
    let item = |key: String, on: bool| {
        row()
            .key(key)
            .role("menuitem")
            .items_center()
            .px(10.0)
            .py(7.0)
            .radius(6.0)
            .fill(if on { p.l3 } else { Color::TRANSPARENT })
            .hover(p.l3)
            .cursor(Cursor::Pointer)
    };
    let mut menu = col()
        .key("carpetas")
        .role("menu")
        .padding(4.0)
        .radius(8.0)
        .fill(p.l2)
        .border(1.0, p.line)
        .shadow(12.0, 30.0, Color::BLACK.alpha(0.45));
    for (i, f) in s.folders.iter().enumerate() {
        let on = s.place.as_ref() == Some(&Where::Folder(f.path.clone()));
        let path = f.path.clone();
        menu = menu.child(
            item(format!("carpeta-{i}"), on)
                .label(f.name.clone())
                .on_press(move |s: &mut State| s.choose(Where::Folder(path.clone())))
                .child(folder_line(f, BODY, 400.0, p.fg, p)),
        );
    }
    if !s.folders.is_empty() {
        menu = menu.child(el().h(1.0).mx(6.0).my(4.0).fill(p.line));
    }
    menu = menu.child(
        item("charla-nueva".to_owned(), s.place == Some(Where::New))
            .label("Charla nueva")
            .on_press(|s: &mut State| s.choose(Where::New))
            .child(folder_line(&Folder { name: "+ Charla nueva".to_owned(), path: charlas_path(s) }, BODY, 400.0, p.fg2, p)),
    );
    // De borde a borde del bloque «Carpeta» (32 de margen en «inicio»), debajo del selector.
    menu.pin(Some(32.0), Some(0.0), Some(32.0), None).anchored("carpeta", Side::Bottom, Align::Start, 6.0)
}

/// «en <charlas>», o nada si el servidor aún no lo dijo.
fn charlas_path(s: &State) -> String {
    if s.charlas.is_empty() { String::new() } else { format!("en {}", s.charlas) }
}

/// Unas 8 líneas de código: un `detail` largo se desplaza y los botones siguen a la vista.
const DETAIL_MAX_H: f32 = 160.0;

fn permission_card(perm: &Permission, p: &Pal) -> El {
    let button = |key: &'static str, label: &'static str, fill: Color, hover: Color, color: Color, allow: bool| {
        row()
            .key(key)
            .role("button")
            .label(label)
            .px(14.0)
            .py(6.0)
            .radius(6.0)
            .fill(fill)
            .hover(hover)
            .cursor(Cursor::Pointer)
            .on_press(move |s: &mut State| s.answer_permission(allow))
            .child(text(label).font(style(13.0, 500.0)).color(color))
    };
    col()
        .key("permiso")
        .w_full()
        .padding(12.0)
        .radius(10.0)
        .fill(p.l1)
        .border(1.0, p.accent)
        .child(text("Claude necesita tu permiso").font(style(BODY, 600.0)).color(p.fg).mb(2.0))
        .child(text(perm.action()).font(style(12.0, 400.0)).color(p.fg3).mb(8.0))
        .child(
            col()
                .key("permiso-detalle")
                .w_full()
                .max_h(DETAIL_MAX_H)
                .mb(10.0)
                .radius(6.0)
                .fill(c("#0f0f0f"))
                .border(1.0, p.line)
                .scroll(ScrollSpec { stick_bottom: false })
                .child(col().w_full().px(10.0).py(8.0).child(text(perm.detail.clone()).font(mono(12.0)).color(p.fg).wrap_anywhere())),
        )
        .child(
            row()
                .gap(8.0)
                .justify_end()
                .child(button("rechazar", "Rechazar", p.l3, c("#363636"), p.fg, false))
                .child(button("aceptar", "Aceptar", c("#ededed"), Color::WHITE, c("#141414"), true)),
        )
}

fn phase_label(s: &State) -> &'static str {
    if s.taken {
        return TAKEN;
    }
    if !s.connected {
        return "Sin conexión. Reintentando…";
    }
    match s.phase {
        Phase::Idle => "Mantén Espacio para hablar",
        Phase::Recording => "Grabando… suelta para enviar",
        Phase::Thinking => "Pensando…",
        Phase::Speaking => "Claude habla…",
        Phase::Permission => "Esperando tu permiso",
    }
}

fn big_label(s: &State) -> &'static str {
    if s.taken {
        return TAKEN;
    }
    if !s.connected {
        return "Sin conexión";
    }
    match s.phase {
        Phase::Idle => "Mantén Espacio para hablar",
        Phase::Recording => "Te escucho",
        Phase::Thinking => "Pensando…",
        Phase::Speaking => "Hablando",
        Phase::Permission => "Esperando tu permiso",
    }
}

fn conversation(s: &State, p: &Pal) -> El {
    let main = match s.view {
        View::Chat => chat(s, p),
        View::Voice => voice(s, p),
    };
    col().key("conversacion").w_full().grow(1.0).min_h(0.0).child(main).child(footer(s, p))
}

fn caret(p: &Pal) -> El {
    el().key("caret").size(7.0, 14.0).fill(p.fg2).ml(2.0).mb(3.0)
}

/// El texto de una respuesta con el cursor al final mientras Claude habla. Rune no mete elementos
/// dentro de un párrafo, así que el cursor son dos espacios duros con fondo al final del propio
/// texto: así salta de línea con la última palabra.
fn reply_text(t: &str, streaming: bool, color: Color, size: f32, p: &Pal) -> El {
    if !streaming {
        return text(t.to_owned()).font(style(size, 400.0)).color(color).wrap();
    }
    if t.is_empty() {
        return row().h((size * 1.45).round()).items_center().child(caret(p));
    }
    let start = t.len() + "\u{202F}".len();
    let content = format!("{t}\u{202F}\u{00A0}\u{00A0}");
    let end = content.len();
    rich(content, vec![Span::new(start, end).bg(p.fg2, 0.0)]).font(style(size, 400.0)).color(color).wrap()
}

fn chat(s: &State, p: &Pal) -> El {
    let speaking = s.phase == Phase::Speaking;
    let n = s.messages.len();
    let mut list = col().key("mensajes").w_full().px(16.0).py(18.0).gap(16.0);
    for (i, m) in s.messages.iter().enumerate() {
        let (who, color) = match m.who {
            Who::You => ("Tú", p.fg2),
            Who::Claude => ("Claude", p.fg),
        };
        let streaming = speaking && i == n - 1 && m.who == Who::Claude;
        if m.text.is_empty() && !streaming {
            continue;
        }
        list = list.child(
            col()
                .key(format!("m{i}"))
                .child(text(who).font(style(12.0, 400.0)).color(p.fg3).mb(3.0))
                .child(reply_text(&m.text, streaming, color, BODY, p)),
        );
    }
    if let Some(e) = &s.error {
        list = list.child(text(format!("Error: {e}")).key("error").font(style(13.0, 400.0)).color(p.rec).wrap());
    }
    if let Some(perm) = s.permission.as_ref().filter(|_| s.phase == Phase::Permission) {
        list = list.child(permission_card(perm, p));
    }
    col()
        .key("chat")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec { stick_bottom: true })
        .child(list)
}

/// Alto de una línea a 13.5 pt.
const VOICE_LINE: f32 = 20.0;
/// Renglones visibles de la pregunta (luego «…») y de la respuesta (los últimos).
const QUESTION_LINES: usize = 3;
const REPLY_LINES: f32 = 4.0;
/// Lo que ocupa el desvanecido de arriba de la respuesta.
const REPLY_FADE: f32 = 22.0;
/// Separación entre la pregunta y la respuesta.
const VOICE_GAP: f32 = 8.0;

/// Lado de la bola en Voz, y con la tarjeta de permiso.
const ORB_SIZE: f32 = 160.0;
const ORB_SIZE_PERMISSION: f32 = 110.0;

fn voice(s: &State, p: &Pal) -> El {
    let view = col().key("voz-vista").w_full().grow(1.0).min_h(0.0).padding(24.0).gap(18.0).center();
    if let Some(perm) = s.permission.as_ref().filter(|_| s.phase == Phase::Permission) {
        return view
            .child(effect(ORB).key("bola").size(ORB_SIZE_PERMISSION, ORB_SIZE_PERMISSION))
            .child(text(big_label(s)).key("estado-grande").font(style(17.0, 600.0)).color(p.fg).align(Align::Center))
            .child(permission_card(perm, p));
    }
    if s.taken {
        return view
            .child(effect(ORB).key("bola").size(ORB_SIZE, ORB_SIZE))
            .child(text(big_label(s)).key("estado-grande").font(style(17.0, 600.0)).color(p.fg).align(Align::Center))
            .child(use_here().px(18.0).py(10.0));
    }
    let show_reply = matches!(s.phase, Phase::Speaking | Phase::Idle);
    let mut reply = col().w_full().items_center().pt(REPLY_FADE);
    if show_reply && let Some(r) = s.last_reply() {
        let streaming = s.phase == Phase::Speaking;
        let body = text(r.to_owned()).key("respuesta").font(style(13.5, 400.0)).color(p.fg2).align(Align::Center).wrap();
        reply = reply.child(if streaming && r.is_empty() { caret(p) } else { body });
    }
    if let Some(e) = &s.error {
        reply = reply.child(text(format!("Error: {e}")).key("error").font(style(13.0, 400.0)).color(p.rec).align(Align::Center).wrap());
    }
    // La respuesta sigue su final y se desvanece arriba. El margen superior del contenido cae
    // bajo el desvanecido, así que una respuesta corta se ve entera.
    let reply = col()
        .key("respuesta-visor")
        .w_full()
        .mt(VOICE_GAP)
        .min_h(0.0)
        .max_h(REPLY_LINES * VOICE_LINE + REPLY_FADE)
        .scroll(ScrollSpec { stick_bottom: true })
        .clip_fade_linear(0.0, REPLY_FADE, 0.0, 0.0)
        .child(reply);
    // Alto fijo para que la bola no se mueva con el texto; encoge si la ventana es baja.
    let mut last = col()
        .key("ultima")
        .w_full()
        .max_w(320.0)
        .h(QUESTION_LINES as f32 * VOICE_LINE + REPLY_LINES * VOICE_LINE + REPLY_FADE + VOICE_GAP)
        .min_h(0.0)
        .items_center();
    if let Some(q) = s.last_question() {
        last = last.child(
            text(format!("«{q}»")).key("pregunta").font(style(13.5, 400.0)).color(p.fg3).align(Align::Center).max_lines(QUESTION_LINES),
        );
    }
    view.child(effect(ORB).key("bola").size(ORB_SIZE, ORB_SIZE))
        .child(text(big_label(s)).key("estado-grande").font(style(17.0, 600.0)).color(p.fg).align(Align::Center))
        .child(last.child(reply))
}

fn meter(s: &State, p: &Pal) -> El {
    let rec = s.phase == Phase::Recording;
    let on = s.connected && matches!(s.phase, Phase::Recording | Phase::Speaking);
    let mut m = row().key("medidor").h(20.0).gap(2.0).items_center();
    for i in 0..14 {
        let shape = ((i as f32) * 1.7).sin().abs();
        let h = if on { 4.0 + (shape * 14.0 * s.level).round() } else { 4.0 };
        m = m.child(el().w(3.0).h(h).radius(2.0).fill(if rec { p.rec } else { p.fg3 }));
    }
    m
}

fn footer(s: &State, p: &Pal) -> El {
    let rec = s.phase == Phase::Recording;
    let key = row()
        .key("tecla")
        .opacity(if s.phase == Phase::Permission || s.taken { 0.35 } else { 1.0 })
        .px(8.0)
        .py(3.0)
        .radius(5.0)
        .fill(if rec { p.rec } else { p.l2 })
        .border(1.0, if rec { Color::TRANSPARENT } else { p.line })
        .child(
            text("Espacio")
                .font(mono(11.5))
                .color(if rec { Color::WHITE } else { p.fg2 }),
        );
    let mut bar = row().key("pie").w_full().px(14.0).py(12.0).gap(12.0).items_center().border_sides([1.0, 0.0, 0.0, 0.0], p.line);
    if s.view == View::Voice {
        return bar.justify_center().child(key);
    }
    if s.taken {
        return bar
            .child(text(phase_label(s)).key("estado").font(style(13.0, 400.0)).color(p.fg2).grow(1.0))
            .child(use_here().px(14.0).py(5.0));
    }
    if rec {
        bar = bar.child(el().key("punto-rec").size(8.0, 8.0).radius(4.0).fill(p.rec));
    }
    bar.child(text(phase_label(s)).key("estado").font(style(13.0, 400.0)).color(p.fg2).grow(1.0))
        .child(meter(s, p))
        .child(key)
}
