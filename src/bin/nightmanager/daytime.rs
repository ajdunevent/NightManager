//! Without the daemon, values are written to the host directly, for recovery
//! when the daemon won't start.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use nightmanager::capabilities::Capabilities;
use nightmanager::keys::HostKey;
use nightmanager::themes;

use crate::client::Client;
use crate::rows::{self, Choice};

struct Day {
    key: HostKey,
    value: glib::Variant,
    at_morning: bool,
}

struct Row {
    key: HostKey,
    widget: adw::ActionRow,
    update: Box<dyn Fn(&glib::Variant)>,
}

struct Dialog {
    client: Option<Client>,
    group: adw::PreferencesGroup,
    rows: RefCell<Vec<Row>>,
    /// Set while rows show outside changes, so their handlers don't write them back.
    updating: Rc<Cell<bool>>,
    refresh_queued: Cell<bool>,
}

pub fn show(parent: &adw::ApplicationWindow, client: Option<Client>) {
    let parent = parent.clone();
    glib::spawn_future_local(async move {
        let fetched = match &client {
            Some(c) => from_daemon(c).await,
            None => None,
        };
        let (days, client) = match fetched {
            Some(days) => (days, client),
            None => (from_host().await, None),
        };
        build(days, client).present(Some(&parent));
    });
}

async fn from_daemon(client: &Client) -> Option<Vec<Day>> {
    let reply = client.call_timeout("GetDaytime", None, 3000).await?;
    let (items,) = reply.get::<(Vec<(String, glib::Variant, bool)>,)>()?;
    Some(
        items
            .into_iter()
            .filter_map(|(name, value, at_morning)| {
                let key = HostKey::from_name(&name).filter(|k| shown(*k))?;
                Some(Day {
                    key,
                    value,
                    at_morning,
                })
            })
            .collect(),
    )
}

async fn from_host() -> Vec<Day> {
    let caps = Capabilities::detect_async().await;
    HostKey::ALL
        .into_iter()
        .filter(|key| caps.allows(*key) && shown(*key))
        .filter_map(|key| {
            Some(Day {
                key,
                value: host_settings(key)?.value(key.key()),
                at_morning: false,
            })
        })
        .collect()
}

fn shown(key: HostKey) -> bool {
    !matches!(key, HostKey::Wallpaper | HostKey::WallpaperDark | HostKey::LockScreen)
}

fn host_settings(key: HostKey) -> Option<gio::Settings> {
    let schema = gio::SettingsSchemaSource::default()?.lookup(key.schema(), true)?;
    schema
        .has_key(key.key())
        .then(|| gio::Settings::new(key.schema()))
}

fn build(days: Vec<Day>, client: Option<Client>) -> adw::PreferencesDialog {
    let dialog = adw::PreferencesDialog::builder().title("Daytime Settings").build();
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::new();
    page.add(&group);
    dialog.add(&page);

    let d = Rc::new(Dialog {
        client,
        group,
        rows: RefCell::default(),
        updating: Rc::default(),
        refresh_queued: Cell::new(false),
    });
    for day in &days {
        let row = make_row(day, &d);
        d.group.add(&row.widget);
        d.rows.borrow_mut().push(row);
    }
    d.show_days(&days);
    d.watch(&dialog);
    dialog
}

impl Dialog {
    fn show_days(&self, days: &[Day]) {
        self.updating.set(true);
        for day in days {
            if let Some(row) = self.rows.borrow().iter().find(|r| r.key == day.key) {
                (row.update)(&day.value);
                row.widget
                    .set_subtitle(if day.at_morning { "Restored when night ends" } else { "" });
            }
        }
        self.updating.set(false);
        self.group.set_description(Some(if self.client.is_none() {
            "The background service isn't running. Changes apply now."
        } else if days.iter().any(|d| d.at_morning) {
            "Night mode is on. Settings it changed are restored with these values when it ends; the rest apply now."
        } else {
            "Changes apply now."
        }));
    }

