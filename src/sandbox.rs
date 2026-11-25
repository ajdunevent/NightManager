//! Flatpak detection and the Background portal.

use std::collections::HashMap;
use std::path::Path;

use gio::prelude::*;

const PORTAL_BUS_NAME: &str = "org.freedesktop.portal.Desktop";
const PORTAL_OBJECT_PATH: &str = "/org/freedesktop/portal/desktop";
const BACKGROUND_INTERFACE: &str = "org.freedesktop.portal.Background";
const REQUEST_INTERFACE: &str = "org.freedesktop.portal.Request";

pub fn is_flatpak() -> bool {
    Path::new("/.flatpak-info").exists()
}

fn request_path(unique_name: &str, token: &str) -> String {
    let sender = unique_name.trim_start_matches(':').replace('.', "_");
    format!("{PORTAL_OBJECT_PATH}/request/{sender}/{token}")
}

/// Asks the portal to start the daemon at login (`autostart`) or to stop doing
/// so. Returns whether autostart is on afterwards. The portal asks the user the
/// first time.
pub async fn request_autostart(autostart: bool) -> anyhow::Result<bool> {
    let conn = gio::bus_get_future(gio::BusType::Session).await?;
    let unique_name = conn
        .unique_name()
        .ok_or_else(|| anyhow::anyhow!("session bus connection has no unique name"))?;
    let token = format!("nightmanager{}", std::process::id());
    let expected_path = request_path(&unique_name, &token);

    // Subscribed before the call: the Response can arrive before the call returns.
    let (sender, receiver) = async_channel::bounded::<(u32, HashMap<String, glib::Variant>)>(1);
    let subscription = conn.signal_subscribe(
        Some(PORTAL_BUS_NAME),
        Some(REQUEST_INTERFACE),
        Some("Response"),
        Some(&expected_path),
        None,
        gio::DBusSignalFlags::NONE,
        move |_, _, _, _, _, params| {
            if let Some(response) = params.get::<(u32, HashMap<String, glib::Variant>)>() {
                let _ = sender.try_send(response);
            }
        },
    );

    let options = glib::VariantDict::new(None);
    options.insert("handle_token", token.as_str());
    options.insert(
        "reason",
        "NightManager runs in the background to switch night mode on schedule.",
    );
    options.insert("autostart", autostart);
    options.insert("commandline", vec!["nightmanager-daemon".to_string()]);
    options.insert("dbus-activatable", false);

    let call = conn
        .call_future(
            Some(PORTAL_BUS_NAME),
            PORTAL_OBJECT_PATH,
            BACKGROUND_INTERFACE,
            "RequestBackground",
            Some(&glib::Variant::tuple_from_iter(["".to_variant(), options.end()])),
            Some(glib::VariantTy::new("(o)").expect("valid type string")),
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await;
    let outcome = match call {
        Ok(_) => receiver.recv().await.map_err(anyhow::Error::from),
        Err(err) => Err(err.into()),
    };
    conn.signal_unsubscribe(subscription);

    let (code, results) = outcome?;
    match code {
        0 => Ok(results
            .get("autostart")
            .and_then(|v| v.get::<bool>())
            .unwrap_or(false)),
        1 => anyhow::bail!("background request was declined"),
        _ => anyhow::bail!("background request failed (response {code})"),
    }
}

pub fn set_background_status(conn: &gio::DBusConnection, message: &str) {
    let options = glib::VariantDict::new(None);
    options.insert("message", message);
    conn.call(
        Some(PORTAL_BUS_NAME),
        PORTAL_OBJECT_PATH,
        BACKGROUND_INTERFACE,
        "SetStatus",
        Some(&glib::Variant::tuple_from_iter([options.end()])),
        None,
        gio::DBusCallFlags::NONE,
        -1,
        gio::Cancellable::NONE,
        |result| {
            if let Err(err) = result {
                tracing::debug!(%err, "background status not set");
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_path_follows_the_portal_naming_rule() {
        assert_eq!(
            request_path(":1.234", "nightmanager42"),
            "/org/freedesktop/portal/desktop/request/1_234/nightmanager42"
        );
    }
}
