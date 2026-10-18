//! The "Download video" panel's markup and the parts of its behaviour that
//! need no page: what it looks like, what a key does, and whether a player
//! is visible enough to offer. The page script (`content.rs`, wasm only)
//! draws it and asks these; they are tested on the host.

/// The panel's style, inside its shadow root.
///
/// `:host { all: initial }` does not reset `direction`, which is inherited
/// from the page: on a right-to-left page the bar came out mirrored, the ×
/// on the left. The host is held to left-to-right, as dm draws it.
/// A message with no break in it (a long address) wraps anywhere rather
/// than running out of the bar.
pub const STYLE: &str = r#"
:host { all: initial; direction: ltr; unicode-bidi: isolate;
  /* The app's own colours, radius and type, dark over a player by default
     and light where the browser is: the panel had an orange of its own. */
  --surface: rgba(31, 31, 31, .95); --surface-solid: rgba(31, 31, 31, .98);
  --text: #ffffff; --text2: #d6d6d6; --weak: #adadad; --line: rgba(255,255,255,.18);
  --hover: rgba(255,255,255,.10); --accent: #479ef5; --on-accent: #0a0a0a;
  --success: #54b054; --error: #ee7b85; --radius: 4px; }
@media (prefers-color-scheme: light) {
  :host { --surface: rgba(255, 255, 255, .96); --surface-solid: rgba(255, 255, 255, .99);
    --text: #242424; --text2: #424242; --weak: #616161; --line: rgba(0,0,0,.18);
    --hover: rgba(0,0,0,.06); --accent: #0f6cbd; --on-accent: #ffffff;
    --success: #0e700e; --error: #c50f1f; }
}
* { box-sizing: border-box; margin: 0; font-family: system-ui, -apple-system, Roboto, sans-serif; }
.panel { width: 240px; color: var(--text); direction: ltr; }
.wrap {
  display: flex; align-items: stretch; height: 32px; border-radius: var(--radius); overflow: hidden;
  background: var(--surface); color: var(--text); user-select: none;
  border: 1px solid var(--line); box-shadow: 0 4px 16px rgba(0,0,0,.35);
  font-size: 13px; line-height: 1;
}
.wrap.done { border-color: var(--success); }
.wrap.dead { border-color: var(--error); }
button { font: inherit; color: inherit; background: none; border: 0; padding: 0; cursor: pointer; }
button:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
.go { flex: 1; min-width: 0; display: flex; align-items: center; gap: 8px; padding: 0 8px; text-align: left; }
.go:hover, .go:focus-visible { background: var(--hover); }
.mark { width: 20px; height: 20px; flex: none; border-radius: 50%; background: var(--accent); color: var(--on-accent);
  display: flex; align-items: center; justify-content: center; font-weight: 700; font-size: 12px; }
.wrap.done .mark { background: var(--success); }
.wrap.dead .mark { background: var(--error); }
.title { flex: 1; min-width: 9.5em; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.caret { flex: none; font-size: 12px; color: var(--text2); }
.help, .close { width: 28px; flex: none; color: var(--weak); font-size: 13px; border-left: 1px solid var(--line); }
.help:hover, .close:hover, .help:focus-visible, .close:focus-visible { background: var(--hover); color: var(--text); }
.list { margin-top: 4px; border-radius: var(--radius); overflow: hidden; background: var(--surface-solid);
  border: 1px solid var(--line); box-shadow: 0 8px 20px rgba(0,0,0,.4); max-height: 260px; overflow-y: auto; }
.item { display: block; width: 100%; padding: 8px 12px; text-align: left; font-size: 13px; white-space: nowrap; color: var(--text);
  overflow: hidden; text-overflow: ellipsis; border-top: 1px solid var(--line); }
.item:first-child { border-top: 0; }
.item:hover, .item:focus-visible { background: var(--accent); color: var(--on-accent); outline: none; }
.sub { margin-top: 4px; padding: 8px 8px; border-radius: var(--radius); background: var(--surface);
  color: var(--text2); font-size: 12px; line-height: 1.4; max-width: 100%; max-height: 160px; overflow-y: auto;
  white-space: normal; overflow-wrap: anywhere; word-break: break-word; }
.live { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0);
  clip-path: inset(50%); white-space: nowrap; }
[hidden] { display: none !important; }
"#;

/// The panel's elements. The bar's button is named by its words (the mark
/// and the ▾ are hidden from screen readers), so what it is called changes
/// with what it says; `.live` says each result aloud.
pub const BODY: &str = r#"<div class="panel" role="group" aria-label="dm">
<div class="wrap">
<button class="go" type="button"><span class="mark" aria-hidden="true">&#8595;</span><span class="title"></span><span class="caret" aria-hidden="true" hidden>&#9662;</span></button>
<button class="help" type="button" aria-label="What is this?" title="What is this?">?</button>
<button class="close" type="button" aria-label="Hide" aria-haspopup="menu" title="Hide">&#215;</button>
</div>
<div class="list" role="menu" hidden></div>
<div class="sub" hidden></div>
<div class="live" role="status" aria-live="polite" aria-atomic="true"></div>
</div>"#;

/// A key pressed in the panel, as far as the panel cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Escape,
    Down,
    Up,
    Home,
    End,
    Tab,
    ShiftTab,
    Other,
}

impl Key {
    /// From a `KeyboardEvent`'s `key` and whether Shift was held.
    pub fn of(key: &str, shift: bool) -> Key {
        match key {
            "Escape" | "Esc" => Key::Escape,
            "ArrowDown" | "Down" => Key::Down,
            "ArrowUp" | "Up" => Key::Up,
            "Home" => Key::Home,
            "End" => Key::End,
            "Tab" if shift => Key::ShiftTab,
            "Tab" => Key::Tab,
            _ => Key::Other,
        }
    }
}

/// Where the focus is in the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// The bar's button.
    Go,
    Help,
    Close,
    /// A line of the open list (qualities, or the × menu), by position.
    Item(usize),
}

