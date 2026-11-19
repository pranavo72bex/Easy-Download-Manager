// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Starting the engine from the saved settings, and putting a change of them into force.

use super::*;

/// The queue the saved settings describe, with what the last run left in it
/// read back, and anything worth saying about how that went.
///
/// Built from the saved proxy rather than a plain client and then corrected:
/// the first thing a queue does on startup is resume what was in flight, and
/// that must not go out directly. A proxy that cannot be built is said out
/// loud rather than quietly bypassed, and turned off for everything — the
/// tools dm runs are told the proxy from the same settings, and a client
/// going direct beside a tool handed a proxy that does not parse is two
/// routes disagreeing about one setting.
pub struct Started {
    /// The queue, its saved downloads read back and the settings applied.
    pub queue: DownloadList,
    /// The settings it was started with, as read (or the defaults).
    pub settings: Settings,
    /// What went wrong on the way and was put right, in words for the window.
    pub notices: Vec<String>,
}

/// Starts the engine for the window or the headless app: reads the
/// settings, readies the tools folder, makes the queue, reads its saved
/// downloads back and applies the settings to it, and adds what `orders`
/// names.
pub async fn start(orders: &Orders) -> Started {
    let (mut saved, unreadable) = Settings::load();
    let mut notices: Vec<String> = unreadable.into_iter().collect();

    let client = match saved.client() {
        Ok(client) => client,
        Err(why) => {
            notices.push(format!(
                "the saved proxy could not be used ({why}), so it is turned off and downloads \
                 go out directly. Fix it under Options › Advanced and tick it again."
            ));
            saved.advanced.proxy.enabled = false;
            // Saved turned off, as the notice says: the browser's native
            // host reads the settings file, and still finding the proxy on
            // there it left every caught download with the browser while
            // the app went direct.
            if let Err(e) = saved.save() {
                notices.push(format!("the proxy could not be saved as turned off: {e}"));
            }
            dm_core::client()
        }
    };
    // The cookies file, read now rather than at the first change of a
    // setting: before, a download that needed it failed until then.
    notices.extend(use_cookies(&saved));
    // So with the engine's name, the dial-up connection and the second network.
    notices.extend(engine_wide(&saved));
    // The served rules' YouTube clients, asked before the engine's own (gaps row Y1).
    crate::host::served::apply(&crate::settings::home());

    let queue = DownloadList::new(
        client,
        // Part files go under dm's own directory unless a folder was chosen:
        // they are not files anyone wants to find in their downloads folder.
        saved.options(
            saved.parts_dir(),
            dm_core::Limiter::new(saved.speed_limit_bytes()),
        ),
        saved.downloads.concurrency,
    );
    // Before anything can serve the queue: applying it from a spawned task
    // raced the scheduler, and a scheduled queue ran at launch.
    queue.set_schedule(saved.schedule.clone());
    queue.set_queue_schedules(saved.queue_schedules.clone());
    queue.set_allowance(saved.allowance());
    queue.set_recheck_every(saved.recheck_every());
    queue.set_queue_limits(saved.queue_limit_map());
    queue.set_queue_starts(saved.queue_start_list());
    watch_rules(&queue);
    // Read back whatever the last run left before anything can add a row:
    // the command line below, the inbox watcher and the window all number
    // new rows, and a row numbered before the store was read collided with
    // a restored one.
    queue
        .persist_to(settings::home().join(dm_core::paths::QUEUE))
        .await;
    for request in &orders.downloads {
        // With a site's saved sign-in, as the window adds a link.
        queue
            .add_with(
                &saved.with_login(&request.url),
                saved.with_site_rule(&request.url, add_options(request)),
            )
            .await;
    }
    if orders.start_queue || saved.downloads.start_queue_on_launch {
        queue.start_queue(dm_core::MAIN_QUEUE).await;
    }

    Started {
        queue,
        settings: saved,
        notices,
    }
}

/// What the settings put in force across the engine rather than on the queue:
/// the name dm gives itself, the language of a dubbed video's sound, the
/// dial-up or VPN connection every download waits for, the second network and
/// where YouTube's cut players are kept. At the start and at every change, so a
/// saved "Identify as dm only", second network or dial-up connection is in force
/// before Options' OK. What went wrong, in words.
fn engine_wide(settings: &Settings) -> Option<String> {
    // The dial-up connection in force and the engine's hook for it, as one turn.
    #[cfg(test)]
    let _turn = crate::dialup::turn();
    dm_core::configure(&engine_config(settings))
        .err()
        .map(|why| format!("downloads use one network: {why}"))
}

/// The engine's program-wide settings the saved `settings` describe (gaps row A11).
fn engine_config(settings: &Settings) -> dm_core::EngineConfig {
    dm_core::EngineConfig {
        identify_as_dm: settings.identify_as_dm,
        // The language a dubbed video's sound is taken in (gaps row Y10).
        sound_language: settings.sound_language.clone(),
        // A dial-up or VPN connection every download waits for (`dialup.rs`).
        connect_first: crate::dialup::set(&settings.advanced.dialup),
        // The second network, where one is chosen and no proxy is on: through
        // a proxy every connection must go by it, and one that did not would
        // be seen by the site from this computer's own address.
        second_path: (!settings.advanced.proxy.enabled).then(|| settings.second_path.clone()),
        // YouTube's cut players, kept between sessions under dm's home (gaps row Y13).
        kept_players: Some(crate::settings::home().join(dm_core::paths::PLAYERS)),
    }
}

