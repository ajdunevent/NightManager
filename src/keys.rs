use serde::{Deserialize, Serialize};

pub const COLOR_SCHEMA: &str = "org.gnome.settings-daemon.plugins.color";
pub const POWER_SCHEMA: &str = "org.gnome.settings-daemon.plugins.power";
pub const INTERFACE_SCHEMA: &str = "org.gnome.desktop.interface";
pub const FEEDBACKD_SCHEMA: &str = "org.sigxcpu.feedbackd";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum HostKey {
    NightLight,
    AmbientLight,
    ColorScheme,
    GtkTheme,
    AccentColor,
    IconTheme,
    FeedbackProfile,
}

impl HostKey {
    pub const ALL: [HostKey; 7] = [
        HostKey::NightLight,
        HostKey::AmbientLight,
        HostKey::ColorScheme,
        HostKey::GtkTheme,
        HostKey::AccentColor,
        HostKey::IconTheme,
        HostKey::FeedbackProfile,
    ];

    pub fn schema(self) -> &'static str {
        match self {
            HostKey::NightLight => COLOR_SCHEMA,
            HostKey::AmbientLight => POWER_SCHEMA,
            HostKey::ColorScheme | HostKey::GtkTheme | HostKey::AccentColor | HostKey::IconTheme => {
                INTERFACE_SCHEMA
            }
            HostKey::FeedbackProfile => FEEDBACKD_SCHEMA,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            HostKey::NightLight => "night-light-enabled",
            HostKey::AmbientLight => "ambient-enabled",
            HostKey::ColorScheme => "color-scheme",
            HostKey::GtkTheme => "gtk-theme",
            HostKey::AccentColor => "accent-color",
            HostKey::IconTheme => "icon-theme",
            HostKey::FeedbackProfile => "profile",
        }
    }

    pub fn variant_type(self) -> &'static glib::VariantTy {
        match self {
            HostKey::NightLight | HostKey::AmbientLight => glib::VariantTy::BOOLEAN,
            _ => glib::VariantTy::STRING,
        }
    }

    /// Theme keys whose night value can follow changes the user makes at night.
    pub fn is_theme(self) -> bool {
        matches!(self, HostKey::GtkTheme | HostKey::AccentColor | HostKey::IconTheme)
    }

    /// The key's name on the daemon's D-Bus interface. GSettings key names are
    /// unique across the managed keys, so they double as names.
    pub fn name(self) -> &'static str {
        self.key()
    }

    pub fn from_name(name: &str) -> Option<HostKey> {
        HostKey::ALL.into_iter().find(|k| k.name() == name)
    }

    pub fn from_schema_key(schema: &str, key: &str) -> Option<HostKey> {
        HostKey::ALL
            .into_iter()
            .find(|k| k.schema() == schema && k.key() == key)
    }
}

/// Parses a value saved with `Variant::print(true)`.
pub fn parse_value(key: HostKey, text: &str) -> Option<glib::Variant> {
    glib::Variant::parse(Some(key.variant_type()), text).ok()
}

pub fn print_value(value: &glib::Variant) -> String {
    value.print(true).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use glib::prelude::*;

    #[test]
    fn values_round_trip_through_text() {
        let b = true.to_variant();
        assert_eq!(parse_value(HostKey::NightLight, &print_value(&b)), Some(b));
        let s = "prefer-dark".to_variant();
        assert_eq!(parse_value(HostKey::ColorScheme, &print_value(&s)), Some(s));
    }

    #[test]
    fn values_from_gsettings_get_output_parse() {
        // The old scripts stored `gsettings get` output.
        assert_eq!(
            parse_value(HostKey::AccentColor, "'blue'"),
            Some("blue".to_variant())
        );
        assert_eq!(parse_value(HostKey::NightLight, "false"), Some(false.to_variant()));
        assert_eq!(parse_value(HostKey::NightLight, "'x'"), None);
    }

    #[test]
    fn lookup_by_schema_and_key() {
        assert_eq!(
            HostKey::from_schema_key(INTERFACE_SCHEMA, "accent-color"),
            Some(HostKey::AccentColor)
        );
        assert_eq!(HostKey::from_schema_key(INTERFACE_SCHEMA, "nope"), None);
    }
}
