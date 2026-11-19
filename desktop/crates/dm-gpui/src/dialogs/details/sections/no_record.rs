// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Download details' "Keep no record" (gaps row W17, contract row B69): ticked, the row goes
//! once the file is whole, with its address, page and sign-in, and the file stays; ticked by
//! itself for a download the browser handed over from a private window.

use super::*;

impl Details {
    /// The "Keep no record" check box, under Size, Note and Delete file after.
    pub(in crate::dialogs::details) fn no_record_row(
        &self,
        look: &Look,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        look.under(
            native::checkbox(
                "keep-no-record",
                t("Keep no record").into(),
                self.keep_no_record,
                look.theme,
                &look.screen,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.keep_no_record = !this.keep_no_record;
                cx.notify();
            }))
            .into_any_element(),
        )
    }

    /// A row begun while the dialog asked keeps no record as the box is ticked when it is
    /// answered: one already whole goes now.
    pub(in crate::dialogs::details) fn early_keeps_no_record(&self) {
        if let Some(id) = self.early {
            self.backend.set_keep_no_record(id, self.keep_no_record);
        }
    }
}
