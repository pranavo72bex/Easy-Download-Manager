// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A backup of dm (File › Back up dm… and Restore…, gaps row W16): one file, `.dmbackup`, of
//! the settings (the queues' limits, starts and schedules, the site rules and the Grabber's
//! saved searches among them) as `settings.json` holds them, and the list's rows as the queue
//! file holds them.
//!
//! The sign-ins (Options › Sign-ins' passwords, the proxy's and the dial-up connection's, a
//! login in a download's address, and the cookies and request headers the browser handed over
//! with a download) are left out unless a passphrase is given; then they are sealed with it
//! (`dm_core::secretstore::passphrase`) and opened only with it.
//!
//! The file says what it is (`"kind": "dm backup"`) and carries its format number
//! (`dm_core::saved`). It is read as far as this dm knows its fields: a newer dm's fields are
//! passed over, a row it cannot read is counted and left out, and a setting it cannot read
//! keeps its default, as in the settings file.
//!
//! Restored, its settings replace these as Options' OK puts settings in force, and a download
//! that is running goes on; what makes dm run a program (the command after each download, the
//! program at the end of the scheduled hours, the proxy script) is never set from a file, as a
//! settings file imported never sets it. Its rows are added to the list, those whose address
//! the list holds already left out, and touch no file here (`DownloadList::adopt`).

use std::collections::HashSet;
use std::sync::Arc;

use dm_core::saved::Stamped;
use dm_core::secretstore::passphrase::{self, NotOpened, Sealed};
use dm_core::{Entry, Id, State};
use serde::{Deserialize, Serialize};

use crate::app::backend::Backend;
use crate::settings::{KeptPrograms, SiteLogin, SETTINGS_FORMAT};
use crate::{Error, Settings};

/// The backup file's format number (`dm_core::saved`), written beside its fields.
pub const BACKUP_FORMAT: u32 = 1;

/// The ending of a backup's file name.
pub const EXTENSION: &str = "dmbackup";

/// The name the save panel offers.
pub const SUGGESTED: &str = "dm.dmbackup";

/// What the file says it is, so a file of something else is not read as one.
const KIND: &str = "dm backup";

/// The file as it is written.
#[derive(Serialize)]
struct Written<'a> {
    kind: &'static str,
    /// When it was made, in seconds since 1970.
    made: u64,
    /// The settings with their own format number, as `settings.json` holds them.
    settings: Stamped<'a, Settings>,
    rows: &'a [Entry],
    #[serde(skip_serializing_if = "Option::is_none")]
    sign_ins: Option<Sealed>,
}

/// The file as it is read: each part kept as JSON until it is wanted, so one that a newer dm
/// shaped otherwise costs only itself.
#[derive(Deserialize, Default)]
#[serde(default)]
struct ReadIn {
    format: u32,
    kind: String,
    made: u64,
    settings: Option<serde_json::Value>,
    rows: Vec<serde_json::Value>,
    sign_ins: Option<serde_json::Value>,
}

/// What is sealed with the passphrase.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct SignIns {
    /// Options › Sign-ins' entries that have a password.
    logins: Vec<SiteLogin>,
    /// The proxy's password.
    proxy: String,
    /// The dial-up connection's password.
    dialup: String,
    /// The rows' own sign-ins.
    rows: Vec<RowSignIn>,
}

/// A row's sign-in: the address with its login, the cookies and the request headers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct RowSignIn {
    /// The row's place in the file's list.
    at: usize,
    url: String,
    cookies: String,
    headers: Vec<(String, String)>,
}

/// Why a backup was not read, or its sign-ins not opened: a kind, so the window can say it in
/// the language in force. `Display` is the English.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// The file is not a backup of dm.
    NotABackup,
    /// The passphrase does not open the sign-ins (or they were changed since).
    WrongPassphrase,
    /// The sign-ins were sealed by a newer dm, in a way this one cannot open.
    SealedByNewer,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Refused::NotABackup => "That file is not a backup of dm.",
            Refused::WrongPassphrase => "That passphrase does not open this backup’s sign-ins.",
            Refused::SealedByNewer => {
                "This backup’s sign-ins were sealed by a newer dm, which this one can’t open."
            }
        })
    }
}

