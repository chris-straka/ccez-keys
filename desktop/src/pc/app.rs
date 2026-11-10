//! The preview window on Linux and Windows: borderless, on top, never taking
//! the keyboard (an override-redirect window on X11; WS_EX_NOACTIVATE on
//! Windows), placed clear of the field being typed in, fading in and out.
//! The word card opens inside it, under the lines.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use eframe::egui::{self, Color32, CursorIcon, FontData, FontDefinitions, FontFamily, Label, RichText, Sense, Vec2, ViewportCommand};

use ccez_keys_core::text::sentence_of;

use crate::model::{Place, Row};
use crate::pc::{Access, copy, log, say};
use crate::prefs::Prefs;
use crate::preview::{Inputs, PENDING, Preview};
use crate::worker::{Translator, Worker};

/// Apps where nothing shows: terminals, code editors, password managers
/// (matched against the app's id or name, lowercase).
const QUIET: &[&str] = &[
    "terminal", "konsole", "alacritty", "kitty", "wezterm", "xterm", "tilix", "ghostty", "windowsterminal", "cmd.exe", "powershell", "code",
    "zed", "jetbrains", "keepass", "1password", "bitwarden", "seahorse",
];

const TICK: Duration = Duration::from_millis(120);
const BG: Color32 = Color32::from_rgba_premultiplied(30, 30, 33, 248);

struct Card {
    lang: String,
    key: String,
    text: String,
    head: String,
    sub: String,
    meaning: String,
    status: u8,
}

struct App {
    engine: Arc<ccez_keys_core::Engine>,
    worker: Worker,
    langs: Vec<String>,
    flags: HashMap<String, String>,
    prefs: Prefs,
    prefs_seen: Option<SystemTime>,
    preview: Preview,
    access: Box<dyn Access>,
    card: Option<Card>,
    /// The window is up (maybe fading out), and whether it's meant to be.
    up: bool,
    wanted: bool,
    size: Vec2,
    ticked: Instant,
    pictures: crate::pc::flags::Flags,
    #[cfg(windows)]
    hwnd: Option<isize>,
}

fn flags() -> HashMap<String, String> {
    let v: serde_json::Value = serde_json::from_str(include_str!("../../../data/languages.json")).unwrap_or_default();
    v["languages"].as_array().into_iter().flatten().filter_map(|l| Some((l[0].as_str()?.to_string(), l[3].as_str()?.to_string()))).collect()
}

/// Chinese, Japanese and Korean (and Arabic) from the system's own fonts:
/// egui's built-in ones have only Latin.
fn fonts(ctx: &egui::Context) {
    let candidates: &[&str] = if cfg!(windows) {
        &[r"C:\Windows\Fonts\YuGothM.ttc", r"C:\Windows\Fonts\msyh.ttc", r"C:\Windows\Fonts\malgun.ttf", r"C:\Windows\Fonts\segoeui.ttf"]
    } else {
        &[
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf",
        ]
    };
    let mut defs = FontDefinitions::default();
    for (i, path) in candidates.iter().enumerate() {
        if let Ok(bytes) = std::fs::read(path) {
            let name = format!("system-{i}");
            defs.font_data.insert(name.clone(), Arc::new(FontData::from_owned(bytes)));
            defs.families.entry(FontFamily::Proportional).or_default().push(name);
        }
    }
    ctx.set_fonts(defs);
}

/// Where the preview goes (top-left screen points): under the box being typed
/// in when it's small (a message box), else under the cursor's line; over it
/// near the screen's foot; at the foot of the screen with nothing to go by.
pub fn spot(size: Vec2, place: Place, screen: Vec2) -> (f32, f32) {
    let (w, h) = (size.x, size.y);
    let anchor = match (place.frame, place.caret) {
        (Some(f), _) if f.h <= 220.0 => Some(f),
        (_, Some(c)) => Some(c),
        (f, None) => f,
    };
    let Some(a) = anchor else {
        return (((screen.x - w) / 2.0).max(4.0), screen.y - h - 48.0);
    };
    let (top, bottom) = (a.y as f32, (a.y + a.h) as f32);
    let y = if bottom + 6.0 + h <= screen.y - 4.0 {
        bottom + 6.0
    } else if top - 6.0 - h >= 4.0 {
        top - 6.0 - h
    } else {
        screen.y - h - 4.0
    };
    let x0 = place.caret.map(|c| c.x as f32 - 6.0).unwrap_or(a.x as f32);
    (x0.min(screen.x - w - 4.0).max(4.0), y)
}

