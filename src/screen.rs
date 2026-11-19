use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gio::prelude::*;

const SCREENSAVER_BUS_NAME: &str = "org.gnome.ScreenSaver";
const SCREENSAVER_PATH: &str = "/org/gnome/ScreenSaver";
const SCREENSAVER_INTERFACE: &str = "org.gnome.ScreenSaver";
const DISPLAY_BUS_NAME: &str = "org.gnome.Mutter.DisplayConfig";
const DISPLAY_PATH: &str = "/org/gnome/Mutter/DisplayConfig";
const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";

/// Screen power state from phosh's org.gnome.ScreenSaver, which follows the primary
/// monitor's power mode. Without that service the screen counts as on.
pub struct Screen {
    conn: gio::DBusConnection,
    off: Cell<bool>,
    waking: Cell<bool>,
}

impl Screen {
    pub fn new(conn: &gio::DBusConnection, on_change: impl Fn(bool) + 'static) -> Rc<Screen> {
        let screen = Rc::new(Screen {
            conn: conn.clone(),
            off: Cell::new(false),
            waking: Cell::new(false),
        });
        let on_change = Rc::new(on_change);

        let set = {
            let screen = Rc::downgrade(&screen);
            let on_change = on_change.clone();
            move |off: bool| {
                if let Some(screen) = screen.upgrade() {
                    if screen.off.replace(off) != off {
                        tracing::debug!(off, "screen state");
                        on_change(off);
                    }
                }
            }
        };

        conn.signal_subscribe(
            Some(SCREENSAVER_BUS_NAME),
            Some(SCREENSAVER_INTERFACE),
            Some("ActiveChanged"),
            Some(SCREENSAVER_PATH),
            None,
            gio::DBusSignalFlags::NONE,
            {
                let set = set.clone();
                move |_, _, _, _, _, params| {
                    if let Some((active,)) = params.get::<(bool,)>() {
                        set(active);
                    }
                }
            },
        );

        if let Ok(reply) = conn.call_sync(
            Some(SCREENSAVER_BUS_NAME),
            SCREENSAVER_PATH,
            SCREENSAVER_INTERFACE,
            "GetActive",
            None,
            Some(glib::VariantTy::new("(b)").expect("valid type string")),
            gio::DBusCallFlags::NO_AUTO_START,
            1000,
            gio::Cancellable::NONE,
        ) {
            if let Some((active,)) = reply.get::<(bool,)>() {
                screen.off.set(active);
            }
        }

        let appeared = set.clone();
        let vanished = set;
        let _ = gio::bus_watch_name_on_connection(
            conn,
            SCREENSAVER_BUS_NAME,
            gio::BusNameWatcherFlags::NONE,
            move |conn, _, owner| {
                let appeared = appeared.clone();
                let conn = conn.clone();
                let owner = owner.to_string();
                glib::spawn_future_local(async move {
                    let reply = conn
                        .call_future(
                            Some(&owner),
                            SCREENSAVER_PATH,
                            SCREENSAVER_INTERFACE,
                            "GetActive",
                            None,
                            Some(glib::VariantTy::new("(b)").expect("valid type string")),
                            gio::DBusCallFlags::NONE,
                            -1,
                        )
                        .await;
                    if let Some((active,)) = reply.ok().and_then(|r| r.get::<(bool,)>()) {
                        appeared(active);
                    }
                });
            },
            move |_, _| vanished(false),
        );

        screen
    }

    pub fn is_off(&self) -> bool {
        self.off.get()
    }

    /// Turns the screen on through DisplayConfig's PowerSaveMode, runs `f` a
    /// second later, and puts PowerSaveMode back a second after that.
    pub fn wake_briefly(self: &Rc<Self>, f: impl FnOnce() + 'static) {
        if !self.is_off() || self.waking.get() {
            f();
            return;
        }
        self.waking.set(true);
        let screen = self.clone();
        glib::spawn_future_local(async move {
            let previous = screen.power_save_mode().await;
            screen.set_power_save_mode(0).await;
            glib::timeout_future(Duration::from_secs(1)).await;
            f();
            glib::timeout_future(Duration::from_secs(1)).await;
            if let Some(mode) = previous.filter(|m| *m != 0) {
                screen.set_power_save_mode(mode).await;
            }
            screen.waking.set(false);
        });
    }

    async fn power_save_mode(&self) -> Option<i32> {
        let reply = self
            .conn
            .call_future(
                Some(DISPLAY_BUS_NAME),
                DISPLAY_PATH,
                PROPERTIES_INTERFACE,
                "Get",
                Some(&(DISPLAY_BUS_NAME, "PowerSaveMode").to_variant()),
                Some(glib::VariantTy::new("(v)").expect("valid type string")),
                gio::DBusCallFlags::NONE,
                2000,
            )
            .await
            .ok()?;
        reply.child_value(0).as_variant()?.get::<i32>()
    }

    async fn set_power_save_mode(&self, mode: i32) {
        let args = (DISPLAY_BUS_NAME, "PowerSaveMode", mode.to_variant()).to_variant();
        if let Err(err) = self
            .conn
            .call_future(
                Some(DISPLAY_BUS_NAME),
                DISPLAY_PATH,
                PROPERTIES_INTERFACE,
                "Set",
                Some(&args),
                None,
                gio::DBusCallFlags::NONE,
                2000,
            )
            .await
        {
            tracing::warn!(%err, mode, "setting PowerSaveMode failed");
        }
    }
}
