// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What a screen reader is told beyond an element's role and name (gaps row U23): that a
//! greyed control is off, how far a bar has come, and a sentence said aloud once in the status
//! bar. None of it changes what is drawn: GPUI writes it into the accessibility tree only while
//! a screen reader is reading the window.
//!
//! GPUI's element API has no "disabled" of its own, so an element told off writes it into its
//! own accessibility node from `a11y_synthetic_children`, which GPUI calls with that node just
//! after the element is laid out. A control greyed by [`crate::native::enabled`] is told off
//! the same way: the greyed wrapper holds [`GREYED`] up while what it holds is laid out, and
//! each of dm's controls asks it as its node is written.

use std::cell::Cell;

use gpui::{
    accesskit, AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, Role, SharedString, StatefulInteractiveElement, Window,
};

thread_local! {
    /// How many greyed wrappers are being laid out around what is laid out now: above none, a
    /// control is greyed. A count, as one greyed part of a page may hold another.
    static GREYED: Cell<usize> = const { Cell::new(0) };
}

/// Whether what is being laid out now is inside a greyed wrapper.
pub(crate) fn greyed() -> bool {
    GREYED.with(|g| g.get() > 0)
}

/// Held while a greyed wrapper's contents are laid out: [`greyed`] says so until it is dropped,
/// a panic part way through included.
struct Held;

impl Held {
    fn new() -> Self {
        GREYED.with(|g| g.set(g.get() + 1));
        Held
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        GREYED.with(|g| g.set(g.get().saturating_sub(1)));
    }
}

/// An accessibility node told it is off: a screen reader says "dimmed" (VoiceOver) or
/// "unavailable" (Narrator, Orca) and does not offer to press it.
pub(crate) fn mark_off(node: &mut accesskit::Node) {
    node.set_disabled();
}

/// The figures a progress bar gives a screen reader: from 0 to 100, at `fraction` of the way,
/// in whole per cent as the window's titles say it.
pub(crate) fn percent(fraction: f64) -> f64 {
    (fraction.clamp(0.0, 1.0) * 100.0).round()
}

/// What dm's elements tell a screen reader beyond their role and name.
pub(crate) trait Told: StatefulInteractiveElement + Sized {
    /// Told off where `off`: a control drawn greyed. Where it is not, nothing is set, so a
    /// control that already listens for a greyed wrapper ([`Told::off_where_greyed`]) still
    /// does.
    fn off_if(self, off: bool) -> Self {
        if off {
            self.a11y_synthetic_children(|built| mark_off(built.parent_node()))
        } else {
            self
        }
    }

    /// Told off while a greyed wrapper ([`crate::native::enabled`]) holds it: what a push
    /// button, a check box, a radio button or a pop-up does, wherever it is put.
    fn off_where_greyed(self) -> Self {
        self.a11y_synthetic_children(|built| {
            if greyed() {
                mark_off(built.parent_node());
            }
        })
    }

    /// A progress bar named `label`, `fraction` of the way: its value from 0 to 100.
    fn progress_bar(self, label: impl Into<SharedString>, fraction: f64) -> Self {
        self.role(Role::ProgressIndicator)
            .aria_label(label)
            .aria_min_numeric_value(0.0)
            .aria_max_numeric_value(100.0)
            .aria_numeric_value(percent(fraction))
    }

    /// Said aloud each time its name changes, after what is being said (a polite live region).
    fn said_aloud(self) -> Self {
        self.a11y_synthetic_children(|built| {
            built.parent_node().set_live(accesskit::Live::Polite);
        })
    }
}

impl<E: StatefulInteractiveElement> Told for E {}

/// What a greyed wrapper holds, laid out with [`GREYED`] held up so each control in it is told
/// off. It takes no room of its own: its place is its child's.
pub(crate) struct Greyed {
    child: AnyElement,
    off: bool,
}

impl Greyed {
    /// `child`, told off where `off`.
    pub(crate) fn new(off: bool, child: impl IntoElement) -> Self {
        Greyed {
            child: child.into_any_element(),
            off,
        }
    }
}

