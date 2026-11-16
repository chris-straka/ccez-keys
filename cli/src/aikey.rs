//! `keys ai-key`: the owner's DeepSeek key, entered once on z and put
//! everywhere that needs it (owner, 2026-11-16):
//!
//! - z's login keychain (service `uk.ccez.ai`, account `deepseek`), where
//!   ccez-words reads it to serve every Mac and tailnet app;
//! - the phone's ccez-keys (over adb), which lends it to the phone apps
//!   through its keyring;
//! - the Cloudflare apps, which can't reach the tailnet: the Worker
//!   ccez-keeps and the Pages projects art-website and ccez-llm's web
//!   build, as the secret `DEEPSEEK_API_KEY` (a Pages secret only takes
//!   effect in a new deployment, so those two are redeployed: an empty
//!   commit on main, which their git-connected builds pick up).
//!
//! The key comes from the clipboard (`pbpaste`), else a hidden prompt, or
//! from stdin with `--stdin`. It must look like a DeepSeek key (`sk-…`)
//! before it goes anywhere. It is never printed, logged or put on a
//! command line: each tool gets it on stdin. `--test` makes one tiny real
//! call through each place and prints ok or the error.
//!
//! The tools are found on PATH; tests point them at stand-ins with
//! KEYS_SECURITY, KEYS_LAUNCHCTL, KEYS_PBPASTE, KEYS_WRANGLER, KEYS_GIT,
//! KEYS_CURL, KEYS_ADB, KEYS_SWE (the repos' folder) and KEYS_HUB
//! (ccez-words); KEYS_NO_PROMPT turns the hidden prompt off.

use std::io::{Read, Write};
use std::process::{Command, Stdio};

const SERVICE: &str = "uk.ccez.ai";
const ACCOUNT: &str = "deepseek";
const SECRET: &str = "DEEPSEEK_API_KEY";
const HUB: &str = "https://z.tailfe37c2.ts.net:8448/ai/deepseek";
const MODEL: &str = "deepseek-flash";
/// Workers with the secret.
const WORKERS: &[&str] = &["ccez-keeps"];
/// Pages projects with the secret, and their repos under ~/SWE.
const PAGES: &[(&str, &str)] = &[("art-website", "art-website"), ("personalized-llm-client", "ccez-llm")];

fn tool(env: &str, default: &str) -> String {
    std::env::var(env).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| default.to_string())
}

