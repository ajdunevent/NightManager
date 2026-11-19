
use crate::config::{Config, FeedbackProfile};
use crate::keys::HostKey;
use crate::schedule::Boundary;

/// Whether a host value counts as "night" for an enabled manual trigger. `None`
/// when the key isn't a trigger in this configuration.
pub fn is_night(config: &Config, key: HostKey, value: &glib::Variant) -> Option<bool> {
    match key {
        HostKey::NightLight if config.trigger_night_light => value.get::<bool>(),
        HostKey::ColorScheme if config.trigger_dark_style => {
            value.str().map(|s| s == "prefer-dark")
        }
        HostKey::FeedbackProfile if !config.trigger_feedback.is_off() => value
            .str()
            .and_then(FeedbackProfile::parse)
            .map(|p| config.trigger_feedback.contains(p)),
        _ => None,
    }
}

/// A trigger fires only when a value moves into or out of its night group.
pub fn edge(before: Option<bool>, after: Option<bool>) -> Option<Boundary> {
    match (before?, after?) {
        (false, true) => Some(Boundary::Start),
        (true, false) => Some(Boundary::End),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::FeedbackSet;
    use glib::prelude::*;

    #[test]
    fn feedback_moves_within_the_group_do_nothing() {
        let config = Config {
            trigger_feedback: FeedbackSet::from_strs(["quiet", "silent"]),
            ..Default::default()
        };
        let night = |p: &str| is_night(&config, HostKey::FeedbackProfile, &p.to_variant());
        assert_eq!(edge(night("full"), night("quiet")), Some(Boundary::Start));
        assert_eq!(edge(night("quiet"), night("silent")), None);
        assert_eq!(edge(night("silent"), night("full")), Some(Boundary::End));
    }

    #[test]
    fn disabled_triggers_never_fire() {
        let config = Config::default();
        assert_eq!(is_night(&config, HostKey::NightLight, &true.to_variant()), None);
        assert_eq!(is_night(&config, HostKey::GtkTheme, &"x".to_variant()), None);
    }

    #[test]
    fn dark_style_trigger() {
        let config = Config {
            trigger_dark_style: true,
            ..Default::default()
        };
        let night = |s: &str| is_night(&config, HostKey::ColorScheme, &s.to_variant());
        assert_eq!(edge(night("default"), night("prefer-dark")), Some(Boundary::Start));
        assert_eq!(edge(night("prefer-dark"), night("prefer-light")), Some(Boundary::End));
        assert_eq!(edge(night("default"), night("prefer-light")), None);
    }
}
