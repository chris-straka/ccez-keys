//! `keys key`: the owner's named keys (API keys for AI services), each
//! entered once on z and put everywhere its name is used (owner,
//! 2026-11-16: "Keys should store more than just deepseek key").
//!
//!     keys key add <name> [--stdin]   from the clipboard, else a hidden prompt (or stdin)
//!     keys key ls                     each name, where it's placed, when it was set (never values)
//!     keys key rm <name>              off every place it's in, after a y/N
//!     keys key test [<name>]          one tiny real call per key
//!     keys ai-key [--stdin | --test]  the old name for `keys key add|test deepseek`
//!
//! Every key is in z's login keychain (service `uk.ccez.ai`, account the
//! name), where ccez-words reads DeepSeek's. Where else a name goes is
//! [PLACES]; a name it doesn't know goes to z and the phone's ccez-keys,
//! which lends keys to the phone apps by name (docs/keyring.md). A Pages
//! secret only takes effect in a new deployment, so those projects are
//! redeployed: an empty commit on main, which their git-connected builds
//! pick up.
//!
//! A key must look right for its name (DeepSeek's `sk-…`, Gemini's `AQ.…`
//! or `AIza…`) before it goes anywhere. It's never printed, logged or put
//! on a command line: each tool gets it on stdin.
//!
//! The tools are found on PATH; tests point them at stand-ins with
//! KEYS_SECURITY, KEYS_LAUNCHCTL, KEYS_PBPASTE, KEYS_WRANGLER, KEYS_GIT,
//! KEYS_CURL, KEYS_ADB, KEYS_SWE (the repos' folder), KEYS_HUB (ccez-words)
//! and KEYS_GEMINI (Google's API); KEYS_NO_PROMPT turns the hidden prompt off.

use std::io::{BufRead, Read, Write};
use std::process::{Command, Stdio};

const SERVICE: &str = "uk.ccez.ai";
/// ccez-words on z itself (keys key runs on z).
const HUB: &str = "http://127.0.0.1:8798/ai/deepseek";
const DEEPSEEK_MODEL: &str = "deepseek-flash";
const GEMINI: &str = "https://generativelanguage.googleapis.com/v1beta";
const GEMINI_MODEL: &str = "gemini-4-hydrogen";

/// Where a key goes.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Place {
    /// z's login keychain: every key.
    Z,
    /// The phone's ccez-keys, over adb; it lends keys to the phone apps by name.
    Phone,
    /// A Cloudflare Worker's secret: (worker, secret).
    Worker(&'static str, &'static str),
    /// A Cloudflare Pages project's secret: (project, its repo under ~/SWE, secret).
    Pages(&'static str, &'static str, &'static str),
}
use Place::*;

/// A name this knows: how its keys look, and where they go.
struct Known {
    name: &'static str,
    label: &'static str,
    shape: &'static str,
    looks: fn(&str) -> bool,
    places: &'static [Place],
}

/// The names and their places (owner, 2026-11-16). A name not here goes
/// to z and the phone.
const PLACES: &[Known] = &[
    Known {
        name: "deepseek",
        label: "DeepSeek",
        shape: "sk-…",
        looks: looks_deepseek,
        places: &[
            Z,
            Phone,
            Worker("ccez-keeps", "DEEPSEEK_API_KEY"),
            Pages("art-website", "art-website", "DEEPSEEK_API_KEY"),
            Pages("personalized-llm-client", "ccez-llm", "DEEPSEEK_API_KEY"),
        ],
    },
    Known { name: "gemini", label: "Gemini", shape: "AQ.… or AIza…", looks: looks_gemini, places: &[Z, Pages("art-website", "art-website", "GEMINI_API_KEY")] },
];
const ELSEWHERE: &[Place] = &[Z, Phone];

fn known(name: &str) -> Option<&'static Known> {
    PLACES.iter().find(|k| k.name == name)
}

fn places(name: &str) -> &'static [Place] {
    known(name).map(|k| k.places).unwrap_or(ELSEWHERE)
}

fn label(name: &str) -> String {
    known(name).map(|k| k.label.to_string()).unwrap_or_else(|| name.to_string())
}

