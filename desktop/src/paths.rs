//! Where things are kept, per system.

use std::path::PathBuf;

/// The user's home folder.
pub fn home() -> PathBuf {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

/// The app's own folder (settings, dictionaries, word statuses, log):
/// ~/Library/Application Support/ccez-keys on the Mac, ~/.local/share/ccez-keys
/// on Linux, %APPDATA%\ccez-keys on Windows. Made if missing.
pub fn data() -> PathBuf {
    let d = if cfg!(target_os = "macos") {
        home().join("Library/Application Support/ccez-keys")
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(home).join("ccez-keys")
    } else {
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/share")).join("ccez-keys")
    };
    let _ = std::fs::create_dir_all(&d);
    d
}