/// What a key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    /// Close the open list and put the focus back on the bar.
    CloseList,
    /// Hide the panel and put the focus back on the player.
    Hide,
    Focus(Focus),
    /// Back to the player, the key's own work stopped (Shift+Tab off the
    /// bar) or let go on from there (Tab off the last button), so Tab
    /// carries on through the page as if the panel sat right after the
    /// player.
    ToPlayer {
        stop: bool,
    },
    /// Nothing: the browser does what it does.
    Nothing,
}

/// What `key` does with the focus at `at` and a list of `lines` lines open
/// (zero when none is). Enter and Space are a button's own and are not
/// handled here.
pub fn act(key: Key, at: Focus, lines: usize) -> Act {
    let last = lines.saturating_sub(1);
    match (key, at) {
        (Key::Escape, _) if lines > 0 => Act::CloseList,
        (Key::Escape, _) => Act::Hide,
        (Key::Down, Focus::Item(i)) => Act::Focus(Focus::Item(if i >= last { 0 } else { i + 1 })),
        (Key::Up, Focus::Item(i)) => Act::Focus(Focus::Item(if i == 0 { last } else { i - 1 })),
        (Key::Home, Focus::Item(_)) => Act::Focus(Focus::Item(0)),
        (Key::End, Focus::Item(_)) => Act::Focus(Focus::Item(last)),
        (Key::Down, _) if lines > 0 => Act::Focus(Focus::Item(0)),
        (Key::Up, _) if lines > 0 => Act::Focus(Focus::Item(last)),
        (Key::ShiftTab, Focus::Go) => Act::ToPlayer { stop: true },
        // Off the last of the bar's buttons, with no list below it.
        (Key::Tab, Focus::Close) if lines == 0 => Act::ToPlayer { stop: false },
        (Key::Tab, Focus::Item(i)) if i >= last => Act::ToPlayer { stop: false },
        _ => Act::Nothing,
    }
}

/// Whether a player is visible enough to be offered, by its computed
/// `opacity` and `visibility`: a transparent or hidden `<video>` (a
/// background loop faded out, a preloaded clip parked out of sight) is
/// nothing a person sees to download.
pub fn looks_visible(opacity: &str, visibility: &str) -> bool {
    let opaque = opacity.trim().parse::<f64>().map_or(true, |o| o > 0.1);
    let visibility = visibility.trim();
    opaque && (visibility.is_empty() || visibility == "visible")
}

/// The point to hit-test for whether a player is what is seen there: the
/// middle of the part of it inside the viewport, or `None` when no part is.
pub fn centre_seen(rect: (f64, f64, f64, f64), viewport: (f64, f64)) -> Option<(f64, f64)> {
    let (left, top, width, height) = rect;
    let (view_w, view_h) = viewport;
    // `NAN.max(0.0)` is 0: an odd layout's NaN is caught before it hides.
    if ![left, top, width, height, view_w, view_h]
        .iter()
        .all(|v| v.is_finite())
    {
        return None;
    }
    let x0 = left.max(0.0);
    let y0 = top.max(0.0);
    let x1 = (left + width).min(view_w);
    let y1 = (top + height).min(view_h);
    (x1 > x0 && y1 > y0).then(|| ((x0 + x1) / 2.0, (y0 + y1) / 2.0))
}

/// Whether a layer at a video's middle, of `layer` square pixels, is the
/// size of a player's own layer: no larger than the player's box (the
/// video's closest positioned ancestor, when it has one) or, without one,
/// than the video, with room for its frame. A dialog over the page is not.
/// A vertical clip letterboxed in a wide player is judged by the player.
pub fn layer_fits(video: f64, player: Option<f64>, layer: f64) -> bool {
    let room = player.filter(|p| p.is_finite()).unwrap_or(0.0).max(video);
    room > 0.0 && layer.is_finite() && layer <= room * 2.5
}

/// Whether `outer` is `inner` or one of its ancestors, by `parent` — the
/// page's composed tree, where a shadow root's child goes up to its host.
pub fn composed_within<N: PartialEq>(
    inner: N,
    outer: &N,
    parent: impl Fn(&N) -> Option<N>,
    limit: usize,
) -> bool {
    let mut at = Some(inner);
    for _ in 0..limit {
        match at {
            Some(node) if &node == outer => return true,
            Some(node) => at = parent(&node),
            None => return false,
        }
    }
    false
}

#[cfg(test)]
mod tests {
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
}
