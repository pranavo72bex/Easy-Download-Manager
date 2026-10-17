//! A cap on how much is downloaded in a period: "Download limits", for a link with a fair-use allowance — no more than so many
//! megabytes every so many hours.
//!
//! Counted by the queue from every row's progress, so a torrent and a
//! yt-dlp run count as much as dm's own transfers. When the allowance is
//! spent the queue holds its downloads, as the scheduler does outside its
//! window, and puts them back when the period rolls over.

use std::time::{Duration, SystemTime};

/// How much may be downloaded, and over how long.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Allowance {
    /// Bytes per period; zero is no cap.
    pub bytes: u64,
    pub period: Duration,
}

/// What has been used of the allowance in the current period.
#[derive(Debug, Default)]
pub struct Meter {
    allowance: Allowance,
    /// When the current period began: at the first byte counted after the
    /// last one ended. By the wall clock, which goes on while the computer
    /// sleeps — a monotonic clock does not, and a day's allowance spent at
    /// night came back two or three days later on a laptop that slept.
    began: Option<SystemTime>,
    used: u64,
}

impl Meter {
    pub fn set(&mut self, allowance: Allowance) {
        self.allowance = allowance;
    }

    pub fn allowance(&self) -> Allowance {
        self.allowance
    }

    fn roll(&mut self, now: SystemTime) {
        if let Some(began) = self.began {
            if now.duration_since(began).unwrap_or_default() >= self.allowance.period {
                self.began = None;
                self.used = 0;
            }
        }
    }

    /// Counts bytes that arrived.
    pub fn add(&mut self, bytes: u64, now: SystemTime) {
        if self.allowance.bytes == 0 || bytes == 0 {
            return;
        }
        self.roll(now);
        self.began.get_or_insert(now);
        self.used = self.used.saturating_add(bytes);
    }

    /// How long until downloading may go on, if the allowance is spent.
    pub fn spent(&mut self, now: SystemTime) -> Option<Duration> {
        if self.allowance.bytes == 0 {
            return None;
        }
        self.roll(now);
        let began = self.began?;
        (self.used >= self.allowance.bytes).then(|| {
            (began + self.allowance.period)
                .duration_since(now)
                .unwrap_or_default()
        })
    }

    pub fn used(&self) -> u64 {
        self.used
    }

    /// What is kept across a restart, as text: when the period began, in
    /// milliseconds since 1970 (empty for none), and the bytes used.
    pub fn kept(&self) -> String {
        let began = self
            .began
            .and_then(|b| b.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map_or(String::new(), |d| d.as_millis().to_string());
        format!("{began} {}", self.used)
    }

    /// Takes back what `kept` wrote; anything else is no use and left.
    pub fn restore(&mut self, text: &str) {
        let mut words = text.split_whitespace();
        let (Some(first), second) = (words.next(), words.next()) else {
            return;
        };
        let (began, used) = match second {
            Some(used) => (first.parse::<u64>().ok(), used.parse::<u64>().ok()),
            None => (None, first.parse::<u64>().ok()),
        };
        if let Some(used) = used {
            self.used = used;
            self.began = began.map(|ms| SystemTime::UNIX_EPOCH + Duration::from_millis(ms));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_allowance_is_spent_and_comes_back_after_its_period() {
        let start = SystemTime::now();
        let mut meter = Meter::default();
        meter.set(Allowance {
            bytes: 1000,
            period: Duration::from_secs(3600),
        });
        assert_eq!(meter.spent(start), None, "nothing used yet");
        meter.add(600, start);
        assert_eq!(meter.spent(start), None);
        meter.add(400, start + Duration::from_secs(60));
        assert_eq!(
            meter.spent(start + Duration::from_secs(60)),
            Some(Duration::from_secs(3540)),
            "spent, until an hour after the first byte"
        );
        let later = start + Duration::from_secs(3600);
        assert_eq!(meter.spent(later), None, "a new period");
        assert_eq!(meter.used(), 0);
    }

    #[test]
    fn what_is_used_survives_a_restart() {
        let start = SystemTime::now();
        let allowance = Allowance {
            bytes: 1000,
            period: Duration::from_secs(3600),
        };
        let mut meter = Meter::default();
        meter.set(allowance);
        meter.add(1000, start);
        let mut again = Meter::default();
        again.set(allowance);
        again.restore(&meter.kept());
        assert!(
            again.spent(start).is_some(),
            "the restart forgot the allowance was spent"
        );
        assert_eq!(
            again.spent(start + Duration::from_secs(3600)),
            None,
            "and its period still ends"
        );
        again.restore("rubbish");
        assert_eq!(again.used(), 0, "rolled over");
    }

    #[test]
    fn no_allowance_is_no_cap() {
        let mut meter = Meter::default();
        let now = SystemTime::now();
        meter.add(u64::MAX, now);
        assert_eq!(meter.spent(now), None);
    }
}