/// A key's name: lowercase letters, digits and -, a letter first, up to 32.
pub fn valid_name(n: &str) -> bool {
    (1..=32).contains(&n.len()) && n.starts_with(|c: char| c.is_ascii_lowercase()) && n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// DeepSeek's: `sk-` and letters, digits, - or _.
pub fn looks_deepseek(k: &str) -> bool {
    k.starts_with("sk-") && (20..=200).contains(&k.len()) && k[3..].chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Gemini's: AI Studio's auth keys (`AQ.…`, since 2026-05-28) or the
/// older standard ones (`AIza…`, 39 characters).
pub fn looks_gemini(k: &str) -> bool {
    let tail_ok = |t: &str| t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
    (k.starts_with("AQ.") && (20..=400).contains(&k.len()) && tail_ok(&k[3..])) || (k.starts_with("AIza") && k.len() == 39 && tail_ok(&k[4..]) && !k.contains('.'))
}

/// A name it doesn't know: one word of printable characters.
fn looks_any(k: &str) -> bool {
    (8..=1000).contains(&k.len()) && k.chars().all(|c| c.is_ascii_graphic())
}

fn looks_right(name: &str, k: &str) -> bool {
    known(name).map(|n| (n.looks)(k)).unwrap_or_else(|| looks_any(k))
}

fn shape(name: &str) -> String {
    match known(name) {
        Some(k) => format!("{} key ({})", k.label, k.shape),
        None => format!("{name} key (one word, no spaces)"),
    }
}

fn tool(env: &str, default: &str) -> String {
    std::env::var(env).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| default.to_string())
}

/// Runs `cmd`, `input` on its stdin; (succeeded, stdout + stderr).
fn run(cmd: &mut Command, input: Option<&str>) -> (bool, String) {
    cmd.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return (false, format!("{} didn't run: {e}", cmd.get_program().to_string_lossy())),
    };
    if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
        let _ = stdin.write_all(text.as_bytes());
    }
    match child.wait_with_output() {
        Ok(o) => (o.status.success(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))),
        Err(e) => (false, e.to_string()),
    }
}

/// The last non-empty line of a tool's output, without colour codes.
fn last_line(out: &str) -> String {
    let plain: String = {
        let mut s = String::new();
        let mut esc = false;
        for c in out.chars() {
            match (esc, c) {
                (false, '\u{1b}') => esc = true,
                (true, c) if c.is_ascii_alphabetic() => esc = false,
                (true, _) => {}
                (false, c) => s.push(c),
            }
        }
        s
    };
    plain.lines().map(str::trim).rfind(|l| !l.is_empty() && !l.starts_with("🪵")).unwrap_or("").chars().take(200).collect()
}

/// The key: stdin with `--stdin`, else the clipboard, else a hidden prompt.
fn read_key(name: &str, from_stdin: bool) -> Result<String, String> {
    let wrong = || format!("that isn't a {}; nothing was stored", shape(name));
    if from_stdin {
        let mut k = String::new();
        std::io::stdin().read_to_string(&mut k).map_err(|e| format!("couldn't read stdin: {e}"))?;
        let k = k.trim().to_string();
        return if looks_right(name, &k) { Ok(k) } else { Err(wrong()) };
    }
    let (ok, clip) = run(&mut Command::new(tool("KEYS_PBPASTE", "pbpaste")), None);
    let clip = clip.trim().to_string();
    if ok && looks_right(name, &clip) {
        println!("Key: from the clipboard.");
        return Ok(clip);
    }
    println!("The clipboard doesn't hold a {}.", shape(name));
    if std::env::var_os("KEYS_NO_PROMPT").is_some() {
        return Err("copy the key and run again".into());
    }
    let tty = std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty").map_err(|_| "no terminal to ask in; copy the key and run again".to_string())?;
    let mut out = tty.try_clone().map_err(|e| e.to_string())?;
    let _ = write!(out, "Paste the key (it won't show): ");
    let _ = out.flush();
    let quiet = |on: bool| {
        let _ = Command::new("stty").arg(if on { "-echo" } else { "echo" }).stdin(tty.try_clone().map(Stdio::from).unwrap_or(Stdio::null())).status();
    };
    quiet(true);
    let mut line = String::new();
    let got = std::io::BufReader::new(&tty).read_line(&mut line);
    quiet(false);
    let _ = writeln!(out);
    got.map_err(|e| e.to_string())?;
    let k = line.trim().to_string();
    if looks_right(name, &k) { Ok(k) } else { Err(wrong()) }
}

fn security() -> Command {
    Command::new(tool("KEYS_SECURITY", "/usr/bin/security"))
}

