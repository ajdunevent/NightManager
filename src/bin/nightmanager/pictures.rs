//! The file chooser portal returns the real path for a file the app can
//! already read, which home:ro covers; Phosh then reads the same path.

use adw::prelude::*;

pub async fn choose(parent: Option<gtk::Window>) -> Option<String> {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Pictures"));
    filter.add_mime_type("image/*");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    // Modal, the window at times ignored input for about 25 s after a pick.
    let dialog = gtk::FileDialog::builder()
        .title("Choose a Picture")
        .filters(&filters)
        .default_filter(&filter)
        .modal(false)
        .build();
    let started = std::time::Instant::now();
    match dialog.open_future(parent.as_ref()).await {
        Ok(file) => {
            tracing::debug!(ms = started.elapsed().as_millis() as u64, uri = %file.uri(), "picture chosen");
            Some(file.uri().to_string())
        }
        Err(err) => {
            tracing::debug!(%err, ms = started.elapsed().as_millis() as u64, "no picture chosen");
            None
        }
    }
}

pub fn display_name(uri: &str) -> String {
    gio::File::for_uri(uri)
        .basename()
        .map(|b| b.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn window_of(widget: &impl IsA<gtk::Widget>) -> Option<gtk::Window> {
    widget.root().and_downcast::<gtk::Window>()
}
