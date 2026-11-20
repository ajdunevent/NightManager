//! The Night Light While the Screen Is Off group.

use adw::prelude::*;
use nightmanager::capabilities::Capabilities;

pub fn group(settings: &gio::Settings) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Night Light While the Screen Is Off")
        .description("On some phones, changing Night Light with the screen off leaves it stuck until a reboot.")
        .visible(false)
        .build();
    let options = [
        ("wake", "Turn the Screen On Briefly", "Changes on time; the screen lights up for about two seconds"),
        ("wait", "Wait for the Screen", "Changes the next time the screen turns on"),
    ];
    let mut first: Option<gtk::CheckButton> = None;
    for (value, title, subtitle) in options {
        let check = gtk::CheckButton::builder().valign(gtk::Align::Center).build();
        check.set_group(first.as_ref());
        first.get_or_insert_with(|| check.clone());
        check.set_active(settings.string("screen-off-mode") == value);
        let row = adw::ActionRow::builder()
            .title(title)
            .subtitle(subtitle)
            .activatable_widget(&check)
            .build();
        row.add_prefix(&check);
        check.connect_toggled({
            let settings = settings.clone();
            move |c| {
                if c.is_active() && settings.string("screen-off-mode") != value {
                    let _ = settings.set_string("screen-off-mode", value);
                }
            }
        });
        settings.connect_changed(Some("screen-off-mode"), move |s, k| {
            if s.string(k) == value && !check.is_active() {
                check.set_active(true);
            }
        });
        group.add(&row);
    }
    let shown = group.clone();
    glib::spawn_future_local(async move {
        shown.set_visible(Capabilities::detect_async().await.night_light);
    });
    group
}
