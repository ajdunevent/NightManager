//! Row helpers shared by the window and the Daytime Settings dialog.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;

pub fn switch(settings: &gio::Settings, key: &str, title: &str) -> adw::SwitchRow {
    let row = adw::SwitchRow::builder().title(title).build();
    settings.bind(key, &row, "active").build();
    row
}

/// A combo row over string values. `set` shows a value without reporting it,
/// and adds values missing from the list, such as an uninstalled theme.
#[derive(Clone)]
pub struct Choice {
    pub row: adw::ComboRow,
    inner: Rc<Inner>,
}

struct Inner {
    list: gtk::StringList,
    values: RefCell<Vec<String>>,
    quiet: Cell<bool>,
}

impl Choice {
    pub fn new(title: &str, options: Vec<(String, String)>, on_select: impl Fn(&str) + 'static) -> Choice {
        let labels: Vec<&str> = options.iter().map(|(_, l)| l.as_str()).collect();
        let list = gtk::StringList::new(&labels);
        let row = adw::ComboRow::builder().title(title).model(&list).build();
        let inner = Rc::new(Inner {
            list,
            values: RefCell::new(options.into_iter().map(|(v, _)| v).collect()),
            quiet: Cell::new(false),
        });
        row.connect_selected_notify({
            let inner = inner.clone();
            move |row| {
                if inner.quiet.get() {
                    return;
                }
                let value = inner.values.borrow().get(row.selected() as usize).cloned();
                if let Some(v) = value {
                    on_select(&v);
                }
            }
        });
        Choice { row, inner }
    }

    pub fn bound(
        settings: &gio::Settings,
        key: &'static str,
        title: &str,
        options: Vec<(String, String)>,
    ) -> Choice {
        let s = settings.clone();
        let choice = Choice::new(title, options, move |v| {
            if s.string(key) != v {
                let _ = s.set_string(key, v);
            }
        });
        choice.set(&settings.string(key));
        let shown = choice.clone();
        settings.connect_changed(Some(key), move |s, k| shown.set(&s.string(k)));
        choice
    }

    pub fn set(&self, value: &str) {
        let inner = &self.inner;
        let index = inner.values.borrow().iter().position(|v| v == value);
        let index = index.unwrap_or_else(|| {
            inner.list.append(value);
            inner.values.borrow_mut().push(value.to_string());
            inner.values.borrow().len() - 1
        });
        if self.row.selected() != index as u32 {
            inner.quiet.set(true);
            self.row.set_selected(index as u32);
            inner.quiet.set(false);
        }
    }
}

pub fn with_no_change(items: Vec<String>, label: fn(&str) -> String) -> Vec<(String, String)> {
    std::iter::once((String::new(), "No Change".to_string()))
        .chain(labeled(items, label))
        .collect()
}

pub fn labeled(items: Vec<String>, label: fn(&str) -> String) -> Vec<(String, String)> {
    items.into_iter().map(|i| (label(&i), i)).map(|(l, i)| (i, l)).collect()
}

pub fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

pub fn show_when(settings: &gio::Settings, key: &'static str, value: &'static str, widget: &impl IsA<gtk::Widget>) {
    let widget = widget.clone().upcast::<gtk::Widget>();
    widget.set_visible(settings.string(key) == value);
    settings.connect_changed(Some(key), move |s, k| widget.set_visible(s.string(k) == value));
}