/// z's keychain, through macOS's `security`, the command on its stdin
/// (never a command line); then ccez-words restarted to read DeepSeek's now.
fn store_z(name: &str, key: &str) -> Result<String, String> {
    let line = format!("add-generic-password -U -s {SERVICE} -a {name} -l \"{} API key (ccez)\" -w {key}\n", label(name));
    let (ok, out) = run(security().arg("-i"), Some(&line));
    if !ok || out.contains("returned") || out.contains("rror") {
        return Err(format!("the keychain refused it: {}", last_line(&out)));
    }
    if name != "deepseek" {
        return Ok("in z's keychain".into());
    }
    Ok(match restart_words() {
        Ok(()) => "in z's keychain; ccez-words restarted".into(),
        Err(e) => format!("in z's keychain; ccez-words didn't restart ({e}), it reads the key within a minute anyway"),
    })
}

fn restart_words() -> Result<(), String> {
    let uid = run(Command::new("id").arg("-u"), None).1.trim().to_string();
    let (ok, out) = run(Command::new(tool("KEYS_LAUNCHCTL", "launchctl")).args(["kickstart", "-k", &format!("gui/{uid}/uk.ccez.words")]), None);
    if ok { Ok(()) } else { Err(last_line(&out)) }
}

fn remove_z(name: &str) -> Result<String, String> {
    let (ok, out) = run(security().args(["delete-generic-password", "-s", SERVICE, "-a", name]), None);
    if ok {
        if name == "deepseek" {
            let _ = restart_words();
        }
        Ok("removed from z's keychain".into())
    } else if out.contains("could not be found") {
        Ok("wasn't in z's keychain".into())
    } else {
        Err(last_line(&out))
    }
}

/// The keys in z's keychain under [SERVICE]: (name, when it was last
/// set, "YYYY-MM-DD HH:MM UTC"). Attributes only: `dump-keychain` without
/// -d never reads a secret.
fn z_keys() -> Result<Vec<(String, String)>, String> {
    let (ok, out) = run(security().arg("dump-keychain"), None);
    if !ok {
        return Err(format!("couldn't list z's keychain: {}", last_line(&out)));
    }
    Ok(parse_dump(&out))
}

fn parse_dump(out: &str) -> Vec<(String, String)> {
    let attr = |item: &str, name: &str| -> Option<String> {
        let line = item.lines().find(|l| l.trim_start().starts_with(&format!("\"{name}\"")))?;
        if let Some(at) = line.find("<blob>=\"") {
            let rest = &line[at + 8..];
            return Some(rest[..rest.rfind('"')?].to_string());
        }
        // "mdat"<timedate>=0x…  "20261116154411Z\000"
        let quoted = line.rsplit_once("  \"")?.1;
        Some(quoted.trim_end_matches('"').trim_end_matches("\\000").to_string())
    };
    let mut keys: Vec<(String, String)> = out
        .split("keychain: ")
        .filter(|item| attr(item, "svce").as_deref() == Some(SERVICE))
        .filter_map(|item| {
            let name = attr(item, "acct")?;
            let t = attr(item, "mdat").unwrap_or_default();
            let when = if t.len() >= 14 && t[..14].chars().all(|c| c.is_ascii_digit()) { local_time(&t[..14]) } else { "at an unknown time".into() };
            Some((name, when))
        })
        .collect();
    keys.sort();
    keys.dedup_by(|a, b| a.0 == b.0);
    keys
}

/// "YYYYMMDDhhmmss" in UTC as z's local time, "YYYY-MM-DD HH:MM" (UTC
/// if `date` can't say).
fn local_time(t: &str) -> String {
    let (ok, out) = run(Command::new("/bin/date").args(["-j", "-f", "%Y%m%d%H%M%S%z", &format!("{t}+0000"), "+%Y-%m-%d %H:%M"]), None);
    if ok && out.trim().len() == 16 { out.trim().to_string() } else { format!("{}-{}-{} {}:{} UTC", &t[0..4], &t[4..6], &t[6..8], &t[8..10], &t[10..12]) }
}

/// The key itself from z's keychain, for a test call (never printed).
fn z_key(name: &str) -> Result<String, String> {
    let (ok, out) = run(security().args(["find-generic-password", "-s", SERVICE, "-a", name, "-w"]), None);
    let k = out.trim().to_string();
    if ok && !k.is_empty() && !k.contains(' ') { Ok(k) } else { Err("no key in z's keychain".into()) }
}

