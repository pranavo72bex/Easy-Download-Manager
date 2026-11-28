// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A running torrent following the computer's network, and a finished one's sharing held with
//! the downloads (gaps row N8). A laptop or a phone moves between networks and comes back with
//! another address: the peers it had cannot reach it there, and its trackers and the DHT name
//! the old one, so a torrent announces itself again on the new one. A hold on the downloads
//! (`DownloadList::hold_for`: a metered connection, a low battery) holds the sharing too, which
//! sends as much as it fetches.

use std::sync::atomic::{AtomicBool, Ordering};

use edm_torrent::{Session, Torrent};

/// Whether every finished torrent's sharing is held, as the downloads are (`hold_sharing`).
static SHARING_HELD: AtomicBool = AtomicBool::new(false);

/// Holds every finished torrent's sharing, or (`false`) lets it go on: paused within a tick,
/// its time not counted while it is held, and announced afresh when it goes on.
pub(crate) fn hold_sharing(on: bool) {
    SHARING_HELD.store(on, Ordering::SeqCst);
}

/// Whether sharing is held now.
pub(super) fn sharing_held() -> bool {
    SHARING_HELD.load(Ordering::SeqCst)
}

/// One torrent's watch on the computer's network (`only_through::moves`).
pub(super) struct Follow {
    seen: u64,
}

impl Follow {
    /// Watching from the network as it is now.
    pub(super) fn new() -> Follow {
        Follow {
            seen: crate::only_through::moves(),
        }
    }

    /// The network taken as it is now, without announcing (a torrent just started again
    /// announced itself already).
    pub(super) fn settle(&mut self) {
        self.seen = crate::only_through::moves();
    }

    /// Announces `torrent` again where the network changed since the last look. One not live
    /// (still checking its files, paused) announces itself when it starts, so the change is
    /// taken as seen either way. A failure is let go: the torrent goes on as it was.
    pub(super) async fn follow(&mut self, session: &Session, torrent: &Torrent) {
        let now = crate::only_through::moves();
        if now != self.seen {
            self.seen = now;
            let _ = session.reannounce(torrent).await;
        }
    }
}
