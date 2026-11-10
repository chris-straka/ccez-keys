//! Linux: the field being typed in, through AT-SPI (the accessibility bus
//! GTK, Qt, Firefox and Chromium apps speak), and speech through Speech
//! Dispatcher. A thread listens for focus and caret events to know which
//! object has the keyboard; the preview asks it for its text and cursor.
//! Never keystrokes: replacing a sentence goes through AT-SPI's EditableText.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use atspi::events::{Event, FocusEvents, ObjectEvents, focus::FocusEvent, object::StateChangedEvent, object::TextCaretMovedEvent};
use atspi::proxy::accessible::AccessibleProxy;
use atspi::proxy::component::ComponentProxy;
use atspi::proxy::editable_text::EditableTextProxy;
use atspi::proxy::text::TextProxy;
use atspi::{CoordType, Role, State};
use futures_lite::StreamExt;

use crate::model::{Field, Rect};

/// A call that takes longer than this (a frozen app) is given up on.
const WAIT: Duration = Duration::from_millis(300);

pub struct Access {
    conn: zbus::Connection,
    /// The object with the keyboard: its bus name and path.
    focused: Arc<Mutex<Option<(String, String)>>>,
}

fn block<T>(f: impl std::future::Future<Output = Option<T>>) -> Option<T> {
    async_io::block_on(futures_lite::future::or(f, async {
        async_io::Timer::after(WAIT).await;
        None
    }))
}

/// A proxy to `$name` at `$path` (no property cache: values change all the time).
macro_rules! proxy {
    ($t:ty, $conn:expr, $name:expr, $path:expr) => {
        async {
            <$t>::builder($conn)
                .destination($name.to_string())
                .ok()?
                .path($path.to_string())
                .ok()?
                .cache_properties(zbus::proxy::CacheProperties::No)
                .build()
                .await
                .ok()
        }
    };
}

impl Access {
    pub fn start() -> Option<Access> {
        // Ask apps to speak AT-SPI (GNOME's "toolkit accessibility"), then listen.
        let _ = async_io::block_on(atspi::connection::set_session_accessibility(true));
        let a11y = async_io::block_on(atspi::AccessibilityConnection::new()).ok()?;
        let conn = a11y.connection().clone();
        // What has the keyboard already (events only tell of changes).
        let first = async_io::block_on(find_focused(&conn));
        let focused: Arc<Mutex<Option<(String, String)>>> = Arc::new(Mutex::new(first));
        let debug = std::env::var_os("CCEZ_DEBUG").is_some();
        let put = focused.clone();
        std::thread::spawn(move || {
            async_io::block_on(async move {
                let _ = a11y.register_event::<StateChangedEvent>().await;
                let _ = a11y.register_event::<TextCaretMovedEvent>().await;
                let _ = a11y.register_event::<FocusEvent>().await;
                let mut events = a11y.event_stream();
                while let Some(ev) = events.next().await {
                    if debug {
                        eprintln!("event {ev:?}");
                    }
                    let item = match ev {
                        Ok(Event::Object(ObjectEvents::StateChanged(e))) if e.state == State::Focused && e.enabled => e.item,
                        Ok(Event::Object(ObjectEvents::TextCaretMoved(e))) => e.item,
                        Ok(Event::Focus(FocusEvents::Focus(e))) => e.item,
                        _ => continue,
                    };
                    if let Some(name) = item.name() {
                        *put.lock().unwrap() = Some((name.to_string(), item.path().to_string()));
                    }
                }
            });
        });
        Some(Access { conn, focused })
    }

    fn current(&self) -> Option<(String, String)> {
        self.focused.lock().unwrap().clone()
    }
}

/// The focused, editable object, found by walking each app's tree (at most a
/// few thousand objects): for when nothing has moved since the app started.
async fn find_focused(conn: &zbus::Connection) -> Option<(String, String)> {
    let root = proxy!(AccessibleProxy, conn, "org.a11y.atspi.Registry", "/org/a11y/atspi/accessible/root").await?;
    let mut todo: Vec<(String, String)> = root.get_children().await.ok()?.iter().filter_map(|c| Some((c.name()?.to_string(), c.path().to_string()))).collect();
    let mut seen = 0;
    while let Some((name, path)) = todo.pop() {
        seen += 1;
        if seen > 4000 {
            break;
        }
        let Some(acc) = proxy!(AccessibleProxy, conn, &name, &path).await else { continue };
        let Ok(states) = acc.get_state().await else { continue };
        if states.contains(State::Focused) && (states.contains(State::Editable) || acc.get_role().await.ok() == Some(Role::PasswordText)) {
            return Some((name, path));
        }
        // Hidden parts of apps can't hold the keyboard.
        if !states.contains(State::Showing) && seen > 1 {
            continue;
        }
        if let Ok(children) = acc.get_children().await {
            todo.extend(children.iter().filter_map(|c| Some((c.name()?.to_string(), c.path().to_string()))));
        }
    }
    None
}

