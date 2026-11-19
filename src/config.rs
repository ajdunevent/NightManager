use chrono::NaiveTime;
use gio::prelude::*;

use crate::keys::HostKey;

pub const APP_ID: &str = "net.dunevent.NightManager";
pub const DAEMON_BUS_NAME: &str = "net.dunevent.NightManager.Daemon";
pub const DAEMON_OBJECT_PATH: &str = "/net/dunevent/NightManager/Daemon";
pub const DAEMON_INTERFACE: &str = "net.dunevent.NightManager.Daemon";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeedbackProfile {
    Full,
    Quiet,
    Silent,
}

impl FeedbackProfile {
    pub const ALL: [FeedbackProfile; 3] = [Self::Full, Self::Quiet, Self::Silent];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Quiet => "quiet",
            Self::Silent => "silent",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "full" => Some(Self::Full),
            "quiet" => Some(Self::Quiet),
            "silent" => Some(Self::Silent),
            _ => None,
        }
    }

    fn bit(self) -> u8 {
        match self {
            Self::Full => 1,
            Self::Quiet => 2,
            Self::Silent => 4,
        }
    }
}

/// The feedback profiles that count as "night" for the manual trigger.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FeedbackSet(u8);

impl FeedbackSet {
    pub const OFF: FeedbackSet = FeedbackSet(0);

    /// The trigger choices offered in the UI, in display order.
    pub const CHOICES: [(FeedbackSet, &'static str); 6] = [
        (FeedbackSet(1), "Full"),
        (FeedbackSet(2), "Quiet"),
        (FeedbackSet(4), "Silent"),
        (FeedbackSet(6), "Not Full"),
        (FeedbackSet(5), "Not Quiet"),
        (FeedbackSet(3), "Not Silent"),
    ];

    pub fn is_off(self) -> bool {
        self.0 == 0
    }

    pub fn contains(self, profile: FeedbackProfile) -> bool {
        self.0 & profile.bit() != 0
    }

    pub fn from_strs<'a>(items: impl IntoIterator<Item = &'a str>) -> Self {
        FeedbackSet(
            items
                .into_iter()
                .filter_map(FeedbackProfile::parse)
                .fold(0, |acc, p| acc | p.bit()),
        )
    }

