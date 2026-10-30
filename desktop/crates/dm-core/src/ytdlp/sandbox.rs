//! yt-dlp, started inside bubblewrap where Linux allows it, and inside
//! the system's own sandbox (`sandbox-exec`) on macOS.
//!
//! yt-dlp runs code a site sends it (a player's challenge, in Deno or
//! QuickJS) and is updated from the internet, so it is given the computer
//! as read-only, with dm's staging and the temporary folders writable and
//! the places a person's secrets live hidden: browser profiles, keys and
//! keyrings, as other download managers run their yt-dlp
//! (docs/reference-behavior.md). Where `bwrap` is missing or not allowed to
//! make namespaces (some distributions, most containers), yt-dlp starts as
//! before: a download that works matters more than the fence around it.
//! On macOS the same is asked of `sandbox-exec`: writes only to the run's
//! folders, the temporary folder and the caches, and the same secrets (with
//! macOS's own places for them) neither read nor written.
//! `DM_SANDBOX=0` turns it off.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use tokio::process::Command;

/// Folders under the home folder yt-dlp has no business reading: browser
/// profiles (cookies, saved passwords), keys, keyrings and cloud logins.
/// dm hands yt-dlp the one page's cookies in a file of its own instead.
const HIDDEN: &[&str] = &[
    ".mozilla",
    ".librewolf",
    ".config/google-chrome",
    ".config/google-chrome-beta",
    ".config/chromium",
    ".config/BraveSoftware",
    ".config/microsoft-edge",
    ".config/vivaldi",
    ".config/opera",
    ".var/app",
    ".ssh",
    ".gnupg",
    ".password-store",
    ".local/share/keyrings",
    ".aws",
    ".azure",
    ".config/gcloud",
    ".kube",
    ".docker",
];

/// A command that starts `binary` (yt-dlp), inside the sandbox where it can
/// be made, with `writable` (the folders this run writes) open to it.
pub(crate) fn command(binary: &Path, writable: &[&Path]) -> Command {
    if let Some(exec) = seatbelt() {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let mut open: Vec<PathBuf> = writable.iter().map(|p| p.to_path_buf()).collect();
        open.push(std::env::temp_dir());
        if let Some(home) = &home {
            // yt-dlp's cache (its player scripts), Deno's and the system's.
            open.push(home.join(".cache"));
            open.push(home.join("Library/Caches"));
        }
        let mut command = Command::new(exec);
        command
            .arg("-p")
            .arg(profile(&open, home.as_deref()))
            .arg("--")
            .arg(binary);
        return command;
    }
    match bwrap() {
        Some(bwrap) => {
            let home = std::env::var_os("HOME").map(PathBuf::from);
            let cache = home.as_ref().map(|h| h.join(".cache/yt-dlp"));
            if let Some(cache) = &cache {
                let _ = std::fs::create_dir_all(cache);
            }
            let mut open: Vec<&Path> = writable.to_vec();
            if let Some(cache) = &cache {
                open.push(cache);
            }
            let temp = std::env::temp_dir();
            open.push(&temp);
            let mut command = Command::new(bwrap);
            command.args(args(binary, &open, home.as_deref(), |p| p.exists()));
            command
        }
        None => Command::new(binary),
    }
}

