//! Local Unix socket (`menu.sock`) serving ccez-keys' live menu tree and
//! handling menu actions from ccez-clip so one menu bar icon runs both.

use std::io::{BufRead, BufReader, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

pub const PROTOCOL_VERSION: u64 = 1;

pub struct Job {
    pub cmd: String,
    pub id: String,
    pub arg: Option<String>,
    pub front: Option<String>,
    pub front_name: Option<String>,
    pub reply: Sender<Value>,
}

pub type Queue = Arc<Mutex<Vec<Job>>>;

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Action { id: String, title: String, arg: Option<String> },
    Check { id: String, title: String, checked: bool, arg: Option<String> },
    Radio { id: String, title: String, checked: bool, arg: Option<String> },
    Submenu { title: String, items: Vec<Node> },
    Sep,
    #[allow(dead_code)]
    Info { title: String },
}

impl Node {
    pub fn to_json(&self) -> Value {
        match self {
            Node::Action { id, title, arg } => {
                let mut v = json!({ "kind": "action", "id": id, "title": title });
                if let Some(a) = arg {
                    v["arg"] = json!(a);
                }
                v
            }
            Node::Check { id, title, checked, arg } => {
                let mut v = json!({ "kind": "check", "id": id, "title": title, "checked": checked });
                if let Some(a) = arg {
                    v["arg"] = json!(a);
                }
                v
            }
            Node::Radio { id, title, checked, arg } => {
                let mut v = json!({ "kind": "radio", "id": id, "title": title, "checked": checked });
                if let Some(a) = arg {
                    v["arg"] = json!(a);
                }
                v
            }
            Node::Submenu { title, items } => {
                json!({ "kind": "submenu", "title": title, "items": items.iter().map(Node::to_json).collect::<Vec<_>>() })
            }
            Node::Sep => json!({ "kind": "sep" }),
            Node::Info { title } => json!({ "kind": "info", "title": title }),
        }
    }
}

pub struct MenuState {
    pub paused: bool,
    pub shown: usize,
    pub readings: bool,
    pub selection: bool,
    pub copy_click: bool,
    pub speak: bool,
    pub words_on_screen: usize,
    pub words_comeback: u64,
    pub words_break: u64,
    pub words_langs: usize,
    pub words_typing: bool,
    pub words_video: bool,
    pub words_hide_shots: bool,
    pub lang_count: usize,
    pub chat_swap: bool,
    /// `(bundle, display_name, currently_quiet)`, omitted for own apps or ccez-clip.
    pub front: Option<(String, String, bool)>,
    pub missing_flags: Vec<String>,
    pub login_on: bool,
}

fn secs_words(n: u64) -> String {
    match n {
        0 => "At once".to_string(),
        1..=59 => format!("{n} s"),
        n if n % 3600 == 0 => format!("{} h", n / 3600),
        n if n % 60 == 0 => format!("{} min", n / 60),
        n => format!("{n} s"),
    }
}

