use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use chrono::{DateTime, Datelike, Local, NaiveTime};
use nightmanager::config::{
    format_time, parse_time, FeedbackProfile, FeedbackSet, APP_ID,
};
use nightmanager::keys::COLOR_SCHEMA;
use nightmanager::schedule::hours_to_time;
use nightmanager::{sandbox, themes};

use nightmanager::capabilities::Capabilities;

use crate::client::{Client, Status};
use crate::time_row::time_row;

const DAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

struct Ui {
    settings: gio::Settings,
    client: RefCell<Option<Client>>,
    toasts: adw::ToastOverlay,
    banner: adw::Banner,
    page: adw::PreferencesPage,
    status_row: adw::SwitchRow,
    updating_status: std::cell::Cell<bool>,
    reload_source: RefCell<Option<glib::SourceId>>,
}

impl Ui {
    fn toast(&self, text: &str) {
        self.toasts.add_toast(adw::Toast::new(text));
    }

    fn client(&self) -> Option<Client> {
        self.client.borrow().clone()
    }

    fn call(&self, method: &'static str) {
        if let Some(client) = self.client() {
            glib::spawn_future_local(async move {
                client.call(method).await;
            });
        }
    }

    fn queue_reload(self: &Rc<Self>) {
        if let Some(id) = self.reload_source.borrow_mut().take() {
            id.remove();
        }
        let ui = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(Duration::from_millis(300), move || {
            if let Some(ui) = ui.upgrade() {
                ui.reload_source.borrow_mut().take();
                ui.call("Reload");
            }
        });
        *self.reload_source.borrow_mut() = Some(id);
    }

    fn show_status(&self, status: Status) {
        self.updating_status.set(true);
        self.status_row.set_active(status.active);
        self.updating_status.set(false);
        self.status_row.set_subtitle(&describe_next(status));
        self.banner.set_revealed(status.legacy_installed);
        self.page.set_sensitive(!status.legacy_installed);
    }
}

fn describe_next(status: Status) -> String {
    if status.next_change == 0 {
        return if status.active {
            "On until turned off".into()
        } else {
            "No scheduled start".into()
        };
    }
    let Some(at) = DateTime::from_timestamp(status.next_change, 0) else {
        return String::new();
    };
    let at = at.with_timezone(&Local);
    let today = Local::now().date_naive();
    let day = match (at.date_naive() - today).num_days() {
        0 => String::new(),
        1 => " tomorrow".into(),
        _ => format!(" {}", DAYS[at.weekday().num_days_from_monday() as usize]),
    };
    let time = at.format("%H:%M");
    match (status.active, status.next_is_start) {
        (true, false) => format!("On until {time}{day}"),
        (false, true) => format!("Starts at {time}{day}"),
        (true, true) => format!("On; next start at {time}{day}"),
        (false, false) => format!("Off; next end at {time}{day}"),
    }
}

