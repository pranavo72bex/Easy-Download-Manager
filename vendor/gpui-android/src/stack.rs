//! dm(android-stack): the windows GPUI opens, as a stack on the phone's one surface.
//!
//! A phone shows one screen, and NativeActivity hands the app one `ANativeWindow`. GPUI (and
//! the app on it) opens many windows: a dialog, a settings window, a question. Each is a
//! layer of this stack: the newest shown is drawn over the whole surface and given the input;
//! when it closes, the one below it is drawn again. Back is Escape given to the window on top
//! (so a dialog's own Escape, Cancel, closes it, and an app's in-window overlays close first);
//! where the window does nothing with it, the window on top is closed as its close button
//! would close it, and with only the first window left the app's task goes to the background
//! (Android's Back on an app's first screen), so its work goes on.
//!
//! This file is plain logic, with no Android in it, so its tests run on any machine
//! (`rustc --edition 2021 --test src/stack.rs`); `android/layers.rs` holds the windows'
//! callbacks in it and does what [`after_back`] decides.

/// The windows open on the surface, oldest first, each with what the platform keeps for it.
#[derive(Debug)]
pub(crate) struct Stack<T> {
    layers: Vec<Layer<T>>,
}

#[derive(Debug)]
struct Layer<T> {
    id: u64,
    /// Whether the window has been shown: one opened hidden (GPUI's `show: false`) is never
    /// on top until it is activated.
    shown: bool,
    value: T,
}

impl<T> Default for Stack<T> {
    fn default() -> Self {
        Stack { layers: Vec::new() }
    }
}

impl<T> Stack<T> {
    /// Opens window `id` over the others; a hidden one waits under the shown ones.
    pub(crate) fn push(&mut self, id: u64, shown: bool, value: T) {
        self.layers.retain(|layer| layer.id != id);
        self.layers.push(Layer { id, shown, value });
    }

    /// Closes window `id`, wherever it is in the stack, and hands back what was kept for it.
    pub(crate) fn remove(&mut self, id: u64) -> Option<T> {
        let at = self.layers.iter().position(|layer| layer.id == id)?;
        Some(self.layers.remove(at).value)
    }

    /// Brings window `id` to the top, shown (GPUI's `activate`). False where it is not open.
    pub(crate) fn raise(&mut self, id: u64) -> bool {
        let Some(at) = self.layers.iter().position(|layer| layer.id == id) else {
            return false;
        };
        let mut layer = self.layers.remove(at);
        layer.shown = true;
        self.layers.push(layer);
        true
    }

    /// The window drawn and given the input: the newest shown.
    pub(crate) fn top(&self) -> Option<(u64, &T)> {
        self.layers
            .iter()
            .rev()
            .find(|layer| layer.shown)
            .map(|layer| (layer.id, &layer.value))
    }

    /// The id of the window on top.
    pub(crate) fn top_id(&self) -> Option<u64> {
        self.top().map(|(id, _)| id)
    }

    /// How many windows are shown.
    pub(crate) fn shown(&self) -> usize {
        self.layers.iter().filter(|layer| layer.shown).count()
    }

    /// Every window, oldest first.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (u64, &T)> {
        self.layers.iter().map(|layer| (layer.id, &layer.value))
    }
}

/// What Back does once the window on top has been given Escape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Back {
    /// The window did something with it (closed an overlay, or itself): nothing more.
    Done,
    /// The window on top did nothing with it and windows are under it: it is closed, as its
    /// close button closes it (asked first whether it may close).
    Close(u64),
    /// The first window alone, doing nothing with it: the task goes to the background (the
    /// activity's `moveTaskToBack`), not ended, so downloads go on.
    ToBackground,
}

/// What Back does: `handled` is whether the window on top stopped Escape (its result said so),
/// `top_before` and `top_after` the window on top before and after Escape was given (a window
/// that closed itself, or opened another, changed it), `shown_after` how many windows are
/// shown after.
pub(crate) fn after_back(
    handled: bool,
    top_before: Option<u64>,
    top_after: Option<u64>,
    shown_after: usize,
) -> Back {
    let Some(top) = top_before else {
        return Back::ToBackground;
    };
    if handled || top_after != Some(top) {
        return Back::Done;
    }
    if shown_after > 1 {
        Back::Close(top)
    } else {
        Back::ToBackground
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newest_shown_window_is_on_top_and_closing_it_shows_the_one_below() {
        let mut stack = Stack::default();
        assert_eq!(stack.top_id(), None);
        stack.push(1, true, "main");
        stack.push(2, true, "options");
        stack.push(3, true, "question");
        assert_eq!(stack.top(), Some((3, &"question")));
        assert_eq!(stack.shown(), 3);
        assert_eq!(stack.remove(3), Some("question"));
        assert_eq!(stack.top_id(), Some(2));
        // A window under the top closing leaves the top where it is.
        assert_eq!(stack.remove(1), Some("main"));
        assert_eq!(stack.top_id(), Some(2));
        assert_eq!(stack.remove(1), None);
        assert_eq!(stack.remove(2), Some("options"));
        assert_eq!(stack.top_id(), None);
    }

    #[test]
    fn a_hidden_window_waits_until_it_is_raised() {
        let mut stack = Stack::default();
        stack.push(1, true, ());
        stack.push(2, false, ());
        assert_eq!(stack.top_id(), Some(1));
        assert_eq!(stack.shown(), 1);
        assert!(stack.raise(2));
        assert_eq!(stack.top_id(), Some(2));
        // Raising an older window brings it over the newer.
        assert!(stack.raise(1));
        assert_eq!(stack.top_id(), Some(1));
        assert_eq!(stack.iter().map(|(id, _)| id).collect::<Vec<_>>(), [2, 1]);
        assert!(!stack.raise(9));
    }

    #[test]
    fn a_window_pushed_twice_is_one_layer() {
        let mut stack = Stack::default();
        stack.push(1, true, 'a');
        stack.push(1, true, 'b');
        assert_eq!(stack.shown(), 1);
        assert_eq!(stack.top(), Some((1, &'b')));
    }

    #[test]
    fn back_closes_overlays_then_windows_then_sends_the_task_back() {
        // The window used Escape (an overlay closed, a dialog's Cancel ran): nothing more.
        assert_eq!(after_back(true, Some(2), Some(2), 2), Back::Done);
        // The window closed itself, or opened a question, with Escape: nothing more.
        assert_eq!(after_back(false, Some(2), Some(1), 1), Back::Done);
        assert_eq!(after_back(false, Some(2), Some(3), 3), Back::Done);
        // Ignored by a window over another: that window closes.
        assert_eq!(after_back(false, Some(2), Some(2), 2), Back::Close(2));
        // Ignored by the first window alone: the task to the background, dm not ended.
        assert_eq!(after_back(false, Some(1), Some(1), 1), Back::ToBackground);
        // No window at all.
        assert_eq!(after_back(false, None, None, 0), Back::ToBackground);
    }
}