impl IntoElement for Greyed {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Greyed {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        // The controls' nodes are written as they are laid out, inside this.
        let _held = self.off.then(Held::new);
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

/// The sentence the status bar says aloud for what a read found: each failure with its reason,
/// then what finished (a batch counted), as the system's notifications say them; none where
/// nothing ended. `finished` and `failed` are the files' names, a failure's with its reason.
pub(crate) fn heard(finished: &[String], failed: &[(String, String)]) -> Option<String> {
    use dm_desktop_base::i18n::tf;
    let mut said: Vec<String> = failed
        .iter()
        .map(|(name, why)| tf("{} failed: {}", &[name, why]))
        .collect();
    match finished {
        [] => {}
        [name] => said.push(tf("{} finished.", &[name])),
        many => said.push(tf("{} downloads finished.", &[&many.len()])),
    }
    (!said.is_empty()).then(|| said.join(" "))
}

/// What GPUI writes into `element`'s accessibility node as it lays it out, read without a
/// window: its role and the properties it was given (not those written once it is laid out).
#[cfg(test)]
pub(crate) fn node_of<E: Element>(element: &E) -> accesskit::Node {
    let mut node = accesskit::Node::new(element.a11y_role().expect("an element with a role"));
    element.write_a11y_info(&mut node);
    node
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{div, InteractiveElement};

    #[test]
    fn a_greyed_wrapper_holds_its_controls_greyed_until_it_is_laid_out() {
        assert!(!greyed());
        {
            let _outer = Held::new();
            assert!(greyed());
            {
                // One greyed part of a page inside another.
                let _inner = Held::new();
                assert!(greyed());
            }
            assert!(greyed(), "the outer one still holds");
        }
        assert!(!greyed(), "laid out, nothing after it is greyed");
    }

    #[test]
    fn a_control_told_off_is_dimmed_to_a_screen_reader() {
        let mut node = accesskit::Node::new(Role::Button);
        assert!(!node.is_disabled());
        mark_off(&mut node);
        assert!(node.is_disabled());
    }

    #[test]
    fn a_progress_bar_gives_its_value_from_0_to_100() {
        let node = node_of(&div().id("bar").progress_bar("Progress", 0.426));
        assert_eq!(node.role(), Role::ProgressIndicator);
        assert_eq!(node.label(), Some("Progress"));
        assert_eq!(node.min_numeric_value(), Some(0.0));
        assert_eq!(node.max_numeric_value(), Some(100.0));
        assert_eq!(node.numeric_value(), Some(43.0));
        // Past either end, held to it.
        assert_eq!(percent(1.7), 100.0);
        assert_eq!(percent(-0.2), 0.0);
    }

    #[test]
    fn a_push_button_and_a_check_box_say_what_they_are_and_how_they_stand() {
        let theme = crate::theme::Theme::of(gpui::WindowAppearance::Light);
        let screen = crate::screen::Screen::default();
        let ok = node_of(&crate::native::button(
            "ok",
            "OK".into(),
            true,
            theme,
            &screen,
        ));
        assert_eq!(ok.role(), Role::Button);
        assert_eq!(ok.label(), Some("OK"));
        let ticked = node_of(&crate::native::checkbox(
            "tick",
            "Ask".into(),
            true,
            theme,
            &screen,
        ));
        assert_eq!(ticked.role(), Role::CheckBox);
        assert_eq!(ticked.toggled(), Some(accesskit::Toggled::True));
    }

    #[test]
    fn what_ended_is_said_aloud_as_the_notifications_say_it() {
        assert_eq!(heard(&[], &[]), None);
        assert_eq!(
            heard(&["film.mp4".into()], &[]).as_deref(),
            Some("film.mp4 finished.")
        );
        assert_eq!(
            heard(&["a".into(), "b".into()], &[]).as_deref(),
            Some("2 downloads finished.")
        );
        assert_eq!(
            heard(
                &["film.mp4".into()],
                &[("song.mp3".into(), "Failed: the server is gone".into())]
            )
            .as_deref(),
            Some("song.mp3 failed: Failed: the server is gone film.mp4 finished.")
        );
    }
}
