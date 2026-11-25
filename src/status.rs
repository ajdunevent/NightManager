//! The one-line status shown in the window and through the Background portal.

use chrono::{DateTime, Datelike, Local, NaiveDate};
use gio::prelude::*;

use crate::keys::INTERFACE_SCHEMA;

pub const DAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub active: bool,
    /// Unix time of the next boundary, 0 for none.
    pub next_change: i64,
    pub next_is_start: bool,
    /// The next boundary is Night Light's sunset or sunrise.
    pub next_is_solar: bool,
}

impl Status {
    /// "until sunrise (07:12)", "starts at 21:30 tomorrow" and the like.
    pub fn detail(&self, today: NaiveDate, clock_24h: bool) -> String {
        let at = (self.next_change != 0)
            .then(|| DateTime::from_timestamp(self.next_change, 0))
            .flatten()
            .map(|at| at.with_timezone(&Local));
        let Some(at) = at else {
            return if self.active {
                "until turned off".into()
            } else {
                "no scheduled start".into()
            };
        };
        let day = match (at.date_naive() - today).num_days() {
            0 => String::new(),
            1 => " tomorrow".into(),
            _ => format!(" on {}", DAYS[at.weekday().num_days_from_monday() as usize]),
        };
        let time = if clock_24h {
            at.format("%H:%M").to_string()
        } else {
            at.format("%-I:%M %p").to_string()
        };
        let when = if self.next_is_solar {
            let sun = if self.next_is_start { "sunset" } else { "sunrise" };
            format!("{sun}{day} ({time})")
        } else {
            format!("{time}{day}")
        };
        match (self.active, self.next_is_start) {
            (true, false) => format!("until {when}"),
            (false, true) => format!("starts at {when}"),
            (true, true) => format!("next start at {when}"),
            (false, false) => format!("next end at {when}"),
        }
    }

    /// The portal's status line.
    pub fn message(&self, today: NaiveDate, clock_24h: bool) -> String {
        let state = if self.active { "on" } else { "off" };
        format!("Night Mode {state} · {}", self.detail(today, clock_24h))
    }

    /// Whether the text names a day, which goes stale at midnight.
    pub fn names_day(&self, today: NaiveDate) -> bool {
        self.next_change != 0
            && DateTime::from_timestamp(self.next_change, 0)
                .is_some_and(|at| at.with_timezone(&Local).date_naive() != today)
    }
}

/// The host's clock-format setting, if its schema is installed.
pub fn clock_settings() -> Option<gio::Settings> {
    let schema = gio::SettingsSchemaSource::default()?.lookup(INTERFACE_SCHEMA, true)?;
    schema
        .has_key("clock-format")
        .then(|| gio::Settings::new(INTERFACE_SCHEMA))
}

pub fn clock_24h(settings: Option<&gio::Settings>) -> bool {
    settings.is_none_or(|s| s.string("clock-format") != "12h")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(d: u32, h: u32, m: u32) -> i64 {
        Local.with_ymd_and_hms(2026, 11, d, h, m, 0).unwrap().timestamp()
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 25).unwrap()
    }

    fn status(active: bool, next_change: i64, next_is_start: bool, next_is_solar: bool) -> Status {
        Status {
            active,
            next_change,
            next_is_start,
            next_is_solar,
        }
    }

    #[test]
    fn plain_times() {
        let s = status(false, at(25, 21, 30), true, false);
        assert_eq!(s.message(today(), true), "Night Mode off · starts at 21:30");
        assert_eq!(s.detail(today(), false), "starts at 9:30 PM");
        let s = status(true, at(26, 7, 5), false, false);
        assert_eq!(s.message(today(), true), "Night Mode on · until 07:05 tomorrow");
        assert_eq!(s.detail(today(), false), "until 7:05 AM tomorrow");
    }

    #[test]
    fn solar_times() {
        let s = status(false, at(25, 17, 8), true, true);
        assert_eq!(s.detail(today(), true), "starts at sunset (17:08)");
        let s = status(true, at(26, 7, 12), false, true);
        assert_eq!(s.detail(today(), true), "until sunrise tomorrow (07:12)");
    }

    #[test]
    fn later_days_and_mismatches() {
        let s = status(false, at(27, 21, 0), true, false);
        assert_eq!(s.detail(today(), true), "starts at 21:00 on Friday");
        assert_eq!(status(true, at(25, 21, 0), true, false).detail(today(), true), "next start at 21:00");
        assert_eq!(status(false, at(26, 7, 0), false, false).detail(today(), true), "next end at 07:00 tomorrow");
    }

    #[test]
    fn unscheduled() {
        assert_eq!(status(true, 0, false, false).message(today(), true), "Night Mode on · until turned off");
        assert_eq!(status(false, 0, false, false).detail(today(), true), "no scheduled start");
        assert!(!status(false, 0, false, false).names_day(today()));
    }

    #[test]
    fn names_day() {
        assert!(!status(false, at(25, 21, 0), true, false).names_day(today()));
        assert!(status(false, at(26, 21, 0), true, false).names_day(today()));
    }

    #[test]
    fn fits_the_portal_limit() {
        let s = status(false, at(29, 23, 59), false, true);
        assert!(s.message(today(), false).chars().count() <= 96);
    }
}