/// A backup read from its file, its sign-ins still sealed until [`Backup::open`].
#[derive(Debug, Clone)]
pub struct Backup {
    /// The file's format number: higher than [`BACKUP_FORMAT`] for a newer dm's, read as far as
    /// this dm knows its fields.
    pub format: u32,
    /// When it was made, in seconds since 1970.
    pub made: u64,
    /// How many of its rows this dm could not read (a newer dm's), left out.
    pub unread_rows: usize,
    /// The settings' JSON.
    settings: String,
    /// The rows, each with its place in the file's list.
    rows: Vec<(usize, Entry)>,
    sealed: Option<serde_json::Value>,
    sign_ins: Option<SignIns>,
}

/// What a restore added to the list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Restored {
    /// The rows added, in order.
    pub added: Vec<Id>,
    /// The backup's rows left out because the list holds their address already.
    pub already: usize,
}

/// The rounds the passphrase is stretched with: fewer in this crate's tests, which make many.
fn rounds() -> u32 {
    if cfg!(test) {
        1_000
    } else {
        passphrase::ROUNDS
    }
}

/// `url` without a user and password in it, as Export writes it.
fn without_login(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(mut parsed) if !parsed.username().is_empty() || parsed.password().is_some() => {
            let _ = parsed.set_username("");
            let _ = parsed.set_password(None);
            parsed.to_string()
        }
        _ => url.to_string(),
    }
}

/// The settings and rows without a password, a login, a cookie or a header the browser gave,
/// and those taken out; without this computer's history, and without a row that keeps no
/// record.
fn split(settings: &Settings, rows: &[Entry]) -> (Settings, SignIns, Vec<Entry>) {
    let mut plain = settings.clone();
    let mut secret = SignIns::default();
    for login in &mut plain.site_logins {
        if !login.password.is_empty() {
            secret.logins.push(login.clone());
            login.password.clear();
        }
    }
    secret.proxy = std::mem::take(&mut plain.advanced.proxy.password);
    secret.dialup = std::mem::take(&mut plain.advanced.dialup.password);
    // This computer's history, which a restore never takes (`Settings::restored`).
    plain.recent_urls.clear();
    plain.last_folders.clear();
    let mut kept = Vec::with_capacity(rows.len());
    // A row marked "Keep no record" (gaps row W17) is named in no file of dm's.
    for row in rows.iter().filter(|r| !r.keep_no_record) {
        let at = kept.len();
        let mut row = row.clone();
        let bare = without_login(&row.url);
        if bare != row.url || !row.cookies.is_empty() || !row.headers.is_empty() {
            secret.rows.push(RowSignIn {
                at,
                url: std::mem::replace(&mut row.url, bare),
                cookies: std::mem::take(&mut row.cookies),
                headers: std::mem::take(&mut row.headers),
            });
        }
        row.post = None;
        kept.push(row);
    }
    (plain, secret, kept)
}

/// The backup file's text of `settings` and `rows`: with `passphrase` (not empty), their
/// sign-ins sealed with it; without, left out.
pub fn write(
    settings: &Settings,
    rows: &[Entry],
    passphrase: Option<&str>,
) -> Result<String, Error> {
    let (plain, secret, rows) = split(settings, rows);
    let sign_ins = match passphrase.filter(|p| !p.is_empty()) {
        Some(passphrase) => {
            let text = serde_json::to_vec(&secret).map_err(|e| Error::new(e.to_string()))?;
            Some(passphrase::seal(passphrase, &text, rounds())?)
        }
        None => None,
    };
    let made = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let written = Written {
        kind: KIND,
        made,
        settings: Stamped {
            format: SETTINGS_FORMAT,
            body: &plain,
        },
        rows: &rows,
        sign_ins,
    };
    serde_json::to_string_pretty(&Stamped {
        format: BACKUP_FORMAT,
        body: &written,
    })
    .map_err(|e| Error::new(format!("the backup could not be made ({e})")))
}

/// A backup file's `text`, read as far as this dm knows its fields.
pub fn read(text: &str) -> Result<Backup, Refused> {
    // Without the mark some editors put before UTF-8, which is not JSON.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let file: ReadIn = serde_json::from_str(text).map_err(|_| Refused::NotABackup)?;
    if file.kind != KIND {
        return Err(Refused::NotABackup);
    }
    let Some(settings) = file.settings.filter(serde_json::Value::is_object) else {
        return Err(Refused::NotABackup);
    };
    let total = file.rows.len();
    let rows: Vec<(usize, Entry)> = file
        .rows
        .into_iter()
        .enumerate()
        .filter_map(|(at, row)| serde_json::from_value(row).ok().map(|row| (at, row)))
        .collect();
    Ok(Backup {
        format: file.format,
        made: file.made,
        unread_rows: total - rows.len(),
        settings: settings.to_string(),
        rows,
        sealed: file.sign_ins,
        sign_ins: None,
    })
}

