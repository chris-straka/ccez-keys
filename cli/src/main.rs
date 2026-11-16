//! keys: the ccez-keys keyboard on the phone, from the Mac, over adb.
//!
//!     keys status                  version, whether it's on, languages
//!     keys get [key]               every setting, or one, as JSON
//!     keys set <key> <value>       true, 105, '["fr","ja"]' or a word
//!     keys schema                  every setting with its kind and default
//!     keys use <lang>              type in this language now (fr, ja…)
//!     keys mistakes                corrections taken, JSON lines
//!     keys update                  look for a new release now
//!     keys word <lang> <word> [new|learning|known]   a word's status (set: synced to ccez-words)
//!     keys log [-n 50]             the keyboard's events and errors
//!     keys key add <name>          a key (deepseek, gemini or any name), once,
//!                                  everywhere that name is used: z's keychain,
//!                                  the phone, the Cloudflare apps; from the
//!                                  clipboard or a hidden prompt (keystore.rs)
//!     keys key ls | rm <name> | test [<name>]   list (never values), remove, try
//!     keys ai-key [--test]         the old name for keys key add|test deepseek
//!     keys keyring [allow|deny|drop <package>]   apps given the AI key
//!                                  (docs/keyring.md); no argument lists them
//!     keys clip put <text|->       onto the phone's clipboard (- or nothing: stdin)
//!     keys clip now                what's on it now ({"secret":true} for passwords)
//!     keys clip ls                 the keyboard's clipboard history, with ids
//!     keys clip get <id>           one item's text
//!     keys clip rm <id>            out of the history; off the clipboard if it's on it
//!                                  (an agent's put: what was there before comes back)
//!     keys clip clear              empties the clipboard (the history stays)
//!
//! The clipboard works with the keyboard closed: nothing opens on the phone.
//!
//! The phone: $KEYS_SERIAL, else the phone adb sees (over USB first when
//! it sees the same phone over USB and wireless debugging). Output is
//! JSON (except log, JSON lines). Errors are plain words, exit code 1.

use std::process::{Command, exit};

mod keystore;

/// adb, or a stand-in for tests (KEYS_ADB).
fn adb() -> String {
    std::env::var("KEYS_ADB").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| "adb".into())
}

const URI: &str = "content://uk.ccez.keys.control";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (method, arg, value) = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["status"] => ("status", None, None),
        ["keymap"] => ("keymap", None, None),
        ["get"] => ("get", None, None),
        ["get", key] => ("get", Some(key.to_string()), None),
        ["set", key, value] => ("set", Some(key.to_string()), Some(value.to_string())),
        ["schema"] => ("schema", None, None),
        ["use", lang] => ("use", Some(lang.to_string()), None),
        ["mistakes"] => ("mistakes", None, None),
        ["update"] => ("update", None, None),
        // The word may be a phrase, in one argument ("parvenir à") or several.
        ["word", lang, rest @ ..] if !rest.is_empty() => ("word", Some(format!("{lang} {}", rest.join(" "))), None),
        ["forget", lang, rest @ ..] if !rest.is_empty() => ("forget", Some(format!("{lang} {}", rest.join(" "))), None),
        ["keyring"] => ("keyring", Some("list".to_string()), None),
        ["keyring", rest @ ..] => ("keyring", Some(rest.join(" ")), None),
        ["log"] => ("log", Some("50".to_string()), None),
        ["log", "-n", n] => ("log", Some(n.to_string()), None),
        ["backup"] | ["backup", _] => {
            let dir = args.get(1).map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Backups/ccez-keys"));
            match backup(&dir) {
                Ok(out) => println!("{out}"),
                Err(e) => {
                    eprintln!("{e}");
                    exit(1);
                }
            }
            return;
        }
        ["restore", file] => {
            match restore(std::path::Path::new(file)) {
                Ok(out) => println!("{out}"),
                Err(e) => {
                    eprintln!("{e}");
                    exit(1);
                }
            }
            return;
        }
        ["clip", "put", rest @ ..] => {
            let text = match rest {
                [] | ["-"] => {
                    use std::io::Read;
                    let mut t = String::new();
                    if let Err(e) = std::io::stdin().read_to_string(&mut t) {
                        eprintln!("couldn't read stdin: {e}");
                        exit(1);
                    }
                    t
                }
                words => words.join(" "),
            };
            match clip_put(&text) {
                Ok(out) => println!("{out}"),
                Err(e) => {
                    eprintln!("{e}");
                    exit(1);
                }
            }
            return;
        }
        ["clip"] => ("clip", Some("now".to_string()), None),
        ["clip", "now" | "ls" | "clear"] => ("clip", Some(args[1].clone()), None),
        ["clip", "get" | "rm", id] => ("clip", Some(format!("{} {id}", args[1])), None),
        ["ai-key", rest @ ..] => {
            let rest: Vec<String> = rest.iter().map(|s| s.to_string()).collect();
            exit(keystore::ai_key(&rest));
        }
        ["key", rest @ ..] => {
            let rest: Vec<String> = rest.iter().map(|s| s.to_string()).collect();
            exit(keystore::main(&rest));
        }
        _ => {
            eprintln!("usage: keys status | get [key] | set <key> <value> | schema | use <lang> | log [-n N] | key add <name> | key ls | key rm <name> | key test [<name>] | ai-key [--test] | keyring [allow|deny|drop <package>] | clip [put <text|-> | now | ls | get <id> | rm <id> | clear] | backup [dir] | restore <file>");
            exit(2);
        }
    };
    match call(method, arg.as_deref(), value.as_deref()) {
        Ok(out) => println!("{out}"),
        Err(e) => {
            eprintln!("{e}");
            exit(1);
        }
    }
}

