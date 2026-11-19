//! Detection of the old script-based NightManager and import of its settings.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::NaiveTime;

use crate::config::{parse_time, FeedbackProfile};
use crate::keys::{parse_value, HostKey};

const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

fn home() -> PathBuf {
    glib::home_dir()
}

/// Host paths are under /run/host inside a Flatpak, given host-os access.
fn host_path(path: &str) -> PathBuf {
    let host = Path::new("/run/host").join(path.trim_start_matches('/'));
    if Path::new("/run/host/usr").exists() {
        host
    } else {
        PathBuf::from(path)
    }
}

/// Whether the old package or a manual install of its timers is still present.
pub fn installed() -> bool {
    let package = ["/usr/bin/nightmanager", "/usr/lib/systemd/user/nightmanager-on.timer"]
        .iter()
        .any(|p| host_path(p).exists());
    if package {
        return true;
    }
    let wants = home().join(".config/systemd/user/timers.target.wants");
    ["nightmanager-on.timer", "nightmanager-off.timer"]
        .iter()
        .any(|name| enabled_unit_exists(&wants.join(name)))
}

fn enabled_unit_exists(link: &Path) -> bool {
    match fs::read_link(link) {
        Ok(target) if target.is_absolute() => {
            host_path(target.to_str().unwrap_or_default()).exists()
        }
        Ok(target) => link.parent().map(|d| d.join(target).exists()).unwrap_or(false),
        Err(_) => false,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Imported {
    pub night_light: bool,
    pub ambient_light: bool,
    pub dark_style: bool,
    pub gtk_theme: Option<String>,
    pub accent_color: Option<String>,
    pub icon_theme: Option<String>,
    pub feedback_level: Option<FeedbackProfile>,
    pub evening: [Option<NaiveTime>; 7],
    pub morning: [Option<NaiveTime>; 7],
    /// Day values the old version saved when its night mode started.
    pub restore: Option<BTreeMap<HostKey, String>>,
}

/// Reads the old configuration, timer drop-ins and restore state from their
/// usual locations. Missing files fall back to the old version's defaults.
pub fn import() -> Imported {
    let config = home().join(".config/nightmanager/nightmanager.conf");
    let on = home().join(".config/systemd/user/nightmanager-on.timer.d/10-override.conf");
    let off = home().join(".config/systemd/user/nightmanager-off.timer.d/10-override.conf");
    let restore = home().join(".local/state/nightmanager/restore.conf");
    import_from(
        fs::read_to_string(config).ok().as_deref(),
        fs::read_to_string(on).ok().as_deref(),
        fs::read_to_string(off).ok().as_deref(),
        fs::read_to_string(restore).ok().as_deref(),
    )
}

pub fn import_from(
    config: Option<&str>,
    on_timer: Option<&str>,
    off_timer: Option<&str>,
    restore: Option<&str>,
) -> Imported {
    let vars = config.map(shell_vars).unwrap_or_default();
    let get = |k: &str, default: &str| vars.get(k).cloned().unwrap_or_else(|| default.to_string());
    let flag = |k: &str| get(k, "1") == "1";
    let name = |k: &str, default: &str| {
        let v = get(k, default);
        (v != "0" && !v.is_empty()).then_some(v)
    };

    let on_default = "OnCalendar=*-*-* 20:30:00";
    let off_default = "OnCalendar=Mon..Fri 07:00:00\nOnCalendar=Sat,Sun 09:00:00";

    Imported {
        night_light: flag("NLT_ON"),
        ambient_light: flag("BRT_ON"),
        dark_style: flag("DRK_ON"),
        gtk_theme: name("LEG_ON", "0"),
        accent_color: name("ACT_ON", "slate"),
        icon_theme: name("ICN_ON", "0"),
        feedback_level: match get("FB_LEV", "1").as_str() {
            "1" => Some(FeedbackProfile::Silent),
            "2" => Some(FeedbackProfile::Quiet),
            _ => None,
        },
        evening: calendar_times(on_timer.unwrap_or(on_default)),
        morning: calendar_times(off_timer.unwrap_or(off_default)),
        restore: restore.map(restore_values).filter(|m| !m.is_empty()),
    }
}

fn shell_vars(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), unquote(v.trim()).to_string()))
        .collect()
}

fn unquote(v: &str) -> &str {
    v.strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(v)
}

/// Per-weekday times from `OnCalendar=` lines. An empty `OnCalendar=` clears the
/// lines before it, as in systemd drop-ins.
fn calendar_times(text: &str) -> [Option<NaiveTime>; 7] {
    let mut out = [None; 7];
    for line in text.lines().map(str::trim) {
        let Some(spec) = line.strip_prefix("OnCalendar=") else {
            continue;
        };
        if spec.trim().is_empty() {
            out = [None; 7];
            continue;
        }
        let Some((days, time)) = parse_calendar(spec) else {
            tracing::warn!(spec, "unrecognized OnCalendar line skipped");
            continue;
        };
        for (slot, on) in out.iter_mut().zip(days) {
            if on && slot.is_none() {
                *slot = Some(time);
            }
        }
    }
    out
}

