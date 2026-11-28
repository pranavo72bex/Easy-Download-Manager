// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

#[test]
fn the_panel_wears_the_apps_colours_and_type_in_either_theme() {
    // Colours only from the variables the app's tokens set, in a dark
    // and a light variant; no text under 12 px; one radius.
    for line in STYLE.lines() {
        if let Some(at) = line.find("font-size:") {
            let size: f32 = line[at + 10..]
                .trim()
                .trim_end_matches(|c: char| !c.is_ascii_digit())
                .split("px")
                .next()
                .unwrap_or("0")
                .trim()
                .parse()
                .unwrap_or(0.0);
            assert!(size >= 12.0, "{line}");
        }
    }
    assert!(STYLE.contains("@media (prefers-color-scheme: light)"));
    assert!(!STYLE.contains("#f08a24"), "the panel's own orange is back");
    assert!(!STYLE.contains("border-radius: 6px"));
}

#[test]
fn the_panel_is_drawn_left_to_right_and_long_messages_wrap() {
    let host = STYLE.lines().find(|l| l.starts_with(":host")).unwrap();
    assert!(host.contains("direction: ltr"), "{host}");
    let sub = STYLE.split(".sub {").nth(1).unwrap();
    let sub = &sub[..sub.find('}').unwrap()];
    assert!(sub.contains("overflow-wrap: anywhere"), "{sub}");
    assert!(sub.contains("max-width"), "{sub}");
    // A line of the list wraps rather than being cut: its quality is the point of it.
    let item = STYLE.split(".item {").nth(1).unwrap();
    let item = &item[..item.find('}').unwrap()];
    assert!(
        item.contains("white-space: normal") && !item.contains("ellipsis"),
        "{item}"
    );
}

#[test]
fn a_screen_reader_hears_what_the_bar_says_and_each_result() {
    // No fixed name on the bar's button: its words are its name.
    let go = BODY.split("<button class=\"go\"").nth(1).unwrap();
    let go = &go[..go.find('>').unwrap()];
    assert!(!go.contains("aria-label"), "{go}");
    assert!(BODY.contains(r#"class="mark" aria-hidden="true""#));
    assert!(BODY.contains(r#"class="caret" aria-hidden="true""#));
    assert!(BODY.contains(r#"role="status" aria-live="polite""#));
}

#[test]
fn keys_move_through_the_list_and_escape_backs_out() {
    assert_eq!(act(Key::Escape, Focus::Item(1), 3), Act::CloseList);
    assert_eq!(act(Key::Escape, Focus::Go, 0), Act::Hide);
    assert_eq!(act(Key::Down, Focus::Go, 3), Act::Focus(Focus::Item(0)));
    assert_eq!(act(Key::Up, Focus::Go, 3), Act::Focus(Focus::Item(2)));
    assert_eq!(
        act(Key::Down, Focus::Item(0), 3),
        Act::Focus(Focus::Item(1))
    );
    assert_eq!(
        act(Key::Down, Focus::Item(2), 3),
        Act::Focus(Focus::Item(0))
    );
    assert_eq!(act(Key::Up, Focus::Item(0), 3), Act::Focus(Focus::Item(2)));
    assert_eq!(
        act(Key::Home, Focus::Item(2), 3),
        Act::Focus(Focus::Item(0))
    );
    assert_eq!(act(Key::End, Focus::Item(0), 3), Act::Focus(Focus::Item(2)));
    assert_eq!(act(Key::Down, Focus::Go, 0), Act::Nothing);
    assert_eq!(
        act(Key::ShiftTab, Focus::Go, 0),
        Act::ToPlayer { stop: true }
    );
    assert_eq!(
        act(Key::Tab, Focus::Close, 0),
        Act::ToPlayer { stop: false }
    );
    assert_eq!(
        act(Key::Tab, Focus::Close, 2),
        Act::Nothing,
        "on into the list"
    );
    assert_eq!(
        act(Key::Tab, Focus::Item(1), 2),
        Act::ToPlayer { stop: false }
    );
    assert_eq!(act(Key::Tab, Focus::Go, 0), Act::Nothing);
    assert_eq!(act(Key::Other, Focus::Go, 3), Act::Nothing);
    assert_eq!(Key::of("Tab", true), Key::ShiftTab);
    assert_eq!(Key::of("Esc", false), Key::Escape);
}

#[test]
fn a_hidden_or_transparent_player_is_not_offered() {
    assert!(looks_visible("1", "visible"));
    assert!(looks_visible("0.6", "visible"));
    assert!(!looks_visible("0", "visible"));
    assert!(!looks_visible("0.01", "visible"));
    assert!(!looks_visible("1", "hidden"));
    assert!(!looks_visible("1", "collapse"));
    assert!(
        looks_visible("", ""),
        "a browser that says nothing hides nothing"
    );
}

#[test]
fn a_player_is_hit_tested_where_it_is_on_screen() {
    assert_eq!(
        centre_seen((40.0, 40.0, 640.0, 360.0), (1280.0, 800.0)),
        Some((360.0, 220.0))
    );
    // Half scrolled off the top: the middle of what shows.
    assert_eq!(
        centre_seen((0.0, -200.0, 640.0, 400.0), (1280.0, 800.0)),
        Some((320.0, 100.0))
    );
    assert_eq!(
        centre_seen((0.0, 900.0, 640.0, 360.0), (1280.0, 800.0)),
        None
    );
    assert!(layer_fits(640.0 * 360.0, None, 660.0 * 400.0));
    assert!(
        !layer_fits(640.0 * 360.0, None, 1280.0 * 2000.0),
        "a page section"
    );
    assert!(!layer_fits(0.0, None, 10.0));
    // A 405×720 clip letterboxed in a 1280×720 player, its layer as wide
    // as the player: by the player's box, it fits.
    assert!(layer_fits(
        405.0 * 720.0,
        Some(1280.0 * 720.0),
        1280.0 * 720.0
    ));
    assert!(!layer_fits(405.0 * 720.0, None, 1280.0 * 720.0));
    assert_eq!(centre_seen((f64::NAN, 0.0, 1.0, 1.0), (10.0, 10.0)), None);
}

#[test]
fn an_embedded_players_frame_is_the_player() {
    // YouTube's embed: a 4:3 video in a 640×360 frame, its controls under the frame's body.
    assert!(frame_is_player(
        true,
        480.0 * 360.0,
        Some(0.0),
        640.0 * 360.0
    ));
    // The same layer in a page of its own is not taken for the player's.
    assert!(!frame_is_player(
        false,
        480.0 * 360.0,
        Some(0.0),
        640.0 * 360.0
    ));
    // A frame as large as a page (an article inside it) is judged as a page.
    assert!(!frame_is_player(true, 480.0 * 360.0, None, 1280.0 * 2000.0));
}

#[test]
fn a_layer_in_another_shadow_tree_is_still_the_players() {
    // mux-player's shape: a gesture layer (3) in mux-player's tree, the
    // <video> (6) inside <mux-video>'s (4) shadow root (5). Composed
    // parents: 6→5→4→2, 3→2, 2→1.
    let parent = |n: &u8| match n {
        6 => Some(5),
        5 => Some(4),
        4 | 3 => Some(2),
        2 => Some(1),
        _ => None,
    };
    // The layer is inside the player element (2) the video is also in.
    assert!(composed_within(3, &2, parent, 64));
    assert!(composed_within(6, &2, parent, 64), "across the shadow root");
    assert!(
        composed_within(6, &4, parent, 64),
        "the host holds the video"
    );
    assert!(!composed_within(3, &4, parent, 64));
    assert!(!composed_within(1, &2, parent, 64));
}
