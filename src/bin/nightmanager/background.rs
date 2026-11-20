//! The Background group: starting the daemon at login through the portal.

use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use nightmanager::sandbox;

pub fn group(settings: &gio::Settings, toasts: &adw::ToastOverlay) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("Background").build();
    let row = adw::SwitchRow::builder()
        .title("Start at Login")
        .subtitle("Needed for the schedule and triggers to work after a restart")
        .active(settings.boolean("autostart-requested"))
        .build();

    let request: Rc<dyn Fn(bool)> = Rc::new({
        let (settings, row, toasts) = (settings.clone(), row.clone(), toasts.clone());
        move |on: bool| {
            let (settings, row, toasts) = (settings.clone(), row.clone(), toasts.clone());
            row.set_sensitive(false);
            glib::spawn_future_local(async move {
                let result = glib::future_with_timeout(
                    Duration::from_secs(120),
                    sandbox::request_autostart(on),
                )
                .await;
                let enabled = match result {
                    Ok(Ok(enabled)) => {
                        if on && !enabled {
                            toasts.add_toast(adw::Toast::new("Starting at login wasn't allowed"));
                        }
                        enabled
                    }
                    Ok(Err(err)) => {
                        tracing::warn!(%err, "autostart request failed");
                        toasts.add_toast(adw::Toast::new("Couldn't change starting at login"));
                        false
                    }
                    Err(_) => {
                        tracing::warn!("autostart request got no answer");
                        toasts.add_toast(adw::Toast::new("Couldn't change starting at login"));
                        false
                    }
                };
                let _ = settings.set_boolean("autostart-requested", enabled);
                row.set_active(enabled);
                row.set_sensitive(true);
            });
        }
    });
    row.connect_active_notify({
        let (request, settings) = (request.clone(), settings.clone());
        move |row| {
            if row.is_sensitive() && row.is_active() != settings.boolean("autostart-requested") {
                request(row.is_active());
            }
        }
    });
    // Asked once on first launch, since nothing runs after a restart without it.
    if settings.user_value("autostart-requested").is_none() {
        request(true);
    }
    group.add(&row);
    group
}