/// What the phone's ccez-keys holds: its key names, or None when it's too
/// old to hold more than DeepSeek's (then Some(["deepseek"]) or Some([])
/// from its old one-key status).
fn phone_names() -> Result<(Vec<String>, bool), String> {
    crate::serial()?;
    let status = crate::call("status", None, None)?;
    let v: serde_json::Value = serde_json::from_str(&status).map_err(|_| "the phone's status wasn't JSON".to_string())?;
    if let Some(names) = v["keys"].as_array() {
        return Ok((names.iter().filter_map(|n| n.as_str().map(str::to_string)).collect(), true));
    }
    let old = if v["ai_key"] == serde_json::json!(true) { vec!["deepseek".to_string()] } else { vec![] };
    Ok((old, false))
}

/// The phone's ccez-keys, over adb, the key on the phone shell's stdin.
/// A ccez-keys from before named keys holds only DeepSeek's.
fn store_phone(name: &str, key: &str) -> Result<String, String> {
    let (_, named) = phone_names()?;
    if !named && name != "deepseek" {
        return Err("the phone's ccez-keys holds only the DeepSeek key: update it (scripts/build.sh), then run this again".into());
    }
    let out = crate::call_stdin("key", Some(name), &crate::b64(key))?;
    Ok(format!("ccez-keys {out} it"))
}

fn remove_phone(name: &str) -> Result<String, String> {
    let (names, named) = phone_names()?;
    if !names.iter().any(|n| n == name) {
        return Ok("wasn't on the phone".into());
    }
    if !named && name != "deepseek" {
        return Err("the phone's ccez-keys is too old to remove it: update it".into());
    }
    let out = crate::call_stdin("key", Some(name), &crate::b64(" "))?;
    Ok(format!("ccez-keys {out} it"))
}

fn wrangler() -> Command {
    let mut c = Command::new(tool("KEYS_WRANGLER", "wrangler"));
    // Away from any project's wrangler config.
    c.current_dir(std::env::temp_dir());
    c
}

fn store_cloudflare(place: Place, key: &str) -> Result<String, String> {
    let (ok, out) = match place {
        Worker(w, secret) => run(wrangler().args(["secret", "put", secret, "--name", w]), Some(key)),
        Pages(p, _, secret) => run(wrangler().args(["pages", "secret", "put", secret, "--project-name", p]), Some(key)),
        _ => unreachable!(),
    };
    let live = if matches!(place, Worker(..)) { " (live now)" } else { "" };
    if ok { Ok(format!("{} set{live}", secret_of(place))) } else { Err(last_line(&out)) }
}

/// Deleted without a second question (wrangler answers its own y/N with
/// yes when nothing is typed; the owner already said yes).
fn remove_cloudflare(place: Place) -> Result<String, String> {
    let (ok, out) = match place {
        Worker(w, secret) => run(wrangler().args(["secret", "delete", secret, "--name", w]), None),
        Pages(p, _, secret) => run(wrangler().args(["pages", "secret", "delete", secret, "--project-name", p]), None),
        _ => unreachable!(),
    };
    if ok {
        Ok(format!("{} removed", secret_of(place)))
    } else if out.contains("not found") || out.contains("does not exist") || out.contains("10056") {
        Ok(format!("{} wasn't set", secret_of(place)))
    } else {
        Err(last_line(&out))
    }
}

fn secret_of(place: Place) -> &'static str {
    match place {
        Worker(_, s) | Pages(_, _, s) => s,
        _ => "",
    }
}

fn place_name(place: Place) -> String {
    match place {
        Z => "z".into(),
        Phone => "phone".into(),
        Worker(w, _) | Pages(w, _, _) => format!("Cloudflare {w}"),
    }
}

/// A Pages project redeployed so a changed secret takes effect: an empty
/// commit on main, made without touching the checkout (other sessions
/// may be working in it), which the project's git-connected build picks up.
fn redeploy(repo: &str, why: &str) -> Result<String, String> {
    let swe = std::env::var("KEYS_SWE").unwrap_or_else(|_| format!("{}/SWE", std::env::var("HOME").unwrap_or_default()));
    let git = || {
        let mut c = Command::new(tool("KEYS_GIT", "git"));
        c.arg("-C").arg(format!("{swe}/{repo}"));
        c
    };
    for _ in 0..2 {
        let (ok, out) = run(git().args(["fetch", "-q", "origin", "main"]), None);
        if !ok {
            return Err(format!("fetch: {}", last_line(&out)));
        }
        let (ok, sha) = run(git().args(["commit-tree", "origin/main^{tree}", "-p", "origin/main", "-m", &format!("Redeploy: {why} (keys key)")]), None);
        let sha = sha.trim().to_string();
        if !ok || sha.len() < 7 {
            return Err(format!("commit: {}", last_line(&sha)));
        }
        let (ok, out) = run(git().args(["push", "-q", "origin", &format!("{sha}:refs/heads/main")]), None);
        if ok {
            return Ok(format!("redeploying (commit {}): live in a few minutes", &sha[..7]));
        }
        if !out.contains("rejected") && !out.contains("fetch first") {
            return Err(format!("push: {}", last_line(&out)));
        }
    }
    Err("push: main kept moving; run it again".into())
}

