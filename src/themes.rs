use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};


/// Host theme directories. Inside a Flatpak the system ones are under /run/host
/// (host-os access) and the user ones are reached through the home directory.
fn search_dirs(kind: &str) -> Vec<PathBuf> {
    let home = glib::home_dir();
    let mut dirs = vec![
        PathBuf::from("/run/host/usr/share").join(kind),
        PathBuf::from("/usr/share").join(kind),
        home.join(".local/share").join(kind),
    ];
    dirs.push(home.join(format!(".{kind}")));
    dirs
}

fn names_where(kind: &str, keep: impl Fn(&Path) -> bool) -> Vec<String> {
    let mut out = BTreeSet::new();
    for dir in search_dirs(kind) {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if keep(&path) {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    out.insert(name.to_string());
                }
            }
        }
    }
    out.into_iter().collect()
}

pub fn gtk3_themes() -> Vec<String> {
    names_where("themes", |p| p.join("gtk-3.0").is_dir())
}

/// Icon themes, leaving out cursor-only themes and the hicolor fallback.
pub fn icon_themes() -> Vec<String> {
    names_where("icons", |p| {
        if p.file_name().is_some_and(|n| n == "hicolor" || n == "default") {
            return false;
        }
        fs::read_to_string(p.join("index.theme"))
            .map(|t| t.lines().any(|l| l.trim_start().starts_with("Directories=")))
            .unwrap_or(false)
    })
}

/// The accent colors the host's org.gnome.desktop.interface schema allows.
pub fn accent_colors() -> Vec<String> {
    let Some(schema) = gio::SettingsSchemaSource::default()
        .and_then(|s| s.lookup(crate::keys::INTERFACE_SCHEMA, true))
    else {
        return Vec::new();
    };
    if !schema.has_key("accent-color") {
        return Vec::new();
    }
    let range = schema.key("accent-color").range();
    range
        .child_value(1)
        .as_variant()
        .and_then(|v| v.get::<Vec<String>>())
        .unwrap_or_default()
}
