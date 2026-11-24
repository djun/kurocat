//! Lightweight "is the user listening to music?" watcher.
//!
//! Rather than capturing system audio (permissions, privacy, false positives from
//! meetings/videos), we only look for well-known music player processes. This keeps
//! the feature permission-free and dependency-free on all three platforms.
//!
//! The watcher polls every few seconds and emits `kuro-media` with the current
//! state, so the frontend simply toggles the cat's relaxed mood.

#[cfg(any(target_os = "macos", windows))]
use std::process::Command;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Lower-case path / executable fragments of popular music players.
/// Matched against the full macOS executable path, Windows image names,
/// or the Linux `/proc/<pid>/comm` value.
#[cfg(target_os = "macos")]
const MUSIC_APP_HINTS: &[&str] = &[
    "/music.app/",
    "/spotify.app/",
    "/qqmusic.app/",
    "/neteasemusic.app/",
    "/kugou.app/",
    "/kugoumusic.app/",
    "/kuwo.app/",
    "/kuwomusic.app/",
    "/qishui.app/",
    "/qishuimusic.app/",
    "/yesplaymusic.app/",
    "/foobar2000.app/",
    "/vox.app/",
    "/strawberry.app/",
    "/swinsian.app/",
    "/netease music.app/",
];

#[cfg(windows)]
const MUSIC_APP_HINTS: &[&str] = &[
    "spotify.exe",
    "qqmusic.exe",
    "cloudmusic.exe",
    "neteasecloudmusic.exe",
    "netease music.exe",
    "kugou.exe",
    "kwmusic.exe",
    "kwplayer.exe",
    "qishui.exe",
    "qishuimusic.exe",
    "douyinmusic",
    "music.ui.exe",
    "itunes.exe",
    "foobar2000.exe",
    "aimp.exe",
    "yesplaymusic.exe",
    "listen1.exe",
];

#[cfg(target_os = "linux")]
const MUSIC_APP_HINTS: &[&str] = &[
    "spotify",
    "qqmusic",
    "cloudmusic",
    "netease-cloud-mus",
    "kugou",
    "kwmusic",
    "qishui",
    "rhythmbox",
    "clementine",
    "strawberry",
    "deadbeef",
    "audacious",
    "elisa",
    "mpd",
    "cmus",
    "museeks",
];

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
const MUSIC_APP_HINTS: &[&str] = &[];

fn line_is_music_app(line: &str) -> bool {
    let lower = line.to_lowercase();
    MUSIC_APP_HINTS
        .iter()
        .any(|hint| lower.contains(hint) && !hint.is_empty())
}

fn env_force_music() -> Option<bool> {
    std::env::var("KUROCAT_MUSIC").ok().and_then(|value| {
        let v = value.trim();
        if v == "1" || v.eq_ignore_ascii_case("true") {
            Some(true)
        } else if v == "0" || v.eq_ignore_ascii_case("false") {
            Some(false)
        } else {
            None
        }
    })
}

#[cfg(target_os = "macos")]
fn music_app_running() -> bool {
    // `comm` alone is often just the app name ("Spotify"), so include the full
    // executable path (`args`) to match `/Applications/Spotify.app/...` reliably.
    let Ok(output) = Command::new("/bin/ps")
        .args(["-axo", "comm=,args="])
        .output()
    else {
        return false;
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines().any(line_is_music_app)
}

#[cfg(windows)]
fn music_app_running() -> bool {
    let Ok(output) = Command::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .output()
    else {
        return false;
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines().any(line_is_music_app)
}

#[cfg(target_os = "linux")]
fn music_app_running() -> bool {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name();
        let pid = name.to_string_lossy();
        if pid.is_empty() || !pid.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
        std::fs::read_to_string(entry.path().join("comm"))
            .map(|comm| line_is_music_app(&comm))
            .unwrap_or(false)
    })
}

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
fn music_app_running() -> bool {
    false
}

/// Public probe kept simple so it can be unit-tested / reused elsewhere.
pub fn is_music_playing() -> bool {
    if let Some(forced) = env_force_music() {
        return forced;
    }
    music_app_running()
}

pub fn start_media_watcher(app: AppHandle) {
    thread::spawn(move || {
        // Emit the current state on every poll: the webview may attach its listener
        // after the first scan, and `setMusicPlaying` is idempotent on the frontend.
        loop {
            let _ = app.emit("kuro-media", is_music_playing());
            thread::sleep(POLL_INTERVAL);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_music_players_configured_for_this_platform() {
        #[cfg(target_os = "macos")]
        {
            assert!(line_is_music_app(
                "/Applications/Spotify.app/Contents/MacOS/Spotify"
            ));
            assert!(line_is_music_app(
                "/System/Applications/Music.app/Contents/MacOS/Music"
            ));
            assert!(line_is_music_app(
                "/Applications/QQMusic.app/Contents/MacOS/QQMusic"
            ));
            assert!(line_is_music_app(
                "/Applications/NeteaseMusic.app/Contents/MacOS/NeteaseMusic"
            ));
        }
        #[cfg(windows)]
        {
            assert!(line_is_music_app(
                "\"Spotify.exe\",\"1234\",\"Console\",\"1\",\"120,000 K\""
            ));
            assert!(line_is_music_app(
                "\"cloudmusic.exe\",\"1234\",\"Console\",\"1\",\"120,000 K\""
            ));
        }
        #[cfg(target_os = "linux")]
        {
            assert!(line_is_music_app("spotify\n"));
            assert!(line_is_music_app("kugou\n"));
        }
    }

    #[test]
    fn ignores_unrelated_processes() {
        assert!(!line_is_music_app("/Applications/Visual Studio Code.app/Contents/MacOS/Electron"));
        assert!(!line_is_music_app("WeChatAppEx"));
        assert!(!line_is_music_app("\"chrome.exe\",\"1\",\"Console\",\"1\",\"1 K\""));
        assert!(!line_is_music_app(""));
    }
}