impl Backup {
    /// Whether it holds sign-ins sealed with a passphrase.
    pub fn sealed(&self) -> bool {
        self.sealed.is_some()
    }

    /// Whether its sealed sign-ins were opened ([`Backup::open`]).
    pub fn opened(&self) -> bool {
        self.sign_ins.is_some()
    }

    /// How many downloads it holds that this dm can read.
    pub fn rows(&self) -> usize {
        self.rows.len()
    }

    /// Its sign-ins opened with `passphrase`; where they are not, nothing of them is taken.
    /// Nothing to open is not an error.
    pub fn open(&mut self, passphrase: &str) -> Result<(), Refused> {
        let Some(sealed) = &self.sealed else {
            return Ok(());
        };
        let sealed: Sealed =
            serde_json::from_value(sealed.clone()).map_err(|_| Refused::SealedByNewer)?;
        let plain = passphrase::open(passphrase, &sealed).map_err(|e| match e {
            NotOpened::Wrong => Refused::WrongPassphrase,
            NotOpened::Unreadable(_) => Refused::SealedByNewer,
        })?;
        let sign_ins = serde_json::from_slice(&plain).map_err(|_| Refused::SealedByNewer)?;
        self.sign_ins = Some(sign_ins);
        Ok(())
    }

    /// The settings it puts in force in place of `mine` (`Settings::restored`), with its
    /// passwords where its sign-ins were opened; `mine`'s programs kept, and which were; and
    /// what of its settings could not be read or was changed to be used, in English.
    pub fn settings_over(
        &self,
        mine: &Settings,
    ) -> Result<(Settings, KeptPrograms, Vec<String>), Error> {
        let (mut wanted, said) = mine.restored(&self.settings)?;
        if let Some(sign_ins) = &self.sign_ins {
            for login in &mut wanted.site_logins {
                let given = sign_ins
                    .logins
                    .iter()
                    .find(|k| k.host == login.host && k.username == login.username);
                if let Some(given) = given {
                    login.password.clone_from(&given.password);
                }
            }
            if !sign_ins.proxy.is_empty() {
                wanted.advanced.proxy.password.clone_from(&sign_ins.proxy);
            }
            if !sign_ins.dialup.is_empty() {
                wanted.advanced.dialup.password.clone_from(&sign_ins.dialup);
            }
        }
        let kept = wanted.keep_programs_of(mine);
        Ok((wanted, kept, said))
    }

    /// The rows to add to a list of `listed`: its own, with their sign-ins where those were
    /// opened, less those whose address `listed` holds already (as Import finds them:
    /// `lists::bare`, a cancelled row's not counted); and how many were left out so.
    pub fn rows_for(&self, listed: &[Arc<Entry>]) -> (Vec<Entry>, usize) {
        let known: HashSet<String> = listed
            .iter()
            .filter(|r| r.state != State::Cancelled)
            .map(|r| crate::lists::bare(&r.url))
            .collect();
        let given = self.sign_ins.as_ref().map(|s| s.rows.as_slice());
        let mut rows = Vec::with_capacity(self.rows.len());
        let mut already = 0;
        for (at, row) in &self.rows {
            if known.contains(&crate::lists::bare(&row.url)) {
                already += 1;
                continue;
            }
            let mut row = row.clone();
            // Only onto the row it was taken from: its place, and its address without the login.
            let sign_in = given
                .unwrap_or_default()
                .iter()
                .find(|g| g.at == *at && without_login(&g.url) == row.url);
            if let Some(sign_in) = sign_in {
                row.url.clone_from(&sign_in.url);
                row.cookies.clone_from(&sign_in.cookies);
                row.headers.clone_from(&sign_in.headers);
            }
            rows.push(row);
        }
        (rows, already)
    }
}

impl Backend {
    /// A backup of dm as it stands ([`write`]): the settings in force and every row, the
    /// sign-ins sealed with `passphrase` where one is given.
    pub fn back_up(&self, passphrase: Option<&str>) -> Result<String, Error> {
        write(&self.settings(), &self.list(), passphrase)
    }

    /// `backup`'s rows added to the list ([`Backup::rows_for`]), the queue written once.
    pub fn restore_rows(&self, backup: &Backup) -> Restored {
        let listed: Vec<Arc<Entry>> = self.list().into_iter().map(Arc::new).collect();
        let (rows, already) = backup.rows_for(&listed);
        let added = self.runtime.block_on(self.queue.adopt(rows));
        Restored { added, already }
    }
}

#[cfg(test)]
mod tests;