/// Runs curl; `stdin` is its input (a body with `--data-binary @-`, or a
/// config with `-K -`). (ran, HTTP code or 0, body).
fn curl(args: &[&str], stdin: Option<&str>) -> (bool, u16, String) {
    let mut c = Command::new(tool("KEYS_CURL", "curl"));
    c.args(["-sS", "-m", "90", "-w", "\n%{http_code}"]).args(args);
    let (ok, out) = run(&mut c, stdin);
    let (text, code) = out.trim_end().rsplit_once('\n').unwrap_or(("", out.trim()));
    (ok, code.trim().parse().unwrap_or(0), text.to_string())
}

fn error_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body).ok().and_then(|v| v.pointer("/error/message").and_then(|m| m.as_str()).map(str::to_string)).unwrap_or_else(|| body.trim().chars().take(160).collect())
}

/// DeepSeek's answer in words.
fn said(code: u16, body: &str) -> String {
    let msg = error_message(body);
    match code {
        401 => "DeepSeek refused the key (401): check it at platform.deepseek.com".into(),
        402 => "DeepSeek's balance is used up (402): it's prepaid, top it up at platform.deepseek.com".into(),
        429 => "DeepSeek is busy or rate-limited (429): try again in a minute".into(),
        503 if msg.contains("no DeepSeek key") => "no key in z's keychain yet".into(),
        0 => format!("ccez-words unreachable: {msg}"),
        _ => format!("{code}: {msg}"),
    }
}

/// Google's answer in words.
fn said_google(code: u16, body: &str) -> String {
    let msg = error_message(body);
    match code {
        429 => "Google's free limit is used up for now (429): try again later".into(),
        400 if body.contains("API_KEY_INVALID") => "Google refused the key (400, not valid): check it at aistudio.google.com/apikey".into(),
        401 | 403 => format!("Google refused the key ({code}): {msg}"),
        0 => format!("unreachable: {msg}"),
        _ => format!("{code}: {msg}"),
    }
}

/// One tiny call through ccez-words (the keychain's DeepSeek key), and the balance.
fn test_hub() -> Result<String, String> {
    let hub = std::env::var("KEYS_HUB").unwrap_or_else(|_| HUB.to_string());
    let body = serde_json::json!({
        "model": DEEPSEEK_MODEL,
        "messages": [{"role": "user", "content": "Reply with the single word OK."}],
        "thinking": {"type": "disabled"},
        "max_tokens": 5,
        "stream": false,
    })
    .to_string();
    let (_, code, text) = curl(&["-H", "content-type: application/json", "--data-binary", "@-", &format!("{hub}/chat/completions")], Some(&body));
    if code != 200 {
        return Err(said(code, &text));
    }
    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    let answer = v.pointer("/choices/0/message/content").and_then(|c| c.as_str()).unwrap_or("").trim().to_string();
    let (_, code, text) = curl(&[&format!("{hub}/user/balance")], None);
    let balance = if code == 200 {
        let b: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        let parts: Vec<String> = b["balance_infos"].as_array().into_iter().flatten().filter_map(|i| Some(format!("{} {}", i["total_balance"].as_str()?, i["currency"].as_str()?))).collect();
        let low = if b["is_available"] == serde_json::json!(false) { " (too low to use: top it up at platform.deepseek.com)" } else { "" };
        format!("; balance {}{low}", if parts.is_empty() { "unknown".into() } else { parts.join(", ") })
    } else {
        format!("; balance unknown ({})", said(code, &text))
    };
    Ok(format!("ok, DeepSeek answered “{answer}”{balance}"))
}

