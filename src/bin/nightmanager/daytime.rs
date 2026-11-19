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
use crate::pictures;

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
    let reply = client.call("GetDaytime").await?;
    let (items,) = reply.get::<(Vec<(String, glib::Variant, bool)>,)>()?;
    Some(
        items
            .into_iter()
            .filter_map(|(name, value, at_morning)| {
                Some(Day {
                    key: HostKey::from_name(&name)?,
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
        .filter(|key| caps.allows(*key))
        .filter_map(|key| {
            Some(Day {
                key,
                value: host_settings(key)?.value(key.key()),
                at_morning: false,
            })
        })
        .collect()
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

    match key {
        HostKey::Wallpaper | HostKey::WallpaperDark | HostKey::LockScreen => {
            return picture_row(key, apply);
        }
        _ => {}
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
        HostKey::AccentColor => ("Accent Color", labeled(themes::accent_colors(), capitalize)),
        HostKey::GtkTheme => ("Legacy Theme (GTK3)", labeled(themes::gtk3_themes(), str::to_string)),
        HostKey::IconTheme => ("Icon Theme", labeled(themes::icon_themes(), str::to_string)),
        _ => (
            "Feedback",
            labeled(vec!["full".into(), "quiet".into(), "silent".into()], capitalize),
        ),
    };
    let list = gtk::StringList::new(&options.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>());
    let values: Rc<RefCell<Vec<String>>> =
        Rc::new(RefCell::new(options.into_iter().map(|(v, _)| v).collect()));
    let row = adw::ComboRow::builder().title(title).model(&list).build();
    row.connect_selected_notify({
        let values = values.clone();
        move |row| {
            if let Some(v) = values.borrow().get(row.selected() as usize) {
                apply(v.to_variant());
            }
        }
    });
    let r = row.clone();
    Row {
        key,
        widget: row.upcast(),
        update: Box::new(move |v| {
            let current = v.str().unwrap_or_default().to_string();
            let index = values.borrow().iter().position(|x| *x == current);
            let index = index.unwrap_or_else(|| {
                // Themes installed after the dialog opened aren't in the list.
                list.append(&current);
                values.borrow_mut().push(current);
                values.borrow().len() - 1
            });
            if r.selected() != index as u32 {
                r.set_selected(index as u32);
            }
        }),
    }
}

fn picture_row(key: HostKey, apply: impl Fn(glib::Variant) + Clone + 'static) -> Row {
    let (title, role) = match key {
        HostKey::Wallpaper => ("Wallpaper", "day-wallpaper"),
        HostKey::WallpaperDark => ("Dark Style Wallpaper", "day-wallpaper-dark"),
        _ => ("Lock Screen", "day-lock-screen"),
    };
    let row = adw::ActionRow::builder().title(title).build();
    let thumb = pictures::Thumb::new();
    let choose = gtk::Button::builder()
        .label("Choose…")
        .valign(gtk::Align::Center)
        .build();
    let clear = gtk::Button::builder()
        .icon_name("edit-clear-symbolic")
        .tooltip_text("No Picture")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    row.add_suffix(&thumb.none);
    row.add_suffix(&thumb.frame);
    row.add_suffix(&choose);
    row.add_suffix(&clear);
    choose.connect_clicked({
        let apply = apply.clone();
        move |button| {
            let (apply, parent) = (apply.clone(), pictures::window_of(button));
            glib::spawn_future_local(async move {
                if let Some(uri) = pictures::choose(parent, role).await {
                    apply(uri.to_variant());
                }
            });
        }
    });
    clear.connect_clicked(move |_| apply("".to_variant()));
    Row {
        key,
        widget: row,
        update: Box::new(move |v| thumb.show(v.str().unwrap_or_default())),
    }
}

fn labeled(items: Vec<String>, label: fn(&str) -> String) -> Vec<(String, String)> {
    items.into_iter().map(|i| (i.clone(), label(&i))).collect()
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}