fn serial() -> Result<String, String> {
    if let Ok(s) = std::env::var("KEYS_SERIAL") {
        return Ok(s);
    }
    let out = Command::new(adb()).arg("devices").output().map_err(|_| "phone not reachable: adb didn't run".to_string())?;
    let text = String::from_utf8_lossy(&out.stdout);
    let devices: Vec<&str> = text.lines().skip(1).filter(|l| l.ends_with("\tdevice")).map(|l| l.split('\t').next().unwrap_or("")).collect();
    // Each one's own serial number: the same phone over USB and over
    // wireless debugging is two devices with one number.
    let seen: Vec<(String, Option<String>)> = if devices.len() > 1 {
        devices.iter().map(|d| (d.to_string(), getprop(d, "ro.serialno"))).collect()
    } else {
        devices.iter().map(|d| (d.to_string(), None)).collect()
    };
    pick_phone(&seen)
}

fn getprop(device: &str, prop: &str) -> Option<String> {
    let out = Command::new(adb()).args(["-s", device, "shell", "getprop", prop]).output().ok()?;
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !v.is_empty()).then_some(v)
}

/// The phone among the devices adb lists, with their serial numbers. One
/// phone seen over USB and the network is one phone (USB preferred); an
/// emulator beside a phone is left out.
fn pick_phone(devices: &[(String, Option<String>)]) -> Result<String, String> {
    let phones: Vec<&(String, Option<String>)> = if devices.len() > 1 { devices.iter().filter(|(d, _)| !d.starts_with("emulator-")).collect() } else { devices.iter().collect() };
    let usb_first = |list: &[&(String, Option<String>)]| {
        // A network device is host:port or an mDNS name (adb-…._adb-tls-connect._tcp).
        list.iter().find(|(d, _)| !d.contains(':') && !d.contains("._adb")).or_else(|| list.first()).map(|(d, _)| d.clone())
    };
    match phones.as_slice() {
        [] => Err("phone not reachable: plug it in or turn on wireless debugging".into()),
        [(one, _)] => Ok(one.clone()),
        many if many.iter().all(|(_, s)| s.is_some() && *s == many[0].1) => usb_first(many).ok_or_else(|| "no phone".into()),
        many => Err(format!("several phones ({}): unplug the ones you don't mean", many.iter().map(|(d, _)| d.as_str()).collect::<Vec<_>>().join(", "))),
    }
}

