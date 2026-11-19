use gio::prelude::*;
use nightmanager::config::DAEMON_BUS_NAME;
use nightmanager::daemon::Daemon;

fn main() -> glib::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let app = gio::Application::new(Some(DAEMON_BUS_NAME), gio::ApplicationFlags::empty());
    let daemon = std::cell::RefCell::new(None);
    app.connect_startup(move |app| {
        let Some(conn) = app.dbus_connection() else {
            tracing::error!("no session bus connection");
            return;
        };
        match Daemon::start(&conn) {
            Ok(d) => {
                *daemon.borrow_mut() = Some(d);
                // Runs until the session ends.
                std::mem::forget(app.hold());
            }
            Err(err) => tracing::error!(%err, "daemon failed to start"),
        }
    });
    app.connect_activate(|_| {});
    app.run_with_args::<&str>(&[])
}
