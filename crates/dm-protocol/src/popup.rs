// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The toolbar popup's recent downloads and what it asks the app to do with one (gaps row C12):
//! open it, show it in its folder, resume or stop it, and clear the finished ones from dm's
//! list. Sent only between sides whose hellos say [`crate::feature::ACTS_ON_DOWNLOADS`].

use serde::{Deserialize, Serialize};

/// The most recent downloads the app lists for the popup.
pub const MOST_RECENT: usize = 10;

/// A download the app has stopped or finished, as the popup lists it under "Recent"
/// ([`crate::Reply::Downloads`]'s `recent`), the newest first.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Recent {
    /// Its row in dm's list, which an [`Act`] names.
    pub id: u64,
    /// Its file's name.
    pub name: String,
    /// How it is: one of [`recent_state`]. A word this side does not know is shown with no
    /// buttons, so a newer app's state does not cost an older extension the list.
    pub state: String,
}

/// How a [`Recent`] download is.
pub mod recent_state {
    /// Finished and in its place: Open and Open folder.
    pub const FINISHED: &str = "finished";
    /// Stopped by the person, to be resumed: Resume.
    pub const STOPPED: &str = "stopped";
    /// Failed: Retry, which resumes it as the window's Resume does.
    pub const FAILED: &str = "failed";
}

/// What the popup asks the app to do ([`crate::Request::Act`]).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Act {
    /// One of [`act_does`].
    pub does: String,
    /// The row it is done to; none for [`act_does::CLEAR_FINISHED`], which is the whole list's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
}

/// What an [`Act`] does: each one of the window's own commands.
pub mod act_does {
    /// The finished file opened with its usual app (the window's Open).
    pub const OPEN: &str = "open";
    /// The finished file shown in its folder (the window's Open folder).
    pub const FOLDER: &str = "folder";
    /// A stopped or failed download resumed (the window's Resume download).
    pub const RESUME: &str = "resume";
    /// A running or waiting download stopped (the window's Stop download).
    pub const STOP: &str = "stop";
    /// The finished rows taken out of dm's list, their files kept (the window's Delete
    /// Completed).
    pub const CLEAR_FINISHED: &str = "clear-finished";
    /// Every one.
    pub const ALL: [&str; 5] = [OPEN, FOLDER, RESUME, STOP, CLEAR_FINISHED];
}

impl Act {
    /// One that this side knows, and that names a row where it needs one.
    pub fn is_known(&self) -> bool {
        act_does::ALL.contains(&self.does.as_str())
            && (self.id.is_some() == (self.does != act_does::CLEAR_FINISHED))
    }
}
