//! The Manual Triggers group.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use nightmanager::capabilities::Capabilities;
use nightmanager::config::{FeedbackProfile, FeedbackSet};

use crate::rows::{self, Choice};

pub fn group(settings: &gio::Settings, toasts: &adw::ToastOverlay) -> adw::PreferencesGroup {
    let s = settings;
    let group = adw::PreferencesGroup::builder()
        .title("Manual Triggers")
        .description(
            "Changing these from the quick settings starts or ends night mode until the next scheduled change.",
        )
        .build();
    group.add(&rows::switch(s, "trigger-dark-style", "Dark Style"));
    let night_light = rows::switch(s, "trigger-night-light", "Night Light");
    group.add(&night_light);

    // Rebuilt when the night feedback level changes: a trigger has to include it.
    let slot: Rc<RefCell<Option<Choice>>> = Rc::default();
    let populate: Rc<dyn Fn()> = Rc::new({
        let (s, group, slot, toasts) = (s.clone(), group.clone(), slot.clone(), toasts.clone());
        move || {
            let level = FeedbackProfile::parse(&s.string("feedback-level"));
            let current = FeedbackSet::from_strs(s.strv("trigger-feedback").iter().map(|x| x.as_str()));
            let mut sets = vec![FeedbackSet::OFF];
            let mut options = vec![(set_key(FeedbackSet::OFF), "Off".to_string())];
            for (set, label) in FeedbackSet::CHOICES {
                if set.compatible_with(level) {
                    sets.push(set);
                    options.push((set_key(set), label.to_string()));
                }
            }
            if !sets.contains(&current) {
                let _ = s.set_strv("trigger-feedback", Vec::<String>::new());
                toasts.add_toast(adw::Toast::new(
                    "Feedback trigger turned off: it has to include the night feedback level",
                ));
            }
            let writer = s.clone();
            let choice = Choice::new("Feedback", options, move |key| {
                let set = FeedbackSet::from_strs(key.split(',').filter(|x| !x.is_empty()));
                let now = FeedbackSet::from_strs(writer.strv("trigger-feedback").iter().map(|x| x.as_str()));
                if set != now {
                    let _ = writer.set_strv("trigger-feedback", set.to_strs());
                }
            });
            choice.row.set_subtitle("Profiles that mean night");
            choice.set(&set_key(FeedbackSet::from_strs(
                s.strv("trigger-feedback").iter().map(|x| x.as_str()),
            )));
            if let Some(old) = slot.borrow_mut().replace(choice.clone()) {
                choice.row.set_visible(old.row.is_visible());
                group.remove(&old.row);
            }
            group.add(&choice.row);
        }
    });
    populate();
    s.connect_changed(Some("feedback-level"), {
        let populate = populate.clone();
        move |_, _| populate()
    });
    s.connect_changed(Some("trigger-feedback"), {
        let slot = slot.clone();
        move |s, k| {
            if let Some(c) = slot.borrow().as_ref() {
                c.set(&set_key(FeedbackSet::from_strs(s.strv(k).iter().map(|x| x.as_str()))));
            }
        }
    });

    glib::spawn_future_local(async move {
        let caps = Capabilities::detect_async().await;
        night_light.set_visible(caps.night_light);
        if let Some(c) = slot.borrow().as_ref() {
            c.row.set_visible(caps.feedback);
        }
    });
    group
}

fn set_key(set: FeedbackSet) -> String {
    set.to_strs().join(",")
}
