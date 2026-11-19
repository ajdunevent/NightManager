use adw::prelude::*;
use chrono::{NaiveTime, Timelike};

/// A row with an on/off switch and a time picked from hour and minute spin buttons.
pub fn time_row(
    title: &str,
    time: Option<NaiveTime>,
    on_change: impl Fn(Option<NaiveTime>) + 'static,
) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(title).build();
    let initial = time.unwrap_or_else(|| NaiveTime::from_hms_opt(20, 0, 0).expect("valid time"));

    let hour = spin(23.0, initial.hour());
    let minute = spin(59.0, initial.minute());
    let picker = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(6)
        .margin_end(6)
        .build();
    picker.append(&hour);
    picker.append(&gtk::Label::new(Some(":")));
    picker.append(&minute);
    let popover = gtk::Popover::builder().child(&picker).build();

    let button = gtk::MenuButton::builder()
        .popover(&popover)
        .valign(gtk::Align::Center)
        .css_classes(["flat", "numeric"])
        .build();
    let toggle = gtk::Switch::builder()
        .valign(gtk::Align::Center)
        .active(time.is_some())
        .build();
    row.add_suffix(&button);
    row.add_suffix(&toggle);

    let current = {
        let (hour, minute) = (hour.clone(), minute.clone());
        move || NaiveTime::from_hms_opt(hour.value() as u32, minute.value() as u32, 0)
    };
    let refresh = {
        let (button, toggle, current) = (button.clone(), toggle.clone(), current.clone());
        move || {
            let on = toggle.is_active();
            button.set_sensitive(on);
            button.set_label(&if on {
                current().map(|t| t.format("%H:%M").to_string()).unwrap_or_default()
            } else {
                "Off".to_string()
            });
        }
    };
    refresh();

    let on_change = std::rc::Rc::new(on_change);
    let emit = {
        let (toggle, refresh, on_change) = (toggle.clone(), refresh.clone(), on_change.clone());
        move || {
            refresh();
            on_change(toggle.is_active().then(&current).flatten());
        }
    };
    toggle.connect_active_notify({
        let emit = emit.clone();
        move |_| emit()
    });
    // Written once the picker closes, so scrolling through values doesn't write
    // every step.
    popover.connect_closed(move |_| emit());
    for s in [&hour, &minute] {
        let refresh = refresh.clone();
        s.connect_value_changed(move |_| refresh());
    }
    row
}

fn spin(max: f64, value: u32) -> gtk::SpinButton {
    let s = gtk::SpinButton::with_range(0.0, max, 1.0);
    s.set_wrap(true);
    s.set_numeric(true);
    s.set_orientation(gtk::Orientation::Vertical);
    s.set_value(value as f64);
    s.connect_output(|s| {
        s.set_text(&format!("{:02}", s.value() as u32));
        glib::Propagation::Stop
    });
    s
}