    pub fn to_strs(self) -> Vec<&'static str> {
        FeedbackProfile::ALL
            .into_iter()
            .filter(|p| self.contains(*p))
            .map(FeedbackProfile::as_str)
            .collect()
    }

    /// A trigger that excludes the night feedback level could start night mode but
    /// never end it from the toggle, since NightManager's own write leaves the group.
    pub fn compatible_with(self, night_level: Option<FeedbackProfile>) -> bool {
        match night_level {
            Some(level) => self.contains(level),
            None => true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScheduleSource {
    #[default]
    Custom,
    NightLight,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScreenOffMode {
    #[default]
    WakeBriefly,
    WaitForScreen,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Config {
    pub night_light: bool,
    pub ambient_light: bool,
    pub dark_style: bool,
    pub gtk_theme: Option<String>,
    pub accent_color: Option<String>,
    pub icon_theme: Option<String>,
    pub feedback_level: Option<FeedbackProfile>,
    pub save_theme_changes: bool,
    pub schedule_source: ScheduleSource,
    /// Monday first. `None` is a day without that boundary.
    pub evening: [Option<NaiveTime>; 7],
    pub morning: [Option<NaiveTime>; 7],
    pub trigger_dark_style: bool,
    pub trigger_night_light: bool,
    pub trigger_feedback: FeedbackSet,
    pub screen_off_mode: ScreenOffMode,
}

impl Config {
    /// The value each managed key takes at night. Keys left out are not touched.
    pub fn night_values(&self) -> Vec<(HostKey, glib::Variant)> {
        let mut out = Vec::new();
        if self.night_light {
            out.push((HostKey::NightLight, true.to_variant()));
        }
        if self.ambient_light {
            out.push((HostKey::AmbientLight, true.to_variant()));
        }
        if self.dark_style {
            out.push((HostKey::ColorScheme, "prefer-dark".to_variant()));
        }
        if let Some(t) = &self.gtk_theme {
            out.push((HostKey::GtkTheme, t.to_variant()));
        }
        if let Some(a) = &self.accent_color {
            out.push((HostKey::AccentColor, a.to_variant()));
        }
        if let Some(i) = &self.icon_theme {
            out.push((HostKey::IconTheme, i.to_variant()));
        }
        if let Some(f) = self.feedback_level {
            out.push((HostKey::FeedbackProfile, f.as_str().to_variant()));
        }
        out
    }

    pub fn load(settings: &gio::Settings) -> Config {
        let opt = |key: &str| {
            let s = settings.string(key);
            (!s.is_empty()).then(|| s.to_string())
        };
        Config {
            night_light: settings.boolean("night-light"),
            ambient_light: settings.boolean("ambient-light"),
            dark_style: settings.boolean("dark-style"),
            gtk_theme: opt("gtk-theme"),
            accent_color: opt("accent-color"),
            icon_theme: opt("icon-theme"),
            feedback_level: FeedbackProfile::parse(&settings.string("feedback-level")),
            save_theme_changes: settings.boolean("save-theme-changes"),
            schedule_source: match settings.string("schedule-source").as_str() {
                "night-light" => ScheduleSource::NightLight,
                _ => ScheduleSource::Custom,
            },
            evening: parse_times(&settings.strv("evening-times")),
            morning: parse_times(&settings.strv("morning-times")),
            trigger_dark_style: settings.boolean("trigger-dark-style"),
            trigger_night_light: settings.boolean("trigger-night-light"),
            trigger_feedback: FeedbackSet::from_strs(
                settings.strv("trigger-feedback").iter().map(|s| s.as_str()),
            ),
            screen_off_mode: match settings.string("screen-off-mode").as_str() {
                "wait" => ScreenOffMode::WaitForScreen,
                _ => ScreenOffMode::WakeBriefly,
            },
        }
    }
}

pub fn theme_setting_key(key: HostKey) -> Option<&'static str> {
    match key {
        HostKey::GtkTheme => Some("gtk-theme"),
        HostKey::AccentColor => Some("accent-color"),
        HostKey::IconTheme => Some("icon-theme"),
        _ => None,
    }
}

pub fn parse_time(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s.trim(), "%H:%M").ok()
}

pub fn format_time(t: Option<NaiveTime>) -> String {
    t.map(|t| t.format("%H:%M").to_string()).unwrap_or_default()
}

fn parse_times(items: &glib::StrV) -> [Option<NaiveTime>; 7] {
    let mut out = [None; 7];
    for (slot, s) in out.iter_mut().zip(items.iter()) {
        *slot = parse_time(s);
    }
    out
}

pub fn times_to_strv(times: &[Option<NaiveTime>; 7]) -> Vec<String> {
    times.iter().map(|t| format_time(*t)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feedback_choices_match_their_labels() {
        for (set, label) in FeedbackSet::CHOICES {
            let members: Vec<_> = set.to_strs();
            match label {
                "Full" => assert_eq!(members, ["full"]),
                "Quiet" => assert_eq!(members, ["quiet"]),
                "Silent" => assert_eq!(members, ["silent"]),
                "Not Full" => assert_eq!(members, ["quiet", "silent"]),
                "Not Quiet" => assert_eq!(members, ["full", "silent"]),
                "Not Silent" => assert_eq!(members, ["full", "quiet"]),
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn trigger_must_include_the_night_level() {
        let silent = Some(FeedbackProfile::Silent);
        let valid: Vec<_> = FeedbackSet::CHOICES
            .iter()
            .filter(|(s, _)| s.compatible_with(silent))
            .map(|(_, l)| *l)
            .collect();
        assert_eq!(valid, ["Silent", "Not Full", "Not Quiet"]);
        assert!(FeedbackSet::CHOICES.iter().all(|(s, _)| s.compatible_with(None)));
    }

    #[test]
    fn times_round_trip() {
        let t = parse_time("07:05");
        assert_eq!(format_time(t), "07:05");
        assert_eq!(parse_time(""), None);
        assert_eq!(parse_time("25:00"), None);
    }
}
