// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! What EDM has done, in figures (gaps row W15): View › Statistics's bytes by day, by site and
//! by category, worked out from the finished rows of the list EDM already keeps, and the average
//! and best speed seen while EDM is open ([`SpeedsSeen`]); and a progress window's speed line
//! over the last minutes ([`SpeedLine`]). Nothing here is written anywhere: the speeds are held
//! in memory and forgotten when EDM closes.

use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use edm_core::clock::{Date, Zone};
use edm_core::{Category, Entry, State};

use crate::format;
use crate::i18n::{t, tf};

/// How many days "By day" goes back, today among them.
pub const DAYS: usize = 30;
/// How many sites "By site" names, the largest first.
pub const SITES: usize = 10;

/// A share of the bytes: a site's or a category's, by its name as shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Share {
    /// The site's host, or the category's name in the language in force.
    pub name: String,
    /// The bytes of its finished downloads.
    pub bytes: u64,
}

/// One day of "By day".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Day {
    /// The day, on this computer's clock.
    pub date: Date,
    /// The bytes of the downloads that finished that day.
    pub bytes: u64,
}

/// View › Statistics, worked out from the list.
#[derive(Debug, Clone, PartialEq)]
pub struct Statistics {
    /// How many downloads finished, of those in the list.
    pub files: usize,
    /// Their bytes, all together.
    pub bytes: u64,
    /// The last [`DAYS`] days, the oldest first, a day with nothing at 0.
    pub days: Vec<Day>,
    /// The [`SITES`] sites the most bytes came from, the most first.
    pub sites: Vec<Share>,
    /// Each category something finished in, the most first.
    pub categories: Vec<Share>,
    /// The average speed while something downloaded, since EDM opened; `None` before any.
    pub average_speed: Option<f64>,
    /// The fastest the downloads went together, since EDM opened.
    pub best_speed: Option<f64>,
}

impl Statistics {
    /// The figures of `rows` as of `today`, each finish placed on a day by `zone` (this
    /// computer's offset from UTC), with the speeds `speeds` has seen.
    pub fn of<'a>(
        rows: impl IntoIterator<Item = &'a Entry>,
        today: Date,
        zone: Zone,
        speeds: &SpeedsSeen,
    ) -> Statistics {
        let mut days = vec![0u64; DAYS];
        let mut sites: HashMap<String, u64> = HashMap::new();
        let mut kinds: HashMap<Category, u64> = HashMap::new();
        let (mut files, mut bytes) = (0usize, 0u64);
        for row in rows.into_iter().filter(|r| r.state == State::Completed) {
            let size = size_of(row);
            files += 1;
            bytes = bytes.saturating_add(size);
            let finished = row
                .finished_at
                .and_then(|s| zone.at(s as i64))
                .map(|at| at.date);
            let ago = finished.map(|day| today.days_since_epoch() - day.days_since_epoch());
            if let Some(ago) = ago {
                if (0..DAYS as i64).contains(&ago) {
                    let at = DAYS - 1 - ago as usize;
                    days[at] = days[at].saturating_add(size);
                }
            }
            let site = sites
                .entry(edm_shell::app::names::host(&row.url))
                .or_default();
            *site = site.saturating_add(size);
            let kind = edm_shell::app::names::kind_of(row).unwrap_or(Category::Other);
            let kind = kinds.entry(kind).or_default();
            *kind = kind.saturating_add(size);
        }
        let days = days
            .into_iter()
            .enumerate()
            .map(|(n, bytes)| Day {
                date: today.add_days(-((DAYS - 1 - n) as i64)).unwrap_or(today),
                bytes,
            })
            .collect();
        let mut sites = largest_first(sites.into_iter().map(|(host, bytes)| Share {
            name: if host.is_empty() {
                t("no site").to_string()
            } else {
                host
            },
            bytes,
        }));
        sites.truncate(SITES);
        let categories = largest_first(kinds.into_iter().map(|(kind, bytes)| Share {
            name: kind_name(kind),
            bytes,
        }));
        Statistics {
            files,
            bytes,
            days,
            sites,
            categories,
            average_speed: speeds.average(),
            best_speed: speeds.best(),
        }
    }

    /// The figures of `rows` today, on this computer's clock.
    pub fn now<'a>(rows: impl IntoIterator<Item = &'a Entry>, speeds: &SpeedsSeen) -> Statistics {
        let today = edm_core::clock::local_now().date;
        Statistics::of(rows, today, Zone::Local, speeds)
    }

    /// The most bytes any day of [`Statistics::days`] has, for the bars to be drawn against.
    pub fn busiest_day(&self) -> u64 {
        self.days.iter().map(|d| d.bytes).max().unwrap_or(0)
    }
}

/// A finished row's size: what came, or the size the server said where nothing is counted.
fn size_of(row: &Entry) -> u64 {
    let p = &row.progress;
    if p.downloaded > 0 {
        p.downloaded
    } else {
        p.total.unwrap_or(0)
    }
}

