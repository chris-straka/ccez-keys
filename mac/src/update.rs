//! Updates from the repo's GitHub releases, as the phone does: a few
//! minutes after start and then every six hours, the latest release's Mac
//! zip, checked against its SHA-256 and against this app's own signature
//! (the same certificate, so Accessibility stays granted), replaces this
//! app and starts again. Nothing is asked: the owner's Macs keep current.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const LATEST: &str = "https://api.github.com/repos/chris-straka/ccez-keys/releases/latest";

/// "0.1.84" or "v0.1.84" → [0, 1, 84].
pub fn parse(v: &str) -> Vec<u64> {
    v.trim().trim_start_matches('v').split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

pub fn newer(latest: &str, mine: &str) -> bool {
    parse(latest) > parse(mine)
}

fn curl(url: &str, to: Option<&Path>) -> Option<String> {
    let mut c = Command::new("curl");
    c.args(["-sSfL", "--max-time", "300", "-H", "User-Agent: ccez-keys-mac"]);
    if let Some(p) = to {
        c.arg("-o").arg(p);
    }
    let out = c.arg(url).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The app bundle this runs from, and its version.
fn me() -> Option<(PathBuf, String)> {
    let exe = std::env::current_exe().ok()?;
    let app = exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app"))?.to_path_buf();
    let plist = std::fs::read_to_string(app.join("Contents/Info.plist")).ok()?;
    let at = plist.find("CFBundleShortVersionString")?;
    let rest = &plist[at..];
    let v = rest.split("<string>").nth(1)?.split("</string>").next()?.to_string();
    Some((app, v))
}

/// One look for a newer release; true when this app was replaced (and should quit).
pub fn check(log: &dyn Fn(&str, &str), relaunch: bool) -> bool {
    let Some((app, mine)) = me() else { return false };
    let Some(json) = curl(LATEST, None) else { return false };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&json) else { return false };
    let tag = v["tag_name"].as_str().unwrap_or("");
    if !newer(tag, &mine) {
        return false;
    }
    let asset = |suffix: &str| {
        v["assets"].as_array()?.iter().find(|a| a["name"].as_str().is_some_and(|n| n.starts_with("ccez-keys-mac-") && n.ends_with(suffix)))?["browser_download_url"]
            .as_str()
            .map(str::to_string)
    };
    let (Some(zip_url), Some(sum_url)) = (asset(".zip"), asset(".zip.sha256")) else { return false };
    let tmp = std::env::temp_dir().join(format!("ccez-keys-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    if std::fs::create_dir_all(&tmp).is_err() {
        return false;
    }
    let done = |ok: bool, what: &str| {
        if !what.is_empty() {
            log(if ok { "info" } else { "warn" }, what);
        }
        ok
    };
    let zip = tmp.join("app.zip");
    let (Some(_), Some(sum)) = (curl(&zip_url, Some(&zip)), curl(&sum_url, None)) else { return done(false, "update: download failed") };
    let want = sum.split_whitespace().next().unwrap_or("").to_string();
    let got = Command::new("shasum").args(["-a", "256"]).arg(&zip).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().next().unwrap_or("").to_string());
    if want.is_empty() || got.as_deref() != Some(want.as_str()) {
        return done(false, &format!("update {tag}: checksum mismatch"));
    }
    let unpacked = tmp.join("x");
    let ok = Command::new("ditto").args(["-x", "-k"]).arg(&zip).arg(&unpacked).status().is_ok_and(|s| s.success());
    let new_app = unpacked.join("ccez-keys.app");
    if !ok || !new_app.exists() {
        return done(false, &format!("update {tag}: unpack failed"));
    }
    // Signed by the same certificate as this app, or not taken.
    let req = Command::new("codesign").args(["-d", "-r-"]).arg(&app).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr));
    let req = req.as_deref().and_then(|r| r.lines().find_map(|l| l.strip_prefix("designated => "))).map(str::to_string);
    let Some(req) = req else { return done(false, "update: no signature to match") };
    let same = Command::new("codesign").args(["--verify", "--deep", "--strict", &format!("-R={req}")]).arg(&new_app).status().is_ok_and(|s| s.success());
    if !same {
        return done(false, &format!("update {tag}: not signed like this app"));
    }
    // Swapped in: the old one aside first, put back if the move fails.
    let old = tmp.join("old.app");
    if std::fs::rename(&app, &old).is_err() {
        return done(false, "update: can't move the app");
    }
    if std::fs::rename(&new_app, &app).is_err() && Command::new("ditto").arg(&new_app).arg(&app).status().map_or(true, |s| !s.success()) {
        let _ = std::fs::rename(&old, &app);
        return done(false, "update: can't put the new app in place");
    }
    let _ = Command::new("xattr").args(["-dr", "com.apple.quarantine"]).arg(&app).status();
    done(true, &format!("updated {mine} → {tag}"));
    if !relaunch {
        let _ = std::fs::remove_dir_all(&tmp);
        return true;
    }
    // The new one starts once this has quit.
    let _ = Command::new("sh").arg("-c").arg(format!("sleep 1; open -n {:?}; rm -rf {:?}", app, tmp)).spawn();
    true
}

/// Looks now and then in the background; quits this app when it's been replaced.
pub fn start(log: impl Fn(&str, &str) + Send + 'static) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(120));
        loop {
            if check(&log, true) {
                std::process::exit(0);
            }
            std::thread::sleep(Duration::from_secs(6 * 3600));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(newer("v0.1.85", "0.1.84"));
        assert!(newer("v0.2.0", "0.1.99"));
        assert!(!newer("v0.1.84", "0.1.84"));
        assert!(!newer("v0.1.83", "0.1.84"));
        assert!(!newer("", "0.1.84"));
    }
}
