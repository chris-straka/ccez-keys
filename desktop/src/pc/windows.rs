//! Windows: the field being typed in, through UI Automation (on its own
//! thread, so a frozen app can't stall the preview), speech through the
//! system's voices (System.Speech), and a preview window that never takes
//! the keyboard (WS_EX_NOACTIVATE).

use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use uiautomation::patterns::{UITextPattern, UIValuePattern};
use uiautomation::types::{TextPatternRangeEndpoint, TextUnit};
use uiautomation::{UIAutomation, UIElement};

use crate::model::{Field, Rect};

/// A call that takes longer than this (a frozen app) is given up on.
const WAIT: Duration = Duration::from_millis(300);

enum Ask {
    Focused(Sender<Option<Field>>),
    Front(Sender<Option<(String, String)>>),
    Swap(String, String, String, Sender<Result<&'static str, String>>),
}

pub struct Access {
    tx: Sender<Ask>,
}

/// The text before the cursor in `el`: from its text pattern (start to caret),
/// else its value (taken as all before the cursor).
fn before_caret(el: &UIElement) -> Option<String> {
    if let Ok(tp) = el.get_pattern::<UITextPattern>() {
        let doc = tp.get_document_range().ok()?;
        let (_, caret) = tp.get_caret_range().ok()?;
        doc.move_endpoint_by_range(TextPatternRangeEndpoint::End, &caret, TextPatternRangeEndpoint::Start).ok()?;
        return doc.get_text(-1).ok();
    }
    el.get_pattern::<UIValuePattern>().ok()?.get_value().ok()
}

fn rect(el: &UIElement) -> Option<Rect> {
    let r = el.get_bounding_rectangle().ok()?;
    (r.get_width() > 0 && r.get_height() > 0).then(|| Rect { x: f64::from(r.get_left()), y: f64::from(r.get_top()), w: f64::from(r.get_width()), h: f64::from(r.get_height()) })
}

/// The process's executable name ("WindowsTerminal.exe").
fn process_name(pid: u32) -> Option<String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 260];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        ok.then(|| String::from_utf16_lossy(&buf[..len as usize]).rsplit('\\').next().unwrap_or("").to_string())
    }
}

fn serve(rx: Receiver<Ask>) {
    let Ok(uia) = UIAutomation::new() else { return };
    let editable = |el: &UIElement| el.get_pattern::<UITextPattern>().is_ok() || el.get_pattern::<UIValuePattern>().is_ok();
    for ask in rx {
        match ask {
            Ask::Focused(reply) => {
                let f = uia.get_focused_element().ok().and_then(|el| {
                    if el.is_password().unwrap_or(false) {
                        return Some(Field { secure: true, ..Default::default() });
                    }
                    if !editable(&el) {
                        return None;
                    }
                    Some(Field { secure: false, before: before_caret(&el)?, caret: None, frame: rect(&el), selected: String::new() })
                });
                let _ = reply.send(f);
            }
            Ask::Front(reply) => {
                let name = uia.get_focused_element().ok().and_then(|el| el.get_process_id().ok()).and_then(process_name);
                let _ = reply.send(name.map(|n| (n.clone(), n.trim_end_matches(".exe").to_string())));
            }
            Ask::Swap(sentence, trailing, text, reply) => {
                let r = (|| {
                    let el = uia.get_focused_element().map_err(|e| e.to_string())?;
                    if el.is_password().unwrap_or(false) {
                        return Err("a password box".to_string());
                    }
                    let before = before_caret(&el).ok_or("no text")?;
                    if !before.ends_with(&format!("{sentence}{trailing}")) {
                        return Err("the field moved on".into());
                    }
                    // The sentence and its spaces selected from the cursor back, then typed over.
                    let tp = el.get_pattern::<UITextPattern>().map_err(|e| e.to_string())?;
                    let (_, range) = tp.get_caret_range().map_err(|e| e.to_string())?;
                    let n = (sentence.chars().count() + trailing.chars().count()) as i32;
                    range.move_endpoint_by_unit(TextPatternRangeEndpoint::Start, TextUnit::Character, -n).map_err(|e| e.to_string())?;
                    range.select().map_err(|e| e.to_string())?;
                    uiautomation::inputs::Keyboard::new().send_text(&format!("{text}{trailing}")).map_err(|e| e.to_string())?;
                    Ok("typed over")
                })();
                let _ = reply.send(r);
            }
        }
    }
}

impl Access {
    pub fn start() -> Option<Access> {
        let (tx, rx) = channel();
        std::thread::spawn(move || serve(rx));
        Some(Access { tx })
    }

    fn ask<T>(&self, make: impl FnOnce(Sender<T>) -> Ask) -> Option<T> {
        let (tx, rx) = channel();
        self.tx.send(make(tx)).ok()?;
        rx.recv_timeout(WAIT).ok()
    }
}

impl crate::pc::Access for Access {
    fn focused(&mut self) -> Option<Field> {
        self.ask(Ask::Focused).flatten()
    }

    fn front_app(&mut self) -> Option<(String, String)> {
        self.ask(Ask::Front).flatten()
    }

    fn swap(&mut self, sentence: &str, trailing: &str, text: &str) -> Result<&'static str, String> {
        let (s, t, x) = (sentence.to_string(), trailing.to_string(), text.to_string());
        let (tx, rx) = channel();
        self.tx.send(Ask::Swap(s, t, x, tx)).map_err(|e| e.to_string())?;
        rx.recv_timeout(Duration::from_secs(3)).map_err(|_| "the field didn't answer".to_string())?
    }
}

/// The system's voice for the language (System.Speech), one at a time.
pub fn say(lang: &str, text: &str) {
    use std::sync::Mutex;
    static LAST: Mutex<Option<std::process::Child>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap();
    if let Some(mut c) = last.take() {
        let _ = c.kill();
    }
    // The text goes in an environment variable: no quoting to get wrong.
    let script = "Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
        $v = $s.GetInstalledVoices() | Where-Object { $_.VoiceInfo.Culture.Name -like ($env:CCEZ_LANG + '*') } | Select-Object -First 1; \
        if ($v) { $s.SelectVoice($v.VoiceInfo.Name) }; $s.Speak($env:CCEZ_TEXT)";
    *last = std::process::Command::new("powershell").args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", script]).env("CCEZ_LANG", lang).env("CCEZ_TEXT", text).spawn().ok();
}

/// The preview window never takes the keyboard from the app being typed in.
pub fn no_activate(hwnd: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{GWL_EXSTYLE, GetWindowLongPtrW, SetWindowLongPtrW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST};
    unsafe {
        let h = HWND(hwnd as *mut core::ffi::c_void);
        let style = GetWindowLongPtrW(h, GWL_EXSTYLE);
        SetWindowLongPtrW(h, GWL_EXSTYLE, style | (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0) as isize);
    }
}
