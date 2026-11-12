// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Windows' toasts, posted by dm itself rather than through notify-rust
//! (gaps row 156).
//!
//! notify-rust hangs a click handler and a dismissal handler on every toast
//! and never takes them off, and Windows keeps three handles (two
//! semaphores and an event) for each such toast for as long as dm runs, the
//! Action Center cleared or not: 30 toasts took 90 handles with the two
//! handlers and none without (measured 2026-11-07). A long session of
//! downloads grew by about three handles a finished download.
//!
//! So a toast with nothing to open has no handler; one with a file to open
//! has its click heard until it is clicked or leaves the screen, and then
//! the handlers are taken off. The toast is the one notify-rust made: its
//! title, an empty second line, the body, short and silent, said to come
//! from PowerShell (notify-rust's id for a program that registers none of
//! its own, as dm does not).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use windows::core::{h, IInspectable, Interface, Ref, HSTRING};
use windows::Data::Xml::Dom::XmlDocument;
use windows::Foundation::TypedEventHandler;
use windows::UI::Notifications::{
    ToastActivatedEventArgs, ToastDismissedEventArgs, ToastNotification, ToastNotificationManager,
};

/// The program the toasts say they come from.
pub const APP_ID: &str =
    r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe";

/// The most toasts whose click is heard at once, as notify-rust's had at
/// most eight threads waiting on them. A toast that never leaves the screen
/// as a banner (sent straight to the Action Center) is never dismissed and
/// would hold its handlers for good: past this many, the oldest lets go.
pub const MOST_HEARD: usize = 8;

/// A toast whose click is heard, and the tokens that stop it.
struct Heard {
    toast: ToastNotification,
    activated: i64,
    dismissed: i64,
}

impl Heard {
    /// Takes both handlers off, which is what gives Windows' handles back.
    fn let_go(self) {
        let _ = self.toast.RemoveActivated(self.activated);
        let _ = self.toast.RemoveDismissed(self.dismissed);
    }
}

type Slot = Arc<Mutex<Option<Heard>>>;

/// The toasts heard now, oldest first.
static HEARD: Mutex<VecDeque<Slot>> = Mutex::new(VecDeque::new());

/// How many toasts' clicks are heard now.
pub fn heard() -> usize {
    HEARD.lock().map_or(0, |heard| heard.len())
}

/// `slot`'s toast, no longer heard: out of the list and out of its slot,
/// once only, whoever asks first (its click, its dismissal, a newer toast).
fn take(slot: &Slot) -> Option<Heard> {
    if let Ok(mut heard) = HEARD.lock() {
        heard.retain(|s| !Arc::ptr_eq(s, slot));
    }
    slot.lock().ok()?.take()
}

/// Shows a toast of `title` and `body`. `on_click` runs at its first click
/// while it is on the screen.
pub fn show<F>(title: &str, body: &str, on_click: Option<F>) -> windows::core::Result<()>
where
    F: Fn() + Send + 'static,
{
    post(
        title,
        body,
        None,
        on_click.map(|f| move |_: &str| f()),
        None,
    )
}

/// The key a toast's button answers with: [`show_file`]'s Open folder.
pub const FOLDER: &str = "folder";

/// As [`show`], for a finished file: a button with `button`'s words under
/// it, and `on_click` told what was clicked, [`FOLDER`] for the button and
/// nothing for the toast itself (gaps row U15).
pub fn show_file<F>(title: &str, body: &str, button: &str, on_click: F) -> windows::core::Result<()>
where
    F: Fn(&str) + Send + 'static,
{
    post(title, body, Some(button), Some(on_click), None)
}

/// As [`show`], into the Action Center without a banner and in `group`, so
/// a test can count what it costs and take its toasts away again
/// ([`remove_group`]) without showing a person anything.
#[doc(hidden)]
pub fn show_unseen<F>(
    title: &str,
    body: &str,
    on_click: Option<F>,
    group: &str,
) -> windows::core::Result<()>
where
    F: Fn() + Send + 'static,
{
    post(
        title,
        body,
        None,
        on_click.map(|f| move |_: &str| f()),
        Some(group),
    )
}

/// Takes the toasts of `group` out of the Action Center.
#[doc(hidden)]
pub fn remove_group(group: &str) -> windows::core::Result<()> {
    ToastNotificationManager::History()?
        .RemoveGroupWithId(&HSTRING::from(group), &HSTRING::from(APP_ID))
}

