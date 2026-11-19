//! Chosen pictures are copied into the app's data folder, which Phosh reads at
//! the same path. File chooser portal paths can stop resolving after a reboot.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

/// Replaces the previous copy for `role` and returns the new copy's `file://` URI.
pub async fn choose(parent: Option<gtk::Window>, role: &str) -> Option<String> {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Pictures"));
    filter.add_mime_type("image/*");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let dialog = gtk::FileDialog::builder()
        .title("Choose a Picture")
        .filters(&filters)
        .default_filter(&filter)
        .modal(true)
        .build();
    let file = dialog.open_future(parent.as_ref()).await.ok()?;
    let (bytes, _) = match file.load_contents_future().await {
        Ok(contents) => contents,
        Err(err) => {
            tracing::warn!(%err, "reading the picture failed");
            return None;
        }
    };
    let dir = glib::user_data_dir().join("nightmanager").join("pictures");
    if let Err(err) = std::fs::create_dir_all(&dir) {
        tracing::warn!(%err, "creating the pictures folder failed");
        return None;
    }
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with(&format!("{role}-")) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    let ext = file
        .basename()
        .and_then(|b| b.extension().map(|e| e.to_string_lossy().to_lowercase()))
        .unwrap_or_else(|| "img".into());
    let path = dir.join(format!("{role}-{}.{ext}", glib::real_time()));
    if let Err(err) = std::fs::write(&path, &bytes) {
        tracing::warn!(%err, "saving the picture failed");
        return None;
    }
    Some(gio::File::for_path(&path).uri().to_string())
}

/// Reloads when the picture file changes on disk.
pub struct Thumb {
    /// A Picture in a row grew to its image's size, so it overlays a box of the
    /// thumbnail's size.
    pub frame: gtk::Overlay,
    picture: gtk::Picture,
    pub none: gtk::Label,
    monitor: RefCell<Option<gio::FileMonitor>>,
}

impl Thumb {
    pub fn new() -> Rc<Thumb> {
        add_css();
        let picture = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Cover)
            .can_shrink(true)
            .build();
        let size = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let frame = gtk::Overlay::builder()
            .child(&size)
            .valign(gtk::Align::Center)
            .overflow(gtk::Overflow::Hidden)
            .css_classes(["card", "thumbnail"])
            .build();
        frame.add_overlay(&picture);
        fit_to_screen(&size);
        let none = gtk::Label::builder().label("None").css_classes(["dim-label"]).build();
        Rc::new(Thumb {
            frame,
            picture,
            none,
            monitor: RefCell::default(),
        })
    }

    pub fn show(self: &Rc<Self>, uri: &str) {
        self.monitor.borrow_mut().take();
        self.frame.set_visible(!uri.is_empty());
        self.none.set_visible(uri.is_empty());
        self.picture.set_file(None::<&gio::File>);
        if uri.is_empty() {
            return;
        }
        let file = gio::File::for_uri(uri);
        self.picture.set_file(Some(&file));
        let Ok(monitor) = file.monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE) else {
            return;
        };
        let weak = Rc::downgrade(self);
        monitor.connect_changed(move |_, file, _, event| {
            if matches!(event, gio::FileMonitorEvent::ChangesDoneHint | gio::FileMonitorEvent::Created) {
                if let Some(thumb) = weak.upgrade() {
                    thumb.picture.set_file(None::<&gio::File>);
                    thumb.picture.set_file(Some(file));
                }
            }
        });
        *self.monitor.borrow_mut() = Some(monitor);
    }
}

/// The card style's radius turned a portrait thumbnail into a pill.
fn add_css() {
    thread_local!(static ADDED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
    if ADDED.with(|a| a.replace(true)) {
        return;
    }
    let Some(display) = gtk::gdk::Display::default() else { return };
    let provider = gtk::CssProvider::new();
    provider.load_from_data(".thumbnail { border-radius: 4px; }");
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

/// Sizes `size` to the screen's proportions, following rotation.
fn fit_to_screen(size: &gtk::Box) {
    const LONGEST: i32 = 48;
    let resize = {
        let size = size.clone();
        move |monitor: &gtk::gdk::Monitor| {
            let g = monitor.geometry();
            let (w, h) = (g.width().max(1), g.height().max(1));
            let (width, height) = if h >= w {
                (LONGEST * w / h, LONGEST)
            } else {
                (LONGEST, LONGEST * h / w)
            };
            size.set_size_request(width, height);
        }
    };
    let monitor = gtk::gdk::Display::default()
        .and_then(|d| d.monitors().item(0))
        .and_downcast::<gtk::gdk::Monitor>();
    match monitor {
        Some(monitor) => {
            resize(&monitor);
            let weak = size.downgrade();
            let id = monitor.connect_geometry_notify(move |m| {
                if weak.upgrade().is_some() {
                    resize(m);
                }
            });
            let monitor = monitor.clone();
            let id = std::cell::Cell::new(Some(id));
            size.connect_destroy(move |_| {
                if let Some(id) = id.take() {
                    monitor.disconnect(id);
                }
            });
        }
        None => size.set_size_request(56, 36),
    }
}

pub fn window_of(widget: &impl IsA<gtk::Widget>) -> Option<gtk::Window> {
    widget.root().and_downcast::<gtk::Window>()
}