/// One tiny call to Gemini with z's key: Gemini 4 Hydrogen, low thinking.
/// curl reads its config (the key header and the body) from stdin.
fn test_gemini() -> Result<String, String> {
    let key = z_key("gemini")?;
    let body = serde_json::json!({
        "contents": [{"parts": [{"text": "Reply with the single word OK."}]}],
        "generationConfig": {"thinkingConfig": {"thinkingLevel": "low"}},
    })
    .to_string();
    let quoted = body.replace('\\', "\\\\").replace('"', "\\\"");
    let config = format!("header = \"x-goog-api-key: {key}\"\nheader = \"content-type: application/json\"\ndata-binary = \"{quoted}\"\n");
    drop(key);
    let api = std::env::var("KEYS_GEMINI").unwrap_or_else(|_| GEMINI.to_string());
    let (_, code, text) = curl(&["-K", "-", &format!("{api}/models/{GEMINI_MODEL}:generateContent")], Some(&config));
    if code != 200 {
        return Err(said_google(code, &text));
    }
    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    let answer: String = v
        .pointer("/candidates/0/content/parts")
        .and_then(|p| p.as_array())
        .into_iter()
        .flatten()
        .filter(|p| p["thought"] != serde_json::json!(true))
        .filter_map(|p| p["text"].as_str())
        .collect::<String>()
        .trim()
        .to_string();
    Ok(format!("ok, {GEMINI_MODEL} answered “{answer}”"))
}

/// One tiny call from the phone with the DeepSeek key ccez-keys holds.
fn test_phone(name: &str) -> Result<String, String> {
    if name != "deepseek" {
        return Ok("on the phone (no test call for this name)".into());
    }
    let (names, _) = phone_names()?;
    if !names.iter().any(|n| n == name) {
        return Err("no DeepSeek key on the phone".into());
    }
    match crate::call("ai-test", Some(name), None) {
        Ok(out) => Ok(format!("ok, DeepSeek answered “{}”", out.trim())),
        // The call reached the phone; its own request to DeepSeek didn't get out.
        Err(e) if e == "no connection" => Err("the phone couldn't reach DeepSeek (no connection): if it's asleep or offline, wake it and test again".into()),
        Err(e) => Err(e),
    }
}

/// Whether a Cloudflare secret is set (the apps are behind the owner's
/// login, so no call is made through them from here).
fn test_secret(place: Place) -> Result<String, String> {
    let secret = secret_of(place);
    let (ok, out) = match place {
        Worker(w, _) => run(wrangler().args(["secret", "list", "--name", w]), None),
        Pages(p, _, _) => run(wrangler().args(["pages", "secret", "list", "--project-name", p]), None),
        _ => unreachable!(),
    };
    if !ok {
        return Err(last_line(&out));
    }
    if out.contains(secret) { Ok(format!("{secret} is set (not called: the app is behind your login)")) } else { Err(format!("{secret} isn't set")) }
}

fn line(place: &str, r: Result<String, String>, failed: &mut bool) {
    match r {
        Ok(s) => println!("  {place}: {s}"),
        Err(e) => {
            *failed = true;
            println!("  {place}: FAILED, {e}");
        }
    }
}

fn test(name: &str, failed: &mut bool) {
    println!("Testing the {} key:", label(name));
    for &place in places(name) {
        let r = match place {
            Z => match name {
                "deepseek" => test_hub(),
                "gemini" => test_gemini(),
                _ => z_keys().and_then(|z| if z.iter().any(|(n, _)| n == name) { Ok("in z's keychain (no test call for this name)".into()) } else { Err("not in z's keychain".into()) }),
            },
            Phone => test_phone(name),
            _ => test_secret(place),
        };
        let what = match place {
            Z if name == "deepseek" => "z (ccez-words, for the Mac apps)".into(),
            Phone if name == "deepseek" => "phone (ccez-keys, for the phone apps)".into(),
            _ => place_name(place),
        };
        line(&what, r, failed);
    }
}

fn add(name: &str, from_stdin: bool) -> i32 {
    let key = match read_key(name, from_stdin) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let mut failed = false;
    println!("Storing the {} key:", label(name));
    let mut redeploys: Vec<(&str, String)> = Vec::new();
    for &place in places(name) {
        let r = match place {
            Z => store_z(name, &key),
            Phone => store_phone(name, &key),
            Worker(..) => store_cloudflare(place, &key),
            Pages(_, repo, _) => store_cloudflare(place, &key).inspect(|_| {
                if !redeploys.iter().any(|(r, _)| *r == repo) {
                    redeploys.push((repo, place_name(place)));
                }
            }),
        };
        line(&place_name(place), r, &mut failed);
    }
    drop(key);
    for (repo, place) in redeploys {
        line(&place, redeploy(repo, &format!("the {name} key changed")), &mut failed);
    }
    test(name, &mut failed);
    if failed {
        println!("Something failed: fix it and run keys key add {name} again (it's safe to repeat).");
    }
    i32::from(failed)
}

