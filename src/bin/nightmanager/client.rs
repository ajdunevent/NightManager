use gio::prelude::*;
use nightmanager::config::{DAEMON_BUS_NAME, DAEMON_INTERFACE, DAEMON_OBJECT_PATH};

/// The GUI's view of the daemon. Creating the proxy starts the daemon through
/// D-Bus activation if it isn't running.
#[derive(Clone)]
pub struct Client {
    proxy: gio::DBusProxy,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Status {
    pub active: bool,
    pub next_change: i64,
    pub next_is_start: bool,
    pub next_is_solar: bool,
    pub legacy_installed: bool,
}

impl Status {
    pub fn schedule(&self) -> nightmanager::status::Status {
        nightmanager::status::Status {
            active: self.active,
            next_change: self.next_change,
            next_is_start: self.next_is_start,
            next_is_solar: self.next_is_solar,
        }
    }
}

impl Client {
    pub async fn connect() -> anyhow::Result<Client> {
        let proxy = gio::DBusProxy::for_bus_future(
            gio::BusType::Session,
            gio::DBusProxyFlags::NONE,
            None,
            DAEMON_BUS_NAME,
            DAEMON_OBJECT_PATH,
            DAEMON_INTERFACE,
        )
        .await?;
        Ok(Client { proxy })
    }

    pub fn status(&self) -> Status {
        let b = |name: &str| {
            self.proxy
                .cached_property(name)
                .and_then(|v| v.get::<bool>())
                .unwrap_or(false)
        };
        Status {
            active: b("Active"),
            next_change: self
                .proxy
                .cached_property("NextChange")
                .and_then(|v| v.get::<i64>())
                .unwrap_or(0),
            next_is_start: b("NextIsStart"),
            next_is_solar: b("NextIsSolar"),
            legacy_installed: b("LegacyInstalled"),
        }
    }

    pub fn connect_changed(&self, f: impl Fn(Status) + 'static) {
        let client = self.clone();
        self.proxy
            .connect_local("g-properties-changed", false, move |_| {
                f(client.status());
                None
            });
    }

    pub fn connect_signal(&self, signal: &'static str, f: impl Fn() + 'static) -> glib::SignalHandlerId {
        self.proxy.connect_local("g-signal", false, move |args| {
            if args.get(2).and_then(|v| v.get::<String>().ok()).as_deref() == Some(signal) {
                f();
            }
            None
        })
    }

    pub fn disconnect(&self, id: glib::SignalHandlerId) {
        self.proxy.disconnect(id);
    }

    pub async fn call(&self, method: &str) -> Option<glib::Variant> {
        self.call_with(method, None).await
    }

    pub async fn call_with(&self, method: &str, args: Option<&glib::Variant>) -> Option<glib::Variant> {
        self.call_timeout(method, args, -1).await
    }

    pub async fn call_timeout(
        &self,
        method: &str,
        args: Option<&glib::Variant>,
        timeout_ms: i32,
    ) -> Option<glib::Variant> {
        match self
            .proxy
            .call_future(method, args, gio::DBusCallFlags::NONE, timeout_ms)
            .await
        {
            Ok(v) => Some(v),
            Err(err) => {
                tracing::warn!(method, %err, "daemon call failed");
                None
            }
        }
    }
}