/// bubblewrap's arguments: the root read-only, `/dev` and `/proc` fresh,
/// `writable` bound over it as it is, each of [`HIDDEN`] that `exists`
/// replaced by an empty folder, then `binary`.
fn args(
    binary: &Path,
    writable: &[&Path],
    home: Option<&Path>,
    exists: impl Fn(&Path) -> bool,
) -> Vec<OsString> {
    let mut out: Vec<OsString> = [
        "--die-with-parent",
        "--unshare-ipc",
        "--unshare-uts",
        "--ro-bind",
        "/",
        "/",
        "--dev",
        "/dev",
        "--proc",
        "/proc",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    for dir in writable.iter().filter(|d| exists(d)) {
        out.extend([
            "--bind".into(),
            dir.as_os_str().into(),
            dir.as_os_str().into(),
        ]);
    }
    if let Some(home) = home {
        for hidden in HIDDEN.iter().map(|h| home.join(h)).filter(|h| exists(h)) {
            out.extend(["--tmpfs".into(), hidden.into_os_string()]);
        }
    }
    out.push("--".into());
    out.push(binary.as_os_str().into());
    out
}

/// The places on macOS, under the home folder, that yt-dlp neither reads
/// nor writes: the browsers' profiles, Safari's and the system's cookies,
/// the keychains, and the same keys and logins as on Linux.
const HIDDEN_MAC: &[&str] = &[
    "Library/Application Support/Google/Chrome",
    "Library/Application Support/Google/Chrome Beta",
    "Library/Application Support/Google/Chrome Canary",
    "Library/Application Support/Microsoft Edge",
    "Library/Application Support/Microsoft Edge Beta",
    "Library/Application Support/Microsoft Edge Dev",
    "Library/Application Support/BraveSoftware",
    "Library/Application Support/Firefox",
    "Library/Application Support/Arc",
    "Library/Application Support/Vivaldi",
    "Library/Application Support/com.operasoftware.Opera",
    "Library/Application Support/Chromium",
    "Library/Safari",
    "Library/Cookies",
    "Library/Containers/com.apple.Safari",
    "Library/Keychains",
    ".ssh",
    ".gnupg",
    ".password-store",
    ".aws",
    ".azure",
    ".config/gcloud",
    ".kube",
    ".docker",
];

/// A path as the sandbox matches it: its real one (`/var` is `/private/var`
/// on macOS), quoted for the profile.
fn quoted(path: &Path) -> String {
    let real = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let text = real.to_string_lossy();
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The macOS sandbox's profile: everything as usual, but writes only into
/// `writable` and `/dev`, and the [`HIDDEN_MAC`] places under `home` neither
/// read nor written. The rules that come later win, so the hidden places
/// stay hidden inside a writable folder.
fn profile(writable: &[PathBuf], home: Option<&Path>) -> String {
    let mut out = String::from("(version 1)\n(allow default)\n(deny file-write*)\n");
    out.push_str("(allow file-write* (subpath \"/dev\")");
    for dir in writable {
        out.push_str(&format!(" (subpath {})", quoted(dir)));
    }
    out.push_str(")\n");
    if let Some(home) = home {
        out.push_str("(deny file-read* file-write*");
        for hidden in HIDDEN_MAC {
            out.push_str(&format!(" (subpath {})", quoted(&home.join(hidden))));
        }
        out.push_str(")\n");
    }
    out
}

/// `sandbox-exec`, on macOS where it can make a sandbox here: tried once a
/// run with the profile's shape and a command that does nothing.
fn seatbelt() -> Option<&'static Path> {
    static FOUND: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    FOUND
        .get_or_init(|| {
            if !cfg!(target_os = "macos")
                || std::env::var_os("DM_SANDBOX").is_some_and(|v| v == "0")
            {
                return None;
            }
            let exec = PathBuf::from("/usr/bin/sandbox-exec");
            let home = std::env::var_os("HOME").map(PathBuf::from);
            let works = std::process::Command::new(&exec)
                .arg("-p")
                .arg(profile(&[std::env::temp_dir()], home.as_deref()))
                .arg("--")
                .arg("/usr/bin/true")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            works.then_some(exec)
        })
        .as_deref()
}

/// `bwrap`, where it is installed and can make a sandbox here: tried once a
/// run with a command that does nothing.
fn bwrap() -> Option<&'static Path> {
    static FOUND: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    FOUND
        .get_or_init(|| {
            if !cfg!(target_os = "linux")
                || std::env::var_os("DM_SANDBOX").is_some_and(|v| v == "0")
            {
                return None;
            }
            let bwrap = ["/usr/bin/bwrap", "/bin/bwrap", "/usr/local/bin/bwrap"]
                .iter()
                .map(PathBuf::from)
                .find(|p| p.is_file())?;
            let truth = ["/usr/bin/true", "/bin/true"]
                .iter()
                .map(Path::new)
                .find(|p| p.is_file())?;
            let works = std::process::Command::new(&bwrap)
                .args(args(truth, &[], None, |_| false))
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            works.then_some(bwrap)
        })
        .as_deref()
}