/// `adb shell content call`; the provider answers with one string extra,
/// "out" (or "error"), base64 so any text survives the shell.
fn call(method: &str, arg: Option<&str>, value: Option<&str>) -> Result<String, String> {
    let serial = serial()?;
    let mut cmd = format!("content call --uri {URI} --method {method}");
    if let Some(a) = arg {
        cmd += &format!(" --arg {}", quote(a));
    }
    if let Some(v) = value {
        cmd += &format!(" --extra value:s:{}", quote(&b64(v)));
    }
    if std::env::var("KEYS_DEBUG").is_ok() {
        eprintln!("adb -s {serial} shell {cmd}");
    }
    let out = Command::new(adb()).args(["-s", &serial, "shell", &cmd]).output().map_err(|e| format!("adb didn't run: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    if text.contains("Could not find provider") || text.contains("Unknown authority") {
        return Err("ccez-keys isn't installed on the phone".into());
    }
    let pick = |name: &str| {
        let at = text.find(&format!("{name}="))? + name.len() + 1;
        let rest = &text[at..];
        let end = rest.find([',', '}']).unwrap_or(rest.len());
        unb64(rest[..end].trim())
    };
    if let Some(e) = pick("error") {
        return Err(e);
    }
    pick("out").ok_or_else(|| format!("unexpected answer from the phone: {}", text.trim()))
}

/// Text for the phone's clipboard, through stdin like the key: any
/// characters, on no command line on the Mac. One shell argument on the
/// phone holds it, so about 90 KB at most.
fn clip_put(text: &str) -> Result<String, String> {
    if text.is_empty() {
        return Err("nothing to put".into());
    }
    let encoded = b64(text);
    if encoded.len() > 120_000 {
        return Err(format!("too long for the phone's clipboard this way ({} KB; 90 KB at most)", text.len() / 1024));
    }
    call_stdin("clip", Some("put"), &encoded)
}

/// `content call` with the value extra read from stdin on the phone's shell.
fn call_stdin(method: &str, arg: Option<&str>, encoded: &str) -> Result<String, String> {
    use std::io::Write;
    let serial = serial()?;
    let arg = arg.map(|a| format!(" --arg {}", quote(a))).unwrap_or_default();
    let script = format!("read -r k; content call --uri {URI} --method {method}{arg} --extra value:s:\"$k\"");
    let mut child = Command::new(adb())
        .args(["-s", &serial, "shell", &script])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("adb didn't run: {e}"))?;
    child.stdin.take().ok_or("no stdin")?.write_all(format!("{encoded}\n").as_bytes()).map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    if let Some(at) = text.find("{error=") {
        let rest = &text[at + 7..];
        return Err(unb64(rest[..rest.find([',', '}']).unwrap_or(rest.len())].trim()).unwrap_or_else(|| text.trim().to_string()));
    }
    let at = text.find("out=").ok_or_else(|| format!("unexpected answer from the phone: {}", text.trim()))? + 4;
    let rest = &text[at..];
    unb64(rest[..rest.find([',', '}']).unwrap_or(rest.len())].trim()).ok_or_else(|| "bad answer".into())
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::new();
    for chunk in b.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn unb64(s: &str) -> Option<String> {
    let mut bits = 0u32;
    let mut n = 0;
    let mut out = Vec::new();
    for c in s.bytes().filter(|&c| c != b'=') {
        let v = B64.iter().position(|&b| b == c)? as u32;
        bits = bits << 6 | v;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((bits >> n & 0xff) as u8);
        }
    }
    String::from_utf8(out).ok()
}

const BACKUP_URI: &str = "content://uk.ccez.keys.control/backup";
const RESTORE_URI: &str = "content://uk.ccez.keys.control/restore";
/// Backups kept (the oldest go).
const KEEP: usize = 30;

/// Today as YYYY-MM-DD (local time, through `date`).
fn today() -> String {
    Command::new("date").arg("+%F").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).filter(|d| d.len() == 10).unwrap_or_else(|| "backup".into())
}

