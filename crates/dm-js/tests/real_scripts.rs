// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! A real script read whole: `DM_JS_FILE=path/to/base.js cargo test -p dm-js --test
//! real_scripts -- --nocapture` (YouTube's player, a proxy configuration script). Not a file of
//! the repository: YouTube's player is YouTube's, and changes every few weeks.

#[test]
fn a_real_script_is_read_whole() {
    let Some(path) = std::env::var_os("DM_JS_FILE") else {
        eprintln!("DM_JS_FILE not set: not run");
        return;
    };
    let src = std::fs::read_to_string(&path).unwrap();
    let started = std::time::Instant::now();
    match dm_js::parser::parse(&src) {
        Ok(body) => eprintln!(
            "{} bytes read as {} statements in {:?}",
            src.len(),
            body.len(),
            started.elapsed()
        ),
        Err(e) => {
            let from = e.at.saturating_sub(120);
            let to = (e.at + 80).min(src.len());
            panic!("{e}\n…{}⟦here⟧{}…", &src[from..e.at], &src[e.at..to]);
        }
    }
}