/// Whether the client must be built again for a change from `previous`.
fn new_client(settings: &Settings, previous: Option<&Settings>) -> bool {
    previous.map(|held| &held.advanced).is_none_or(|was| {
        was.proxy != settings.advanced.proxy || was.read_timeout != settings.advanced.read_timeout
    })
}

/// Why a change cannot be put into force, found before any of it is: a
/// value it refuses, or a proxy that cannot be built. `apply` after this
/// changes everything or nothing, where before a proxy refused halfway left
/// the half before it live.
pub fn check(settings: &Settings, previous: Option<&Settings>) -> Result<(), String> {
    if let Some(why) = previous.and_then(|held| settings.refusal(held)) {
        return Err(why);
    }
    if new_client(settings, previous) {
        settings.client()?;
    }
    Ok(())
}

/// Puts the cookies file in force: read again each time, so one exported
/// again since is in force without choosing it again. One that can no
/// longer be read is a problem to show, not a reason to refuse the change
/// it came with — refused, a cookies file moved away would fail every
/// change of every setting after it, the language and a saved search's last
/// run among them. Said once, until it reads again or another problem replaces
/// it, since this runs on every change.
pub fn use_cookies(settings: &Settings) -> Option<String> {
    static SAID: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
    let outcome = dm_core::cookies::use_file(Path::new(settings.advanced.cookies_file.trim()));
    let mut said = dm_core::lock(&SAID);
    match outcome {
        Ok(_) => {
            *said = None;
            None
        }
        Err(why) if said.as_deref() == Some(why.as_str()) => None,
        Err(why) => {
            *said = Some(why.clone());
            Some(format!(
                "{why}; downloads go without its cookies until it is back or another is \
                 chosen in Options › Advanced"
            ))
        }
    }
}

/// Puts settings into effect on a running queue — everything but the login
/// item, which belongs to whichever window registers it. What can fail is
/// tried first (`check`), so a change is in force whole or not at all; the
/// problems it found that do not stop it come back to be shown.
///
/// The client is only rebuilt when the proxy actually changed. A new client
/// means a new TLS backend, a fresh root store, and every keep-alive
/// connection dropped — for ticking a box about folders.
pub async fn apply(
    settings: &Settings,
    previous: Option<&Settings>,
    queue: &DownloadList,
) -> Result<Vec<String>, String> {
    check(settings, previous)?;
    if new_client(settings, previous) {
        queue
            .set_client(&settings.advanced.proxy, settings.read_timeout())
            .await?;
    }
    let mut problems: Vec<String> = use_cookies(settings).into_iter().collect();
    problems.extend(engine_wide(settings));

    let current = queue.options().await;
    // The limiter belongs to the running queue. The parts folder is the
    // settings' now: a row records the folder it began in, so a change
    // here reaches rows not yet started and strands nothing.
    queue
        .set_options(settings.options(settings.parts_dir(), current.limiter))
        .await;

    queue.set_concurrency(settings.downloads.concurrency);
    // The slow hours' limit where they are in force now.
    queue
        .set_speed_limit(settings.speed_limit_bytes_now())
        .await;
    queue.set_schedule(settings.schedule.clone());
    queue.set_queue_schedules(settings.queue_schedules.clone());
    queue.set_allowance(settings.allowance());
    queue.set_recheck_every(settings.recheck_every());
    queue.set_queue_limits(settings.queue_limit_map());
    queue.set_queue_starts(settings.queue_start_list());
    Ok(problems)
}

/// How often [`watch_rules`] looks at the schedule, the download limit, the recheck timer and
/// the queues' start times.
const RULES_EVERY: std::time::Duration = std::time::Duration::from_secs(2);

/// Starts the scheduler again when the schedule's window opens or the
/// download limit's period rolls over, for the rows they held.
///
/// The scheduler runs only while something is queued or running, and a held
/// row is neither: without this, a window opening at one in the morning
/// would start nothing until a person touched the queue.
pub fn watch_rules(queue: &DownloadList) {
    let queue = queue.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(RULES_EVERY).await;
            if queue.may_run() && queue.held().await {
                queue.run().await;
            }
            // The recheck queue's own timer, where one is set.
            if queue.recheck_if_due().await.is_some() {
                queue.run().await;
            }
            // Queues that start on their own at a time of day.
            let (day, minute) = dm_core::now_local();
            for name in queue.starts_due(day, minute) {
                if queue.start_queue(&name).await > 0 {
                    wake(&queue);
                }
            }
        }
    });
}

/// Starts the scheduler, if it is not already running.
///
/// It runs until the queue empties, so a new arrival while it is idle needs
/// it started again. Starting a second one is harmless: the first returns
/// as soon as it finds nothing to do. Must be called inside the runtime.
pub fn wake(queue: &DownloadList) {
    let queue = queue.clone();
    tokio::spawn(async move { queue.run().await });
}