pub fn build(app: &adw::Application) -> adw::ApplicationWindow {
    let settings = gio::Settings::new(APP_ID);

    let status_row = adw::SwitchRow::builder()
        .title("Night Mode")
        .subtitle("Connecting…")
        .build();
    let banner = adw::Banner::builder()
        .title("Remove the old package to start: sudo apt remove nightmanager")
        .button_label("Check Again")
        .build();
    let page = adw::PreferencesPage::new();
    let toasts = adw::ToastOverlay::new();

    let ui = Rc::new(Ui {
        settings: settings.clone(),
        client: RefCell::default(),
        toasts: toasts.clone(),
        banner: banner.clone(),
        page: page.clone(),
        status_row: status_row.clone(),
        updating_status: std::cell::Cell::new(false),
        reload_source: RefCell::default(),
    });

    let status_group = adw::PreferencesGroup::new();
    status_group.add(&status_row);
    page.add(&status_group);
    page.add(&night_group(&ui));
    page.add(&schedule_group(&ui));
    page.add(&triggers_group(&ui));
    page.add(&screen_group(&ui));
    page.add(&background_group(&ui));

    status_row.connect_active_notify({
        let ui = ui.clone();
        move |row| {
            if !ui.updating_status.get() {
                ui.call(if row.is_active() { "StartNight" } else { "EndNight" });
            }
        }
    });
    banner.connect_button_clicked({
        let ui = ui.clone();
        move |_| {
            let Some(client) = ui.client() else { return };
            let ui = ui.clone();
            glib::spawn_future_local(async move {
                let installed = client
                    .call("CheckLegacy")
                    .await
                    .and_then(|v| v.get::<(bool,)>())
                    .is_none_or(|(b,)| b);
                if installed {
                    ui.toast("The old package is still installed");
                }
            });
        }
    });
    settings.connect_changed(None, {
        let ui = ui.clone();
        move |_, _| ui.queue_reload()
    });

    let primary_menu = gio::Menu::new();
    primary_menu.append(Some("Daytime Settings…"), Some("app.daytime"));
    primary_menu.append(Some("About NightManager"), Some("app.about"));
    primary_menu.append(Some("Quit"), Some("app.quit"));
    let menu_button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .menu_model(&primary_menu)
        .tooltip_text("Main Menu")
        .build();
    let header = adw::HeaderBar::new();
    header.pack_end(&menu_button);

    let content = adw::ToolbarView::new();
    content.add_top_bar(&header);
    content.add_top_bar(&banner);
    toasts.set_child(Some(&page));
    content.set_content(Some(&toasts));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("NightManager")
        .default_width(400)
        .default_height(720)
        .content(&content)
        .build();
    window.set_size_request(360, 294);
    add_actions(app, &window, &ui);

    glib::spawn_future_local({
        let ui = ui.clone();
        async move {
            match Client::connect().await {
                Ok(client) => {
                    ui.show_status(client.status());
                    let weak = Rc::downgrade(&ui);
                    client.connect_changed(move |status| {
                        if let Some(ui) = weak.upgrade() {
                            ui.show_status(status);
                        }
                    });
                    *ui.client.borrow_mut() = Some(client);
                }
                Err(err) => {
                    tracing::error!(%err, "daemon unavailable");
                    ui.status_row.set_subtitle("Background service unavailable");
                    ui.status_row.set_sensitive(false);
                }
            }
        }
    });
    window
}

fn add_actions(app: &adw::Application, window: &adw::ApplicationWindow, ui: &Rc<Ui>) {
    let daytime = gio::SimpleAction::new("daytime", None);
    daytime.connect_activate({
        let window = window.clone();
        let ui = Rc::downgrade(ui);
        move |_, _| {
            let client = ui.upgrade().and_then(|ui| ui.client());
            crate::daytime::show(&window, client);
        }
    });
    app.add_action(&daytime);

    let about = gio::SimpleAction::new("about", None);
    about.connect_activate({
        let window = window.clone();
        move |_, _| show_about(&window)
    });
    app.add_action(&about);

    // Quit also stops the daemon; opening the app or logging in starts it.
    let quit = gio::SimpleAction::new("quit", None);
    quit.connect_activate({
        let app = app.clone();
        let ui = Rc::downgrade(ui);
        move |_, _| {
            let app = app.clone();
            let client = ui.upgrade().and_then(|ui| ui.client());
            glib::spawn_future_local(async move {
                if let Some(client) = client {
                    client.call("Quit").await;
                }
                app.quit();
            });
        }
    });
    app.add_action(&quit);
    app.set_accels_for_action("app.quit", &["<Control>q"]);
}

fn show_about(parent: &adw::ApplicationWindow) {
    let about = adw::AboutWindow::builder()
        .transient_for(parent)
        .modal(true)
        .application_name("NightManager")
        .application_icon(APP_ID)
        .version(env!("CARGO_PKG_VERSION"))
        .developer_name("NightManager contributors")
        .developers(["AJ Dunevent"])
        .copyright("\u{a9} 2026 NightManager contributors")
        .license_type(gtk::License::Gpl30)
        .comments("Switch to night mode on a schedule.")
        .website("https://dev.dunevent.net/aj/NightManager")
        .issue_url("https://dev.dunevent.net/aj/NightManager/issues")
        .build();
    about.add_link("Donate", "https://dunevent.net/donate");
    about.add_acknowledgement_section(
        Some("With thanks to"),
        &["FuriLabs — for the FLX1s"],
    );
    about.present();
}

fn switch_row(settings: &gio::Settings, key: &str, title: &str, subtitle: Option<&str>) -> adw::SwitchRow {
    let row = adw::SwitchRow::builder().title(title).build();
    if let Some(s) = subtitle {
        row.set_subtitle(s);
    }
    settings.bind(key, &row, "active").build();
    row
}

