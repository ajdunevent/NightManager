use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gio::prelude::*;

use crate::keys::{parse_value, print_value, HostKey};

pub struct Host {
    settings: HashMap<&'static str, gio::Settings>,
    /// Values this process wrote and whose change notification is still due.
    own_writes: Rc<RefCell<HashMap<HostKey, glib::Variant>>>,
}

impl Host {
    pub fn new() -> Host {
        let mut settings = HashMap::new();
        if let Some(source) = gio::SettingsSchemaSource::default() {
            for key in HostKey::ALL {
                if settings.contains_key(key.schema()) {
                    continue;
                }
                if source.lookup(key.schema(), true).is_some() {
                    settings.insert(key.schema(), gio::Settings::new(key.schema()));
                }
            }
        }
        Host {
            settings,
            own_writes: Rc::default(),
        }
    }

    fn settings(&self, key: HostKey) -> Option<&gio::Settings> {
        let s = self.settings.get(key.schema())?;
        s.settings_schema()?.has_key(key.key()).then_some(s)
    }

    pub fn supports(&self, key: HostKey) -> bool {
        self.settings(key).is_some()
    }

    pub fn value(&self, key: HostKey) -> Option<glib::Variant> {
        Some(self.settings(key)?.value(key.key()))
    }

    pub fn default_value(&self, key: HostKey) -> Option<glib::Variant> {
        self.settings(key)?.default_value(key.key())
    }

    /// The value to save for a morning restore. `Some(None)` means the key has no
    /// user value and gets reset.
    pub fn saved_value(&self, key: HostKey) -> Option<Option<String>> {
        let s = self.settings(key)?;
        Some(s.user_value(key.key()).map(|v| print_value(&v)))
    }

    /// Writes `value` unless the key already holds it. Returns whether it wrote.
    pub fn set(&self, key: HostKey, value: &glib::Variant) -> bool {
        let Some(s) = self.settings(key) else {
            return false;
        };
        if s.value(key.key()) == *value {
            return false;
        }
        self.own_writes.borrow_mut().insert(key, value.clone());
        if let Err(err) = s.set_value(key.key(), value) {
            self.own_writes.borrow_mut().remove(&key);
            tracing::warn!(?key, %err, "write failed");
            return false;
        }
        true
    }

    pub fn restore(&self, key: HostKey, saved: &Option<String>) {
        let Some(s) = self.settings(key) else {
            return;
        };
        match saved.as_deref().and_then(|t| parse_value(key, t)) {
            Some(value) => {
                self.set(key, &value);
            }
            None => {
                if s.user_value(key.key()).is_none() {
                    return;
                }
                if let Some(default) = s.default_value(key.key()) {
                    self.own_writes.borrow_mut().insert(key, default);
                }
                s.reset(key.key());
            }
        }
    }

    /// Calls `f` with the new value when something other than this process
    /// changes `key`.
    pub fn connect_changed(&self, key: HostKey, f: impl Fn(HostKey, glib::Variant) + 'static) {
        let Some(s) = self.settings(key) else {
            return;
        };
        let own_writes = self.own_writes.clone();
        s.connect_changed(Some(key.key()), move |s, k| {
            let value = s.value(k);
            let mut own = own_writes.borrow_mut();
            if own.get(&key) == Some(&value) {
                own.remove(&key);
                return;
            }
            drop(own);
            f(key, value);
        });
    }
}

impl Default for Host {
    fn default() -> Self {
        Self::new()
    }
}
