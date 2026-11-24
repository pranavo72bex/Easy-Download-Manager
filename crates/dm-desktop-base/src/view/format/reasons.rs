// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A failure in plain words: the reasons dm knows, a retry's cause, and the
//! next step and button that go with them.

/// The common engine failures in plain words, by what their text holds
/// (lower case): the engine's own sentence where it has one, and a plain
/// one for what the network library says in its own terms ("error decoding
/// response body" is a connection cut mid-file). The first that matches
/// wins; anything else is shown as the engine said it.
/// Words, not a row's `FailKind`: what is reworded is mostly the network library's, the
/// system's and servers' own words, and the same rewording serves texts that carry no kind (a
/// stalled row's why, a retry's cause, a file error). It chooses only the words shown, and the
/// next step and button that go with them (gaps row A9).
pub const REASONS: [(&[&str], &str); 38] = [
    // First: the failure text of a row waiting for a fresh address also names
    // the refusal that expired it (`dm_core::LINK_EXPIRED`).
    (
        &["dm is waiting for the browser to hand over a new one"],
        "this link has expired",
    ),
    // Second: the dial-up or VPN connection a download waited for (dm-shell's `dialup.rs`),
    // whose failure ends in Windows' own reason, which can read as any of those below.
    // Reference A's row says "Cannot connect to the Internet." (2026-11-08).
    (
        &["did not come up"],
        "the dial-up or VPN connection didn't come up",
    ),
    // Third: a download waiting for the one network downloads are held to (dm-net's
    // `only_through`, gaps row W12), before any of the words below its adapter's name may hold.
    (
        &["that downloads go only through"],
        "the network downloads go only through is not connected",
    ),
    (
        &[
            "error decoding response body",
            "error reading a body",
            "unexpected eof",
            "unexpected end of file",
            "connection reset",
            "connection closed before",
            "broken pipe",
            "the connection ended after",
            "incomplete message",
        ],
        "the connection dropped before the file was complete",
    ),
    (
        &["the server sent ", "bytes of a file it said was"],
        "the server sent only part of the file",
    ),
    (
        &["too many redirects"],
        "the site keeps redirecting in a loop, so the file can't be reached",
    ),
    (
        &["the server's name could not be found"],
        "the site couldn't be found",
    ),
    (
        &["the server refused the connection"],
        "the server refused the connection",
    ),
    (
        &["the server did not answer in time", "timed out"],
        "the server took too long to answer",
    ),
    // A certificate failure by its cause, as the TLS library names it (gaps row H8); any
    // other cause is said as one that could not be checked, the next line.
    (
        &["certificate: expired"],
        "the site's security certificate has expired, so dm didn't connect",
    ),
    (
        &["certificate: notvalidyet"],
        "the site's security certificate isn't valid yet, so dm didn't connect",
    ),
    (
        &["certificate: notvalidforname"],
        "the site's security certificate is for another site, so dm didn't connect",
    ),
    (
        &["certificate: unknownissuer"],
        "the site's security certificate is from an issuer dm doesn't trust, so dm didn't connect",
    ),
    (
        &["certificate"],
        "the site's security certificate couldn't be checked, so dm didn't connect",
    ),
    (
        &["cannot be reached (was its drive"],
        "its folder's drive isn't connected",
    ),
    (
        &["insecure (http)"],
        "the secure link led to an insecure one, where the file could be tampered with on the way, so dm stopped",
    ),
    (
        &["copy-protected", "drm protected"],
        "this video is copy-protected (DRM), so only the site's own player can play it",
    ),
    (
        &["no space left", "os error 28", "disk full"],
        "the disk is full",
    ),
    (
        &["too large for this drive"],
        "the file is too big for this drive, which can't hold files over 4 GB (FAT32)",
    ),
    (
        &["disk: permission denied", "os error 13"],
        "dm isn't allowed to save in that folder",
    ),
    (
        &["the server answered 404"],
        "this link no longer works",
    ),
    (
        &["error decoding torrent", "this file is not a torrent"],
        "the file is not a torrent",
    ),
    (
        &["could not read the torrent file"],
        "the .torrent file couldn't be read",
    ),
    // Raw engine text shown for these.
    (
        &["the server answered 401"],
        "the server wants you to sign in",
    ),
    (
        &[
            "socks error",
            "proxy connect",
            "error connecting to proxy",
            "proxy server",
        ],
        "the proxy couldn't be reached",
    ),
    // A file's own errors, as `dm_core::file_error` words them.
    (&["it is not there"], "it is not there"),
    (
        &["dm is not allowed to use it"],
        "dm is not allowed to use it",
    ),
    (
        &["something of that name is already there"],
        "something of that name is already there",
    ),
    // The server's own trouble, and its refusals, said as what to do: the
    // row read "the server answered 500 Internal Server Error for http://…".
    (
        &[
            "the server answered 500",
            "the server answered 502",
            "the server answered 503",
            "the server answered 504",
        ],
        "the server had a problem on its end",
    ),
    (
        &["the server answered 429"],
        "the site is busy and turning downloads away",
    ),
    (
        &["the server answered 403"],
        "the site wouldn't give dm the file",
    ),
    (
        &["video helper", "video tools"],
        "dm couldn't get ready for videos",
    ),
    (
        &[
            "that is a web page",
            "dm found no video",
        ],
        "dm found no video on this page",
    ),
    (
        &["confirm you're not a bot", "sign in to confirm"],
        "the site wants to check you're not a bot",
    ),
    (
        &[
            "video unavailable",
            "private video",
            "this video is not available",
            "has been removed",
            "not available in your country",
        ],
        "this video isn't available",
    ),
    (
        &[
            "the player names no stream",
            "no stream at that quality",
            "the page names no player script",
            "the player API answered no JSON",
            "a stream with no address",
            "a cipher with no",
            "the signature was not solved",
            "the solver",
            "no JavaScript engine",
        ],
        VIDEO_NOT_GOT,
    ),
    (
        &["an encrypted stream dm cannot record"],
        "dm can't record this kind of encrypted stream",
    ),
    // Last: the specific failures above say more.
    (
        &["error sending request"],
        "the server couldn't be reached",
    ),
];

