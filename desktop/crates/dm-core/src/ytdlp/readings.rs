//! The readings of pages yt-dlp makes for the panel's list, kept for the download
//! a click on it starts (so the page is not read twice), and the marker that says
//! one is being made.

use super::*;

/// Has yt-dlp read a page and keep the reading for the download, unless one
/// was kept already: what a list made without yt-dlp leaves undone, so a
/// click on a quality does not start with the page being read. A marker
/// beside the reading says one is being made, so a download that starts
/// before it is done waits for it (`wait_for_reading`) instead of having
/// yt-dlp read the page a second time at once.
pub async fn warm_reading(
    url: &str,
    referrer: Option<&str>,
    proxy: Option<&str>,
    sign_in: Option<&crate::cookies::SignIn>,
) {
    if fresh_reading(url).is_some() {
        return;
    }
    let marker = reading_path(url).map(|p| p.with_extension("busy"));
    if let Some(marker) = &marker {
        if let Some(dir) = marker.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(marker, b"");
    }
    // The marker is touched each second while the read goes on, so one left
    // by a host that was ended mid-read is seen to be stale in seconds.
    let read = read_choices(url, referrer, proxy, sign_in);
    tokio::pin!(read);
    loop {
        tokio::select! {
            _ = &mut read => break,
            () = tokio::time::sleep(READING_BEAT) => {
                if let Some(marker) = &marker {
                    let _ = std::fs::write(marker, b"");
                }
            }
        }
    }
    if let Some(marker) = marker {
        let _ = std::fs::remove_file(marker);
    }
}

/// How often a reading being made says so, and how long without it a
/// waiter takes the read to have been ended.
const READING_BEAT: std::time::Duration = std::time::Duration::from_secs(1);

const READING_STALE: std::time::Duration = std::time::Duration::from_secs(3);

/// The longest a reading being made is waited for: as long as yt-dlp is
/// given to read a page, so a reading that never comes costs no more than
/// the second one would have.
const READING_WAIT: std::time::Duration = std::time::Duration::from_secs(45);

/// Waits for a reading of `url` that `warm_reading` is making, if one is,
/// and returns when it is kept, when the run making it ends without one, or
/// after [`READING_WAIT`]. A pause or cancel ends the wait.
pub(crate) async fn wait_for_reading(
    url: &str,
    control: &crate::control::Control,
) -> Result<(), Error> {
    let Some(marker) = reading_path(url).map(|p| p.with_extension("busy")) else {
        return Ok(());
    };
    let started = std::time::Instant::now();
    let busy = |marker: &Path| {
        std::fs::metadata(marker)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|at| at.elapsed().ok())
            .is_some_and(|age| age < READING_STALE)
    };
    while fresh_reading(url).is_none() && busy(&marker) && started.elapsed() < READING_WAIT {
        control.check()?;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Ok(())
}

/// Where yt-dlp's readings of pages are kept for a few minutes, shared by
/// the browser's host (which reads a page for the panel's list) and the app
/// (which downloads it). None set, none kept.
static READINGS: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// How long a reading serves a download: as long as the panel's list of
/// the same page is believed. A page's stream addresses last hours.
const READING_KEPT: std::time::Duration = std::time::Duration::from_secs(600);

/// Keeps readings of pages in `dir` (in dm's home).
pub fn keep_readings_in(dir: PathBuf) {
    let _ = READINGS.set(dir);
}

/// Where a reading of `url` is kept: a name made from the page's address,
/// so the address itself is in no file name.
pub(super) fn reading_path(url: &str) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let page = url.trim().split('#').next().unwrap_or("");
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    page.hash(&mut hasher);
    Some(
        READINGS
            .get()?
            .join(format!("{:016x}.json", hasher.finish())),
    )
}

/// Keeps a reading, for this account alone, and lets go of the stale ones.
/// Best effort: a reading not kept only means the page is read again.
pub(super) async fn remember_reading(url: &str, json: &[u8]) {
    let (Some(path), Ok(text)) = (reading_path(url), std::str::from_utf8(json)) else {
        return;
    };
    let Some(dir) = path.parent() else {
        return;
    };
    if tokio::fs::create_dir_all(dir).await.is_err() {
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = tokio::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).await;
    }
    if let Ok(mut listing) = tokio::fs::read_dir(dir).await {
        while let Ok(Some(entry)) = listing.next_entry().await {
            let stale = entry
                .metadata()
                .await
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|at| at.elapsed().ok())
                .is_none_or(|age| age >= READING_KEPT);
            if stale {
                let _ = tokio::fs::remove_file(entry.path()).await;
            }
        }
    }
    let _ = write_private(&path, text.trim()).await;
}

/// A reading of `url` made in the last few minutes, if one was kept.
pub(super) fn fresh_reading(url: &str) -> Option<PathBuf> {
    let path = reading_path(url)?;
    let age = std::fs::metadata(&path)
        .ok()?
        .modified()
        .ok()?
        .elapsed()
        .ok()?;
    (age < READING_KEPT).then_some(path)
}

/// Says a reading of `url` is about to be made (`warm_reading`, in another
/// process that has not started yet), so a download or a list asked for in
/// the meantime waits for it rather than reading the page itself.
pub fn mark_reading(url: &str) {
    if let Some(marker) = reading_path(url).map(|p| p.with_extension("busy")) {
        if let Some(dir) = marker.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(marker, b"");
    }
}

/// The choices in the reading of `url` that is kept or being made, waiting
/// for one that is being made; `None` where there is none, or it lists
/// nothing.
pub async fn kept_choices(url: &str) -> Option<Vec<super::Choice>> {
    wait_for_reading(url, &crate::control::Control::new())
        .await
        .ok()?;
    let text = tokio::fs::read_to_string(fresh_reading(url)?).await.ok()?;
    let list = super::choices(&text);
    (!list.is_empty()).then(|| super::with_mp3(list))
}