/// The keyboard's state (settings, words learned, per-app languages, review
/// schedule) into `dir/YYYY-MM-DD.json`; the newest 30 kept.
fn backup(dir: &std::path::Path) -> Result<String, String> {
    let serial = serial()?;
    let out = Command::new(adb())
        .args(["-s", &serial, "exec-out", "content", "read", "--uri", BACKUP_URI])
        .output()
        .map_err(|e| format!("adb didn't run: {e}"))?;
    let body = String::from_utf8_lossy(&out.stdout).into_owned();
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|_| format!("the keyboard didn't send a backup: {}", body.chars().take(200).collect::<String>()))?;
    if v.get("ccez-keys-backup").and_then(|n| n.as_i64()) != Some(1) {
        return Err("not a ccez-keys backup".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let file = dir.join(format!("{}.json", today()));
    let tmp = file.with_extension("tmp");
    std::fs::write(&tmp, &body).and_then(|_| std::fs::rename(&tmp, &file)).map_err(|e| format!("{}: {e}", file.display()))?;
    // Only backups this made (YYYY-MM-DD.json): nothing else in the folder is touched.
    let dated = |p: &std::path::Path| {
        let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        n.len() == 15 && n.ends_with(".json") && n[..10].chars().enumerate().all(|(i, c)| if i == 4 || i == 7 { c == '-' } else { c.is_ascii_digit() })
    };
    let mut all: Vec<_> = std::fs::read_dir(dir).map_err(|e| e.to_string())?.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| dated(p)).collect();
    all.sort();
    let over = all.len().saturating_sub(KEEP);
    for old in &all[..over] {
        let _ = std::fs::remove_file(old);
    }
    let files = v["files"].as_object().map(|o| o.len()).unwrap_or(0);
    Ok(format!("{} ({} files, {} KB)", file.display(), files, body.len() / 1024))
}

/// A backup put back on the phone; the keyboard restarts to read it.
fn restore(file: &std::path::Path) -> Result<String, String> {
    let body = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|_| format!("{} isn't a backup", file.display()))?;
    if v.get("ccez-keys-backup").and_then(|n| n.as_i64()) != Some(1) {
        return Err("not a ccez-keys backup".into());
    }
    let serial = serial()?;
    // The keyboard logs how a restore went: its newest "restore" line, before and after.
    let last = || call("log", Some("60"), None).unwrap_or_default().lines().rfind(|l| l.contains("\"event\":\"restore\"")).map(str::to_string);
    let before = last();
    let mut child = Command::new(adb())
        .args(["-s", &serial, "shell", "content", "write", "--uri", RESTORE_URI])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("adb didn't run: {e}"))?;
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().ok_or("no stdin")?;
        stdin.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
    }
    let ok = child.wait().map_err(|e| e.to_string())?.success();
    if !ok {
        return Err("the phone didn't take it".into());
    }
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let Some(now) = last().filter(|l| Some(l) != before.as_ref()) else { continue };
        let v: serde_json::Value = serde_json::from_str(&now).unwrap_or_default();
        return match v["ctx"]["result"].as_str() {
            Some(r) => Ok(format!("{r}; the keyboard restarts")),
            None => Err(format!("not restored: {}", v["ctx"]["error"].as_str().unwrap_or(&now))),
        };
    }
    Err("no word from the keyboard about the restore".into())
}

#[cfg(test)]
mod tests {
    use super::pick_phone;

    fn d(serial: &str, number: Option<&str>) -> (String, Option<String>) {
        (serial.to_string(), number.map(str::to_string))
    }

    #[test]
    fn one_phone_over_usb_and_wifi_is_one_phone() {
        let usb_and_wifi = [d("100.82.119.62:40465", Some("RFCX")), d("RFCX10YQ4GZ", Some("RFCX"))];
        assert_eq!(pick_phone(&usb_and_wifi).unwrap(), "RFCX10YQ4GZ");
        let wifi_twice = [d("adb-RFCX-abc._adb-tls-connect._tcp", Some("RFCX")), d("100.82.119.62:40465", Some("RFCX"))];
        assert_eq!(pick_phone(&wifi_twice).unwrap(), "adb-RFCX-abc._adb-tls-connect._tcp");
        assert_eq!(pick_phone(&[d("100.82.119.62:40465", None)]).unwrap(), "100.82.119.62:40465");
    }

    #[test]
    fn no_phone_or_two_phones_say_so_plainly() {
        assert_eq!(pick_phone(&[]).unwrap_err(), "phone not reachable: plug it in or turn on wireless debugging");
        assert!(pick_phone(&[d("A1", Some("A")), d("B1", Some("B"))]).unwrap_err().contains("several phones"));
        // An emulator beside the phone isn't the phone.
        assert_eq!(pick_phone(&[d("emulator-5554", Some("EMU")), d("RFCX10YQ4GZ", Some("RFCX"))]).unwrap(), "RFCX10YQ4GZ");
    }
}
