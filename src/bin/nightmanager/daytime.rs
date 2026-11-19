//! The Daytime Settings dialog: the settings night changes, with their day
//! values. Through the daemon, a value night changed waits for the morning
//! restore and anything else applies now. Without the daemon, values are
//! written to the host directly so the dialog still works for recovery.

use std::rc::Rc;

use adw::prelude::*;
use nightmanager::capabilities::Capabilities;
use nightmanager::keys::HostKey;
use nightmanager::themes;

use crate::client::Client;

/// One setting as the dialog shows it.
struct Day {
    key: HostKey,
    value: glib::Variant,
    at_morning: bool,
}

pub fn show(parent: &adw::ApplicationWindow, client: Option<Client>) {
    let parent = parent.clone();
    glib::spawn_future_local(async move {
        let fetched = match &client {
            Some(c) => from_daemon(c).await,
            None => None,
        };
        // A daemon that is missing or doesn't answer leaves the direct path.
        let (days, client) = match fetched {
            Some(days) => (days, client),
            None => (from_host().await, None),
        };
        build(&days, client).present(Some(&parent));
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

fn build(days: &[Day], client: Option<Client>) -> adw::PreferencesDialog {
    let dialog = adw::PreferencesDialog::builder().title("Daytime Settings").build();
    let page = adw::PreferencesPage::new();
    let waiting = days.iter().any(|d| d.at_morning);
    let group = adw::PreferencesGroup::builder()
        .description(if client.is_none() {
            "The background service isn't running. Changes apply now."
        } else if waiting {
            "Night mode is on. Settings it changed are restored with these values when it ends; the rest apply now."
        } else {
            "Changes apply now."
        })
        .build();
    for day in days {
        let row = row_for(day, client.clone());
        if day.at_morning {
            row.set_subtitle("Restored when night ends");
        }
        group.add(&row);
    }
    page.add(&group);
    dialog.add(&page);
    dialog
}

/// Sends a new day value to the daemon, or writes it directly without one.
fn apply(client: &Option<Client>, key: HostKey, value: glib::Variant) {
    match client {
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

fn row_for(day: &Day, client: Option<Client>) -> adw::ActionRow {
    let key = day.key;
    if let Some(on) = day.value.get::<bool>() {
        let title = match key {
            HostKey::NightLight => "Night Light",
            _ => "Automatic Brightness",
        };
        let row = adw::SwitchRow::builder().title(title).active(on).build();
        row.connect_active_notify(move |row| apply(&client, key, row.is_active().to_variant()));
        return row.upcast();
    }

    let current = day.value.get::<String>().unwrap_or_default();
    let (title, mut options): (&str, Vec<(String, String)>) = match key {
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
    if !options.iter().any(|(v, _)| *v == current) {
        options.push((current.clone(), current.clone()));
    }
    let labels: Vec<&str> = options.iter().map(|(_, l)| l.as_str()).collect();
    let row = adw::ComboRow::builder()
        .title(title)
        .model(&gtk::StringList::new(&labels))
        .build();
    if let Some(i) = options.iter().position(|(v, _)| *v == current) {
        row.set_selected(i as u32);
    }
    let values: Rc<Vec<String>> = Rc::new(options.into_iter().map(|(v, _)| v).collect());
    row.connect_selected_notify(move |row| {
        if let Some(v) = values.get(row.selected() as usize) {
            apply(&client, key, v.to_variant());
        }
    });
    row.upcast()
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
