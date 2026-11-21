// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

use super::*;

#[test]
fn a_frame_is_its_length_then_its_body_and_reads_back() {
    let framed = frame(br#"{"type":"engine.hello"}"#);
    assert_eq!(&framed[..4], &23u32.to_le_bytes());
    assert_eq!(unframe(&framed), Some(&br#"{"type":"engine.hello"}"#[..]));
}

#[test]
fn no_frame_or_a_cut_one_is_no_answer() {
    assert_eq!(unframe(b""), None);
    assert_eq!(unframe(&[5, 0]), None);
    assert_eq!(unframe(&[5, 0, 0, 0, b'{']), None);
    // A length no host sends: the stream has lost its place.
    assert_eq!(unframe(&u32::MAX.to_le_bytes()), None);
    // Only the first frame is the answer.
    let mut two = frame(b"1");
    two.extend(frame(b"2"));
    assert_eq!(unframe(&two), Some(&b"1"[..]));
}

#[test]
fn the_container_is_the_app_whose_plugins_hold_the_extension() {
    let program = Path::new(
        "/Applications/dm.app/Contents/PlugIns/dm Safari Extension.appex/Contents/MacOS/dm-safari",
    );
    assert_eq!(
        container_app(program),
        Some(PathBuf::from("/Applications/dm.app"))
    );
    for elsewhere in [
        "/usr/local/bin/dm-safari",
        "/Applications/dm.app/Contents/MacOS/dm-safari",
        "/tmp/x.appex/Contents/MacOS/dm-safari",
        "/Applications/dm/Contents/PlugIns/x.appex/Contents/MacOS/dm-safari",
        "/Applications/dm.app/Contents/Frameworks/x.appex/Contents/MacOS/dm-safari",
    ] {
        assert_eq!(container_app(Path::new(elsewhere)), None, "{elsewhere}");
    }
}

/// A host written as a shell script: what it is started with, and what it reads, are what it
/// writes back.
#[cfg(unix)]
mod host {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(body: &str) -> (PathBuf, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("dm-safari-{}-{}", std::process::id(), body.len()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("host");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        (dir, path)
    }

    #[test]
    fn the_host_is_started_as_the_native_host_with_the_home_given_and_its_answer_returned() {
        // Answers with a frame of its own: `$1` and `$HOME` as JSON, after reading the request.
        let (dir, host) = script(
            r#"cat > "$(dirname "$0")/request"
body="{\"flag\":\"$1\",\"home\":\"$HOME\"}"
printf "$(printf '\\%03o' ${#body})\000\000\000%s" "$body""#,
        );
        let answer = ask(
            &host,
            Some(Path::new("/Users/someone")),
            br#"{"type":"settings.get"}"#,
            Duration::from_secs(10),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            String::from_utf8(answer).unwrap(),
            r#"{"flag":"--native-host","home":"/Users/someone"}"#
        );
        let request = std::fs::read(dir.join("request")).unwrap();
        assert_eq!(request, frame(br#"{"type":"settings.get"}"#));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_host_that_answers_nothing_is_no_answer_and_one_that_hangs_is_stopped() {
        let (dir, silent) = script("cat > /dev/null");
        assert_eq!(ask(&silent, None, b"{}", Duration::from_secs(10)), Ok(None));
        let _ = std::fs::remove_dir_all(dir);

        let (dir, hangs) = script("exec sleep 30");
        let started = std::time::Instant::now();
        let said = ask(&hangs, None, b"{}", Duration::from_millis(300)).unwrap_err();
        assert!(said.contains("did not answer"), "{said}");
        assert!(started.elapsed() < Duration::from_secs(10));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_host_that_cannot_be_started_says_so() {
        let said = ask(
            Path::new("/nonexistent/dm"),
            None,
            b"{}",
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert!(said.contains("could not be started"), "{said}");
    }
}
