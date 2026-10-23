//! The window, driven the way a person drives it — through the names a
//! screen reader reads, the words on its buttons and dialogs — against a real queue fetching from a real server on loopback.
//! Nothing is stubbed but the operating system: the login item,
//! notifications, the clipboard, opening files and choosing them.

//! The tests live in `tests/window/`, by what they check; `support` holds the
//! fake system and the helpers they share.

#[path = "../../../test-support/secrets.rs"]
mod test_secrets;

#[path = "window/commands.rs"]
mod commands;
#[path = "window/control.rs"]
mod control;
#[path = "window/dialogs.rs"]
mod dialogs;
#[path = "window/downloads.rs"]
mod downloads;
#[path = "window/keyboard.rs"]
mod keyboard;
#[path = "window/layout.rs"]
mod layout;
#[path = "window/lists.rs"]
mod lists;
#[path = "window/options.rs"]
mod options;
#[path = "window/speed_schedule.rs"]
mod speed_schedule;
#[path = "window/support.rs"]
mod support;
