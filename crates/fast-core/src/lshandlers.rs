//! The Launch Services preferences file (`com.apple.launchservices.secure`), as far as the handler for folders goes.
//!
//! Current macOS refuses `LSSetDefaultRoleHandlerForContentType` for `public.folder` (it answers -50). The way other
//! file managers take over folders is to write the same preference that call would have written: an entry in
//! `LSHandlers` with `LSHandlerContentType` = `public.folder`. This file only edits the dictionary; reading and
//! writing it through `defaults` (so the preferences daemon sees the change) is in `filemanager_macos.rs`.

use plist::{Dictionary, Value};

const FOLDER: &str = "public.folder";

fn is_folder_entry(v: &Value) -> bool {
    v.as_dictionary()
        .and_then(|d| d.get("LSHandlerContentType"))
        .and_then(Value::as_string)
        .is_some_and(|t| t.eq_ignore_ascii_case(FOLDER))
}

/// The program the dictionary names for folders, if any.
pub fn folder_handler(dict: &Dictionary) -> Option<String> {
    let list = dict.get("LSHandlers")?.as_array()?;
    list.iter().filter(|v| is_folder_entry(v)).find_map(|v| {
        let d = v.as_dictionary()?;
        ["LSHandlerRoleAll", "LSHandlerRoleViewer"].iter().find_map(|k| d.get(k).and_then(Value::as_string).filter(|s| !s.is_empty()).map(str::to_string))
    })
}

/// The dictionary with the folder entry replaced by one naming `handler`, or removed when `handler` is None. Entries for
/// every other kind of file are kept as they were.
pub fn with_folder_handler(mut dict: Dictionary, handler: Option<&str>) -> Dictionary {
    let mut list: Vec<Value> = match dict.remove("LSHandlers") {
        Some(Value::Array(a)) => a,
        _ => Vec::new(),
    };
    list.retain(|v| !is_folder_entry(v));
    if let Some(id) = handler {
        let mut entry = Dictionary::new();
        entry.insert("LSHandlerContentType".into(), Value::String(FOLDER.into()));
        entry.insert("LSHandlerRoleAll".into(), Value::String(id.into()));
        let mut versions = Dictionary::new();
        versions.insert("LSHandlerRoleAll".into(), Value::String("-".into()));
        entry.insert("LSHandlerPreferredVersions".into(), Value::Dictionary(versions));
        list.push(Value::Dictionary(entry));
    }
    dict.insert("LSHandlers".into(), Value::Array(list));
    dict
}

#[cfg(test)]
mod tests {
    use super::*;

    fn other(kind: &str, id: &str) -> Value {
        let mut d = Dictionary::new();
        d.insert("LSHandlerContentType".into(), Value::String(kind.into()));
        d.insert("LSHandlerRoleAll".into(), Value::String(id.into()));
        Value::Dictionary(d)
    }

    #[test]
    fn the_folder_entry_is_replaced_and_the_others_are_kept() {
        let mut dict = Dictionary::new();
        dict.insert("LSHandlers".into(), Value::Array(vec![other("public.html", "com.google.chrome"), other("public.folder", "com.old.thing"), other("public.folder", "com.dup")]));
        dict.insert("LSQuarantine".into(), Value::Boolean(true));
        assert_eq!(folder_handler(&dict).as_deref(), Some("com.old.thing"));
        let now = with_folder_handler(dict, Some("dev.example.app"));
        assert_eq!(folder_handler(&now).as_deref(), Some("dev.example.app"));
        let list = now.get("LSHandlers").unwrap().as_array().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list.iter().filter(|v| is_folder_entry(v)).count(), 1);
        assert!(now.get("LSQuarantine").is_some());
    }

    #[test]
    fn removing_and_starting_from_nothing_work() {
        let fresh = with_folder_handler(Dictionary::new(), Some("a.b"));
        assert_eq!(folder_handler(&fresh).as_deref(), Some("a.b"));
        let gone = with_folder_handler(fresh, None);
        assert_eq!(folder_handler(&gone), None);
        assert!(gone.get("LSHandlers").unwrap().as_array().unwrap().is_empty());
    }

    #[test]
    fn it_survives_a_round_trip_through_the_xml_form() {
        let dict = with_folder_handler(Dictionary::new(), Some("a.b"));
        let mut xml = Vec::new();
        Value::Dictionary(dict).to_writer_xml(&mut xml).unwrap();
        let back = Value::from_reader(std::io::Cursor::new(xml)).unwrap().into_dictionary().unwrap();
        assert_eq!(folder_handler(&back).as_deref(), Some("a.b"));
    }
}