/// A combo row over string values; `""` is "No Change". A configured value that
/// isn't in `options` (an uninstalled theme, say) stays selectable.
fn choice_row(
    settings: &gio::Settings,
    key: &'static str,
    title: &str,
    mut options: Vec<(String, String)>,
) -> adw::ComboRow {
    let current = settings.string(key).to_string();
    if !options.iter().any(|(v, _)| *v == current) {
        options.push((current.clone(), current.clone()));
    }
    let labels: Vec<&str> = options.iter().map(|(_, l)| l.as_str()).collect();
    let row = adw::ComboRow::builder()
        .title(title)
        .model(&gtk::StringList::new(&labels))
        .build();
    let values: Rc<Vec<String>> = Rc::new(options.into_iter().map(|(v, _)| v).collect());
    let select = {
        let (row, values) = (row.clone(), values.clone());
        move |value: &str| {
            if let Some(i) = values.iter().position(|v| v == value) {
                if row.selected() != i as u32 {
                    row.set_selected(i as u32);
                }
            }
        }
    };
    select(&current);
    row.connect_selected_notify({
        let (settings, values) = (settings.clone(), values.clone());
        move |row| {
            if let Some(v) = values.get(row.selected() as usize) {
                if settings.string(key) != v.as_str() {
                    let _ = settings.set_string(key, v);
                }
            }
        }
    });
    settings.connect_changed(Some(key), move |s, k| select(&s.string(k)));
    row
}

fn with_no_change(items: Vec<String>, label: fn(&str) -> String) -> Vec<(String, String)> {
    std::iter::once((String::new(), "No Change".to_string()))
        .chain(items.into_iter().map(|i| (label(&i), i)).map(|(l, i)| (i, l)))
        .collect()
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

fn night_group(ui: &Rc<Ui>) -> adw::PreferencesGroup {
    let s = &ui.settings;
    let group = adw::PreferencesGroup::builder().title("At Night").build();

    let night_light = switch_row(s, "night-light", "Night Light", None);
    let ambient = switch_row(s, "ambient-light", "Automatic Brightness", None);
    let dark = switch_row(s, "dark-style", "Dark Style", None);
    let accent = choice_row(
        s,
        "accent-color",
        "Accent Color",
        with_no_change(themes::accent_colors(), capitalize),
    );
    let gtk3 = choice_row(
        s,
        "gtk-theme",
        "Legacy Theme (GTK3)",
        with_no_change(themes::gtk3_themes(), str::to_string),
    );
    let icons = choice_row(
        s,
        "icon-theme",
        "Icon Theme",
        with_no_change(themes::icon_themes(), str::to_string),
    );
    let feedback = choice_row(
        s,
        "feedback-level",
        "Feedback",
        vec![
            (String::new(), "No Change".into()),
            ("quiet".into(), "Quiet".into()),
            ("silent".into(), "Silent".into()),
        ],
    );
    let (wallpaper, wallpaper_picture) =
        picture_rows(s, "wallpaper", "Wallpaper", "wallpaper-image", "night-wallpaper");
    let (lock, lock_picture) =
        picture_rows(s, "lock-screen", "Lock Screen", "lock-screen-image", "night-lock-screen");
    let keep = switch_row(
        s,
        "save-theme-changes",
        "Keep Theme Changes Made at Night",
        Some("Accent, theme and icon changes made at night become the night values"),
    );

    accent.set_visible(!themes::accent_colors().is_empty());
    for row in [
        night_light.upcast_ref::<gtk::Widget>(),
        ambient.upcast_ref(),
        dark.upcast_ref(),
        accent.upcast_ref(),
        gtk3.upcast_ref(),
        icons.upcast_ref(),
        feedback.upcast_ref(),
        wallpaper.upcast_ref(),
        wallpaper_picture.upcast_ref(),
        lock.upcast_ref(),
        lock_picture.upcast_ref(),
        keep.upcast_ref(),
    ] {
        group.add(row);
    }

    glib::spawn_future_local(async move {
        let caps = Capabilities::detect_async().await;
        night_light.set_visible(caps.night_light);
        ambient.set_visible(caps.ambient_light);
        feedback.set_visible(caps.feedback);
    });
    group
}

fn picture_row(settings: &gio::Settings, uri_key: &'static str, role: &'static str) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title("Picture").build();
    let thumb = crate::pictures::Thumb::new();
    let button = gtk::Button::builder()
        .label("Choose…")
        .valign(gtk::Align::Center)
        .build();
    row.add_suffix(&thumb.none);
    row.add_suffix(&thumb.frame);
    row.add_suffix(&button);
    thumb.show(&settings.string(uri_key));
    settings.connect_changed(Some(uri_key), move |s, k| thumb.show(&s.string(k)));
    button.connect_clicked({
        let settings = settings.clone();
        move |button| {
            let (settings, parent) = (settings.clone(), crate::pictures::window_of(button));
            glib::spawn_future_local(async move {
                if let Some(uri) = crate::pictures::choose(parent, role).await {
                    let _ = settings.set_string(uri_key, &uri);
                }
            });
        }
    });
    row
}

