// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A live broadcast recorded through YouTube's framed stream (gaps row Y2), where its player
//! names that and no HLS playlist: each format's pieces added to the file a video's stream is
//! fetched to, from where the broadcast is until it ends or the person stops it
//! (`youtube::framed::record_live`), then joined and placed as a video's picture and sound are.
//! A stop finishes the recording, as a live HLS recording's does, rather than pausing it.

use super::streams::{framed_wanted, join_and_place, say_named};
use super::*;

/// `plan`, the live formats of the page `url` read as `read`, recorded through the framed
/// stream and placed. While the broadcast stands still the page is read again: no longer live
/// through the framed stream, the recording ends.
pub(super) async fn record(
    client: &reqwest::Client,
    url: &str,
    plan: Plan,
    read: &Read,
    options: &Options,
    report: Option<tokio::sync::mpsc::Sender<Update>>,
) -> Result<Finished, Error> {
    let Some(framed) = plan.framed.as_ref() else {
        return Err(Error::Internal(
            "this video's streams cannot be fetched by dm's own engine".into(),
        ));
    };
    say_named(&report, &plan).await;
    let wanted = framed_wanted(&plan);
    // The video read again: the framed stream it names where it is still live through one.
    let read_again = move || async move {
        let again = resolve(url, options, false).await?;
        let live = again.info.is_live == Some(true);
        Ok::<_, Error>(again.info.dm_framed.filter(|_| live))
    };
    let recorded =
        crate::youtube::framed::record_live(client, framed, &wanted, options, report, read_again)
            .await;
    if let Err(why) = recorded {
        if matches!(why, Error::Cancelled) {
            remove_staging(&options.parts_dir).await;
        }
        return Err(why);
    }
    // Stopped by the person: the recording is finished, not paused, so placing it must not
    // wait on the stop that ended it.
    let placing = Options {
        control: crate::control::Control::new(),
        ..options.clone()
    };
    join_and_place(client, &plan, read, &placing).await
}