/// `shares`, the most bytes first, then by name.
fn largest_first(shares: impl Iterator<Item = Share>) -> Vec<Share> {
    let mut shares: Vec<Share> = shares.collect();
    shares.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
    shares
}

/// A category's name as the side panel shows it, in the language in force.
pub fn kind_name(kind: Category) -> String {
    let word = edm_shell::app::names::category_word(kind);
    crate::i18n::name(&format::titled(&word))
}

/// A day of "By day" in words: "19 Nov", the month in the language in force.
pub fn day_words(date: Date) -> String {
    let month = format::MONTHS
        .get(date.month0() as usize)
        .copied()
        .map(t)
        .unwrap_or_default();
    tf("{} {}", &[&date.day(), &month])
}

/// How far a bar of `bytes` reaches against the largest, `top`: from 0 to 1.
pub fn fraction(bytes: u64, top: u64) -> f32 {
    if top == 0 {
        0.0
    } else {
        (bytes as f64 / top as f64).clamp(0.0, 1.0) as f32
    }
}

/// The speeds the downloads went at together while EDM is open, for Statistics' average and
/// best: held in memory, never written, and gone when EDM closes.
#[derive(Debug, Default, Clone)]
pub struct SpeedsSeen {
    at: Option<Instant>,
    /// Seconds something downloaded, and the bytes a second times those seconds.
    seconds: f64,
    bytes: f64,
    best: f64,
}

impl SpeedsSeen {
    /// The longest a gap between two samples counts: a computer asleep is not a download.
    const LONGEST: f64 = 5.0;

    /// Takes the downloads' speed together, bytes a second, at `now`: the time since the
    /// last sample counts at it where it is above nothing.
    pub fn sample(&mut self, now: Instant, speed: f64) {
        let speed = if speed.is_finite() {
            speed.max(0.0)
        } else {
            0.0
        };
        if let Some(at) = self.at {
            let seconds = now
                .saturating_duration_since(at)
                .as_secs_f64()
                .min(Self::LONGEST);
            if speed > 0.0 {
                self.seconds += seconds;
                self.bytes += speed * seconds;
            }
        }
        self.at = Some(now);
        self.best = self.best.max(speed);
    }

    /// The average speed while something downloaded; `None` before anything did.
    pub fn average(&self) -> Option<f64> {
        if self.seconds > 0.0 {
            Some(self.bytes / self.seconds)
        } else {
            None
        }
    }

    /// The fastest seen; `None` before anything downloaded.
    pub fn best(&self) -> Option<f64> {
        (self.best > 0.0).then_some(self.best)
    }
}

/// A progress window's speed line: the download's speed once a second over the last
/// [`SpeedLine::SECONDS`], the oldest first. Held by the window while it is open, and nowhere
/// else.
#[derive(Debug, Default, Clone)]
pub struct SpeedLine {
    samples: VecDeque<f64>,
    at: Option<Instant>,
}

impl SpeedLine {
    /// How far back the line goes: five minutes.
    pub const SECONDS: usize = 300;

    /// Takes the download's speed at `now`. Within a second of the last it stands for that
    /// second; each whole second missed since (no frame was drawn, as nothing changed) is
    /// nothing.
    pub fn sample(&mut self, now: Instant, speed: f64) {
        let speed = if speed.is_finite() {
            speed.max(0.0)
        } else {
            0.0
        };
        let Some(at) = self.at else {
            self.samples.push_back(speed);
            self.at = Some(now);
            return;
        };
        let gone = now.saturating_duration_since(at).as_secs() as usize;
        if gone == 0 {
            if let Some(last) = self.samples.back_mut() {
                *last = speed;
            }
            return;
        }
        let missed = (gone - 1).min(Self::SECONDS);
        self.samples.extend(std::iter::repeat_n(0.0, missed));
        self.samples.push_back(speed);
        while self.samples.len() > Self::SECONDS {
            self.samples.pop_front();
        }
        self.at = Some(at + std::time::Duration::from_secs(gone as u64));
    }

    /// How many seconds it holds.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether it holds none yet.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// The fastest second it holds, bytes a second.
    pub fn best(&self) -> f64 {
        self.samples.iter().copied().fold(0.0, f64::max)
    }

    /// The line as `bars` bars, the oldest first, each the average of its seconds and as tall
    /// (0 to 1) as it is against the tallest; the seconds before the first sample are nothing.
    pub fn bars(&self, bars: usize) -> Vec<f32> {
        let bars = bars.max(1);
        let each = (Self::SECONDS / bars).max(1);
        let wanted = each * bars;
        let held = self.samples.len().min(wanted);
        let seconds: Vec<f64> = std::iter::repeat_n(0.0, wanted - held)
            .chain(self.samples.iter().skip(self.samples.len() - held).copied())
            .collect();
        let means: Vec<f64> = seconds
            .chunks(each)
            .map(|c| c.iter().sum::<f64>() / c.len() as f64)
            .collect();
        let top = means.iter().copied().fold(0.0, f64::max);
        means
            .into_iter()
            .map(|m| if top > 0.0 { (m / top) as f32 } else { 0.0 })
            .collect()
    }
}

#[cfg(test)]
mod tests;