fn parse_calendar(spec: &str) -> Option<([bool; 7], NaiveTime)> {
    let mut days = [true; 7];
    let mut time = None;
    for token in spec.split_whitespace() {
        if token.contains(':') {
            let hm: String = token.splitn(3, ':').take(2).collect::<Vec<_>>().join(":");
            time = parse_time(&hm);
        } else if token.contains('-') || token == "*" {
            continue;
        } else {
            days = parse_days(token)?;
        }
    }
    Some((days, time?))
}

fn parse_days(token: &str) -> Option<[bool; 7]> {
    let index = |d: &str| DAYS.iter().position(|x| x.eq_ignore_ascii_case(&d[..d.len().min(3)]));
    let mut out = [false; 7];
    for part in token.split(',') {
        if let Some((a, b)) = part.split_once("..") {
            let (a, b) = (index(a)?, index(b)?);
            let mut i = a;
            loop {
                out[i] = true;
                if i == b {
                    break;
                }
                i = (i + 1) % 7;
            }
        } else {
            out[index(part)?] = true;
        }
    }
    Some(out)
}

fn restore_values(text: &str) -> BTreeMap<HostKey, String> {
    let names = [
        ("NLT_STATE", HostKey::NightLight),
        ("BRT_STATE", HostKey::AmbientLight),
        ("DRK_STATE", HostKey::ColorScheme),
        ("LEG_STATE", HostKey::GtkTheme),
        ("ACT_STATE", HostKey::AccentColor),
        ("ICN_STATE", HostKey::IconTheme),
        ("FBK_STATE", HostKey::FeedbackProfile),
    ];
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let Some((k, v)) = line.trim().split_once('=') else {
            continue;
        };
        let Some((_, key)) = names.iter().find(|(n, _)| *n == k.trim()) else {
            continue;
        };
        let v = v.trim();
        if v == "0" {
            continue;
        }
        if let Some(value) = parse_value(*key, v) {
            out.insert(*key, crate::keys::print_value(&value));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Option<NaiveTime> {
        parse_time(s)
    }

    #[test]
    fn defaults_without_files() {
        let i = import_from(None, None, None, None);
        assert!(i.night_light && i.ambient_light && i.dark_style);
        assert_eq!(i.accent_color.as_deref(), Some("slate"));
        assert_eq!(i.gtk_theme, None);
        assert_eq!(i.feedback_level, Some(FeedbackProfile::Silent));
        assert_eq!(i.evening, [t("20:30"); 7]);
        assert_eq!(
            i.morning,
            [t("07:00"), t("07:00"), t("07:00"), t("07:00"), t("07:00"), t("09:00"), t("09:00")]
        );
        assert_eq!(i.restore, None);
    }

    #[test]
    fn config_written_by_the_yad_tool() {
        let conf = "NLT_ON=1\nBRT_ON=0\nDRK_ON=1\nLEG_ON=\"adw-gtk3-dark\"\nACT_ON=\"0\"\nICN_ON=\"Adwaita-slate\"\nFB_LEV=2\n";
        let i = import_from(Some(conf), None, None, None);
        assert!(i.night_light && !i.ambient_light && i.dark_style);
        assert_eq!(i.gtk_theme.as_deref(), Some("adw-gtk3-dark"));
        assert_eq!(i.accent_color, None);
        assert_eq!(i.icon_theme.as_deref(), Some("Adwaita-slate"));
        assert_eq!(i.feedback_level, Some(FeedbackProfile::Quiet));
    }

    #[test]
    fn drop_ins_written_by_the_yad_tool() {
        let on = "[Timer]\nOnCalendar=\nOnCalendar=Mon..Fri 21:00:00\nOnCalendar=Sat,Sun 22:15:00\n";
        let off = "[Timer]\nOnCalendar=\nOnCalendar=*-*-* 06:45:00\n";
        let i = import_from(None, Some(on), Some(off), None);
        assert_eq!(
            i.evening,
            [t("21:00"), t("21:00"), t("21:00"), t("21:00"), t("21:00"), t("22:15"), t("22:15")]
        );
        assert_eq!(i.morning, [t("06:45"); 7]);
    }

    #[test]
    fn day_lists_and_wrapping_ranges() {
        assert_eq!(
            parse_days("Mon,Wed,Fri"),
            Some([true, false, true, false, true, false, false])
        );
        assert_eq!(
            parse_days("Sat..Mon"),
            Some([true, false, false, false, false, true, true])
        );
        assert_eq!(parse_days("Funday"), None);
    }

    #[test]
    fn restore_file_from_the_old_script() {
        let text = "NLT_STATE=false\nBRT_STATE=0\nDRK_STATE='default'\nLEG_STATE=0\nACT_STATE='blue'\nICN_STATE=0\nFBK_STATE='full'\n";
        let r = import_from(None, None, None, Some(text)).restore.unwrap();
        assert_eq!(r.get(&HostKey::NightLight).map(String::as_str), Some("false"));
        assert_eq!(r.get(&HostKey::ColorScheme).map(String::as_str), Some("'default'"));
        assert_eq!(r.get(&HostKey::AccentColor).map(String::as_str), Some("'blue'"));
        assert_eq!(r.get(&HostKey::FeedbackProfile).map(String::as_str), Some("'full'"));
        assert!(!r.contains_key(&HostKey::AmbientLight));
    }
}
