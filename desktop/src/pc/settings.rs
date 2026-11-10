//! The settings window (`ccez-keys-desktop --settings`): pause, how many
//! languages, readings, open at login. Kept in desktop.json; the running
//! preview reads it again when it changes. Opened from the app list on Linux
//! (GNOME has no tray) and from the tray icon on Windows.

use eframe::egui;

use crate::prefs::Prefs;

fn launcher() -> Option<auto_launch::AutoLaunch> {
    let exe = std::env::current_exe().ok()?;
    auto_launch::AutoLaunchBuilder::new().set_app_name("ccez-keys").set_app_path(&exe.to_string_lossy()).build().ok()
}

pub fn login_on() -> bool {
    launcher().and_then(|l| l.is_enabled().ok()).unwrap_or(false)
}

pub fn set_login(on: bool) {
    if let Some(l) = launcher() {
        let r = if on { l.enable() } else { l.disable() };
        if let Err(e) = r {
            crate::pc::log("warn", &format!("login: {e}"));
        }
    }
}

/// On at login from the first start, as decided; the settings change it after.
pub fn first_start() {
    let dir = crate::paths::data();
    let mut p = Prefs::load(&dir);
    if !p.login_set {
        set_login(true);
        p.login_set = true;
        p.save(&dir);
    }
}

struct Settings {
    prefs: Prefs,
    login: bool,
    off: String,
}

impl eframe::App for Settings {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let before = (self.prefs.paused, self.prefs.shown, self.prefs.readings, self.off.clone());
        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            ui.heading("ccez-keys");
            ui.add_space(8.0);
            ui.checkbox(&mut self.prefs.paused, "Pause");
            ui.add_space(6.0);
            ui.label("Languages shown");
            ui.horizontal(|ui| {
                for n in [1usize, 2, 4, 6, 8, 0] {
                    ui.radio_value(&mut self.prefs.shown, n, if n == 0 { "All".to_string() } else { n.to_string() });
                }
            });
            ui.add_space(6.0);
            ui.checkbox(&mut self.prefs.readings, "Readings over Japanese and Chinese");
            if ui.checkbox(&mut self.login, "Open at login").changed() {
                set_login(self.login);
            }
            ui.add_space(6.0);
            ui.label("Off in these apps (one a line)");
            ui.add(egui::TextEdit::multiline(&mut self.off).desired_rows(3).desired_width(f32::INFINITY));
        });
        if before != (self.prefs.paused, self.prefs.shown, self.prefs.readings, self.off.clone()) {
            self.prefs.off = self.off.lines().map(|l| l.trim().to_lowercase()).filter(|l| !l.is_empty()).collect();
            self.prefs.save(&crate::paths::data());
        }
    }
}

pub fn run() {
    let prefs = Prefs::load(&crate::paths::data());
    let off = prefs.off.join("\n");
    let options = eframe::NativeOptions { viewport: egui::ViewportBuilder::default().with_title("ccez-keys").with_inner_size([380.0, 360.0]), ..Default::default() };
    let _ = eframe::run_native("ccez-keys settings", options, Box::new(move |_cc| Ok(Box::new(Settings { prefs, login: login_on(), off }))));
}