    fn queue_refresh(self: &Rc<Self>) {
        if self.refresh_queued.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_millis(150), move || {
            let Some(d) = weak.upgrade() else { return };
            d.refresh_queued.set(false);
            glib::spawn_future_local(async move {
                let days = match &d.client {
                    Some(c) => from_daemon(c).await,
                    None => Some(from_host().await),
                };
                if let Some(days) = days {
                    d.show_days(&days);
                }
            });
        });
    }

    fn watch(self: &Rc<Self>, dialog: &adw::PreferencesDialog) {
        let mut watched: Vec<(gio::Settings, glib::SignalHandlerId)> = Vec::new();
        let mut schemas: Vec<&str> = Vec::new();
        for row in self.rows.borrow().iter() {
            if schemas.contains(&row.key.schema()) {
                continue;
            }
            schemas.push(row.key.schema());
            if let Some(settings) = host_settings(row.key) {
                let weak = Rc::downgrade(self);
                let schema = row.key.schema();
                let id = settings.connect_changed(None, move |_, key| {
                    if HostKey::from_schema_key(schema, key).is_some() {
                        if let Some(d) = weak.upgrade() {
                            d.queue_refresh();
                        }
                    }
                });
                watched.push((settings, id));
            }
        }
        let signal = self.client.as_ref().map(|c| {
            let weak = Rc::downgrade(self);
            c.connect_signal("DaytimeChanged", move || {
                if let Some(d) = weak.upgrade() {
                    d.queue_refresh();
                }
            })
        });
        let watched = RefCell::new(Some((watched, signal)));
        let this = self.clone();
        dialog.connect_closed(move |_| {
            if let Some((settings, signal)) = watched.take() {
                for (s, id) in settings {
                    s.disconnect(id);
                }
                if let (Some(c), Some(id)) = (&this.client, signal) {
                    c.disconnect(id);
                }
            }
        });
    }

    fn apply(&self, key: HostKey, value: glib::Variant) {
        if self.updating.get() {
            return;
        }
        match &self.client {
            Some(client) => {
                let client = client.clone();
                glib::spawn_future_local(async move {
                    let args = (key.name(), value).to_variant();
                    client.call_with("SetDaytime", Some(&args)).await;
                });
            }
            None => {
                if let Some(s) = host_settings(key) {
                    if let Err(err) = s.set_value(key.key(), &value) {
                        tracing::warn!(?key, %err, "write failed");
                    }
                }
            }
        }
    }
}

fn make_row(day: &Day, d: &Rc<Dialog>) -> Row {
    let key = day.key;
    let weak = Rc::downgrade(d);
    let apply = move |value: glib::Variant| {
        if let Some(d) = weak.upgrade() {
            d.apply(key, value);
        }
    };

    if day.value.get::<bool>().is_some() {
        let title = match key {
            HostKey::NightLight => "Night Light",
            _ => "Automatic Brightness",
        };
        let row = adw::SwitchRow::builder().title(title).build();
        row.connect_active_notify(move |row| apply(row.is_active().to_variant()));
        let r = row.clone();
        return Row {
            key,
            widget: row.upcast(),
            update: Box::new(move |v| r.set_active(v.get::<bool>().unwrap_or(false))),
        };
    }

    let (title, options): (&str, Vec<(String, String)>) = match key {
        HostKey::ColorScheme => (
            "Style",
            vec![
                ("default".into(), "Default".into()),
                ("prefer-dark".into(), "Dark".into()),
                ("prefer-light".into(), "Light".into()),
            ],
        ),
        HostKey::AccentColor => ("Accent Color", rows::labeled(themes::accent_colors(), rows::capitalize)),
        HostKey::GtkTheme => ("Legacy Theme (GTK3)", rows::labeled(themes::gtk3_themes(), str::to_string)),
        HostKey::IconTheme => ("Icon Theme", rows::labeled(themes::icon_themes(), str::to_string)),
        _ => (
            "Feedback",
            rows::labeled(vec!["full".into(), "quiet".into(), "silent".into()], rows::capitalize),
        ),
    };
    let choice = Choice::new(title, options, move |v| apply(v.to_variant()));
    Row {
        key,
        widget: choice.row.clone().upcast(),
        update: Box::new(move |v| choice.set(v.str().unwrap_or_default())),
    }
}
