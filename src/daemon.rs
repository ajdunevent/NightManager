use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use chrono::{DateTime, Local};
use gio::prelude::*;

use crate::capabilities::Capabilities;
use crate::config::{
    theme_setting_key, times_to_strv, Config, ScheduleSource, ScreenOffMode, APP_ID,
    DAEMON_INTERFACE, DAEMON_OBJECT_PATH,
};
use crate::host::Host;
use crate::keys::{parse_value, print_value, HostKey, COLOR_SCHEMA};
use crate::legacy;
use crate::schedule::{hours_to_time, Boundary, Event, Weekly};
use crate::screen::Screen;
use crate::state::State;
use crate::triggers;
use crate::wake_timer::WallClockTimer;

const INTROSPECTION: &str = r#"
<node>
  <interface name="net.dunevent.NightManager.Daemon">
    <method name="Reload"/>
    <method name="StartNight"/>
    <method name="EndNight"/>
    <method name="Quit"/>
    <method name="GetDaytime">
      <arg name="values" type="a(svb)" direction="out"/>
    </method>
    <signal name="DaytimeChanged"/>
    <method name="SetDaytime">
      <arg name="key" type="s" direction="in"/>
      <arg name="value" type="v" direction="in"/>
      <arg name="at_morning" type="b" direction="out"/>
    </method>
    <method name="CheckLegacy">
      <arg name="installed" type="b" direction="out"/>
    </method>
    <property name="Active" type="b" access="read"/>
    <property name="NextChange" type="x" access="read"/>
    <property name="NextIsStart" type="b" access="read"/>
    <property name="LegacyInstalled" type="b" access="read"/>
  </interface>
</node>
"#;

type Deferred = Box<dyn FnOnce()>;

pub struct Daemon {
    conn: gio::DBusConnection,
    settings: gio::Settings,
    color: Option<gio::Settings>,
    host: Host,
    config: RefCell<Config>,
    state: RefCell<State>,
    state_path: PathBuf,
    blocked: Cell<bool>,
    screen: RefCell<Option<Rc<Screen>>>,
    timer: RefCell<Option<WallClockTimer>>,
    next: RefCell<Option<Event<Local>>>,
    /// Last "is night" reading per trigger key, to detect edges.
    trigger_state: RefCell<HashMap<HostKey, Option<bool>>>,
    /// A Night Light change waiting for the screen to turn on.
    pending_night_light: RefCell<Option<Deferred>>,
    reload_queued: Cell<bool>,
    caps: RefCell<Rc<Cell<Capabilities>>>,
}

/// Without the dconf patch, sandboxed writes reached the host while every read
/// returned the schema default, and saved day values overwrote the user's
/// settings in the morning.
fn check_settings_round_trip(settings: &gio::Settings) -> anyhow::Result<()> {
    let token = format!("{}-{}", std::process::id(), glib::real_time());
    settings.set_string("settings-probe", &token)?;
    gio::Settings::sync();
    if settings.string("settings-probe") != token {
        anyhow::bail!("settings written here can't be read back; refusing to touch host settings");
    }
    Ok(())
}

