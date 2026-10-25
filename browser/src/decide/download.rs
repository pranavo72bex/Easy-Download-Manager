//! A browser download, and whether it is one to hand over.

use super::*;

/// A browser's `DownloadItem`, as much of it as matters. Chrome and Firefox
/// fill different fields, and Firefox sends `null` where Chrome sends `""`,
/// so everything is optional.
#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct DownloadItem {
    pub id: Option<f64>,
    pub url: Option<String>,
    /// Chrome only: where the redirects ended.
    pub final_url: Option<String>,
    pub referrer: Option<String>,
    pub filename: Option<String>,
    pub mime: Option<String>,
    pub file_size: Option<f64>,
    pub total_bytes: Option<f64>,
    pub state: Option<String>,
    /// Set when an extension started the download.
    pub by_extension_id: Option<String>,
    pub incognito: Option<bool>,
    /// Firefox: the container (cookie store) the download was made in.
    pub cookie_store_id: Option<String>,
}

/// A browser download that could be handed over, if the app wants it.
#[derive(Debug, Clone, PartialEq)]
pub struct Catchable {
    pub id: i64,
    /// Where the bytes come from: after redirects, where the browser says.
    pub url: String,
    /// The address the download was asked for, before any redirect.
    pub asked_url: String,
    pub referrer: String,
    pub filename: String,
    pub mime: String,
    pub size: Option<u64>,
    /// The cookie store the download was made in (Firefox's containers),
    /// whose cookies are its sign-in; `None` is the browser's default.
    pub store: Option<String>,
}

impl Catchable {
    pub fn candidate(&self) -> Candidate<'_> {
        Candidate {
            url: &self.url,
            asked_url: &self.asked_url,
            referrer: &self.referrer,
            filename: &self.filename,
            mime: &self.mime,
            size: self.size,
        }
    }
}

pub(super) fn text(field: &Option<String>) -> String {
    field.as_deref().unwrap_or("").trim().to_string()
}

pub(super) fn is_web(url: &str) -> bool {
    let lower = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// Whether a download is one the app could be asked about at all, before
/// anyone asks it.
pub fn first_look(item: &DownloadItem, own_id: &str) -> Result<Catchable, &'static str> {
    // The one this extension started itself, giving a download back to the
    // browser: catching it again would loop.
    if item
        .by_extension_id
        .as_deref()
        .is_some_and(|by| by == own_id)
    {
        return Err("started by dm itself");
    }
    // A finished one is still a candidate. Chromium completes a small file
    // served fast, a PDF above all, before it can be held: three of four
    // 300 KB PDFs from a local server were complete before or despite the
    // pause, where a `.bin` of the same size waits for its danger check.
    // A cache hit looks no different. Handed over, dm fetches it
    // again; the browser's copy is removed only once dm has taken it.
    if item.state.as_deref() == Some("interrupted") {
        return Err("already stopped");
    }
    // A private window's download stays private: handed over, it would sit
    // in the app's queue and history after the window had closed.
    if item.incognito == Some(true) {
        return Err("from a private window");
    }
    let id = item.id.ok_or("the browser gave it no id")? as i64;
    let asked_url = text(&item.url);
    let url = match text(&item.final_url) {
        found if !found.is_empty() => found,
        _ => asked_url.clone(),
    };
    // blob:, data: and the like are bytes a page already holds; nothing
    // outside the browser can fetch them.
    if !is_web(&url) {
        return Err("not an http(s) download");
    }
    let size = [item.file_size, item.total_bytes]
        .into_iter()
        .flatten()
        .find(|n| *n > 0.0)
        .map(|n| n as u64);
    Ok(Catchable {
        id,
        url,
        asked_url,
        referrer: text(&item.referrer),
        filename: text(&item.filename),
        mime: text(&item.mime),
        size,
        store: Some(text(&item.cookie_store_id)).filter(|s| !s.is_empty()),
    })
}

