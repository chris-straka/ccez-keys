//! keys: the ccez-keys keyboard on the phone, from the Mac, over adb.
//!
//!     keys status                  version, whether it's on, languages
//!     keys get [key]               every setting, or one, as JSON
//!     keys set <key> <value>       true, 105, '["fr","ja"]' or a word
//!     keys schema                  every setting with its kind and default
//!     keys use <lang>              type in this language now (fr, ja…)
//!     keys log [-n 50]             the keyboard's events and errors
//!     keys key < file              the AI key, from stdin (empty removes it);
//!                                  it never appears on a command line
//!
//! The phone: $KEYS_SERIAL, else the only device adb sees. Output is
//! JSON (except log, JSON lines). Errors are plain words, exit code 1.

use std::process::{Command, exit};

const URI: &str = "content://uk.ccez.keys.control";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (method, arg, value) = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["status"] => ("status", None, None),
        ["get"] => ("get", None, None),
        ["get", key] => ("get", Some(key.to_string()), None),
        ["set", key, value] => ("set", Some(key.to_string()), Some(value.to_string())),
        ["schema"] => ("schema", None, None),
        ["use", lang] => ("use", Some(lang.to_string()), None),
        ["log"] => ("log", Some("50".to_string()), None),
        ["log", "-n", n] => ("log", Some(n.to_string()), None),
        ["key"] => {
            match send_key() {
                Ok(out) => println!("{out}"),
                Err(e) => {
                    eprintln!("{e}");
                    exit(1);
                }
            }
            return;
        }
        _ => {
            eprintln!("usage: keys status | get [key] | set <key> <value> | schema | use <lang> | log [-n N] | key < file");
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
    let out = Command::new("adb").arg("devices").output().map_err(|e| format!("adb didn't run: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let devices: Vec<&str> = text.lines().skip(1).filter(|l| l.ends_with("\tdevice")).map(|l| l.split('\t').next().unwrap_or("")).collect();
    match devices.as_slice() {
        [one] => Ok(one.to_string()),
        [] => Err("no phone: plug it in or connect it with adb".into()),
        _ => Err(format!("several devices ({}); set KEYS_SERIAL", devices.join(", "))),
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
    let out = Command::new("adb").args(["-s", &serial, "shell", &cmd]).output().map_err(|e| format!("adb didn't run: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    if text.contains("Could not find provider") || text.contains("Unknown authority") {
        return Err("ccez-keys isn't installed on the phone".into());
    }
    let pick = |name: &str| {
        let at = text.find(&format!("{name}="))? + name.len() + 1;
        let rest = &text[at..];
        let end = rest.find(|c: char| c == ',' || c == '}').unwrap_or(rest.len());
        unb64(rest[..end].trim())
    };
    if let Some(e) = pick("error") {
        return Err(e);
    }
    pick("out").ok_or_else(|| format!("unexpected answer from the phone: {}", text.trim()))
}

/// The key goes through stdin to the phone's shell (`read`), so it is on
/// no command line on the Mac.
fn send_key() -> Result<String, String> {
    use std::io::{Read, Write};
    let mut key = String::new();
    std::io::stdin().read_to_string(&mut key).map_err(|e| format!("couldn't read the key: {e}"))?;
    let serial = serial()?;
    let script = format!("read -r k; content call --uri {URI} --method key --extra value:s:\"$k\"");
    let mut child = Command::new("adb")
        .args(["-s", &serial, "shell", &script])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("adb didn't run: {e}"))?;
    let encoded = if key.trim().is_empty() { b64(" ") } else { b64(key.trim()) };
    child.stdin.take().ok_or("no stdin")?.write_all(format!("{encoded}\n").as_bytes()).map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let at = text.find("out=").ok_or_else(|| format!("unexpected answer from the phone: {}", text.trim()))? + 4;
    let rest = &text[at..];
    unb64(rest[..rest.find(|c: char| c == ',' || c == '}').unwrap_or(rest.len())].trim()).ok_or_else(|| "bad answer".into())
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
