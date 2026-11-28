// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The window's scale and the side panel's widths, as Options and View set them.

/// The window's scale (Options › General › Appearance's slider, View ›
/// Larger and Smaller): 60, 80, 100 and 120 percent, 100 the default (the
/// maintainer's word, docs/decisions.md 2026-11-11), and 150 and 200 for a
/// person who needs them (gaps row E12, the maintainer's word of 2026-11-24).
pub struct TextSizes {
    /// The window as it is designed.
    pub default: f32,
    /// The stops, smallest first.
    pub stops: [f32; 6],
}

/// The scales the window offers.
pub const TEXT_SIZE: TextSizes = TextSizes {
    default: 1.0,
    stops: [0.6, 0.8, 1.0, 1.2, 1.5, 2.0],
};

/// How wide the GPUI window's side panel may be, in points at 100% (gaps row U12).
pub struct PanelWidths {
    /// The narrowest its edge is dragged to: a place's picture, a short name and its count.
    pub least: f32,
    /// A new install's, and an older settings file's: the panel as it always was.
    pub default: f32,
    /// The widest its edge is dragged to, so the list keeps the most of the window.
    pub most: f32,
}

/// The side panel's widths: 200 points by default, dragged between 150 and 400.
pub const SIDEBAR_WIDTH: PanelWidths = PanelWidths {
    least: 150.0,
    default: 200.0,
    most: 400.0,
};

impl PanelWidths {
    /// `width` held between the least and the most, in whole points; one that is no number
    /// (a hand-edited file's) is the default.
    pub fn held(&self, width: f32) -> f32 {
        if !width.is_finite() {
            return self.default;
        }
        width.clamp(self.least, self.most).round()
    }
}

/// The named text sizes an older settings file may still say, and the stop
/// each is read as.
const TEXT_SIZE_NAMES: [(&str, f32); 3] = [("small", 0.8), ("medium", 1.0), ("large", 1.2)];

impl TextSizes {
    /// The stop nearest to `size`; one that is no number is the default.
    pub fn held(&self, size: f32) -> f32 {
        if !size.is_finite() {
            return self.default;
        }
        self.stops
            .iter()
            .copied()
            .min_by(|a, b| (a - size).abs().total_cmp(&(b - size).abs()))
            .unwrap_or(self.default)
    }

    /// The stop above (`up`) or below the one nearest to `size`, held at
    /// the ends.
    pub fn stepped(&self, size: f32, up: bool) -> f32 {
        let at = self.index(size);
        let next = if up {
            (at + 1).min(self.stops.len() - 1)
        } else {
            at.saturating_sub(1)
        };
        self.stops.get(next).copied().unwrap_or(self.default)
    }

    /// Which stop `size` is nearest to.
    pub fn index(&self, size: f32) -> usize {
        let held = self.held(size);
        self.stops
            .iter()
            .position(|s| (s - held).abs() < 0.001)
            .unwrap_or(2)
    }

    /// `size` as the percentage shown beside the slider.
    pub fn percent(&self, size: f32) -> u32 {
        (self.held(size) * 100.0).round() as u32
    }

    /// A scale as a settings file says it: a number (a factor, 0.85, or a
    /// percentage, 80), an older file's name for it (Small, Medium, Large),
    /// or a percentage in words ("80%").
    /// Anything else is `None`.
    pub fn read(&self, said: &serde_json::Value) -> Option<f32> {
        let number = |n: f64| {
            let n = n as f32;
            // 60 to 120 is a percentage; a factor is never that large.
            Some(self.held(if n > 10.0 { n / 100.0 } else { n }))
        };
        match said {
            serde_json::Value::Number(n) => n.as_f64().and_then(number),
            serde_json::Value::String(words) => {
                let words = words.trim().to_ascii_lowercase();
                if let Some((_, size)) = TEXT_SIZE_NAMES.iter().find(|(n, _)| *n == words) {
                    return Some(*size);
                }
                words
                    .trim_end_matches('%')
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .and_then(number)
            }
            _ => None,
        }
    }
}