impl crate::pc::Access for Access {
    fn focused(&mut self) -> Option<Field> {
        let (name, path) = self.current()?;
        let conn = self.conn.clone();
        block(async move {
            let acc: AccessibleProxy = proxy!(AccessibleProxy, &conn, &name, &path).await?;
            let states = acc.get_state().await.ok()?;
            if !states.contains(State::Focused) {
                return None;
            }
            // A password box: never read.
            if acc.get_role().await.ok()? == Role::PasswordText {
                return Some(Field { secure: true, ..Default::default() });
            }
            if !states.contains(State::Editable) {
                return None;
            }
            let text: TextProxy = proxy!(TextProxy, &conn, &name, &path).await?;
            let caret = text.caret_offset().await.ok()?.max(0);
            let before = text.get_text(0, caret).await.ok()?;
            let at = (caret - 1).max(0);
            let (x, y, w, h) = text.get_character_extents(at, CoordType::Screen).await.unwrap_or((0, 0, 0, 0));
            let caret_rect = (h > 0 && (x != 0 || y != 0)).then(|| Rect { x: f64::from(x + if caret > 0 { w } else { 0 }), y: f64::from(y), w: 1.0, h: f64::from(h) });
            let comp: Option<ComponentProxy> = proxy!(ComponentProxy, &conn, &name, &path).await;
            let frame = match comp {
                Some(c) => c.get_extents(CoordType::Screen).await.ok().filter(|e| e.2 > 0 && e.3 > 0).map(|(x, y, w, h)| Rect { x: f64::from(x), y: f64::from(y), w: f64::from(w), h: f64::from(h) }),
                None => None,
            };
            Some(Field { secure: false, before, caret: caret_rect, frame })
        })
    }

    fn front_app(&mut self) -> Option<(String, String)> {
        let (name, path) = self.current()?;
        let conn = self.conn.clone();
        block(async move {
            let acc: AccessibleProxy = proxy!(AccessibleProxy, &conn, &name, &path).await?;
            let app = acc.get_application().await.ok()?;
            let app_name = app.name()?.to_string();
            let app_path = app.path().to_string();
            let appacc: AccessibleProxy = proxy!(AccessibleProxy, &conn, &app_name, &app_path).await?;
            let n = appacc.name().await.ok()?;
            Some((n.clone(), n))
        })
    }

    fn swap(&mut self, sentence: &str, trailing: &str, text: &str) -> Result<&'static str, String> {
        let (name, path) = self.current().ok_or("no field")?;
        let conn = self.conn.clone();
        let (sentence, trailing, text) = (sentence.to_string(), trailing.to_string(), text.to_string());
        block(async move {
            let t: TextProxy = proxy!(TextProxy, &conn, &name, &path).await?;
            let caret = t.caret_offset().await.ok()?;
            let before = t.get_text(0, caret).await.ok()?;
            // Only while the field still ends with the sentence (and its spaces) at the cursor.
            if !before.ends_with(&format!("{sentence}{trailing}")) {
                return Some(Err("the field moved on".to_string()));
            }
            // AT-SPI counts characters.
            let end = caret - trailing.chars().count() as i32;
            let start = end - sentence.chars().count() as i32;
            let e: EditableTextProxy = proxy!(EditableTextProxy, &conn, &name, &path).await?;
            if !e.delete_text(start, end).await.ok()? {
                return Some(Err("the field wouldn't change".to_string()));
            }
            // Its length in bytes, as AT-SPI (ATK) counts it ("ù" is two).
            let ok = e.insert_text(start, &text, text.len() as i32).await.ok()?;
            Some(if ok { Ok("at-spi") } else { Err("the field wouldn't take it".to_string()) })
        })
        .unwrap_or_else(|| Err("the field didn't answer".into()))
    }
}

/// Speech Dispatcher's `spd-say`, in the language's voice (a new line stops the last).
pub fn say(lang: &str, text: &str) {
    let _ = std::process::Command::new("spd-say").args(["-C"]).status();
    let _ = std::process::Command::new("spd-say").args(["-l", lang, "--", text]).spawn();
}