fn ls() -> i32 {
    let z = match z_keys() {
        Ok(z) => z,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let phone = phone_names();
    let mut names: Vec<String> = PLACES.iter().map(|k| k.name.to_string()).collect();
    for n in z.iter().map(|(n, _)| n.clone()).chain(phone.as_ref().map(|(p, _)| p.clone()).unwrap_or_default()) {
        if !names.contains(&n) {
            names.push(n);
        }
    }
    for name in &names {
        let when = z.iter().find(|(n, _)| n == name).map(|(_, w)| format!("set {w}")).unwrap_or_else(|| "not on z".into());
        println!("{name}: {when}");
        let on_phone = phone.as_ref().ok().map(|(p, _)| p.contains(name));
        let at: Vec<String> = places(name)
            .iter()
            .map(|&p| match p {
                Z => format!("z{}", if z.iter().any(|(n, _)| n == name) { "" } else { " (missing)" }),
                Phone => format!(
                    "phone{}",
                    match on_phone {
                        Some(true) => "",
                        Some(false) => " (missing)",
                        None => " (not reachable)",
                    }
                ),
                Worker(w, s) | Pages(w, _, s) => format!("{w} {s}"),
            })
            .collect();
        println!("  {}", at.join(", "));
    }
    if let Err(e) = phone {
        println!("(phone: {e})");
    }
    0
}

fn rm(name: &str) -> i32 {
    let at: Vec<String> = places(name)
        .iter()
        .map(|&p| match p {
            Worker(..) | Pages(..) => format!("{} {}", place_name(p), secret_of(p)),
            _ => place_name(p),
        })
        .collect();
    print!("Remove the {} key from {}? [y/N] ", label(name), at.join(", "));
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    let _ = std::io::stdin().lock().read_line(&mut answer);
    if !matches!(answer.trim(), "y" | "Y" | "yes" | "Yes") {
        println!("Nothing removed.");
        return 1;
    }
    let mut failed = false;
    let mut redeploys: Vec<(&str, String)> = Vec::new();
    for &place in places(name) {
        let r = match place {
            Z => remove_z(name),
            Phone => remove_phone(name),
            Worker(..) => remove_cloudflare(place),
            Pages(_, repo, _) => remove_cloudflare(place).inspect(|_| {
                if !redeploys.iter().any(|(r, _)| *r == repo) {
                    redeploys.push((repo, place_name(place)));
                }
            }),
        };
        line(&place_name(place), r, &mut failed);
    }
    for (repo, place) in redeploys {
        line(&place, redeploy(repo, &format!("the {name} key was removed")), &mut failed);
    }
    i32::from(failed)
}

const USAGE: &str = "usage: keys key add <name> [--stdin] | keys key ls | keys key rm <name> | keys key test [<name>]";

/// `keys key …`; the exit code: 0, 1 if anything failed, 2 for a wrong command.
pub fn main(args: &[String]) -> i32 {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let named = |n: &str, run: &dyn Fn(&str) -> i32| {
        if valid_name(n) {
            run(n)
        } else {
            eprintln!("{n:?} can't be a key's name: lowercase letters, digits and -, a letter first");
            2
        }
    };
    match args.as_slice() {
        ["add", n] => named(n, &|n| add(n, false)),
        ["add", n, "--stdin"] | ["add", "--stdin", n] => named(n, &|n| add(n, true)),
        ["ls"] => ls(),
        ["rm", n] => named(n, &rm),
        ["test"] => {
            let mut failed = false;
            let mut names: Vec<String> = match z_keys() {
                Ok(z) => z.into_iter().map(|(n, _)| n).collect(),
                Err(e) => {
                    eprintln!("{e}");
                    return 1;
                }
            };
            if let Ok((p, _)) = phone_names() {
                names.extend(p.into_iter().filter(|n| valid_name(n)));
            }
            names.sort();
            names.dedup();
            if names.is_empty() {
                println!("No keys yet: keys key add <name>.");
            }
            for n in names {
                test(&n, &mut failed);
            }
            i32::from(failed)
        }
        ["test", n] => named(n, &|n| {
            let mut failed = false;
            test(n, &mut failed);
            i32::from(failed)
        }),
        [] if !std::io::IsTerminal::is_terminal(&std::io::stdin()) => {
            eprintln!("`keys key < file` is now `keys key add deepseek --stdin` (it puts the key everywhere DeepSeek's is used)");
            2
        }
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}

/// `keys ai-key [--stdin] | keys ai-key --test`: the old name for
/// `keys key add deepseek` and `keys key test deepseek`.
pub fn ai_key(args: &[String]) -> i32 {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let mapped: Vec<String> = match a.as_slice() {
        [] => vec!["add", "deepseek"],
        ["--stdin"] => vec!["add", "deepseek", "--stdin"],
        ["--test"] => vec!["test", "deepseek"],
        _ => {
            eprintln!("usage: keys ai-key [--stdin] | keys ai-key --test (the same as keys key add|test deepseek)");
            return 2;
        }
    }
    .into_iter()
    .map(str::to_string)
    .collect();
    main(&mapped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_must_look_right_for_their_name() {
        // Fakes built here, so no key-shaped string sits in the source.
        let ds = format!("sk-{}", "0123456789abcdef".repeat(2));
        assert!(looks_deepseek(&ds));
        assert!(!looks_deepseek(&ds[3..]));
        assert!(!looks_deepseek("sk-short"));
        assert!(!looks_deepseek(&format!("sk-{} {}", "0123456789abcdef", "0123456789abcdef")));
        assert!(!looks_deepseek(&format!("sk-{}\"; rm -rf /", "0123456789abcdef")));
        assert!(!looks_deepseek(&format!("Paste me: {ds}")));
        let aq = format!("AQ.{}-_x", "Ab8fake0".repeat(5));
        let aiza = format!("AIza{}", "Fake0".repeat(7));
        assert!(looks_gemini(&aq));
        assert!(looks_gemini(&aiza));
        assert!(!looks_gemini(&aiza[..38]));
        assert!(!looks_gemini(&ds));
        assert!(!looks_gemini("AQ.short"));
        assert!(!looks_gemini(&format!("{aq}\"x")));
        assert!(looks_right("gemini", &aq));
        assert!(!looks_right("deepseek", &aq));
        assert!(looks_right("other", "anything-without-spaces"));
        assert!(!looks_right("other", "two words"));
    }

    #[test]
    fn names_are_plain_words() {
        assert!(valid_name("deepseek"));
        assert!(valid_name("open-router2"));
        assert!(!valid_name("DeepSeek"));
        assert!(!valid_name("-x"));
        assert!(!valid_name("a b"));
        assert!(!valid_name("a;rm"));
        assert!(!valid_name(""));
    }

    #[test]
    fn each_name_has_its_places() {
        assert_eq!(places("gemini"), &[Z, Pages("art-website", "art-website", "GEMINI_API_KEY")]);
        assert!(places("deepseek").contains(&Phone));
        assert!(places("deepseek").contains(&Worker("ccez-keeps", "DEEPSEEK_API_KEY")));
        assert_eq!(places("anything-else"), &[Z, Phone]);
    }

    #[test]
    fn the_keychain_listing_gives_names_and_times_only() {
        let dump = "keychain: \"/x/login.keychain-db\"\nversion: 512\nclass: \"genp\"\nattributes:\n    \"acct\"<blob>=\"deepseek\"\n    \"mdat\"<timedate>=0x32303236313131363135343431315A00  \"20261116154411Z\\000\"\n    \"svce\"<blob>=\"uk.ccez.ai\"\nkeychain: \"/x/login.keychain-db\"\nclass: \"genp\"\nattributes:\n    \"acct\"<blob>=\"someone\"\n    \"svce\"<blob>=\"other.service\"\n";
        let got = parse_dump(dump);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, "deepseek");
        // z's local time (Calgary: 09:44), or UTC where `date` can't convert.
        assert!(got[0].1.starts_with("2026-11-16 ") && got[0].1.len() >= 16, "{}", got[0].1);
    }

    #[test]
    fn answers_are_worded_for_the_owner() {
        assert!(said(402, "{}").contains("prepaid"));
        assert!(said(401, "{}").contains("refused the key"));
        assert_eq!(said(503, "no DeepSeek key on this Mac: run keys ai-key"), "no key in z's keychain yet");
        assert_eq!(said(500, r#"{"error":{"message":"boom"}}"#), "500: boom");
        assert!(said_google(429, "{}").contains("free limit is used up"));
        assert!(said_google(400, r#"{"error":{"message":"API key not valid.","details":[{"reason":"API_KEY_INVALID"}]}}"#).contains("refused the key"));
        assert_eq!(last_line("\u{1b}[31m✘ [ERROR]\u{1b}[0m Project \"x\" does not exist.\n\n🪵  Logs were written\n"), "✘ [ERROR] Project \"x\" does not exist.");
    }
}