// Unix paths throughout, and the sandbox is Linux's: nothing here runs, or
// is needed, on Windows.
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn the_computer_is_read_only_but_for_the_runs_folders_and_secrets_are_hidden() {
        let home = Path::new("/home/p");
        let there = |p: &Path| {
            ["/stage", "/home/p/.ssh", "/home/p/.config/google-chrome"]
                .iter()
                .any(|x| p == Path::new(x))
        };
        let got: Vec<String> = args(
            Path::new("/opt/yt-dlp"),
            &[Path::new("/stage"), Path::new("/gone")],
            Some(home),
            there,
        )
        .into_iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
        let joined = got.join(" ");
        assert!(joined.starts_with("--die-with-parent"), "{joined}");
        assert!(joined.contains("--ro-bind / /"), "{joined}");
        assert!(joined.contains("--bind /stage /stage"), "{joined}");
        assert!(
            !joined.contains("/gone"),
            "a folder that is not there is not bound: {joined}"
        );
        assert!(joined.contains("--tmpfs /home/p/.ssh"), "{joined}");
        assert!(
            joined.contains("--tmpfs /home/p/.config/google-chrome"),
            "{joined}"
        );
        assert!(
            !joined.contains(".mozilla"),
            "only what exists is hidden: {joined}"
        );
        assert!(joined.ends_with("-- /opt/yt-dlp"), "{joined}");
        // Hidden after the binds, so a writable folder cannot uncover one.
        let bind = joined.find("--bind /stage").unwrap();
        let hide = joined.find("--tmpfs /home/p/.ssh").unwrap();
        assert!(bind < hide, "{joined}");
    }

    #[test]
    fn the_mac_profile_writes_only_where_it_may_and_hides_the_secrets_last() {
        let got = profile(
            &[PathBuf::from("/stage \"one\"")],
            Some(Path::new("/Users/p")),
        );
        assert!(got.starts_with("(version 1)\n(allow default)\n(deny file-write*)"));
        assert!(got.contains("(subpath \"/stage \\\"one\\\"\")"), "{got}");
        assert!(got.contains("(subpath \"/Users/p/.ssh\")"), "{got}");
        assert!(
            got.contains("(subpath \"/Users/p/Library/Application Support/Microsoft Edge\")"),
            "{got}"
        );
        let allow = got.find("(allow file-write*").unwrap();
        let deny = got.find("(deny file-read* file-write*").unwrap();
        assert!(allow < deny, "the hidden places must come last: {got}");
    }

    /// On macOS: what runs in the sandbox cannot read a hidden folder or
    /// write outside its own, and can write in its own.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn inside_the_mac_sandbox_secrets_are_hidden_and_only_staging_is_writable() {
        if seatbelt().is_none() {
            eprintln!("no sandbox can be made here; skipped");
            return;
        }
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap();
        let secret_dir = home.join(".ssh");
        let made = !secret_dir.exists();
        std::fs::create_dir_all(&secret_dir).unwrap();
        let secret = secret_dir.join(format!("dm-sandbox-test-{}", std::process::id()));
        std::fs::write(&secret, "secret").unwrap();
        let staging = tempfile::tempdir_in(&home).unwrap();
        let outside = tempfile::tempdir_in(&home).unwrap();
        let script = format!(
            "cat '{s}' 2>/dev/null && echo READ; touch '{o}/x' 2>/dev/null && echo WROTE-OUTSIDE; touch '{w}/x' && echo WROTE-STAGING",
            s = secret.display(),
            o = outside.path().display(),
            w = staging.path().display()
        );
        let out = command(Path::new("/bin/sh"), &[staging.path()])
            .arg("-c")
            .arg(script)
            .output()
            .await
            .unwrap();
        let said = String::from_utf8_lossy(&out.stdout);
        let _ = std::fs::remove_file(&secret);
        if made {
            let _ = std::fs::remove_dir(&secret_dir);
        }
        assert!(!said.contains("READ"), "a hidden secret was read: {said}");
        assert!(
            !said.contains("WROTE-OUTSIDE"),
            "wrote outside its folders: {said}"
        );
        assert!(
            said.contains("WROTE-STAGING"),
            "could not write its own folder: {said}"
        );
    }

    /// On a Linux that allows the sandbox: what runs in it cannot read a
    /// hidden folder or write outside its own, and can write in its own.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn inside_the_sandbox_secrets_are_hidden_and_only_staging_is_writable() {
        if bwrap().is_none() {
            eprintln!("no sandbox can be made here; skipped");
            return;
        }
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap();
        let secret = home.join(".ssh/dm-sandbox-test");
        std::fs::create_dir_all(secret.parent().unwrap()).unwrap();
        std::fs::write(&secret, "secret").unwrap();
        let staging = tempfile::tempdir_in(&home).unwrap();
        let outside = tempfile::tempdir_in(&home).unwrap();
        let script = format!(
            "cat {s} 2>/dev/null && echo READ; touch {o}/x 2>/dev/null && echo WROTE-OUTSIDE; touch {w}/x && echo WROTE-STAGING",
            s = secret.display(),
            o = outside.path().display(),
            w = staging.path().display()
        );
        let out = command(Path::new("/bin/sh"), &[staging.path()])
            .arg("-c")
            .arg(script)
            .output()
            .await
            .unwrap();
        let said = String::from_utf8_lossy(&out.stdout);
        let _ = std::fs::remove_file(&secret);
        assert!(!said.contains("READ"), "a hidden secret was read: {said}");
        assert!(
            !said.contains("WROTE-OUTSIDE"),
            "wrote outside its folders: {said}"
        );
        assert!(
            said.contains("WROTE-STAGING"),
            "could not write its own folder: {said}"
        );
    }
}