fn post<F>(
    title: &str,
    body: &str,
    button: Option<&str>,
    on_click: Option<F>,
    unseen: Option<&str>,
) -> windows::core::Result<()>
where
    F: Fn(&str) + Send + 'static,
{
    let toast = template(title, body, button)?;
    if let Some(group) = unseen {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        toast.SetSuppressPopup(true)?;
        toast.SetGroup(&HSTRING::from(group))?;
        // A tag of its own: toasts of one tag and group replace each other.
        toast.SetTag(&HSTRING::from(n.to_string()))?;
    }
    let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))?;
    let Some(on_click) = on_click else {
        return notifier.Show(&toast);
    };
    let slot: Slot = Arc::new(Mutex::new(None));
    let clicked = Arc::clone(&slot);
    let activated = toast.Activated(&TypedEventHandler::new(
        move |_: Ref<ToastNotification>, args: Ref<IInspectable>| {
            if let Some(heard) = take(&clicked) {
                heard.let_go();
                // A button's `arguments`; empty for the toast itself.
                let what = args
                    .ok()
                    .and_then(|args| args.cast::<ToastActivatedEventArgs>())
                    .and_then(|args| args.Arguments())
                    .map(|said| said.to_string())
                    .unwrap_or_default();
                on_click(&what);
            }
            Ok(())
        },
    ))?;
    let gone = Arc::clone(&slot);
    let dismissed = match toast.Dismissed(&TypedEventHandler::new(
        move |_: Ref<ToastNotification>, _: Ref<ToastDismissedEventArgs>| {
            if let Some(heard) = take(&gone) {
                heard.let_go();
            }
            Ok(())
        },
    )) {
        Ok(token) => token,
        Err(e) => {
            let _ = toast.RemoveActivated(activated);
            return Err(e);
        }
    };
    // In its slot before it is shown, so a click that comes at once is heard.
    if let Ok(mut held) = slot.lock() {
        *held = Some(Heard {
            toast: toast.clone(),
            activated,
            dismissed,
        });
    }
    let oldest: Vec<Slot> = match HEARD.lock() {
        Ok(mut heard) => {
            heard.push_back(Arc::clone(&slot));
            let over = heard.len().saturating_sub(MOST_HEARD);
            heard.drain(..over).collect()
        }
        Err(_) => Vec::new(),
    };
    // Let go outside the list's lock: a handler running now takes it.
    for old in oldest {
        if let Some(heard) = old.lock().ok().and_then(|mut h| h.take()) {
            heard.let_go();
        }
    }
    let shown = notifier.Show(&toast);
    if shown.is_err() {
        if let Some(heard) = take(&slot) {
            heard.let_go();
        }
    }
    shown
}

/// The toast's XML, made as a document rather than as text, so a file's
/// name is never read as markup.
fn template(
    title: &str,
    body: &str,
    button: Option<&str>,
) -> windows::core::Result<ToastNotification> {
    let doc = XmlDocument::new()?;
    let toast = doc.CreateElement(h!("toast"))?;
    toast.SetAttribute(h!("duration"), h!("short"))?;
    let visual = doc.CreateElement(h!("visual"))?;
    let binding = doc.CreateElement(h!("binding"))?;
    binding.SetAttribute(h!("template"), h!("ToastGeneric"))?;
    for (id, text) in [("1", title), ("2", ""), ("3", body)] {
        let line = doc.CreateElement(h!("text"))?;
        line.SetAttribute(h!("id"), &HSTRING::from(id))?;
        line.SetInnerText(&HSTRING::from(text))?;
        binding.AppendChild(&line)?;
    }
    visual.AppendChild(&binding)?;
    toast.AppendChild(&visual)?;
    if let Some(words) = button {
        let actions = doc.CreateElement(h!("actions"))?;
        let action = doc.CreateElement(h!("action"))?;
        action.SetAttribute(h!("content"), &HSTRING::from(words))?;
        action.SetAttribute(h!("arguments"), &HSTRING::from(FOLDER))?;
        actions.AppendChild(&action)?;
        toast.AppendChild(&actions)?;
    }
    let audio = doc.CreateElement(h!("audio"))?;
    audio.SetAttribute(h!("silent"), h!("true"))?;
    toast.AppendChild(&audio)?;
    doc.AppendChild(&toast)?;
    ToastNotification::CreateToastNotification(&doc)
}
