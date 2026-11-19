//! Registers the next schedule boundary with `waked`
//! (<https://gitlab.com/seath1/waked>), a system daemon that wakes the device from
//! suspend for unprivileged apps. Absent on most systems; failures are logged at
//! debug level.

use gio::prelude::*;

const BUS_NAME: &str = "de.seath.Waked";
const OBJECT_PATH: &str = "/de/seath/Waked/Alarm";
const INTERFACE: &str = "de.seath.Waked";
const ALARM_ID: &str = "nightmanager";

async fn proxy() -> Option<gio::DBusProxy> {
    let conn = gio::bus_get_future(gio::BusType::System).await.ok()?;
    match gio::DBusProxy::new_future(
        &conn,
        gio::DBusProxyFlags::DO_NOT_LOAD_PROPERTIES | gio::DBusProxyFlags::DO_NOT_CONNECT_SIGNALS,
        None,
        Some(BUS_NAME),
        OBJECT_PATH,
        INTERFACE,
    )
    .await
    {
        Ok(p) if p.name_owner().is_some() => Some(p),
        _ => None,
    }
}

pub async fn update(unix_time: i64) {
    let Some(proxy) = proxy().await else { return };
    let args = (ALARM_ID, unix_time as u64).to_variant();
    if let Err(err) = proxy
        .call_future("update", Some(&args), gio::DBusCallFlags::NONE, -1)
        .await
    {
        tracing::debug!(%err, "waked update failed");
    }
}

pub async fn remove() {
    let Some(proxy) = proxy().await else { return };
    let args = (ALARM_ID,).to_variant();
    if let Err(err) = proxy
        .call_future("remove", Some(&args), gio::DBusCallFlags::NONE, -1)
        .await
    {
        tracing::debug!(%err, "waked remove failed");
    }
}