impl Daemon {
    pub fn start(conn: &gio::DBusConnection) -> anyhow::Result<Rc<Daemon>> {
        let settings = gio::Settings::new(APP_ID);
        check_settings_round_trip(&settings)?;
        let color = gio::SettingsSchemaSource::default()
            .and_then(|s| s.lookup(COLOR_SCHEMA, true))
            .map(|_| gio::Settings::new(COLOR_SCHEMA));
        let state_path = State::default_path();
        let daemon = Rc::new(Daemon {
            conn: conn.clone(),
            config: RefCell::new(Config::load(&settings)),
            settings,
            color,
            host: Host::new(),
            state: RefCell::new(State::load(&state_path)),
            state_path,
            blocked: Cell::new(false),
            screen: RefCell::default(),
            timer: RefCell::default(),
            next: RefCell::default(),
            trigger_state: RefCell::default(),
            pending_night_light: RefCell::default(),
            reload_queued: Cell::new(false),
            caps: RefCell::default(),
        });

        let weak = Rc::downgrade(&daemon);
        *daemon.caps.borrow_mut() = crate::capabilities::watch(conn, move || {
            if let Some(d) = weak.upgrade() {
                if !d.blocked.get() && d.state.borrow().active {
                    d.reconcile_night();
                }
            }
        });

        if legacy::installed() {
            tracing::info!("old NightManager package found; staying idle until it is removed");
            daemon.blocked.set(true);
            if !daemon.state.borrow().legacy_imported {
                daemon.import_legacy_config();
                *daemon.config.borrow_mut() = Config::load(&daemon.settings);
            }
        }

        let weak = Rc::downgrade(&daemon);
        *daemon.screen.borrow_mut() = Some(Screen::new(conn, move |off| {
            if let Some(d) = weak.upgrade() {
                d.on_screen(off);
            }
        }));

        let weak = Rc::downgrade(&daemon);
        *daemon.timer.borrow_mut() = Some(WallClockTimer::new(move || {
            if let Some(d) = weak.upgrade() {
                d.on_timer();
            }
        })?);

        for key in HostKey::ALL {
            let weak = Rc::downgrade(&daemon);
            daemon.host.connect_changed(key, move |key, value| {
                if let Some(d) = weak.upgrade() {
                    d.on_host_changed(key, value);
                }
            });
        }

        let weak = Rc::downgrade(&daemon);
        daemon.settings.connect_changed(None, move |_, _| {
            if let Some(d) = weak.upgrade() {
                d.queue_reload();
            }
        });
        if let Some(color) = &daemon.color {
            let weak = Rc::downgrade(&daemon);
            color.connect_changed(None, move |_, key| {
                if key.starts_with("night-light-schedule") {
                    if let Some(d) = weak.upgrade() {
                        d.reschedule();
                    }
                }
            });
        }

        daemon.register_object()?;
        daemon.refresh_triggers();
        if !daemon.blocked.get() {
            daemon.catch_up(Local::now());
        }
        daemon.reschedule();
        Ok(daemon)
    }

    /// Also emits `DaytimeChanged`, since the saved day values live in the state.
    fn save_state(&self) {
        if let Err(err) = self.state.borrow().save(&self.state_path) {
            tracing::warn!(%err, "saving state failed");
        }
        if let Err(err) =
            self.conn
                .emit_signal(None, DAEMON_OBJECT_PATH, DAEMON_INTERFACE, "DaytimeChanged", None)
        {
            tracing::debug!(%err, "DaytimeChanged not sent");
        }
    }

    fn caps(&self) -> Capabilities {
        self.caps.borrow().get()
    }

    fn weekly(&self) -> Weekly {
        let config = self.config.borrow();
        match config.schedule_source {
            ScheduleSource::Custom => Weekly {
                evening: config.evening,
                morning: config.morning,
            },
            ScheduleSource::NightLight => self.night_light_schedule(),
        }
    }

    /// Night Light's own schedule: sunset to sunrise when automatic, otherwise its
    /// from/to times.
    fn night_light_schedule(&self) -> Weekly {
        let Some(color) = &self.color else {
            return Weekly::default();
        };
        if color.boolean("night-light-schedule-automatic") {
            if let (Some(sunset), Some(sunrise)) =
                (self.gsd_color_hours("Sunset"), self.gsd_color_hours("Sunrise"))
            {
                return Weekly::daily(hours_to_time(sunset), hours_to_time(sunrise));
            }
            tracing::debug!("sunrise/sunset unavailable, using Night Light's manual times");
        }
        Weekly::daily(
            hours_to_time(color.double("night-light-schedule-from")),
            hours_to_time(color.double("night-light-schedule-to")),
        )
    }

    fn gsd_color_hours(&self, property: &str) -> Option<f64> {
        let reply = self
            .conn
            .call_sync(
                Some("org.gnome.SettingsDaemon.Color"),
                "/org/gnome/SettingsDaemon/Color",
                "org.freedesktop.DBus.Properties",
                "Get",
                Some(&("org.gnome.SettingsDaemon.Color", property).to_variant()),
                Some(glib::VariantTy::new("(v)").expect("valid type string")),
                gio::DBusCallFlags::NO_AUTO_START,
                2000,
                gio::Cancellable::NONE,
            )
            .ok()?;
        let hours = reply.child_value(0).as_variant()?.get::<f64>()?;
        (0.0..24.0).contains(&hours).then_some(hours)
    }