/// What a video page that failed is said to be when nothing more
/// specific is known (`REASONS`).
pub const VIDEO_NOT_GOT: &str = "dm couldn't get the video from this page";

/// The video reader's own words for why it could not get a page's video, for
/// the status to say after [`VIDEO_NOT_GOT`]: its first line, without any
/// address (one may be signed), and short enough for a row. `None` for a
/// failure that said nothing.
pub fn video_said(raw: &str) -> Option<String> {
    let said = raw.lines().map(str::trim).find(|l| !l.is_empty())?;
    let said: String = said
        .split_whitespace()
        .filter(|word| !word.contains("://"))
        .collect::<Vec<_>>()
        .join(" ");
    let mut chars = said.chars();
    let short: String = chars.by_ref().take(120).collect();
    let said = if chars.next().is_some() {
        format!("{}…", short.trim_end())
    } else {
        short
    };
    (!said.is_empty()).then_some(said)
}

/// Why a download is being tried again, in two or three words that fit
/// the Status column beside the attempt: the server's own trouble, its
/// turning dm away briefly, and the network failures `REASONS` names.
/// `None` for anything else, which is then not guessed at.
pub fn retry_cause(raw: &str) -> Option<&'static str> {
    let lower = raw.to_lowercase();
    if ["500", "502", "503", "504"]
        .iter()
        .any(|code| lower.starts_with(&format!("the server answered {code}")))
    {
        return Some("Server error");
    }
    if lower.starts_with("the server answered 429") {
        return Some("Too many requests");
    }
    match reason(raw)? {
        "the connection dropped before the file was complete" => Some("Connection dropped"),
        "the site couldn't be found" => Some("Server not found"),
        "the server refused the connection" => Some("Connection refused"),
        "the server took too long to answer" => Some("No answer"),
        "the server couldn't be reached" => Some("Server unreachable"),
        "the dial-up or VPN connection didn't come up" => Some("Redialling"),
        "the network downloads go only through is not connected" => Some("Waiting for the network"),
        _ => None,
    }
}

