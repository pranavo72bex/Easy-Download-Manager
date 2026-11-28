// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Download details' Networks row: what each network set answered last (gaps row K4).

use super::*;

impl Details {
    /// The Networks row's words: each network set in Options with what it answered last.
    pub(in crate::dialogs::details) fn networks_said(&self) -> Option<String> {
        let s = &self.settings;
        let names = [
            t("This computer’s connection").to_string(),
            s.network_name(&s.second_path),
            s.network_name(&s.third_path),
        ];
        let set = [true, !s.second_path.is_empty(), !s.third_path.is_empty()];
        answered_last(
            [&names[0], &names[1], &names[2]],
            set,
            [0, 1, 2].map(dm_core::answer_time),
        )
    }
}

/// Each network that is set, by `names`, with how long it took to answer last: none where only
/// the computer's own connection is set, or no network has answered yet (gaps row K4).
pub(super) fn answered_last(
    names: [&str; 3],
    set: [bool; 3],
    answers: [Option<std::time::Duration>; 3],
) -> Option<String> {
    if !set[1] && !set[2] {
        return None;
    }
    let parts: Vec<String> = (0..3)
        .filter(|&n| set[n])
        .filter_map(|n| {
            let took = answers[n]?;
            let ms = took.as_millis().max(1).to_string();
            Some(format!(
                "{}: {}",
                names[n],
                dm_desktop_base::i18n::tf("answers in {} ms", &[&ms])
            ))
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join("   "))
}

#[cfg(test)]
mod network_tests {
    use super::answered_last;
    use std::time::Duration;

    /// Download details says what each network set answered last, before the download runs,
    /// and nothing where one network is all there is or none has answered (gaps row K4).
    #[test]
    fn each_network_set_is_said_with_its_last_answer() {
        let names = ["This computer’s connection", "Phone", "Office"];
        let ms = |n| Some(Duration::from_millis(n));
        assert_eq!(
            answered_last(names, [true, false, false], [ms(20), None, None]),
            None,
            "one network: nothing to compare"
        );
        assert_eq!(
            answered_last(names, [true, true, false], [None, None, None]),
            None,
            "none has answered yet"
        );
        assert_eq!(
            answered_last(names, [true, true, false], [ms(20), ms(85), ms(5)]).as_deref(),
            Some("This computer’s connection: answers in 20 ms   Phone: answers in 85 ms"),
            "a network not set is not said, though it answered once"
        );
        assert_eq!(
            answered_last(names, [true, true, true], [ms(20), None, ms(0)]).as_deref(),
            Some("This computer’s connection: answers in 20 ms   Office: answers in 1 ms"),
        );
    }
}