    /// Acts on the latest boundary at or before `now` unless it was already handled,
    /// which also covers boundaries missed while the daemon wasn't running.
    fn catch_up(self: &Rc<Self>, now: DateTime<Local>) {
        let Some(last) = self.weekly().last_at_or_before(&now) else {
            return;
        };
        let ts = last.at.timestamp();
        if self.state.borrow().last_boundary.is_some_and(|b| ts <= b) {
            return;
        }
        self.state.borrow_mut().last_boundary = Some(ts);
        self.save_state();
        tracing::info!(at = %last.at, kind = ?last.kind, "schedule boundary");
        match last.kind {
            Boundary::Start => self.start_night(),
            Boundary::End => self.end_night(),
        }
    }

    fn on_timer(self: &Rc<Self>) {
        if !self.blocked.get() {
            self.catch_up(Local::now());
        }
        self.reschedule();
    }

    fn reschedule(self: &Rc<Self>) {
        let next = if self.blocked.get() {
            None
        } else {
            self.weekly().next_after(&Local::now())
        };
        if let Some(timer) = self.timer.borrow().as_ref() {
            let result = match &next {
                Some(e) => timer.arm(e.at),
                None => timer.disarm(),
            };
            if let Err(err) = result {
                tracing::error!(%err, "arming the schedule timer failed");
            }
        }
        let wake = next.as_ref().map(|e| e.at.timestamp());
        glib::spawn_future_local(async move {
            match wake {
                Some(t) => crate::waked::update(t).await,
                None => crate::waked::remove().await,
            }
        });
        if let Some(e) = &next {
            tracing::info!(at = %e.at, kind = ?e.kind, "next boundary");
        }
        *self.next.borrow_mut() = next;
        self.emit_properties();
    }

    /// The configured night values, without features this device lacks.
    fn night_values(&self) -> Vec<(HostKey, glib::Variant)> {
        let caps = self.caps();
        self.config
            .borrow()
            .night_values()
            .into_iter()
            .filter(|(key, _)| caps.allows(*key))
            .collect()
    }

    pub fn start_night(self: &Rc<Self>) {
        if self.blocked.get() || self.state.borrow().active {
            return;
        }
        let night = self.night_values();
        {
            let mut state = self.state.borrow_mut();
            state.active = true;
            state.day.clear();
            for (key, _) in &night {
                if let Some(saved) = self.host.saved_value(*key) {
                    state.day.insert(*key, saved);
                }
            }
        }
        self.save_state();
        tracing::info!("night mode on");
        for (key, value) in night {
            self.write(key, value);
        }
        self.emit_properties();
    }

    /// The flag marks saved values that apply when night ends.
    fn daytime_values(&self) -> Vec<(String, glib::Variant, bool)> {
        let caps = self.caps();
        let state = self.state.borrow();
        HostKey::ALL
            .into_iter()
            .filter(|key| caps.allows(*key))
            .filter_map(|key| {
                let saved = state.day.get(&key).filter(|_| state.active);
                let value = match saved {
                    Some(text) => text
                        .as_deref()
                        .and_then(|t| parse_value(key, t))
                        .or_else(|| self.host.default_value(key))?,
                    None => self.host.value(key)?,
                };
                Some((key.name().to_string(), value, saved.is_some()))
            })
            .collect()
    }

    /// Returns whether the value waits for the morning, or `None` for an
    /// unsupported key or a wrong value type.
    fn set_daytime(self: &Rc<Self>, key: HostKey, value: glib::Variant) -> Option<bool> {
        if value.type_() != key.variant_type() || !self.host.supports(key) {
            return None;
        }
        {
            let mut state = self.state.borrow_mut();
            if state.active {
                if let Some(saved) = state.day.get_mut(&key) {
                    *saved = Some(print_value(&value));
                    drop(state);
                    self.save_state();
                    tracing::info!(?key, "day value saved for the morning");
                    return Some(true);
                }
            }
        }
        tracing::info!(?key, "day value applied");
        self.write(key, value);
        Some(false)
    }

