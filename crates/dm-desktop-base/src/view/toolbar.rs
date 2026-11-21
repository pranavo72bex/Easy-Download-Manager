// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! How the toolbar's buttons are drawn (View › Customize toolbar), the same
//! for every window: a picture with its word, the picture alone, or the
//! word alone, kept in the settings as `toolbar_style`.

/// The toolbar styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolStyle {
    /// Icons with their words.
    #[default]
    Both,
    /// Icons only.
    Icons,
    /// Words only.
    Text,
}

impl ToolStyle {
    /// The style called `name`; the default for any other word.
    #[must_use]
    pub fn named(name: &str) -> ToolStyle {
        match name {
            "icons" => ToolStyle::Icons,
            "text" => ToolStyle::Text,
            _ => ToolStyle::Both,
        }
    }

    /// The word the style is saved under.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            ToolStyle::Both => "both",
            ToolStyle::Icons => "icons",
            ToolStyle::Text => "text",
        }
    }
}

/// Each style and its words in View › Customize toolbar, in English.
pub const TOOL_STYLES: [(ToolStyle, &str); 3] = [
    (ToolStyle::Both, "Icons and text"),
    (ToolStyle::Icons, "Small buttons"),
    (ToolStyle::Text, "Text only"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_style_is_saved_by_its_name_and_read_back() {
        for (style, _) in TOOL_STYLES {
            assert_eq!(ToolStyle::named(style.name()), style);
        }
        assert_eq!(ToolStyle::named("anything else"), ToolStyle::Both);
        assert_eq!(ToolStyle::default(), ToolStyle::Both);
    }
}
