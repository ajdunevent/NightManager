//! The Schedule group.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use chrono::NaiveTime;
use nightmanager::config::{format_time, parse_time};
use nightmanager::keys::COLOR_SCHEMA;
use nightmanager::schedule::hours_to_time;
use nightmanager::status::DAYS;

use crate::rows::{self, Choice};
use crate::time_row::time_row;

pub fn group(settings: &gio::Settings) -> adw::PreferencesGroup {
    let s = settings;
    let group = adw::PreferencesGroup::builder().title("Schedule").build();
    let source = Choice::bound(
        s,
        "schedule-source",
        "Times From",
        vec![
            ("custom".into(), "Custom".into()),
            ("night-light".into(), "Night Light Schedule".into()),
        ],
    );
    let starts = times_expander(s, "Starts", "evening-times", "evening-same");
    let ends = times_expander(s, "Ends", "morning-times", "morning-same");
    let follows = adw::ActionRow::builder().title("Night Light Schedule").build();

    let color = gio::SettingsSchemaSource::default()
        .and_then(|src| src.lookup(COLOR_SCHEMA, true))
        .map(|_| gio::Settings::new(COLOR_SCHEMA));
    let describe = {
        let (follows, color) = (follows.clone(), color.clone());
        move || {
            let text = match &color {
                Some(c) if c.boolean("night-light-schedule-automatic") => "Sunset to sunrise".to_string(),
                Some(c) => format!(
                    "{} to {}",
                    format_time(hours_to_time(c.double("night-light-schedule-from"))),
                    format_time(hours_to_time(c.double("night-light-schedule-to")))
                ),
                None => "Night Light isn't available".to_string(),
            };
            follows.set_subtitle(&text);
        }
    };
    describe();
    if let Some(c) = &color {
        c.connect_changed(None, move |_, _| describe());
    }

    rows::show_when(s, "schedule-source", "custom", &starts);
    rows::show_when(s, "schedule-source", "custom", &ends);
    rows::show_when(s, "schedule-source", "night-light", &follows);

    group.add(&source.row);
    group.add(&starts);
    group.add(&ends);
    group.add(&follows);
    group
}

fn read_times(settings: &gio::Settings, key: &str) -> [Option<NaiveTime>; 7] {
    let mut out = [None; 7];
    for (slot, s) in out.iter_mut().zip(settings.strv(key).iter()) {
        *slot = parse_time(s);
    }
    out
}

fn write_times(settings: &gio::Settings, key: &str, times: &[Option<NaiveTime>; 7]) {
    let strv: Vec<String> = times.iter().map(|t| format_time(*t)).collect();
    if let Err(err) = settings.set_strv(key, strv) {
        tracing::warn!(%err, key, "saving times failed");
    }
}

/// An expander with a "Same Every Day" switch and either one time row or seven.
fn times_expander(
    settings: &gio::Settings,
    title: &str,
    times_key: &'static str,
    same_key: &'static str,
) -> adw::ExpanderRow {
    let expander = adw::ExpanderRow::builder().title(title).build();
    expander.add_row(&rows::switch(settings, same_key, "Same Every Day"));
    let rows: Rc<RefCell<Vec<adw::ActionRow>>> = Rc::default();

    let summary = {
        let (settings, expander) = (settings.clone(), expander.clone());
        move || {
            let times = read_times(&settings, times_key);
            let text = if times.iter().all(Option::is_none) {
                "Off".to_string()
            } else if times.iter().all(|t| *t == times[0]) {
                format!("{} every day", format_time(times[0]))
            } else {
                "Varies by day".to_string()
            };
            expander.set_subtitle(&text);
        }
    };

    let rebuild: Rc<dyn Fn()> = Rc::new({
        let (settings, expander, rows, summary) =
            (settings.clone(), expander.clone(), rows.clone(), summary.clone());
        move || {
            for row in rows.borrow_mut().drain(..) {
                expander.remove(&row);
            }
            let times = read_times(&settings, times_key);
            let mut new_rows = Vec::new();
            if settings.boolean(same_key) {
                let s = settings.clone();
                new_rows.push(time_row("Every Day", times[0], move |t| {
                    write_times(&s, times_key, &[t; 7]);
                }));
            } else {
                for (i, day) in DAYS.iter().enumerate() {
                    let s = settings.clone();
                    new_rows.push(time_row(day, times[i], move |t| {
                        let mut all = read_times(&s, times_key);
                        all[i] = t;
                        write_times(&s, times_key, &all);
                    }));
                }
            }
            for row in &new_rows {
                expander.add_row(row);
            }
            *rows.borrow_mut() = new_rows;
            summary();
        }
    });
    rebuild();

    settings.connect_changed(Some(same_key), move |s, k| {
        if s.boolean(k) {
            let times = read_times(s, times_key);
            let first = times.iter().flatten().next().copied();
            write_times(s, times_key, &[first; 7]);
        }
        rebuild();
    });
    settings.connect_changed(Some(times_key), move |_, _| summary());
    expander
}