    pub fn end_night(self: &Rc<Self>) {
        if self.blocked.get() || !self.state.borrow().active {
            return;
        }
        let day = {
            let mut state = self.state.borrow_mut();
            state.active = false;
            std::mem::take(&mut state.day)
        };
        self.save_state();
        tracing::info!("night mode off");
        for (key, saved) in day {
            self.restore(key, saved);
        }
        self.emit_properties();
    }

    fn write(self: &Rc<Self>, key: HostKey, value: glib::Variant) {
        let this = self.clone();
        self.change(key, move || {
            this.host.set(key, &value);
        });
    }

    fn restore(self: &Rc<Self>, key: HostKey, saved: Option<String>) {
        let this = self.clone();
        self.change(key, move || this.host.restore(key, &saved));
    }

    /// Runs a host change. Night Light changes wait for the screen or wake it, since
    /// a change made while the panel is off can leave the display's PQ service stuck.
    fn change(self: &Rc<Self>, key: HostKey, apply: impl FnOnce() + 'static) {
        let this = self.clone();
        let apply = move || {
            apply();
            this.refresh_trigger(key);
        };
        let screen = self.screen.borrow().clone();
        let Some(screen) = screen.filter(|s| key == HostKey::NightLight && s.is_off()) else {
            apply();
            return;
        };
        match self.config.borrow().screen_off_mode {
            ScreenOffMode::WakeBriefly => screen.wake_briefly(apply),
            ScreenOffMode::WaitForScreen => {
                tracing::debug!("screen off, Night Light change waits");
                *self.pending_night_light.borrow_mut() = Some(Box::new(apply));
            }
        }
    }

    fn on_screen(self: &Rc<Self>, off: bool) {
        if off {
            return;
        }
        let pending = self.pending_night_light.borrow_mut().take();
        if let Some(apply) = pending {
            tracing::debug!("screen on, applying the waiting Night Light change");
            apply();
        }
    }

    fn refresh_trigger(&self, key: HostKey) {
        let config = self.config.borrow();
        let night = self
            .host
            .value(key)
            .and_then(|v| triggers::is_night(&config, key, &v));
        self.trigger_state.borrow_mut().insert(key, night);
    }

    fn refresh_triggers(&self) {
        for key in HostKey::ALL {
            self.refresh_trigger(key);
        }
    }

    fn on_host_changed(self: &Rc<Self>, key: HostKey, value: glib::Variant) {
        if self.blocked.get() {
            return;
        }
        if key.is_theme() && self.state.borrow().active {
            self.learn_theme_change(key, &value);
        }
        let (before, after) = {
            let config = self.config.borrow();
            let after = triggers::is_night(&config, key, &value);
            (self.trigger_state.borrow_mut().insert(key, after).flatten(), after)
        };
        match triggers::edge(before, after) {
            Some(Boundary::Start) => {
                tracing::info!(?key, "manual trigger: start");
                self.start_night();
            }
            Some(Boundary::End) => {
                tracing::info!(?key, "manual trigger: end");
                self.end_night();
            }
            None => {}
        }
    }

    /// A theme the user picks at night becomes the night value, when enabled.
    fn learn_theme_change(&self, key: HostKey, value: &glib::Variant) {
        let config = self.config.borrow();
        let configured = match key {
            HostKey::GtkTheme => config.gtk_theme.is_some(),
            HostKey::AccentColor => config.accent_color.is_some(),
            HostKey::IconTheme => config.icon_theme.is_some(),
            _ => false,
        };
        if !config.save_theme_changes || !configured {
            return;
        }
        if let (Some(setting), Some(new)) = (theme_setting_key(key), value.str()) {
            tracing::info!(?key, new, "night theme follows user change");
            if let Err(err) = self.settings.set_string(setting, new) {
                tracing::warn!(%err, "saving theme change failed");
            }
        }
    }

