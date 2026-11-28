// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! "The same file is already here" (contract row B73, gaps row W18): a download whose server
//! gave the name, size and entity tag of a finished download, or of a file in its folder, is
//! stopped before its body; asked here as "Already in the list" is, with "Remember my choice"
//! kept apart from that question's.

use super::*;

impl Downloads {
    /// Asks about row `id`, stopped as the same file as one already here: "Show it" or
    /// "Download again", or answers at once where the person ticked "Remember my choice".
    pub(super) fn ask_same_file(&mut self, id: edm_core::Id, cx: &mut Context<Self>) {
        use edm_desktop_base::i18n::t;
        use edm_shell::app::rules::{same_file_as, Dup, SameAs};
        let settings = self.backend.settings();
        let remembered = Dup::from_word(&settings.same_file_choice)
            .filter(|how| matches!(how, Dup::Show | Dup::Again));
        if let Some(how) = remembered {
            self.answer_same_file(id, how, cx);
            self.promote_asks(cx);
            return;
        }
        let name = self
            .rows
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.filename.clone())
            .unwrap_or_default();
        let listed = matches!(
            same_file_as(&self.rows, id, &settings.categories()),
            Some(SameAs::Row(_))
        );
        let ask = edm_desktop_base::asks::same_file(&name, listed);
        let asked = crate::dialogs::question::Asked {
            title: ask.title.into(),
            lines: ask.lines.into_iter().map(Into::into).collect(),
            named: None,
            detail: None,
            toggle: Some(t("Remember my choice").into()),
            ticked: false,
            buttons: ask.buttons.into_iter().map(Into::into).collect(),
            // Show it, which fetches nothing, is Return's and Escape's.
            default: 0,
            escape: 0,
            destroys: None,
        };
        let view = cx.entity().downgrade();
        let answer: crate::dialogs::question::Answer = Box::new(move |button, remember, cx| {
            let how = if button == 1 { Dup::Again } else { Dup::Show };
            let _ = view.update(cx, |view, cx| {
                view.questions.asking = false;
                if remember {
                    let mut wanted = view.backend.settings();
                    wanted.same_file_choice = how.word().to_string();
                    if let Err(e) = view.backend.set_settings(wanted) {
                        view.say(e);
                    }
                }
                view.answer_same_file(id, how, cx);
                view.read();
                view.promote_asks(cx);
                cx.notify();
            });
        });
        self.sheet(asked, None, None, answer, cx);
        self.questions.asking = true;
    }

    /// Carries out an answer about row `id`, asked or remembered: Download again lets it
    /// through and tries it again; Show it takes the row off the list (nothing of it was
    /// fetched) and chooses the finished download, or shows the file in its folder.
    fn answer_same_file(
        &mut self,
        id: edm_core::Id,
        how: edm_shell::app::rules::Dup,
        cx: &mut Context<Self>,
    ) {
        use edm_shell::app::rules::{same_file_as, Dup, SameAs};
        if how == Dup::Again {
            self.later_then_read(move |backend| backend.accept_same(id), cx);
            return;
        }
        let categories = self.backend.settings().categories();
        match same_file_as(&self.rows, id, &categories) {
            Some(SameAs::Row(other)) => {
                self.list.selected.clear();
                self.list.selected.insert(other);
            }
            Some(SameAs::File(path)) => self.reveal_path(&path.to_string_lossy()),
            None => {}
        }
        self.later_then_read(move |backend| backend.remove(id), cx);
        cx.notify();
    }
}
