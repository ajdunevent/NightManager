//! Which of the optional features this device has.

use std::cell::Cell;
use std::rc::Rc;

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

/// Follows the services behind each capability, so a service starting after
/// the daemon (gsd at login) is picked up. `on_gain` runs when one appears.
pub fn watch(session: &gio::DBusConnection, on_gain: impl Fn() + 'static) -> Rc<Cell<Capabilities>> {
    #[derive(Clone, Copy, Default)]
    struct Seen {
        night_light: bool,
        power: bool,
        feedback: bool,
        ambient_sensor: bool,
    }
    let initial = Capabilities::detect(session);
    let seen = Rc::new(Cell::new(Seen {
        night_light: initial.night_light,
        power: has_service(session, POWER_SERVICE),
        feedback: initial.feedback,
        ambient_sensor: has_ambient_light(),
    }));
    let current = Rc::new(Cell::new(initial));
    let on_gain = Rc::new(on_gain);
    let update = {
        let (seen, current, on_gain) = (seen.clone(), current.clone(), on_gain.clone());
        Rc::new(move |edit: &dyn Fn(&mut Seen)| {
            let mut s = seen.get();
            edit(&mut s);
            seen.set(s);
            let caps = Capabilities {
                night_light: s.night_light,
                ambient_light: s.power && s.ambient_sensor,
                feedback: s.feedback,
            };
            let before = current.replace(caps);
            if before != caps {
                tracing::info!(?caps, "capabilities changed");
                let gained = (caps.night_light && !before.night_light)
                    || (caps.ambient_light && !before.ambient_light)
                    || (caps.feedback && !before.feedback);
                if gained {
                    on_gain();
                }
            }
        })
    };

    let session_watch = |name: &'static str, field: fn(&mut Seen) -> &mut bool| {
        let (up, down) = (update.clone(), update.clone());
        let _ = gio::bus_watch_name_on_connection(
            session,
            name,
            gio::BusNameWatcherFlags::NONE,
            move |_, _, _| up(&|s| *field(s) = true),
            move |_, _| down(&|s| *field(s) = false),
        );
    };
    session_watch(NIGHT_LIGHT_SERVICE, |s| &mut s.night_light);
    session_watch(POWER_SERVICE, |s| &mut s.power);
    session_watch(FEEDBACK_SERVICE, |s| &mut s.feedback);

    // gio-rs panics in bus_watch_name's vanished callback when the bus can't
    // be reached, so the watch is set up on a connection that exists.
    glib::spawn_future_local(async move {
        let Ok(system) = gio::bus_get_future(gio::BusType::System).await else {
            return;
        };
        let (up, down) = (update.clone(), update);
        let _ = gio::bus_watch_name_on_connection(
            &system,
            SENSOR_PROXY,
            gio::BusNameWatcherFlags::NONE,
            move |_, _, _| {
                let up = up.clone();
                glib::spawn_future_local(async move {
                    let has = has_ambient_light_async().await;
                    up(&|s| s.ambient_sensor = has);
                });
            },
            move |_, _| down(&|s| s.ambient_sensor = false),
        );
    });
    current
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
