use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::keys::HostKey;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub active: bool,
    /// Day values captured when night mode started, as `Variant::print` text.
    /// `None` means the key had no user value and is reset in the morning.
    #[serde(default)]
    pub day: BTreeMap<HostKey, Option<String>>,
    /// Unix time of the last schedule boundary acted on.
    #[serde(default)]
    pub last_boundary: Option<i64>,
    #[serde(default)]
    pub legacy_imported: bool,
}

impl State {
    pub fn default_path() -> PathBuf {
        glib::user_state_dir().join("nightmanager").join("state.json")
    }

    pub fn load(path: &Path) -> State {
        match fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|err| {
                tracing::warn!(%err, "state file unreadable, starting fresh");
                State::default()
            }),
            Err(_) => State::default(),
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self).map_err(io::Error::other)?)?;
        fs::rename(tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("nm-state-{}", std::process::id()));
        let path = dir.join("state.json");
        let mut s = State {
            active: true,
            last_boundary: Some(1_800_000_000),
            ..Default::default()
        };
        s.day.insert(HostKey::AccentColor, Some("'blue'".into()));
        s.day.insert(HostKey::NightLight, None);
        s.save(&path).unwrap();
        assert_eq!(State::load(&path), s);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn missing_or_corrupt_file_gives_default() {
        let dir = std::env::temp_dir().join(format!("nm-state-bad-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.json");
        assert_eq!(State::load(&path), State::default());
        fs::write(&path, "{nope").unwrap();
        assert_eq!(State::load(&path), State::default());
        fs::remove_dir_all(dir).ok();
    }
}