/// Whether `k` looks like a DeepSeek key: `sk-` and letters, digits, - or _.
pub fn looks_right(k: &str) -> bool {
    k.starts_with("sk-") && (20..=200).contains(&k.len()) && k[3..].chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
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
fn read_key(from_stdin: bool) -> Result<String, String> {
    if from_stdin {
        let mut k = String::new();
        std::io::stdin().read_to_string(&mut k).map_err(|e| format!("couldn't read stdin: {e}"))?;
        let k = k.trim().to_string();
        return if looks_right(&k) { Ok(k) } else { Err("that isn't a DeepSeek key (they start with sk-); nothing was stored".into()) };
    }
    let (ok, clip) = run(&mut Command::new(tool("KEYS_PBPASTE", "pbpaste")), None);
    let clip = clip.trim().to_string();
    if ok && looks_right(&clip) {
        println!("Key: from the clipboard.");
        return Ok(clip);
    }
    println!("The clipboard doesn't hold a DeepSeek key (sk-…).");
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
    let got = std::io::BufRead::read_line(&mut std::io::BufReader::new(&tty), &mut line);
    quiet(false);
    let _ = writeln!(out);
    got.map_err(|e| e.to_string())?;
    let k = line.trim().to_string();
    if looks_right(&k) { Ok(k) } else { Err("that isn't a DeepSeek key (they start with sk-); nothing was stored".into()) }
}

/// z's keychain, through macOS's `security`, the command on its stdin
/// (never a command line); then ccez-words restarted to read it now.
fn store_keychain(key: &str) -> Result<String, String> {
    let line = format!("add-generic-password -U -s {SERVICE} -a {ACCOUNT} -l \"DeepSeek API key (ccez)\" -w {key}\n");
    let (ok, out) = run(Command::new(tool("KEYS_SECURITY", "/usr/bin/security")).arg("-i"), Some(&line));
    if !ok || out.contains("returned") || out.contains("rror") {
        return Err(format!("the keychain refused it: {}", last_line(&out)));
    }
    let uid = run(Command::new("id").arg("-u"), None).1.trim().to_string();
    let (ok, out) = run(Command::new(tool("KEYS_LAUNCHCTL", "launchctl")).args(["kickstart", "-k", &format!("gui/{uid}/uk.ccez.words")]), None);
    Ok(if ok { "in z's keychain; ccez-words restarted".into() } else { format!("in z's keychain; ccez-words didn't restart ({}), it reads the key within a minute anyway", last_line(&out)) })
}

/// The phone's ccez-keys, over adb.
fn store_phone(key: &str) -> Result<String, String> {
    crate::serial()?;
    let out = crate::call_stdin("key", None, &crate::b64(key))?;
    Ok(format!("ccez-keys {out} it"))
}

fn wrangler() -> Command {
    let mut c = Command::new(tool("KEYS_WRANGLER", "wrangler"));
    // Away from any project's wrangler config.
    c.current_dir(std::env::temp_dir());
    c
}

fn store_worker(name: &str, key: &str) -> Result<String, String> {
    let (ok, out) = run(wrangler().args(["secret", "put", SECRET, "--name", name]), Some(key));
    if ok { Ok(format!("{SECRET} set (live now)")) } else { Err(last_line(&out)) }
}

fn store_pages(project: &str, key: &str) -> Result<String, String> {
    let (ok, out) = run(wrangler().args(["pages", "secret", "put", SECRET, "--project-name", project]), Some(key));
    if ok { Ok(format!("{SECRET} set")) } else { Err(last_line(&out)) }
}

/// A Pages project redeployed so its new secret takes effect: an empty
/// commit on main, made without touching the checkout (other sessions
/// may be working in it), which the project's git-connected build picks up.
fn redeploy(repo: &str) -> Result<String, String> {
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
        let (ok, sha) = run(
            git().args(["commit-tree", "origin/main^{tree}", "-p", "origin/main", "-m", "Redeploy: the DeepSeek key changed (keys ai-key)"]),
            None,
        );
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
    Err("push: main kept moving; run keys ai-key again".into())
}

fn curl(args: &[&str], body: Option<&str>) -> (bool, u16, String) {
    let mut c = Command::new(tool("KEYS_CURL", "curl"));
    c.args(["-sS", "-m", "90", "-w", "\n%{http_code}"]).args(args);
    let (ok, out) = run(&mut c, body);
    let (text, code) = out.trim_end().rsplit_once('\n').unwrap_or(("", out.trim()));
    (ok, code.trim().parse().unwrap_or(0), text.to_string())
}

/// An HTTP answer in words.
fn said(code: u16, body: &str) -> String {
    let msg = serde_json::from_str::<serde_json::Value>(body).ok().and_then(|v| v.pointer("/error/message").and_then(|m| m.as_str()).map(str::to_string)).unwrap_or_else(|| body.trim().chars().take(160).collect());
    match code {
        401 => "DeepSeek refused the key (401): check it at platform.deepseek.com".into(),
        402 => "DeepSeek's balance is used up (402): it's prepaid, top it up at platform.deepseek.com".into(),
        429 => "DeepSeek is busy or rate-limited (429): try again in a minute".into(),
        503 if msg.contains("no DeepSeek key") => "no key in z's keychain yet".into(),
        0 => format!("unreachable: {msg}"),
        _ => format!("{code}: {msg}"),
    }
}

/// One tiny call through ccez-words (the keychain's key), and the balance.
fn test_hub() -> Result<String, String> {
    let hub = std::env::var("KEYS_HUB").unwrap_or_else(|_| HUB.to_string());
    let body = serde_json::json!({
        "model": MODEL,
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

/// One tiny call from the phone with the key ccez-keys holds.
fn test_phone() -> Result<String, String> {
    crate::serial()?;
    let out = crate::call("ai-test", None, None)?;
    Ok(format!("ok, DeepSeek answered “{}”", out.trim()))
}

/// Whether the secret is there (the apps are behind the owner's login,
/// so no call is made through them from here).
fn test_secret(list: &mut Command) -> Result<String, String> {
    let (ok, out) = run(list, None);
    if !ok {
        return Err(last_line(&out));
    }
    if out.contains(SECRET) { Ok(format!("{SECRET} is set (not called: the app is behind your login)")) } else { Err(format!("{SECRET} isn't set")) }
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

fn tests(failed: &mut bool) {
    println!("Testing:");
    line("z (ccez-words, for the Mac apps)", test_hub(), failed);
    line("phone (ccez-keys, for the phone apps)", test_phone(), failed);
    for w in WORKERS {
        line(&format!("Cloudflare {w}"), test_secret(wrangler().args(["secret", "list", "--name", w])), failed);
    }
    for (p, _) in PAGES {
        line(&format!("Cloudflare {p}"), test_secret(wrangler().args(["pages", "secret", "list", "--project-name", p])), failed);
    }
}

/// `keys ai-key [--stdin] [--test]`; the exit code: 0, or 1 if anything failed.
pub fn main(args: &[String]) -> i32 {
    let only_test = args.iter().any(|a| a == "--test") && !args.iter().any(|a| a == "--set");
    let from_stdin = args.iter().any(|a| a == "--stdin");
    if let Some(bad) = args.iter().find(|a| !matches!(a.as_str(), "--test" | "--stdin" | "--set")) {
        eprintln!("keys ai-key: unknown option {bad}\nusage: keys ai-key [--stdin] | keys ai-key --test");
        return 2;
    }
    let mut failed = false;
    if only_test {
        tests(&mut failed);
        return i32::from(failed);
    }
    let key = match read_key(from_stdin) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    println!("Storing the DeepSeek key:");
    line("z", store_keychain(&key), &mut failed);
    line("phone", store_phone(&key), &mut failed);
    for w in WORKERS {
        line(&format!("Cloudflare {w}"), store_worker(w, &key), &mut failed);
    }
    for (p, repo) in PAGES {
        let r = store_pages(p, &key).and_then(|s| redeploy(repo).map(|d| format!("{s}; {d}")));
        line(&format!("Cloudflare {p}"), r, &mut failed);
    }
    drop(key);
    tests(&mut failed);
    if failed {
        println!("Something failed: fix it and run keys ai-key again (it's safe to repeat).");
    }
    i32::from(failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_deepseek_shaped_keys_pass() {
        assert!(looks_right("sk-0123456789abcdef0123456789abcdef"));
        assert!(!looks_right("0123456789abcdef0123456789abcdef"));
        assert!(!looks_right("sk-short"));
        assert!(!looks_right("sk-0123456789abcdef 0123456789abcdef"));
        assert!(!looks_right("sk-0123456789abcdef\"; rm -rf /"));
        assert!(!looks_right("Paste me: sk-0123456789abcdef0123456789abcdef"));
    }

    #[test]
    fn answers_are_worded_for_the_owner() {
        assert!(said(402, "{}").contains("prepaid"));
        assert!(said(401, "{}").contains("refused the key"));
        assert_eq!(said(503, "no DeepSeek key on this Mac: run keys ai-key"), "no key in z's keychain yet");
        assert_eq!(said(500, r#"{"error":{"message":"boom"}}"#), "500: boom");
        assert_eq!(last_line("\u{1b}[31m✘ [ERROR]\u{1b}[0m Project \"x\" does not exist.\n\n🪵  Logs were written\n"), "✘ [ERROR] Project \"x\" does not exist.");
    }
}
