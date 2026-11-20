//! The At Night group.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use nightmanager::capabilities::Capabilities;
use nightmanager::themes;

use crate::pictures;
use crate::rows::{self, Choice};

pub fn group(settings: &gio::Settings) -> adw::PreferencesGroup {
    let s = settings;
    let group = adw::PreferencesGroup::builder().title("At Night").build();

    let night_light = rows::switch(s, "night-light", "Night Light");
    let ambient = rows::switch(s, "ambient-light", "Automatic Brightness");
    let dark = rows::switch(s, "dark-style", "Dark Style");
    let accent = Choice::bound(
        s,
        "accent-color",
        "Accent Color",
        rows::with_no_change(themes::accent_colors(), rows::capitalize),
    );
    let gtk3 = Choice::bound(
        s,
        "gtk-theme",
        "Legacy Theme (GTK3)",
        rows::with_no_change(themes::gtk3_themes(), str::to_string),
    );
    let icons = Choice::bound(
        s,
        "icon-theme",
        "Icon Theme",
        rows::with_no_change(themes::icon_themes(), str::to_string),
    );
    let feedback = Choice::bound(
        s,
        "feedback-level",
        "Feedback",
        vec![
            (String::new(), "No Change".into()),
            ("quiet".into(), "Quiet".into()),
            ("silent".into(), "Silent".into()),
        ],
    );
    let keep = rows::switch(s, "save-theme-changes", "Keep Theme Changes Made at Night");
    keep.set_subtitle("Accent, theme and icon changes made at night become the night values");

    accent.row.set_visible(!themes::accent_colors().is_empty());
    group.add(&night_light);
    group.add(&ambient);
    group.add(&dark);
    group.add(&accent.row);
    group.add(&gtk3.row);
    group.add(&icons.row);
    group.add(&feedback.row);
    picture_rows(&group, s, "wallpaper", "Wallpaper", "wallpaper-image");
    picture_rows(&group, s, "lock-screen", "Lock Screen", "lock-screen-image");
    group.add(&keep);

    let feedback = feedback.row.clone();
    glib::spawn_future_local(async move {
        let caps = Capabilities::detect_async().await;
        night_light.set_visible(caps.night_light);
        ambient.set_visible(caps.ambient_light);
        feedback.set_visible(caps.feedback);
    });
    group
}

fn picture_rows(
    group: &adw::PreferencesGroup,
    s: &gio::Settings,
    mode_key: &'static str,
    title: &str,
    image_key: &'static str,
) {
    let mode = Choice::bound(
        s,
        mode_key,
        title,
        vec![
            (String::new(), "No Change".into()),
            ("image".into(), "Picture".into()),
            ("blank".into(), "No Picture".into()),
        ],
    );
    let row = adw::ActionRow::builder()
        .title("Picture")
        .subtitle_lines(1)
        .build();
    let button = gtk::Button::builder()
        .label("Choose…")
        .valign(gtk::Align::Center)
        .build();
    row.add_suffix(&button);
    let describe = |row: &adw::ActionRow, uri: &str| {
        row.set_subtitle(&if uri.is_empty() { "None chosen".into() } else { pictures::display_name(uri) });
    };
    describe(&row, &s.string(image_key));
    s.connect_changed(Some(image_key), {
        let row = row.clone();
        move |s, k| describe(&row, &s.string(k))
    });

    let choosing = Rc::new(Cell::new(false));
    button.connect_clicked({
        let s = s.clone();
        move |button| {
            if choosing.replace(true) {
                return;
            }
            button.set_sensitive(false);
            let (s, button, choosing) = (s.clone(), button.clone(), choosing.clone());
            glib::spawn_future_local(async move {
                if let Some(uri) = pictures::choose(pictures::window_of(&button)).await {
                    let _ = s.set_string(image_key, &uri);
                }
                button.set_sensitive(true);
                choosing.set(false);
            });
        }
    });
    rows::show_when(s, mode_key, "image", &row);
    group.add(&mode.row);
    group.add(&row);
}