impl App {
    fn quiet(&self, front: &Option<(String, String)>) -> bool {
        let Some((id, name)) = front else { return false };
        let (id, name) = (id.to_lowercase(), name.to_lowercase());
        let hit = |s: &String| s == &id || s == &name;
        self.prefs.off.iter().any(hit) || (QUIET.iter().any(|q| id.contains(q) || name.contains(q)) && !self.prefs.on.iter().any(hit))
    }

    /// The settings file changed (the settings window, another copy): read again.
    fn reload_prefs(&mut self) {
        let file = crate::prefs::path(&crate::paths::data());
        let seen = std::fs::metadata(&file).and_then(|m| m.modified()).ok();
        if seen != self.prefs_seen {
            self.prefs_seen = seen;
            self.prefs = Prefs::load(&crate::paths::data());
            self.preview.redo();
        }
    }

    fn tick(&mut self, ctx: &egui::Context) {
        self.ticked = Instant::now();
        self.reload_prefs();
        let front = self.access.front_app();
        let off = self.prefs.paused || self.quiet(&front);
        let field = if off { None } else { self.access.focused() };
        let answer = self.worker.out.lock().unwrap().take();
        let under_mouse = self.up && ctx.input(|i| i.pointer.hover_pos().is_some());
        let rows_for = |s: &crate::worker::Shown| crate::cards::rows(s, &self.prefs, &self.flags);
        let inputs = Inputs { now: Instant::now(), off, field, held: false, under_mouse, shown: self.prefs.shown, langs: &self.langs };
        let debug = std::env::var_os("CCEZ_DEBUG").is_some();
        if debug {
            eprintln!("tick front={front:?} off={off} field={:?} answer={}", inputs.field.as_ref().map(|f| (f.secure, f.before.clone(), f.caret, f.frame)), answer.is_some());
        }
        let out = self.preview.tick(inputs, answer, &rows_for);
        if debug {
            eprintln!("  -> show={} ask={:?} line={}", out.show, out.ask.as_ref().map(|a| &a.text), self.preview.line.len());
        }
        if out.close_card {
            self.card = None;
        }
        if let Some(ask) = out.ask {
            self.worker.ask(ask);
        }
        if out.show && !self.up {
            self.place(ctx);
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
            self.up = true;
        }
        self.wanted = out.show;
        if !out.show {
            self.card = None;
        }
    }