/// Whether a download the browser would not hold can still be handed over,
/// from the state the browser reports for it once the pause failed: one
/// running or finished can; one stopped or gone has nothing to hand.
pub fn unheld(state: Option<&str>) -> Result<(), &'static str> {
    match state {
        Some("in_progress") | Some("complete") => Ok(()),
        Some(_) => Err("it stopped first"),
        None => Err("it is gone from the browser's list"),
    }
}

/// What becomes of the browser's copy once the app has taken a download,
/// from the state the browser reports for it then.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserCopy {
    /// Still running or held: cancelled, which removes its partial file.
    Cancel,
    /// Finished before it could be held, or despite the hold: the file is
    /// whole in the browser's folder and is removed, or it would sit there
    /// beside dm's.
    Remove,
}

pub fn after_taken(state: Option<&str>) -> BrowserCopy {
    if state == Some("complete") {
        BrowserCopy::Remove
    } else {
        BrowserCopy::Cancel
    }
}

/// Downloads whose file name Chrome is waiting on. Chrome picks a
/// download's name, and asks where to save it when it must (a name too long
/// for the disk, or "Ask where to save each file" on), only once every
/// `onDeterminingFilename` listener has answered. So the answer is held
/// until dm has decided: one dm takes is cancelled before any Save dialog
/// can open, and one it leaves is answered at once, as if dm were not there.
/// `T` is the browser's answer function; plain here so it can be tested.
/// Chrome may ask before or after the download's `onCreated` is decided, so
/// a decision that comes first is remembered for the question that follows.
#[derive(Debug)]
pub struct Holds<T> {
    waiting: Vec<(i64, T)>,
    pub(super) decided: Vec<i64>,
}

impl<T> Default for Holds<T> {
    fn default() -> Self {
        Holds {
            waiting: Vec::new(),
            decided: Vec::new(),
        }
    }
}

/// How many decided ids are remembered for a question still to come.
pub(super) const DECIDED_KEPT: usize = 64;

impl<T> Holds<T> {
    /// Chrome asks about `id`. The answer comes back to be given now when
    /// that download is already decided; otherwise it is held.
    pub fn ask(&mut self, id: i64, answer: T) -> Option<T> {
        if let Some(at) = self.decided.iter().position(|d| *d == id) {
            self.decided.remove(at);
            return Some(answer);
        }
        self.waiting.push((id, answer));
        None
    }

    /// `id` is decided: the held answer, if Chrome asked, to be given now.
    pub fn decide(&mut self, id: i64) -> Option<T> {
        match self.waiting.iter().position(|(w, _)| *w == id) {
            Some(at) => Some(self.waiting.remove(at).1),
            None => {
                if self.decided.len() >= DECIDED_KEPT {
                    self.decided.remove(0);
                }
                self.decided.push(id);
                None
            }
        }
    }

    /// `id` was taken by dm and cancelled in the browser: the held answer
    /// is dropped, never given, since there is no download left to name.
    pub fn drop_taken(&mut self, id: i64) {
        self.waiting.retain(|(w, _)| *w != id);
    }

    /// The held answer for `id` when the time to hold it ran out.
    pub fn expire(&mut self, id: i64) -> Option<T> {
        let at = self.waiting.iter().position(|(w, _)| *w == id)?;
        Some(self.waiting.remove(at).1)
    }
}

/// What a person is told the app took: the file's own name where the
/// browser gave one, else the last part of its address. A caught link
/// otherwise vanished from the browser without a word.
pub fn taken_name(filename: &str, url: &str) -> String {
    let from_file = filename.rsplit(['/', '\\']).next().unwrap_or("").trim();
    if !from_file.is_empty() {
        return from_file.to_string();
    }
    let path = url.split(['?', '#']).next().unwrap_or("");
    let last = path.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    if last.is_empty() {
        url.to_string()
    } else {
        last.to_string()
    }
}