/// Every word [`retry_cause`] can give, for the catalogs.
pub const RETRY_CAUSES: [&str; 9] = [
    "Server error",
    "Too many requests",
    "Connection dropped",
    "Server not found",
    "Connection refused",
    "No answer",
    "Server unreachable",
    "Redialling",
    "Waiting for the network",
];

/// A failure's plain reason, in English, if it is one dm knows; see
/// `REASONS`.
///
/// Only the words of dm and of its network library decide, never an
/// address or path quoted in them or what a server sent back after its
/// status: a 404 for `birth-certificate.pdf` was shown as a certificate
/// that could not be checked, and a body saying "timed out" as a timeout.
pub fn reason(raw: &str) -> Option<&'static str> {
    let said = raw.split(" — ").next().unwrap_or(raw);
    let lower = said
        .split_whitespace()
        .filter(|word| !word.contains('/') && !word.contains('\\'))
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    REASONS
        .iter()
        .find(|(signs, _)| signs.iter().any(|sign| lower.contains(sign)))
        .map(|(_, plain)| *plain)
}

/// What a person can do about a failure, keyed by its plain words (`REASONS`): the row says
/// what happened in a few words, the details what to do, in one sentence or two (gaps row V4:
/// a failure was told in several shapes, what to do often in the row's own sentence). A
/// failure with nothing to do but wait or look elsewhere (the disk's own words, a video's
/// copy protection) has none here.
pub const NEXT_STEPS: [(&str, &str); 30] = [
    (
        "the disk is full",
        "Free some space, or choose a folder on another drive (Options › Folders), then click Retry",
    ),
    (
        "this link no longer works",
        "Open the page it came from in your browser and download from there, which gives dm a new link",
    ),
    (
        "the server refused the connection",
        "Check the address, or try again later. If a firewall or security program asks about dm, allow it, then click Retry",
    ),
    (
        "the site keeps redirecting in a loop, so the file can't be reached",
        "Open the page in your browser and download from there, which hands dm the real link",
    ),
    (
        "something of that name is already there",
        "Choose another name or folder with Move/Rename, or remove the other file",
    ),
    (
        "the site's security certificate couldn't be checked, so dm didn't connect",
        "Check that the computer's date and time are right, then click Retry",
    ),
    (
        "the site's security certificate has expired, so dm didn't connect",
        "The site must renew it. If many sites say this, check the computer's date and time",
    ),
    (
        "the site's security certificate isn't valid yet, so dm didn't connect",
        "Check that the computer's date and time are right, then click Retry",
    ),
    (
        "the site's security certificate is for another site, so dm didn't connect",
        "Check the address. On a public network someone may be in between: try another network",
    ),
    (
        "the site's security certificate is from an issuer dm doesn't trust, so dm didn't connect",
        "A network that inspects traffic (at work or school) may be in between: try another network",
    ),
    (
        "the dial-up or VPN connection didn't come up",
        "Choose another connection in Options › Advanced › Dial Up / VPN, or turn off Use Windows Dial Up / VPN Networking, then click Retry",
    ),
    (
        "this link has expired",
        "dm is waiting for the browser to hand over a new one for the same file",
    ),
    (
        "the network downloads go only through is not connected",
        "dm goes on when it is back",
    ),
    (
        "the connection dropped before the file was complete",
        "Try again",
    ),
    (
        "the server sent only part of the file",
        "Try again",
    ),
    (
        "the site couldn't be found",
        "Check the address, and that you're online",
    ),
    (
        "the server took too long to answer",
        "Try again in a moment",
    ),
    (
        "its folder's drive isn't connected",
        "Plug it back in, or choose another folder",
    ),
    (
        "the file is too big for this drive, which can't hold files over 4 GB (FAT32)",
        "Choose a folder on another drive",
    ),
    (
        "dm isn't allowed to save in that folder",
        "Choose another folder",
    ),
    (
        "the .torrent file couldn't be read",
        "It may be damaged",
    ),
    (
        "the server wants you to sign in",
        "Add the login in Properties › Login, or in Options › Sign-ins for the whole site",
    ),
    (
        "the proxy couldn't be reached",
        "Check its address in Options › Advanced",
    ),
    (
        "the server had a problem on its end",
        "Try again later",
    ),
    (
        "the site is busy and turning downloads away",
        "Try again in a few minutes",
    ),
    (
        "the site wouldn't give dm the file",
        "It may need you to sign in (Properties › Login)",
    ),
    (
        "dm couldn't get ready for videos",
        "Check that you're online, then click Retry",
    ),
    (
        "the site wants to check you're not a bot",
        "Play the video in your browser, then use the Download video button on it",
    ),
    (
        "this video isn't available",
        "It may be private, removed, or not shown in your country",
    ),
    (
        "the server couldn't be reached",
        "Check that you're online",
    ),
];