    fn place(&self, ctx: &egui::Context) {
        let screen = ctx.input(|i| i.viewport().monitor_size).unwrap_or(Vec2::new(1920.0, 1080.0));
        let ppp = ctx.input(|i| i.viewport().native_pixels_per_point).unwrap_or(1.0);
        // The accessibility APIs give pixels; egui wants points.
        let mut p = self.preview.place;
        for r in [&mut p.caret, &mut p.frame].into_iter().flatten() {
            *r = crate::model::Rect { x: r.x / ppp as f64, y: r.y / ppp as f64, w: r.w / ppp as f64, h: r.h / ppp as f64 };
        }
        let (x, y) = spot(self.size, p, screen);
        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(egui::pos2(x, y)));
    }

    /// A line clicked (its flag): said and copied.
    fn line(&mut self, row: &Row) {
        let text: String = row.toks.iter().map(|t| t.text.as_str()).collect::<String>().trim().to_string();
        say(&row.lang, &text);
        copy(&text);
        self.preview.touch(Instant::now());
    }

    /// A word clicked: said, and its card under the lines.
    fn word(&mut self, row: &Row, ti: usize) {
        let mut tok = row.toks[ti].clone();
        // Japanese and Chinese: a piece clicked is its whole word.
        if tok.key != tok.text && !tok.key.is_empty() && (row.lang.starts_with("ja") || row.lang.starts_with("zh")) {
            let (mut a, mut b) = (ti, ti);
            while a > 0 && row.toks[a - 1].key == tok.key {
                a -= 1;
            }
            while b + 1 < row.toks.len() && row.toks[b + 1].key == tok.key {
                b += 1;
            }
            let reading = row.toks[a..=b].iter().map(|t| if t.reading.is_empty() { t.text.clone() } else { t.reading.clone() }).collect::<String>();
            tok = crate::model::Tok { text: tok.key.clone(), reading, ..tok };
        }
        say(&row.lang, &tok.text);
        let card = self.engine.word_card(row.lang.clone(), tok.text.clone(), if tok.key != tok.text { tok.key.clone() } else { String::new() });
        let (head, sub, meaning) = crate::cards::card_face(&self.engine, &row.lang, &tok, card.as_ref());
        let key = card.as_ref().map(|c| if c.lemma.is_empty() { tok.text.clone() } else { c.lemma.clone() }).unwrap_or_else(|| tok.key.clone());
        let status = match self.engine.word_status(row.lang.clone(), key.clone()) {
            ccez_keys_core::WordStatus::Learning => 1,
            ccez_keys_core::WordStatus::Known => 2,
            _ => 0,
        };
        self.card = Some(Card { lang: row.lang.clone(), key, text: tok.text.clone(), head, sub, meaning, status });
        self.preview.touch(Instant::now());
    }

    fn set_status(&mut self, want: ccez_keys_core::WordStatus) {
        let Some(c) = self.card.take() else { return };
        let now = self.engine.word_status(c.lang.clone(), c.key.clone());
        let next = if now == want { ccez_keys_core::WordStatus::New } else { want };
        self.engine.set_word_status(c.lang.clone(), c.key.clone(), next);
        let e = self.engine.clone();
        std::thread::spawn(move || {
            let _ = e.sync_words("ccez-keys-desktop".into());
        });
        self.preview.redo();
    }

    /// A line right-clicked: it replaces the sentence typed.
    fn swap(&mut self, row: &Row) {
        if row.toks.iter().any(|t| t.key == PENDING) {
            return;
        }
        let text: String = row.toks.iter().map(|t| t.text.as_str()).collect::<String>().trim().to_string();
        let Some(f) = self.access.focused().filter(|f| !f.secure) else { return };
        let sentence = sentence_of(f.before.clone());
        let p = &self.preview;
        if sentence.is_empty() || p.sentence != sentence || p.line_for != sentence || !p.complete || ccez_keys_core::text::looks_secret(f.before.clone()) {
            return;
        }
        let trimmed = f.before.trim_end();
        let trailing = f.before[trimmed.len()..].to_string();
        match self.access.swap(&sentence, &trailing, &text) {
            Ok(how) => {
                log("info", &format!("swapped a sentence for {} ({how})", row.lang));
                self.preview.swapped_in(text);
                self.card = None;
            }
            Err(e) => log("info", &format!("swap skipped: {e}")),
        }
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        let rows = self.preview.line.clone();
        let mut clicked: Option<(usize, Option<usize>, bool)> = None;
        egui::Frame::new().fill(BG).corner_radius(9.0).inner_margin(egui::Margin::symmetric(12, 7)).show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(0.0, 4.0);
            // Lines never wrap: the window grows to them.
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            for (ri, r) in rows.iter().enumerate() {
                let cjk = r.toks.iter().any(|t| !t.reading.is_empty());
                ui.horizontal(|ui| {
                    let flag = match self.pictures.get(ui.ctx(), &r.lang) {
                        Some(t) => ui.add(egui::Image::new((t.id(), Vec2::new(18.0, 18.0))).tint(Color32::from_white_alpha(150)).sense(Sense::click())),
                        None => ui.add(Label::new(RichText::new(&r.lang).size(11.0).color(Color32::from_rgb(140, 140, 148))).sense(Sense::click())),
                    };
                    if flag.clicked() {
                        clicked = Some((ri, None, false));
                    }
                    if flag.secondary_clicked() {
                        clicked = Some((ri, None, true));
                    }
                    ui.add_space(6.0);
                    for (ti, t) in r.toks.iter().enumerate() {
                        if !t.word && t.text.trim().is_empty() {
                            ui.add_space(4.0);
                            continue;
                        }
                        let colour = match t.status {
                            1 => Color32::from_rgb(250, 140, 115),
                            2 => Color32::from_rgb(140, 140, 148),
                            _ if t.key == PENDING => Color32::from_rgb(140, 140, 148),
                            _ => Color32::from_rgb(242, 242, 242),
                        };
                        let size = if cjk { 16.0 } else { 14.0 };
                        let resp = ui
                            .vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                if cjk {
                                    ui.label(RichText::new(if t.reading.is_empty() { " " } else { &t.reading }).size(10.0).color(Color32::from_rgb(185, 185, 190)));
                                }
                                ui.add(Label::new(RichText::new(&t.text).size(size).color(colour)).sense(Sense::click()))
                            })
                            .inner;
                        if t.word {
                            let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
                            if resp.clicked() {
                                clicked = Some((ri, Some(ti), false));
                            }
                            if resp.secondary_clicked() {
                                clicked = Some((ri, Some(ti), true));
                            }
                        } else if resp.secondary_clicked() {
                            clicked = Some((ri, None, true));
                        } else if resp.clicked() {
                            clicked = Some((ri, None, false));
                        }
                    }
                });
            }
            let mut action: Option<u8> = None;
            if let Some(c) = &self.card {
                // At most 300 wide: the window grows to its content, so nothing here may stretch.
                ui.scope(|ui| {
                    ui.set_max_width(300.0);
                    ui.add_space(4.0);
                    ui.add(egui::Separator::default().horizontal().shrink(0.0));
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&c.head).size(20.0).strong().color(Color32::WHITE));
                        ui.add_space(14.0);
                        if ui.add(Label::new(RichText::new("✕").size(13.0).color(Color32::from_rgb(185, 185, 190))).sense(Sense::click())).on_hover_cursor(CursorIcon::PointingHand).clicked() {
                            action = Some(0);
                        }
                    });
                    if !c.sub.is_empty() {
                        ui.label(RichText::new(&c.sub).size(11.5).color(Color32::from_rgb(185, 185, 190)));
                    }
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    ui.add(Label::new(RichText::new(&c.meaning).size(13.0).color(Color32::from_rgb(218, 218, 222))).wrap());
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 18.0;
                        let lit = |on: bool| if on { Color32::from_rgb(250, 140, 115) } else { Color32::from_rgb(242, 242, 242) };
                        for (label, on, n) in [("Learning", c.status == 1, 1u8), ("Known", c.status == 2, 2), ("Copy", false, 3)] {
                            if ui.add(Label::new(RichText::new(label).size(13.0).color(lit(on))).sense(Sense::click())).on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                action = Some(n);
                            }
                        }
                    });
                });
            }
            match action {
                Some(0) => self.card = None,
                Some(1) => self.set_status(ccez_keys_core::WordStatus::Learning),
                Some(2) => self.set_status(ccez_keys_core::WordStatus::Known),
                Some(3) => {
                    if let Some(c) = &self.card {
                        copy(&c.text);
                    }
                }
                _ => {}
            }
        });
        if let Some((ri, ti, right)) = clicked
            && let Some(r) = rows.get(ri)
        {
            match (ti, right) {
                (_, true) => self.swap(r),
                (Some(ti), false) => self.word(r, ti),
                (None, false) => self.line(r),
            }
        }
    }
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(windows)]
        if let Some(h) = self.hwnd.take() {
            crate::pc::windows::no_activate(h);
        }
        if self.ticked.elapsed() >= TICK {
            self.tick(ctx);
        }
        ctx.request_repaint_after(TICK);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Fades in and out; off screen once faded out.
        let alpha = ctx.animate_bool_with_time(egui::Id::new("preview"), self.wanted, if self.wanted { 0.18 } else { 0.3 });
        if !self.wanted && alpha == 0.0 && self.up {
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
            self.up = false;
            return;
        }
        ui.set_opacity(alpha);
        let before = ui.cursor().min;
        self.draw(ui);
        let size = ui.min_rect().max - before + Vec2::splat(2.0);
        if (size - self.size).length() > 1.0 {
            self.size = size;
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(size));
            self.place(&ctx);
        }
    }
}

