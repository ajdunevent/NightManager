use chrono::{DateTime, Datelike, Duration, LocalResult, NaiveDate, NaiveTime, TimeZone};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boundary {
    Start,
    End,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event<Tz: TimeZone> {
    pub at: DateTime<Tz>,
    pub kind: Boundary,
}

/// Evening (start) and morning (end) times per weekday, Monday first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Weekly {
    pub evening: [Option<NaiveTime>; 7],
    pub morning: [Option<NaiveTime>; 7],
}

impl Weekly {
    pub fn daily(evening: Option<NaiveTime>, morning: Option<NaiveTime>) -> Weekly {
        Weekly {
            evening: [evening; 7],
            morning: [morning; 7],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.evening.iter().chain(self.morning.iter()).all(Option::is_none)
    }

    fn events_on<Tz: TimeZone>(&self, tz: &Tz, date: NaiveDate) -> Vec<Event<Tz>> {
        let day = date.weekday().num_days_from_monday() as usize;
        let mut out = Vec::new();
        if let Some(t) = self.morning[day] {
            if let Some(at) = resolve(tz, date, t) {
                out.push(Event { at, kind: Boundary::End });
            }
        }
        if let Some(t) = self.evening[day] {
            if let Some(at) = resolve(tz, date, t) {
                out.push(Event { at, kind: Boundary::Start });
            }
        }
        out
    }

    fn events_around<Tz: TimeZone>(&self, now: &DateTime<Tz>) -> Vec<Event<Tz>> {
        let tz = now.timezone();
        let today = now.date_naive();
        let mut out: Vec<Event<Tz>> = (-8..=8)
            .flat_map(|d| self.events_on(&tz, today + Duration::days(d)))
            .collect();
        out.sort_by(|a, b| a.at.cmp(&b.at));
        out
    }

    /// The first boundary strictly after `now`.
    pub fn next_after<Tz: TimeZone>(&self, now: &DateTime<Tz>) -> Option<Event<Tz>> {
        self.events_around(now).into_iter().find(|e| e.at > *now)
    }

    /// The latest boundary at or before `now`.
    pub fn last_at_or_before<Tz: TimeZone>(&self, now: &DateTime<Tz>) -> Option<Event<Tz>> {
        self.events_around(now).into_iter().rev().find(|e| e.at <= *now)
    }
}

/// A wall-clock time inside a DST gap is moved past the gap; one that occurs twice
/// uses the first occurrence.
fn resolve<Tz: TimeZone>(tz: &Tz, date: NaiveDate, time: NaiveTime) -> Option<DateTime<Tz>> {
    let naive = date.and_time(time);
    match tz.from_local_datetime(&naive) {
        LocalResult::Single(t) => Some(t),
        LocalResult::Ambiguous(first, _) => Some(first),
        LocalResult::None => (1..=4).find_map(|h| {
            tz.from_local_datetime(&(naive + Duration::hours(h)))
                .earliest()
        }),
    }
}

/// Hours since midnight, as gsd stores Night Light times, to a time of day.
pub fn hours_to_time(hours: f64) -> Option<NaiveTime> {
    if !hours.is_finite() {
        return None;
    }
    let total = (hours.rem_euclid(24.0) * 60.0).round() as u32 % (24 * 60);
    NaiveTime::from_hms_opt(total / 60, total % 60, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn t(s: &str) -> Option<NaiveTime> {
        NaiveTime::parse_from_str(s, "%H:%M").ok()
    }

    fn at(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_str(&format!("{s} -0600"), "%Y-%m-%d %H:%M %z").unwrap()
    }

    // 2026-11-16 is a Monday.
    fn weekdays_and_weekend() -> Weekly {
        let mut w = Weekly::daily(t("20:30"), t("07:00"));
        w.morning[5] = t("09:00");
        w.morning[6] = t("09:00");
        w
    }

    #[test]
    fn next_and_last_around_a_weekday_evening() {
        let w = weekdays_and_weekend();
        let now = at("2026-11-17 19:00");
        let next = w.next_after(&now).unwrap();
        assert_eq!((next.at, next.kind), (at("2026-11-17 20:30"), Boundary::Start));
        let last = w.last_at_or_before(&now).unwrap();
        assert_eq!((last.at, last.kind), (at("2026-11-17 07:00"), Boundary::End));
    }

    #[test]
    fn weekend_morning_uses_its_own_time() {
        let w = weekdays_and_weekend();
        let next = w.next_after(&at("2026-11-21 01:00")).unwrap(); // Saturday
        assert_eq!((next.at, next.kind), (at("2026-11-21 09:00"), Boundary::End));
    }

    #[test]
    fn an_event_exactly_now_is_last_not_next() {
        let w = weekdays_and_weekend();
        let now = at("2026-11-17 20:30");
        assert_eq!(w.last_at_or_before(&now).unwrap().kind, Boundary::Start);
        assert_eq!(w.next_after(&now).unwrap().at, at("2026-11-18 07:00"));
    }

    #[test]
    fn days_switched_off_are_skipped() {
        let mut w = weekdays_and_weekend();
        w.evening[5] = None; // no night mode starting Saturday
        let next = w.next_after(&at("2026-11-21 10:00")).unwrap();
        assert_eq!((next.at, next.kind), (at("2026-11-22 09:00"), Boundary::End));
        let next = w.next_after(&next.at).unwrap();
        assert_eq!((next.at, next.kind), (at("2026-11-22 20:30"), Boundary::Start));
    }

    #[test]
    fn empty_schedule_has_no_events() {
        let w = Weekly::default();
        assert!(w.is_empty());
        assert!(w.next_after(&at("2026-11-17 12:00")).is_none());
        assert!(w.last_at_or_before(&at("2026-11-17 12:00")).is_none());
    }

    #[test]
    fn night_light_hours_convert() {
        assert_eq!(hours_to_time(20.0), t("20:00"));
        assert_eq!(hours_to_time(6.931_167_374_741_309), t("06:56"));
        assert_eq!(hours_to_time(23.999), t("00:00"));
        assert_eq!(hours_to_time(f64::NAN), None);
    }
}
