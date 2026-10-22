//! The computer's regional formats: the decimal mark, the order a date is
//! written in, and a 12- or 24-hour clock. They are the person's choice in
//! the system's settings, apart from the language dm speaks: someone who
//! reads dm in English may write "3,8" and "05.03.2026". Where the system
//! says nothing, the language's own conventions stand, as before.

use std::sync::OnceLock;

/// The order of a date's parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    /// 5 Mar 2026.
    DayMonthYear,
    /// Mar 5, 2026.
    MonthDayYear,
    /// 2026 Mar 5.
    YearMonthDay,
}

/// How numbers and dates are written where this computer is set up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub decimal: char,
    pub order: Order,
    pub hour12: bool,
}

/// The computer's, read once; `None` where it does not say. A region
/// pinned with [`pin`] stands in for it.
pub fn system() -> Option<Region> {
    if let Some(pinned) = *dm_core::lock(&PINNED) {
        return pinned;
    }
    static FOUND: OnceLock<Option<Region>> = OnceLock::new();
    *FOUND.get_or_init(ask)
}

static PINNED: std::sync::Mutex<Option<Option<Region>>> = std::sync::Mutex::new(None);

/// Fixes the region whatever the computer says: for tests, which must read
/// the same on every machine (`None`: the language's own conventions).
pub fn pin(region: Option<Region>) {
    *dm_core::lock(&PINNED) = Some(region);
}

#[cfg(windows)]
fn ask() -> Option<Region> {
    // The person's own choices in Region › Additional settings, which
    // Windows keeps whatever the region's defaults are.
    let key = r"Control Panel\International";
    let read = |name: &str| crate::autostart::registry::string(key, name);
    let decimal = read("sDecimal")?.chars().next()?;
    let order = match read("iDate").as_deref() {
        Some("0") => Order::MonthDayYear,
        Some("2") => Order::YearMonthDay,
        _ => Order::DayMonthYear,
    };
    let hour12 = read("iTime").as_deref() == Some("0");
    Some(Region {
        decimal,
        order,
        hour12,
    })
}

#[cfg(target_os = "macos")]
fn ask() -> Option<Region> {
    let read = |name: &str| {
        let out = std::process::Command::new("defaults")
            .args(["read", "-g", name])
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let mut region = from_code(&read("AppleLocale")?)?;
    if read("AppleICUForce24HourTime").as_deref() == Some("1") {
        region.hour12 = false;
    }
    if read("AppleICUForce12HourTime").as_deref() == Some("1") {
        region.hour12 = true;
    }
    Some(region)
}

#[cfg(not(any(windows, target_os = "macos")))]
fn ask() -> Option<Region> {
    // As the C library reads them: LC_ALL over the category, over LANG.
    let var = |names: &[&str]| {
        names
            .iter()
            .filter_map(|n| std::env::var(n).ok())
            .find(|v| !v.is_empty() && v != "C" && v != "POSIX")
    };
    let numbers = from_code(&var(&["LC_ALL", "LC_NUMERIC", "LANG"])?)?;
    let dates = var(&["LC_ALL", "LC_TIME", "LANG"])
        .and_then(|code| from_code(&code))
        .unwrap_or(numbers);
    Some(Region {
        decimal: numbers.decimal,
        order: dates.order,
        hour12: dates.hour12,
    })
}

/// A locale's conventions from its code (`de_DE.UTF-8`, `en-US`,
/// `en_NP@rg=…`): its region where it names one, else its language.
pub fn from_code(code: &str) -> Option<Region> {
    let base = code.split(['.', '@']).next()?.trim();
    let mut parts = base.split(['_', '-']);
    let language = parts.next()?.to_ascii_lowercase();
    if language.is_empty() {
        return None;
    }
    let country = parts
        .find(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_alphabetic()))
        .map(|p| p.to_ascii_uppercase());
    let place = country.as_deref().unwrap_or("");
    let comma = if place.is_empty() {
        COMMA_LANGUAGES.contains(&language.as_str())
    } else {
        COMMA_REGIONS.contains(&place)
    };
    let order = if MONTH_FIRST.contains(&place) || (place.is_empty() && language == "en") {
        Order::MonthDayYear
    } else if YEAR_FIRST.contains(&place)
        || (place.is_empty() && ["zh", "ja", "ko", "hu", "lt"].contains(&language.as_str()))
    {
        Order::YearMonthDay
    } else {
        Order::DayMonthYear
    };
    let hour12 = TWELVE_HOUR.contains(&place) || (place.is_empty() && language == "en");
    Some(Region {
        decimal: if comma { ',' } else { '.' },
        order,
        hour12,
    })
}

/// Regions that write a decimal comma.
const COMMA_REGIONS: [&str; 62] = [
    "AD", "AL", "AM", "AO", "AR", "AT", "AZ", "BA", "BE", "BG", "BO", "BR", "BY", "CL", "CM", "CO",
    "CR", "CU", "CY", "CZ", "DE", "DK", "DZ", "EC", "EE", "ES", "FI", "FO", "FR", "GE", "GL", "GR",
    "HR", "HU", "ID", "IS", "IT", "KZ", "LT", "LU", "LV", "MA", "MD", "ME", "MK", "MZ", "NL", "NO",
    "PL", "PT", "PY", "RO", "RS", "RU", "SE", "SI", "SK", "TN", "TR", "UA", "UY", "VN",
];

/// Languages that write one, where no region is named.
const COMMA_LANGUAGES: [&str; 26] = [
    "de", "fr", "es", "it", "nl", "pt", "ru", "pl", "tr", "id", "cs", "sk", "sl", "hr", "sr", "bg",
    "ro", "el", "uk", "be", "da", "nb", "sv", "fi", "hu", "vi",
];

/// Regions that write the month first.
const MONTH_FIRST: [&str; 4] = ["US", "PH", "FM", "BZ"];

/// Regions that write the year first.
const YEAR_FIRST: [&str; 9] = ["CN", "JP", "KR", "TW", "HU", "LT", "MN", "SE", "IR"];

/// Regions whose clock runs to 12 by habit.
const TWELVE_HOUR: [&str; 12] = [
    "US", "CA", "AU", "NZ", "PH", "IN", "PK", "EG", "SA", "MY", "BD", "NP",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_region_sets_the_mark_the_order_and_the_clock() {
        let germany = from_code("de_DE.UTF-8").unwrap();
        assert_eq!(
            (germany.decimal, germany.order, germany.hour12),
            (',', Order::DayMonthYear, false)
        );
        let us = from_code("en-US").unwrap();
        assert_eq!(
            (us.decimal, us.order, us.hour12),
            ('.', Order::MonthDayYear, true)
        );
        // English read in Germany writes as Germany does.
        assert_eq!(from_code("en_DE").unwrap().decimal, ',');
        assert_eq!(from_code("ja_JP").unwrap().order, Order::YearMonthDay);
        // macOS's region-qualified form, as this Mac's "en_NP" is.
        assert_eq!(from_code("en_NP@rg=npzzzz").unwrap().decimal, '.');
        // A language alone: its own habit.
        assert_eq!(from_code("fr").unwrap().decimal, ',');
        assert_eq!(from_code(""), None);
    }
}