/// What the button beside Retry does for a failure, where its next step is one dm can take
/// (gaps row U10): its words name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepButton {
    /// The disk is full: the download moved to a folder chosen, as the stalled bar does.
    ChooseFolder,
    /// The link no longer works: Update download link, for the new one.
    UpdateLink,
    /// The site redirects in a loop: the page the download came from, opened in the browser.
    OpenPage,
    /// Something of that name is there: Move/Rename.
    MoveRename,
    /// The dial-up or VPN connection did not come up: Options, at that setting's tab.
    Options,
}

impl StepButton {
    /// Every one, for the catalogs' check.
    pub const ALL: [StepButton; 5] = [
        StepButton::ChooseFolder,
        StepButton::UpdateLink,
        StepButton::OpenPage,
        StepButton::MoveRename,
        StepButton::Options,
    ];

    /// Its words, in English (look them up with `t`), as the menus and bars name the same.
    pub fn words(self) -> &'static str {
        match self {
            StepButton::ChooseFolder => "Choose another folder…",
            StepButton::UpdateLink => "Update download link",
            StepButton::OpenPage => "Open the page",
            StepButton::MoveRename => "Move/Rename",
            StepButton::Options => "Options",
        }
    }
}

/// The button a failure's window offers beside Retry, where its next step ([`next_step`]) is
/// one dm can take; `None` where it is the person's alone (a firewall, the computer's clock).
pub fn step_button(raw: &str) -> Option<StepButton> {
    Some(match reason(raw)? {
        // Each failure whose step is another folder has the button that moves it there.
        "the disk is full"
        | "its folder's drive isn't connected"
        | "the file is too big for this drive, which can't hold files over 4 GB (FAT32)"
        | "dm isn't allowed to save in that folder" => StepButton::ChooseFolder,
        "this link no longer works" => StepButton::UpdateLink,
        "the site keeps redirecting in a loop, so the file can't be reached" => {
            StepButton::OpenPage
        }
        "something of that name is already there" => StepButton::MoveRename,
        "the dial-up or VPN connection didn't come up" => StepButton::Options,
        _ => return None,
    })
}

/// The next step for a failure, in English (look it up with `t`); `None`
/// where its plain words already say what to do, or are not known.
pub fn next_step(raw: &str) -> Option<&'static str> {
    let plain = reason(raw)?;
    NEXT_STEPS
        .iter()
        .find(|(said, _)| *said == plain)
        .map(|(_, step)| *step)
}
