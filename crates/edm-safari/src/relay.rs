// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! One message of the Safari extension handed to EDM's own native host, and its answer brought
//! back: what Chrome and Firefox do by starting the host themselves, done for Safari, which
//! sends the message to this program instead (docs/how-it-works.md, Safari).
//!
//! No Objective-C here: the frames, where EDM is, and the host started, so all of it is tested
//! on every system.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// The argument that starts EDM as the browser's native host, for a program that is not a
/// browser (`edm_shell::shell::browser_launch`).
pub const HOST_FLAG: &str = "--native-host";

/// How long EDM's host is given for one answer: it may ask the server once and start EDM.
pub const WITHIN: Duration = Duration::from_secs(30);

/// The largest answer read back, as the host's own frames are held to.
const LARGEST: usize = 64 * 1024 * 1024;

/// One native messaging frame: the length of `json` in four bytes, least significant first,
/// then `json` itself, as the host reads them (`edm_shell::host::read_frame`).
pub fn frame(json: &[u8]) -> Vec<u8> {
    let mut out = (json.len() as u32).to_le_bytes().to_vec();
    out.extend_from_slice(json);
    out
}

/// The body of the first whole frame in `bytes`, or `None` where there is none: the host
/// answers nothing to a message it skips, and a frame cut short is no answer.
pub fn unframe(bytes: &[u8]) -> Option<&[u8]> {
    let header: [u8; 4] = bytes.get(..4)?.try_into().ok()?;
    let len = u32::from_le_bytes(header) as usize;
    if len > LARGEST {
        return None;
    }
    bytes.get(4..4 + len)
}

/// The app whose `Contents/PlugIns` holds the extension whose program is `program`:
/// `EDM.app/Contents/PlugIns/EDM Safari Extension.appex/Contents/MacOS/edm-safari` gives `EDM.app`.
/// `None` for a program that is not in an extension inside an app.
pub fn container_app(program: &Path) -> Option<PathBuf> {
    let macos = program.parent()?;
    let contents = macos.parent()?;
    let appex = contents.parent()?;
    let plugins = appex.parent()?;
    let app = plugins.parent()?.parent()?;
    let named = |dir: &Path, name: &str| dir.file_name().is_some_and(|n| n == name);
    let ends = |dir: &Path, ext: &str| dir.extension().is_some_and(|e| e == ext);
    (named(macos, "MacOS")
        && named(contents, "Contents")
        && ends(appex, "appex")
        && named(plugins, "PlugIns")
        && plugins.parent().is_some_and(|c| named(c, "Contents"))
        && ends(app, "app"))
    .then(|| app.to_path_buf())
}

/// Hands `request` (one message, as JSON) to EDM's host, `program --native-host`, and returns its
/// answer (JSON), `None` where it answered nothing, or why there is no answer. `home` is the
/// person's home folder, given to the host as `HOME`: inside Safari's sandbox `HOME` is the
/// extension's own container, where EDM keeps nothing. A host that has not answered within
/// `within` is stopped.
pub fn ask(
    program: &Path,
    home: Option<&Path>,
    request: &[u8],
    within: Duration,
) -> Result<Option<Vec<u8>>, String> {
    let mut command = Command::new(program);
    command
        .arg(HOST_FLAG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(home) = home {
        command.env("HOME", home);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("EDM could not be started ({}): {e}", program.display()))?;
    // A host that stopped before reading leaves its answer (or none) to be read all the same.
    if let Some(mut input) = child.stdin.take() {
        let _ = input.write_all(&frame(request));
    }
    let Some(mut output) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("EDM's answer could not be read".into());
    };
    let (tell, heard) = mpsc::channel();
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let read = output
            .by_ref()
            .take(LARGEST as u64 + 4)
            .read_to_end(&mut out)
            .map(|_| out);
        let _ = tell.send(read);
    });
    match heard.recv_timeout(within) {
        Ok(Ok(out)) => {
            let _ = child.wait();
            Ok(unframe(&out).map(<[u8]>::to_vec))
        }
        Ok(Err(e)) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(format!("EDM's answer could not be read: {e}"))
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(format!(
                "EDM did not answer within {} seconds",
                within.as_secs()
            ))
        }
    }
}

#[cfg(test)]
mod tests;
