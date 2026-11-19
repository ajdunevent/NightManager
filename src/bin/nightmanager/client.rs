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
    pub legacy_installed: bool,
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
        let client = Client { proxy };
        client.call("Reload").await;
        Ok(client)
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

    pub async fn call(&self, method: &str) -> Option<glib::Variant> {
        self.call_with(method, None).await
    }

    pub async fn call_with(&self, method: &str, args: Option<&glib::Variant>) -> Option<glib::Variant> {
        match self
            .proxy
            .call_future(method, args, gio::DBusCallFlags::NONE, -1)
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
