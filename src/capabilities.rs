//! Which of the optional features this device has.

use gio::prelude::*;

use crate::keys::HostKey;

pub const NIGHT_LIGHT_SERVICE: &str = "org.gnome.SettingsDaemon.Color";
pub const POWER_SERVICE: &str = "org.gnome.SettingsDaemon.Power";
pub const FEEDBACK_SERVICE: &str = "org.sigxcpu.Feedback";
const SENSOR_PROXY: &str = "net.hadess.SensorProxy";
const SENSOR_PROXY_PATH: &str = "/net/hadess/SensorProxy";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub night_light: bool,
    pub ambient_light: bool,
    pub feedback: bool,
}

impl Capabilities {
    /// Whether NightManager may touch `key` on this device.
    pub fn allows(self, key: HostKey) -> bool {
        match key {
            HostKey::NightLight => self.night_light,
            HostKey::AmbientLight => self.ambient_light,
            HostKey::FeedbackProfile => self.feedback,
            _ => true,
        }
    }

    pub fn detect(session: &gio::DBusConnection) -> Capabilities {
        Capabilities {
            night_light: has_service(session, NIGHT_LIGHT_SERVICE),
            ambient_light: has_service(session, POWER_SERVICE) && has_ambient_light(),
            feedback: has_service(session, FEEDBACK_SERVICE),
        }
    }

    pub async fn detect_async() -> Capabilities {
        let Ok(session) = gio::bus_get_future(gio::BusType::Session).await else {
            return Capabilities::default();
        };
        Capabilities {
            night_light: has_service_async(&session, NIGHT_LIGHT_SERVICE).await,
            ambient_light: has_service_async(&session, POWER_SERVICE).await
                && has_ambient_light_async().await,
            feedback: has_service_async(&session, FEEDBACK_SERVICE).await,
        }
    }
}

fn name_has_owner_args(name: &str) -> glib::Variant {
    (name,).to_variant()
}

fn bool_reply() -> &'static glib::VariantTy {
    glib::VariantTy::new("(b)").expect("valid type string")
}

fn variant_reply() -> &'static glib::VariantTy {
    glib::VariantTy::new("(v)").expect("valid type string")
}

fn has_service(conn: &gio::DBusConnection, name: &str) -> bool {
    conn.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
        Some(&name_has_owner_args(name)),
        Some(bool_reply()),
        gio::DBusCallFlags::NONE,
        2000,
        gio::Cancellable::NONE,
    )
    .ok()
    .and_then(|v| v.get::<(bool,)>())
    .is_some_and(|(b,)| b)
}

async fn has_service_async(conn: &gio::DBusConnection, name: &str) -> bool {
    conn.call_future(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
        Some(&name_has_owner_args(name)),
        Some(bool_reply()),
        gio::DBusCallFlags::NONE,
        2000,
    )
    .await
    .ok()
    .and_then(|v| v.get::<(bool,)>())
    .is_some_and(|(b,)| b)
}

fn sensor_args() -> glib::Variant {
    (SENSOR_PROXY, "HasAmbientLight").to_variant()
}

fn unwrap_bool(reply: glib::Variant) -> Option<bool> {
    reply.child_value(0).as_variant()?.get::<bool>()
}

fn has_ambient_light() -> bool {
    let Ok(conn) = gio::bus_get_sync(gio::BusType::System, gio::Cancellable::NONE) else {
        return false;
    };
    conn.call_sync(
        Some(SENSOR_PROXY),
        SENSOR_PROXY_PATH,
        "org.freedesktop.DBus.Properties",
        "Get",
        Some(&sensor_args()),
        Some(variant_reply()),
        gio::DBusCallFlags::NONE,
        2000,
        gio::Cancellable::NONE,
    )
    .ok()
    .and_then(unwrap_bool)
    .unwrap_or(false)
}

async fn has_ambient_light_async() -> bool {
    let Ok(conn) = gio::bus_get_future(gio::BusType::System).await else {
        return false;
    };
    conn.call_future(
        Some(SENSOR_PROXY),
        SENSOR_PROXY_PATH,
        "org.freedesktop.DBus.Properties",
        "Get",
        Some(&sensor_args()),
        Some(variant_reply()),
        gio::DBusCallFlags::NONE,
        2000,
    )
    .await
    .ok()
    .and_then(unwrap_bool)
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_optional_features_are_gated() {
        let none = Capabilities::default();
        assert!(!none.allows(HostKey::NightLight));
        assert!(!none.allows(HostKey::AmbientLight));
        assert!(!none.allows(HostKey::FeedbackProfile));
        for key in [HostKey::ColorScheme, HostKey::GtkTheme, HostKey::AccentColor, HostKey::IconTheme] {
            assert!(none.allows(key));
        }
    }
}