/// Pure menu tree matching ccez-keys' live preferences and state.
pub fn build_menu(s: &MenuState) -> Vec<Node> {
    let shown_items = [1usize, 2, 4, 6, 8, 0]
        .iter()
        .map(|&n| Node::Radio {
            id: format!("shown={n}"),
            title: if n == 0 { "All".to_string() } else { n.to_string() },
            checked: s.shown == n,
            arg: None,
        })
        .collect();

    let mut word_vals: Vec<usize> = vec![0, 2, 4, 6, 8, 12];
    if !word_vals.contains(&s.words_on_screen) {
        let at = word_vals.iter().position(|&n| n > s.words_on_screen).unwrap_or(word_vals.len());
        word_vals.insert(at, s.words_on_screen);
    }
    let mut words_items: Vec<Node> = word_vals
        .iter()
        .map(|&n| Node::Radio {
            id: format!("words.count={n}"),
            title: if n == 0 { "Off".to_string() } else { n.to_string() },
            checked: s.words_on_screen == n,
            arg: None,
        })
        .collect();

    let comeback_items = (1u64..=5)
        .map(|n| Node::Radio {
            id: format!("words.comeback={n}"),
            title: format!("{n} min"),
            checked: s.words_comeback == n,
            arg: None,
        })
        .collect();

    let mut break_vals: Vec<u64> = vec![0, 30, 60, 300, 900, 3600];
    if !break_vals.contains(&s.words_break) {
        let at = break_vals.iter().position(|&n| n > s.words_break).unwrap_or(break_vals.len());
        break_vals.insert(at, s.words_break);
    }
    let break_items = break_vals
        .iter()
        .map(|&n| Node::Radio {
            id: format!("words.break={n}"),
            title: secs_words(n),
            checked: s.words_break == n,
            arg: None,
        })
        .collect();

    let mut lang_vals: Vec<usize> = vec![0];
    for n in [1usize, 2, 3, 6] {
        if n <= s.lang_count {
            lang_vals.push(n);
        }
    }
    if !lang_vals.contains(&s.words_langs) {
        let at = lang_vals.iter().position(|&n| n > s.words_langs).unwrap_or(lang_vals.len());
        lang_vals.insert(at, s.words_langs);
    }
    let lang_items = lang_vals
        .iter()
        .map(|&n| Node::Radio {
            id: format!("words.langs={n}"),
            title: if n == 0 { "All".to_string() } else { format!("First {n}") },
            checked: s.words_langs == n,
            arg: None,
        })
        .collect();

    words_items.push(Node::Sep);
    words_items.push(Node::Submenu { title: "Come back in".into(), items: comeback_items });
    words_items.push(Node::Submenu { title: "Back after clearing".into(), items: break_items });
    words_items.push(Node::Submenu { title: "Languages".into(), items: lang_items });
    words_items.push(Node::Sep);
    words_items.push(Node::Check { id: "words.typing".into(), title: "While typing".into(), checked: s.words_typing, arg: None });
    words_items.push(Node::Check { id: "words.video".into(), title: "While a video plays".into(), checked: s.words_video, arg: None });
    words_items.push(Node::Check { id: "words.shots".into(), title: "Hide in screenshots".into(), checked: s.words_hide_shots, arg: None });

    let mut items = vec![
        Node::Action { id: "pause".into(), title: if s.paused { "Resume" } else { "Pause" }.into(), arg: None },
        Node::Sep,
        Node::Submenu { title: "Languages shown".into(), items: shown_items },
        Node::Check { id: "readings".into(), title: "Readings over Japanese and Chinese".into(), checked: s.readings, arg: None },
        Node::Check { id: "selection".into(), title: "Preview selected text".into(), checked: s.selection, arg: None },
        Node::Check { id: "copy_click".into(), title: "Copy a word when clicked".into(), checked: s.copy_click, arg: None },
        Node::Check { id: "speak".into(), title: "Say a word when clicked".into(), checked: s.speak, arg: None },
        Node::Submenu { title: "Words on screen".into(), items: words_items },
        Node::Check { id: "chat_swap".into(), title: "⌥⏎ in ChatGPT and Claude: into my language".into(), checked: s.chat_swap, arg: None },
    ];

    if let Some((bundle, name, is_quiet)) = &s.front
        && !bundle.is_empty()
        && bundle != "uk.ccez.clip"
        && bundle != "uk.ccez.keys.mac"
    {
        items.push(Node::Check {
            id: "off_in".into(),
            title: format!("Off in {name}"),
            checked: *is_quiet,
            arg: Some(bundle.clone()),
        });
    }

    let langs_title = if s.missing_flags.is_empty() {
        "ⓘ Translation languages…".to_string()
    } else {
        format!("ⓘ Translation languages… ({} missing)", s.missing_flags.join(" "))
    };
    items.push(Node::Sep);
    items.push(Node::Action { id: "languages".into(), title: langs_title, arg: None });
    items.push(Node::Check { id: "login".into(), title: "Open at login".into(), checked: s.login_on, arg: None });
    items.push(Node::Sep);
    items.push(Node::Action { id: "quit".into(), title: "Quit ccez-keys".into(), arg: None });
    items
}

fn same_peer_uid(fd: i32) -> bool {
    unsafe extern "C" {
        fn getpeereid(fd: i32, euid: *mut u32, egid: *mut u32) -> i32;
        fn geteuid() -> u32;
    }
    let mut euid: u32 = u32::MAX;
    let mut egid: u32 = u32::MAX;
    unsafe { getpeereid(fd, &mut euid, &mut egid) == 0 && euid == geteuid() }
}

pub fn serve(sock: PathBuf, queue: Queue, wake: Arc<dyn Fn() + Send + Sync>, log: fn(&str, &str)) {
    let _ = std::fs::remove_file(&sock);
    let listener = match UnixListener::bind(&sock) {
        Ok(l) => l,
        Err(e) => {
            log("warn", &format!("menu.sock at {}: {e}", sock.display()));
            return;
        }
    };
    log("info", &format!("menu.sock ready at {}", sock.display()));
    std::thread::Builder::new()
        .name("menu-ipc".into())
        .spawn(move || {
            for conn in listener.incoming().flatten() {
                let (queue, wake) = (queue.clone(), wake.clone());
                std::thread::spawn(move || {
                    let _ = handle(conn, &queue, &wake);
                });
            }
        })
        .ok();
}