fn show_for_mode(settings: &gio::Settings, mode_key: &'static str, mode: &'static str, row: &impl IsA<gtk::Widget>) {
    let row = row.clone().upcast::<gtk::Widget>();
    row.set_visible(settings.string(mode_key) == mode);
    settings.connect_changed(Some(mode_key), move |s, k| row.set_visible(s.string(k) == mode));
}

fn picture_rows(
    s: &gio::Settings,
    mode_key: &'static str,
    title: &str,
    image_key: &'static str,
    role: &'static str,
) -> (adw::ComboRow, adw::ActionRow) {
    let mode = choice_row(
        s,
        mode_key,
        title,
        vec![
            (String::new(), "No Change".into()),
            ("image".into(), "Picture".into()),
            ("blank".into(), "No Picture".into()),
        ],
    );
    let picture = picture_row(s, image_key, role);
    show_for_mode(s, mode_key, "image", &picture);
    (mode, picture)
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
    let same = switch_row(settings, same_key, "Same Every Day", None);
    expander.add_row(&same);
    let rows: Rc<RefCell<Vec<adw::ActionRow>>> = Rc::default();

    let summary = {
        let (settings, expander) = (settings.clone(), expander.clone());
        move || {
            let times = read_times(&settings, times_key);
            let set: Vec<_> = times.iter().flatten().collect();
            let text = if set.is_empty() {
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

    settings.connect_changed(Some(same_key), {
        let rebuild = rebuild.clone();
        move |s, k| {
            if s.boolean(k) {
                let times = read_times(s, times_key);
                let first = times.iter().flatten().next().copied();
                write_times(s, times_key, &[first; 7]);
            }
            rebuild();
        }
    });
    settings.connect_changed(Some(times_key), move |_, _| summary());
    expander
}

fn schedule_group(ui: &Rc<Ui>) -> adw::PreferencesGroup {
    let s = &ui.settings;
    let group = adw::PreferencesGroup::builder().title("Schedule").build();
    let source = choice_row(
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
                Some(c) if c.boolean("night-light-schedule-automatic") => {
                    "Sunset to sunrise".to_string()
                }
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
        let describe = describe.clone();
        c.connect_changed(None, move |_, _| describe());
    }

    let show = {
        let (starts, ends, follows) = (starts.clone(), ends.clone(), follows.clone());
        move |s: &gio::Settings| {
            let custom = s.string("schedule-source") != "night-light";
            starts.set_visible(custom);
            ends.set_visible(custom);
            follows.set_visible(!custom);
        }
    };
    show(s);
    s.connect_changed(Some("schedule-source"), move |s, _| show(s));

    group.add(&source);
    group.add(&starts);
    group.add(&ends);
    group.add(&follows);
    group
}

fn triggers_group(ui: &Rc<Ui>) -> adw::PreferencesGroup {
    let s = &ui.settings;
    let group = adw::PreferencesGroup::builder()
        .title("Manual Triggers")
        .description(
            "Changing these from the quick settings starts or ends night mode until the next scheduled change.",
        )
        .build();
    group.add(&switch_row(s, "trigger-dark-style", "Dark Style", None));
    let night_light = switch_row(s, "trigger-night-light", "Night Light", None);
    group.add(&night_light);

    let feedback = adw::ComboRow::builder()
        .title("Feedback")
        .subtitle("Profiles that mean night")
        .build();
    let choices: Rc<RefCell<Vec<FeedbackSet>>> = Rc::default();
    let populate: Rc<dyn Fn()> = Rc::new({
        let (s, feedback, choices, ui) = (s.clone(), feedback.clone(), choices.clone(), ui.clone());
        move || {
            let level = FeedbackProfile::parse(&s.string("feedback-level"));
            let current = FeedbackSet::from_strs(s.strv("trigger-feedback").iter().map(|x| x.as_str()));
            let mut sets = vec![FeedbackSet::OFF];
            let mut labels = vec!["Off"];
            for (set, label) in FeedbackSet::CHOICES {
                if set.compatible_with(level) {
                    sets.push(set);
                    labels.push(label);
                }
            }
            if !sets.contains(&current) {
                let _ = s.set_strv("trigger-feedback", Vec::<String>::new());
                ui.toast("Feedback trigger turned off: it has to include the night feedback level");
            }
            let index = sets.iter().position(|x| *x == current).unwrap_or(0) as u32;
            *choices.borrow_mut() = sets;
            feedback.set_model(Some(&gtk::StringList::new(&labels)));
            feedback.set_selected(index);
        }
    });
    populate();
    feedback.connect_selected_notify({
        let (s, choices) = (s.clone(), choices.clone());
        move |row| {
            let Some(set) = choices.borrow().get(row.selected() as usize).copied() else {
                return;
            };
            let current = FeedbackSet::from_strs(s.strv("trigger-feedback").iter().map(|x| x.as_str()));
            if set != current {
                let _ = s.set_strv("trigger-feedback", set.to_strs());
            }
        }
    });
    s.connect_changed(Some("feedback-level"), {
        let populate = populate.clone();
        move |_, _| populate()
    });
    group.add(&feedback);

    glib::spawn_future_local(async move {
        let caps = Capabilities::detect_async().await;
        night_light.set_visible(caps.night_light);
        feedback.set_visible(caps.feedback);
    });
    group
}

fn screen_group(ui: &Rc<Ui>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Night Light While the Screen Is Off")
        .description(
            "On some phones, changing Night Light with the screen off leaves it stuck until a reboot.",
        )
        .build();
    let options = [
        ("wake", "Turn the Screen On Briefly", "Changes on time; the screen lights up for about two seconds"),
        ("wait", "Wait for the Screen", "Changes the next time the screen turns on"),
    ];
    let mut first: Option<gtk::CheckButton> = None;
    for (value, title, subtitle) in options {
        let check = gtk::CheckButton::builder().valign(gtk::Align::Center).build();
        if let Some(f) = &first {
            check.set_group(Some(f));
        } else {
            first = Some(check.clone());
        }
        check.set_active(ui.settings.string("screen-off-mode") == value);
        let row = adw::ActionRow::builder()
            .title(title)
            .subtitle(subtitle)
            .activatable_widget(&check)
            .build();
        row.add_prefix(&check);
        check.connect_toggled({
            let settings = ui.settings.clone();
            move |c| {
                if c.is_active() && settings.string("screen-off-mode") != value {
                    let _ = settings.set_string("screen-off-mode", value);
                }
            }
        });
        ui.settings.connect_changed(Some("screen-off-mode"), {
            let check = check.clone();
            move |s, k| {
                if s.string(k) == value && !check.is_active() {
                    check.set_active(true);
                }
            }
        });
        group.add(&row);
    }
    group.set_visible(false);
    glib::spawn_future_local({
        let group = group.clone();
        async move {
            group.set_visible(Capabilities::detect_async().await.night_light);
        }
    });
    group
}

fn background_group(ui: &Rc<Ui>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("Background").build();
    let row = adw::SwitchRow::builder()
        .title("Start at Login")
        .subtitle("Needed for the schedule and triggers to work after a restart")
        .active(ui.settings.boolean("autostart-requested"))
        .build();
    let busy = Rc::new(std::cell::Cell::new(false));

    let request = {
        let (ui, row, busy) = (ui.clone(), row.clone(), busy.clone());
        move |on: bool| {
            let (ui, row, busy) = (ui.clone(), row.clone(), busy.clone());
            glib::spawn_future_local(async move {
                let result = sandbox::request_autostart(on).await;
                busy.set(true);
                match result {
                    Ok(enabled) => {
                        let _ = ui.settings.set_boolean("autostart-requested", enabled);
                        row.set_active(enabled);
                        if on && !enabled {
                            ui.toast("Starting at login wasn't allowed");
                        }
                    }
                    Err(err) => {
                        tracing::warn!(%err, "autostart request failed");
                        let _ = ui.settings.set_boolean("autostart-requested", false);
                        row.set_active(false);
                        ui.toast("Couldn't change starting at login");
                    }
                }
                busy.set(false);
            });
        }
    };
    row.connect_active_notify({
        let (busy, request) = (busy.clone(), request.clone());
        move |row| {
            if !busy.get() {
                request(row.is_active());
            }
        }
    });
    // Asked once on first launch, since nothing runs after a restart without it.
    if ui.settings.user_value("autostart-requested").is_none() {
        request(true);
    }
    group.add(&row);
    group
}