pub fn run() {
    let dir = crate::paths::data();
    let engine = ccez_keys_core::Engine::new(dir.to_string_lossy().into_owned());
    let langs = engine.setting_strings("languages.learning".into());
    crate::learn::start(engine.clone(), dir.clone(), log);
    let models = dir.join("models");
    let start: crate::worker::Start = Box::new(move || Some(Box::new(crate::firefox::Firefox::new(&models)) as Box<dyn Translator>));
    let worker = Worker::start(engine.clone(), start, log);
    let Some(access) = crate::pc::sys::Access::start() else {
        log("error", "no accessibility: the preview can't read what's typed");
        eprintln!("ccez-keys: the system's accessibility isn't on (Linux: AT-SPI; Windows: UI Automation)");
        return;
    };
    log("info", "start");
    let viewport = egui::ViewportBuilder::default()
        .with_title("ccez-keys")
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top()
        .with_taskbar(false)
        .with_resizable(false)
        .with_active(false)
        .with_visible(false)
        .with_has_shadow(true)
        .with_inner_size([320.0, 80.0])
        // X11: a window the window manager doesn't manage, so it never takes the keyboard.
        .with_override_redirect(true);
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    let _ = eframe::run_native(
        "ccez-keys",
        options,
        Box::new(move |cc| {
            fonts(&cc.egui_ctx);
            #[cfg(windows)]
            let hwnd = {
                use raw_window_handle::{HasWindowHandle, RawWindowHandle};
                cc.window_handle().ok().and_then(|h| match h.as_raw() {
                    RawWindowHandle::Win32(w) => Some(w.hwnd.get()),
                    _ => None,
                })
            };
            Ok(Box::new(App {
                engine,
                worker,
                langs,
                flags: flags(),
                prefs: Prefs::load(&crate::paths::data()),
                prefs_seen: None,
                preview: Preview::new(Instant::now()),
                access: Box::new(access),
                card: None,
                up: false,
                wanted: false,
                size: Vec2::new(320.0, 80.0),
                ticked: Instant::now() - TICK,
                pictures: Default::default(),
                #[cfg(windows)]
                hwnd,
            }))
        }),
    );
}