fn handle(conn: UnixStream, queue: &Queue, wake: &Arc<dyn Fn() + Send + Sync>) -> std::io::Result<()> {
    if !same_peer_uid(conn.as_raw_fd()) {
        return Ok(());
    }
    let _ = conn.set_read_timeout(Some(Duration::from_millis(150)));
    let _ = conn.set_write_timeout(Some(Duration::from_millis(150)));
    let mut line = String::new();
    BufReader::new(&conn).read_line(&mut line)?;
    let req: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
    let reply = if req["v"].as_u64() != Some(PROTOCOL_VERSION) {
        json!({ "v": PROTOCOL_VERSION, "ok": false, "error": "unsupported version" })
    } else {
        let cmd = req["cmd"].as_str().unwrap_or("").to_string();
        let id = req["id"].as_str().unwrap_or("").to_string();
        let arg = req["arg"].as_str().map(str::to_string);
        let front = req["front"].as_str().map(str::to_string).filter(|s| !s.is_empty());
        let front_name = req["front_name"].as_str().map(str::to_string).filter(|s| !s.is_empty());
        let (tx, rx) = channel();
        queue.lock().unwrap().push(Job { cmd, id, arg, front, front_name, reply: tx });
        wake();
        rx.recv_timeout(Duration::from_millis(120))
            .unwrap_or_else(|_| json!({ "v": PROTOCOL_VERSION, "ok": false, "error": "timed out" }))
    };
    let mut w = &conn;
    writeln!(w, "{reply}")?;
    w.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_state() -> MenuState {
        MenuState {
            paused: false,
            shown: 6,
            readings: true,
            selection: true,
            copy_click: true,
            speak: true,
            words_on_screen: 5,
            words_comeback: 2,
            words_break: 45,
            words_langs: 2,
            words_typing: true,
            words_video: false,
            words_hide_shots: true,
            lang_count: 3,
            chat_swap: true,
            front: Some(("com.google.antigravity".into(), "Antigravity".into(), false)),
            missing_flags: vec!["🇫🇷".into()],
            login_on: true,
        }
    }

    #[test]
    fn builds_menu_with_custom_presets_and_off_in() {
        let items = build_menu(&sample_state());
        assert!(items.iter().any(|n| matches!(n, Node::Check { id, title, checked: false, arg: Some(a) } if id == "off_in" && title == "Off in Antigravity" && a == "com.google.antigravity")));
        assert!(items.iter().any(|n| matches!(n, Node::Action { id, title, .. } if id == "languages" && title == "ⓘ Translation languages… (🇫🇷 missing)")));
        let Some(Node::Submenu { items: words, .. }) = items.iter().find(|n| matches!(n, Node::Submenu { title, .. } if title == "Words on screen")) else {
            panic!("missing Words on screen submenu");
        };
        assert!(words.iter().any(|n| matches!(n, Node::Radio { id, checked: true, .. } if id == "words.count=5")));
        let json: Vec<Value> = items.iter().map(Node::to_json).collect();
        assert_eq!(json[0]["id"], "pause");
        assert_eq!(json[0]["title"], "Pause");
    }

    #[test]
    fn omits_off_in_for_ccez_clip_and_self() {
        for pkg in ["uk.ccez.clip", "uk.ccez.keys.mac"] {
            let mut s = sample_state();
            s.front = Some((pkg.into(), "App".into(), false));
            let items = build_menu(&s);
            assert!(!items.iter().any(|n| matches!(n, Node::Check { id, .. } if id == "off_in")), "{pkg} leaked off_in");
        }
        assert_eq!(Node::Info { title: "v1".into() }.to_json(), json!({ "kind": "info", "title": "v1" }));
    }

    #[test]
    fn socket_round_trip_serves_and_wakes() {
        let dir = std::env::temp_dir().join(format!("ccez-keys-ipc-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let sock = dir.join("menu.sock");
        let queue: Queue = Arc::default();
        let woke = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let q2 = queue.clone();
        let w2 = woke.clone();
        serve(
            sock.clone(),
            queue,
            Arc::new(move || {
                w2.store(true, std::sync::atomic::Ordering::Release);
                if let Some(job) = q2.lock().unwrap().pop() {
                    let _ = job.reply.send(json!({ "v": 1, "ok": true, "cmd": job.cmd }));
                }
            }),
            |_, _| {},
        );
        std::thread::sleep(Duration::from_millis(20));
        let mut s = UnixStream::connect(&sock).unwrap();
        writeln!(s, "{{\"v\":1,\"cmd\":\"menu\"}}").unwrap();
        s.flush().unwrap();
        let mut line = String::new();
        BufReader::new(s).read_line(&mut line).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["cmd"], "menu");
        assert!(woke.load(std::sync::atomic::Ordering::Acquire));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
