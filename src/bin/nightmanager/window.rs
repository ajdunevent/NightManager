use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use chrono::Local;
use nightmanager::config::APP_ID;
use nightmanager::status;

use crate::client::{Client, Status};
use crate::rows;

struct Ui {
    client: RefCell<Option<Client>>,
    toasts: adw::ToastOverlay,
    banner: adw::Banner,
    page: adw::PreferencesPage,
    status_row: adw::SwitchRow,
    clock: Option<gio::Settings>,
    /// Set while the status row shows the daemon's state, so it isn't sent back.
    quiet: Cell<bool>,
}

impl Ui {
    fn toast(&self, text: &str) {
        self.toasts.add_toast(adw::Toast::new(text));
    }

    fn client(&self) -> Option<Client> {
        self.client.borrow().clone()
    }

    fn show_status(&self, status: Status) {
        self.quiet.set(true);
        self.status_row.set_active(status.active);
        self.quiet.set(false);
        let detail = status
            .schedule()
            .detail(Local::now().date_naive(), status::clock_24h(self.clock.as_ref()));
        self.status_row.set_subtitle(&rows::capitalize(&detail));
        self.banner.set_revealed(status.legacy_installed);
        self.page.set_sensitive(!status.legacy_installed);
    }

    fn toggle_night(self: &Rc<Self>, on: bool) {
        let Some(client) = self.client() else { return };
        let ui = self.clone();
        glib::spawn_future_local(async move {
            let method = if on { "StartNight" } else { "EndNight" };
            if client.call(method).await.is_none() {
                ui.show_status(client.status());
                ui.toast("The background service didn't respond");
            }
        });
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
        client: RefCell::default(),
        toasts: toasts.clone(),
        banner: banner.clone(),
        page: page.clone(),
        status_row: status_row.clone(),
        clock: status::clock_settings(),
        quiet: Cell::new(false),
    });
    if let Some(clock) = &ui.clock {
        let weak = Rc::downgrade(&ui);
        clock.connect_changed(Some("clock-format"), move |_, _| {
            if let Some(ui) = weak.upgrade() {
                if let Some(client) = ui.client() {
                    ui.show_status(client.status());
                }
            }
        });
    }

    let status_group = adw::PreferencesGroup::new();
    status_group.add(&status_row);
    page.add(&status_group);
    page.add(&crate::night::group(&settings));
    page.add(&crate::schedule::group(&settings));
    page.add(&crate::triggers::group(&settings, &toasts));
    page.add(&crate::screen_off::group(&settings));
    page.add(&crate::background::group(&settings, &toasts));

    status_row.connect_active_notify({
        let ui = ui.clone();
        move |row| {
            if !ui.quiet.get() {
                ui.toggle_night(row.is_active());
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
                    client.call_timeout("Quit", None, 3000).await;
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
    about.add_acknowledgement_section(Some("With thanks to"), &["FuriLabs — for the FLX1s"]);
    about.present();
}
