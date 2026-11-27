// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

impl DownloadList {
    /// How each connection of a download is getting on, from the plan and
    /// the part files on disk. Empty for one with nothing staged.
    pub async fn connections(&self, id: Id) -> Vec<Connection> {
        let parts = self.parts_dir_of(id).await;
        tokio::task::spawn_blocking(move || {
            let Some(plan) = crate::plan::recorded(&parts) else {
                return Vec::new();
            };
            let networks = crate::second_path::networks_of(&parts);
            plan.ranges
                .iter()
                .enumerate()
                .map(|(i, &(start, end))| {
                    // Its ledger, or the part file an earlier dm wrote.
                    let ledger = crate::download::read_ledger(&parts, i);
                    let part = std::fs::metadata(parts.join(format!("part-{i:04}"))).ok();
                    let on_disk = ledger.or(part.as_ref().map(|m| m.len())).unwrap_or(0);
                    let span = (end - start).saturating_add(1);
                    Connection {
                        start,
                        end,
                        done: on_disk.min(span),
                        started: ledger.is_some() || part.is_some(),
                        second: networks.contains_key(&i),
                        network: networks.get(&i).copied().unwrap_or(0),
                    }
                })
                .collect()
        })
        .await
        .unwrap_or_default()
    }

    /// What a download has fetched so far, as a file a player can open
    /// while it goes on: `preview.<its extension>` beside its parts, a hard
    /// link to the staged file where the disk allows one (so it grows as the
    /// download does), a copy otherwise. A player plays from the start for
    /// as far as the bytes are there; a file whose index is at its end will
    /// not play until it is. Replaced at each call; removed with the parts.
    pub async fn preview(&self, id: Id) -> Result<PathBuf, String> {
        let entry = self.get(id).await.ok_or("there is no such download")?;
        if entry.state == State::Completed {
            return Err("it has finished: open it instead".into());
        }
        // What is left is fetched from the file's start onwards, so the player reading the
        // preview finds more of it as it plays (gaps row W13).
        if let Some(slot) = self.inner.slots.lock().await.get(&id) {
            slot.control.fetch_in_file_order();
        }
        let parts = self.parts_dir_of(id).await;
        tokio::task::spawn_blocking(move || preview_of(&parts, &entry.filename))
            .await
            .map_err(|e| e.to_string())?
    }

    /// Whether the server said it serves ranges — "Can resume".
    /// `None` until the download has been planned.
    pub async fn resumable(&self, id: Id) -> Option<bool> {
        let parts = self.parts_dir_of(id).await;
        tokio::task::spawn_blocking(move || crate::plan::recorded(&parts).map(|p| p.resumable))
            .await
            .ok()
            .flatten()
    }
}