    fn queue_reload(self: &Rc<Self>) {
        if self.reload_queued.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(d) = weak.upgrade() {
                d.reload_queued.set(false);
                d.reload();
            }
        });
    }

    pub fn reload(self: &Rc<Self>) {
        let config = Config::load(&self.settings);
        if *self.config.borrow() == config {
            return;
        }
        tracing::info!("configuration changed");
        *self.config.borrow_mut() = config;
        self.refresh_triggers();
        if !self.blocked.get() && self.state.borrow().active {
            self.reconcile_night();
        }
        if !self.blocked.get() {
            self.catch_up(Local::now());
        }
        self.reschedule();
    }

    /// Brings an active night in line with a changed configuration: keys no longer
    /// managed get their day value back, and new or changed ones are applied.
    /// A key whose service went away stays saved, so the morning still restores it.
    fn reconcile_night(self: &Rc<Self>) {
        let configured: Vec<HostKey> =
            self.config.borrow().night_values().into_iter().map(|(k, _)| k).collect();
        let night: HashMap<HostKey, glib::Variant> = self.night_values().into_iter().collect();
        let dropped: Vec<(HostKey, Option<String>)> = {
            let mut state = self.state.borrow_mut();
            let keys: Vec<HostKey> = state
                .day
                .keys()
                .filter(|k| !configured.contains(k))
                .copied()
                .collect();
            keys.into_iter()
                .filter_map(|k| state.day.remove(&k).map(|v| (k, v)))
                .collect()
        };
        {
            let mut state = self.state.borrow_mut();
            for key in night.keys() {
                if !state.day.contains_key(key) {
                    if let Some(saved) = self.host.saved_value(*key) {
                        state.day.insert(*key, saved);
                    }
                }
            }
        }
        self.save_state();
        for (key, saved) in dropped {
            self.restore(key, saved);
        }
        for (key, value) in night {
            self.write(key, value);
        }
    }

    fn import_legacy_config(&self) {
        let imported = legacy::import();
        tracing::info!("importing the old NightManager configuration");
        let s = &self.settings;
        let results = [
            s.set_boolean("night-light", imported.night_light),
            s.set_boolean("ambient-light", imported.ambient_light),
            s.set_boolean("dark-style", imported.dark_style),
            s.set_string("gtk-theme", imported.gtk_theme.as_deref().unwrap_or("")),
            s.set_string("accent-color", imported.accent_color.as_deref().unwrap_or("")),
            s.set_string("icon-theme", imported.icon_theme.as_deref().unwrap_or("")),
            s.set_string(
                "feedback-level",
                imported.feedback_level.map(|f| f.as_str()).unwrap_or(""),
            ),
            s.set_string("schedule-source", "custom"),
            s.set_strv("evening-times", times_to_strv(&imported.evening)),
            s.set_strv("morning-times", times_to_strv(&imported.morning)),
            s.set_boolean(
                "evening-same",
                imported.evening.iter().all(|t| *t == imported.evening[0]),
            ),
            s.set_boolean(
                "morning-same",
                imported.morning.iter().all(|t| *t == imported.morning[0]),
            ),
        ];
        for err in results.into_iter().filter_map(Result::err) {
            tracing::warn!(%err, "importing a setting failed");
        }
        gio::Settings::sync();
        self.state.borrow_mut().legacy_imported = true;
        self.save_state();
    }

    /// Adopts the old version's saved day values if it was in night mode when it
    /// was removed.
    fn adopt_legacy_restore(&self) {
        let Some(restore) = legacy::import().restore else {
            return;
        };
        let mut state = self.state.borrow_mut();
        if state.active {
            return;
        }
        tracing::info!("old version was in night mode; adopting its saved day values");
        state.active = true;
        state.day = restore
            .into_iter()
            .filter(|(k, v)| parse_value(*k, v).is_some())
            .map(|(k, v)| (k, Some(v)))
            .collect();
    }

    pub fn check_legacy(self: &Rc<Self>) -> bool {
        let installed = legacy::installed();
        if self.blocked.get() && !installed {
            tracing::info!("old NightManager package removed; starting");
            self.blocked.set(false);
            *self.config.borrow_mut() = Config::load(&self.settings);
            if self.state.borrow().legacy_imported {
                self.adopt_legacy_restore();
                self.save_state();
            }
            self.refresh_triggers();
            self.catch_up(Local::now());
            self.reschedule();
        }
        installed
    }

    fn property(&self, name: &str) -> Option<glib::Variant> {
        let next = self.next.borrow();
        Some(match name {
            "Active" => self.state.borrow().active.to_variant(),
            "NextChange" => next.as_ref().map(|e| e.at.timestamp()).unwrap_or(0).to_variant(),
            "NextIsStart" => next
                .as_ref()
                .is_some_and(|e| e.kind == Boundary::Start)
                .to_variant(),
            "LegacyInstalled" => self.blocked.get().to_variant(),
            _ => return None,
        })
    }

    fn emit_properties(&self) {
        let changed = glib::VariantDict::new(None);
        for name in ["Active", "NextChange", "NextIsStart", "LegacyInstalled"] {
            if let Some(v) = self.property(name) {
                changed.insert_value(name, &v);
            }
        }
        // A tuple built with to_variant boxes the dict as `v`, giving `(svas)`.
        let args = glib::Variant::tuple_from_iter([
            DAEMON_INTERFACE.to_variant(),
            changed.end(),
            Vec::<String>::new().to_variant(),
        ]);
        if let Err(err) = self.conn.emit_signal(
            None,
            DAEMON_OBJECT_PATH,
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            Some(&args),
        ) {
            tracing::debug!(%err, "PropertiesChanged not sent");
        }
    }

    fn register_object(self: &Rc<Self>) -> anyhow::Result<()> {
        let info = gio::DBusNodeInfo::for_xml(INTROSPECTION)?;
        let iface = info
            .lookup_interface(DAEMON_INTERFACE)
            .ok_or_else(|| anyhow::anyhow!("interface missing from introspection data"))?;
        let for_calls = Rc::downgrade(self);
        let for_props = Rc::downgrade(self);
        let _ = self
            .conn
            .register_object(DAEMON_OBJECT_PATH, &iface)
            .method_call(move |_, _, _, _, method, params, invocation| {
                let Some(d) = for_calls.upgrade() else {
                    return;
                };
                match method {
                    "Reload" => {
                        d.reload();
                        invocation.return_value(None);
                    }
                    "StartNight" => {
                        d.start_night();
                        invocation.return_value(None);
                    }
                    "EndNight" => {
                        d.end_night();
                        invocation.return_value(None);
                    }
                    "Quit" => {
                        tracing::info!("quitting on request");
                        invocation.return_value(None);
                        // Flushed first so the caller gets its reply.
                        let conn = d.conn.clone();
                        glib::spawn_future_local(async move {
                            let _ = conn.flush_future().await;
                            if let Some(app) = gio::Application::default() {
                                app.quit();
                            }
                        });
                    }
                    "GetDaytime" => {
                        let values = d.daytime_values();
                        invocation.return_value(Some(&(values,).to_variant()));
                    }
                    "SetDaytime" => {
                        let parsed = params
                            .get::<(String, glib::Variant)>()
                            .and_then(|(name, value)| Some((HostKey::from_name(&name)?, value)));
                        match parsed.and_then(|(key, value)| d.set_daytime(key, value)) {
                            Some(at_morning) => {
                                invocation.return_value(Some(&(at_morning,).to_variant()))
                            }
                            None => invocation.return_dbus_error(
                                "org.freedesktop.DBus.Error.InvalidArgs",
                                "unknown key or wrong value type",
                            ),
                        }
                    }
                    "CheckLegacy" => {
                        let installed = d.check_legacy();
                        d.emit_properties();
                        invocation.return_value(Some(&(installed,).to_variant()));
                    }
                    _ => invocation.return_error(
                        gio::DBusError::UnknownMethod,
                        &format!("no method {method}"),
                    ),
                }
            })
            .property(move |_, _, _, _, name| {
                for_props
                    .upgrade()
                    .and_then(|d| d.property(name))
                    .unwrap_or_else(|| false.to_variant())
            })
            .build()?;
        Ok(())
    }
}
