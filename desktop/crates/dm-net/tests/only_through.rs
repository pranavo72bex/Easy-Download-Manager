// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The one network downloads are held to (`only_through`, gaps row W12): what is bound to,
//! looked for and waited on as the adapter comes and goes, with a made-up adapter list. The
//! adapter held to binds every client the program builds, so these tests are a program of
//! their own, taking turns, where no other test's client is bound by them.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dm_net::lock;
use dm_net::only_through::{
    before_download, bound_to, connect_tcp, generation, look, restarts, set_lister,
    set_only_through, Lister,
};
use dm_net::second_path::Interface;
use dm_net::{Control, Error};

/// The address bound to while the adapter has never had one: one no computer has.
const NOWHERE: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1));

/// The state is the program's own, one at a time: the tests that set it take turns.
static TURN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn listing(found: Arc<Mutex<Vec<Interface>>>) -> Lister {
    Arc::new(move || lock(&found).clone())
}

fn none_listed() -> Lister {
    Arc::new(Vec::<Interface>::new)
}

fn adapter(address: [u8; 4]) -> Interface {
    Interface {
        name: "dm-test0".into(),
        address: IpAddr::V4(Ipv4Addr::from(address)),
    }
}

#[tokio::test]
async fn none_chosen_binds_nothing_and_waits_for_nothing() {
    let _turn = TURN.lock().await;
    set_only_through(None);
    assert_eq!(bound_to(), None);
    assert_eq!(look(), None);
    assert!(before_download(&Control::new(), |_, _, _| {}).await.is_ok());
}

#[tokio::test]
async fn an_adapter_never_seen_binds_to_an_address_no_computer_has() {
    let _turn = TURN.lock().await;
    set_lister(Some(none_listed()));
    set_only_through(Some("dm-test0"));
    let bound = bound_to();
    let said = look().unwrap_or_default();
    set_only_through(None);
    set_lister(None);
    assert_eq!(bound, Some(NOWHERE));
    assert!(said.contains("dm-test0"), "{said}");
}

#[tokio::test]
async fn a_gone_adapter_keeps_its_last_address_and_stops_what_runs() {
    let _turn = TURN.lock().await;
    let found = Arc::new(Mutex::new(vec![adapter([127, 0, 0, 1])]));
    set_lister(Some(listing(Arc::clone(&found))));
    let chosen_at = restarts();
    set_only_through(Some("dm-test0"));
    // Chosen anew: what runs starts again, bound to it.
    assert_eq!(restarts(), chosen_at + 1);
    let at = generation();
    assert_eq!(bound_to(), Some(IpAddr::V4(Ipv4Addr::LOCALHOST)));
    let lost = restarts();
    lock(&found).clear();
    set_lister(Some(listing(Arc::clone(&found))));
    assert!(look().is_some());
    assert_eq!(restarts(), lost + 1);
    assert_eq!(bound_to(), Some(IpAddr::V4(Ipv4Addr::LOCALHOST)));
    // Back with another address: clients are built again on it.
    lock(&found).push(adapter([127, 0, 0, 3]));
    set_lister(Some(listing(Arc::clone(&found))));
    assert_eq!(look(), None);
    assert!(generation() > at);
    assert_eq!(bound_to(), Some(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 3))));
    set_only_through(None);
    set_lister(None);
}

#[tokio::test]
async fn a_pause_while_it_waits_is_heard() {
    let _turn = TURN.lock().await;
    set_lister(Some(none_listed()));
    set_only_through(Some("dm-test0"));
    let control = Control::new();
    let pausing = control.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        pausing.pause();
    });
    let heard = Arc::new(Mutex::new(Vec::new()));
    let into = Arc::clone(&heard);
    let outcome = tokio::time::timeout(
        Duration::from_secs(5),
        before_download(&control, move |look, why, _| {
            lock(&into).push((look, why));
        }),
    )
    .await;
    set_only_through(None);
    set_lister(None);
    assert!(matches!(outcome, Ok(Err(Error::Paused))), "{outcome:?}");
    let heard = lock(&heard).clone();
    assert_eq!(heard.first().map(|h| h.0), Some(1));
    assert!(heard[0].1.contains("only through"), "{heard:?}");
}

#[tokio::test]
async fn a_connection_from_an_address_this_computer_lacks_goes_nowhere() {
    let _turn = TURN.lock().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    set_lister(Some(none_listed()));
    set_only_through(Some("dm-test0"));
    let refused = connect_tcp("127.0.0.1", port).await;
    set_only_through(None);
    set_lister(None);
    assert!(
        refused.is_err(),
        "connected from an address not this computer's"
    );
    // Unheld, the same connection is made.
    assert!(connect_tcp("127.0.0.1", port).await.is_ok());
}
