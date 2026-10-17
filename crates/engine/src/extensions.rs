//! Commands added by a product built on SolidCraft (an extension): registered once at start-up,
//! they join the built-in commands, so the toolbar, palette, scripts, CLI, MCP and control
//! channel all reach them as they reach any command. An id may be registered once; a second
//! registration, or an id a built-in command has, is refused.
//!
//! An extension's commands keep its model data in the design's extension data area
//! ([`solidcraft_doc::extension_data`]) through [`set_data`] and [`remove_data`].

use std::sync::{Mutex, RwLock};

use crate::cmd::CommandSpec;
pub use crate::cmd::extension_feature::add_extension_feature;
use crate::{EngineError, Result, Session};

/// The commands extensions registered, in registration order.
static REGISTERED: RwLock<Vec<&'static [CommandSpec]>> = RwLock::new(Vec::new());
/// Held while one registration checks its ids against everything registered before it.
static REGISTERING: Mutex<()> = Mutex::new(());

/// Add `specs` to the commands. Refused, with nothing added, when an id is empty, repeats among
/// `specs`, or is already a command.
pub fn register_commands(specs: &'static [CommandSpec]) -> Result<()> {
    let _one = REGISTERING.lock().map_err(|_| EngineError::Other("command registry is unavailable".into()))?;
    let taken: std::collections::HashSet<&str> = crate::cmd::command_specs().iter().map(|c| c.id).collect();
    let mut seen = std::collections::HashSet::new();
    for spec in specs {
        if spec.id.trim().is_empty() || !seen.insert(spec.id) || taken.contains(spec.id) {
            return Err(EngineError::Other(format!("command id `{}` is empty or already taken", spec.id)));
        }
    }
    REGISTERED.write().map_err(|_| EngineError::Other("command registry is unavailable".into()))?.push(specs);
    Ok(())
}

/// The commands extensions registered.
pub(crate) fn registered() -> Vec<&'static CommandSpec> {
    REGISTERED.read().map(|r| r.iter().flat_map(|s| s.iter()).collect()).unwrap_or_default()
}

/// Set `ext`'s values under `entries`' keys in the design's extension data (checked as
/// [`solidcraft_doc::extension_data::ExtensionData::set_many`] checks them). Called from a
/// command, a change is one undo step and marks the design changed; setting values the design
/// already holds changes nothing. Returns whether anything changed.
pub fn set_data(s: &mut Session, ext: &str, entries: Vec<(String, serde_json::Value)>) -> Result<bool> {
    let mut next = s.doc.extensions.clone();
    let changed = next.set_many(ext, entries)?;
    if changed {
        s.doc_mut().extensions = next;
    }
    Ok(changed)
}

/// Remove `ext`'s values under `keys` from the design's extension data, as [`set_data`] sets
/// them. Returns whether anything was there.
pub fn remove_data(s: &mut Session, ext: &str, keys: &[&str]) -> bool {
    let mut next = s.doc.extensions.clone();
    let mut changed = false;
    for key in keys {
        changed |= next.remove(ext, key);
    }
    if changed {
        s.doc_mut().extensions = next;
    }
    changed
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn echo(_: &mut Session, p: &Value) -> Result<Value> {
        Ok(p.clone())
    }

    static ECHO: [CommandSpec; 1] = [CommandSpec::new("test_ext.echo", "Echo", echo).params("anything → it, unchanged")];
    static CLASH: [CommandSpec; 1] = [CommandSpec::new("solid.box", "Box again", echo)];
    static TWICE: [CommandSpec; 2] = [CommandSpec::new("test_ext.twice", "One", echo), CommandSpec::new("test_ext.twice", "Two", echo)];

    #[test]
    fn registered_commands_run_like_built_in_ones() {
        // Other tests may have registered it already; a second registration is refused.
        let _ = register_commands(&ECHO);
        assert!(register_commands(&ECHO).is_err());
        assert_eq!(crate::cmd::find_command("test_ext.echo").map(|c| c.label), Some("Echo"));
        let mut s = Session::default();
        assert_eq!(s.execute("test_ext.echo", &json!({"a": 1})).unwrap(), json!({"a": 1}));
    }

    #[test]
    fn taken_and_repeated_ids_are_refused() {
        assert!(register_commands(&CLASH).is_err());
        assert!(register_commands(&TWICE).is_err());
        assert!(crate::cmd::find_command("test_ext.twice").is_none());
        assert_eq!(crate::cmd::find_command("solid.box").map(|c| c.label), Some("Box"));
    }

    const DATA_EXT: &str = "test_ext.data";

    fn data_put(s: &mut Session, p: &Value) -> Result<Value> {
        let key = p.get("key").and_then(Value::as_str).unwrap_or_default().to_owned();
        let changed = set_data(s, DATA_EXT, vec![(key, p.get("value").cloned().unwrap_or(Value::Null))])?;
        Ok(json!({"changed": changed}))
    }

    fn data_drop(s: &mut Session, p: &Value) -> Result<Value> {
        let key = p.get("key").and_then(Value::as_str).unwrap_or_default();
        Ok(json!({"changed": remove_data(s, DATA_EXT, &[key])}))
    }

    static DATA: [CommandSpec; 2] = [
        CommandSpec::new("test_ext.data_put", "Put", data_put).params("key, value → sets the test extension's value"),
        CommandSpec::new("test_ext.data_drop", "Drop", data_drop).params("key → removes the test extension's value"),
    ];

    /// A session with a box, the test extension's data version and its commands registered.
    fn data_session() -> Session {
        let _ = solidcraft_doc::extension_data::register_data_version(DATA_EXT, 1);
        let _ = register_commands(&DATA);
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        s
    }

    #[test]
    fn extension_data_changes_are_undone_and_never_recompute_the_model() {
        let mut s = data_session();
        let run = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).unwrap();
        let put = json!({"key": "covers/Body1", "value": {"top": 30}});
        assert_eq!(run(&mut s, "test_ext.data_put", put.clone())["changed"], true);
        assert_eq!(s.model.last_recomputed, 0, "the box is reused, not rebuilt");
        assert_eq!(s.doc.extensions.get(DATA_EXT, "covers/Body1"), Some((1, &json!({"top": 30}))));
        // Setting what the design holds is no change and no undo step.
        assert_eq!(run(&mut s, "test_ext.data_put", put)["changed"], false);
        run(&mut s, "edit.undo", json!({}));
        assert!(s.doc.extensions.is_empty());
        assert_eq!(s.doc.features.len(), 1, "one undo took back the one change");
        run(&mut s, "edit.redo", json!({}));
        assert_eq!(s.doc.extensions.get(DATA_EXT, "covers/Body1").map(|(_, v)| v.clone()), Some(json!({"top": 30})));
        // Refused values leave the design as it was.
        let before = s.doc.clone();
        assert!(s.execute("test_ext.data_put", &json!({"key": "far", "value": 1e300})).is_err());
        assert!(s.execute("test_ext.data_put", &json!({"key": "", "value": 1})).is_err());
        assert_eq!(s.doc, before);
        assert_eq!(run(&mut s, "test_ext.data_drop", json!({"key": "covers/Body1"}))["changed"], true);
        assert_eq!(run(&mut s, "test_ext.data_drop", json!({"key": "covers/Body1"}))["changed"], false);
        assert!(s.doc.extensions.is_empty());
    }

    #[test]
    fn extension_data_is_saved_and_reopened() {
        let mut s = data_session();
        s.execute("test_ext.data_put", &json!({"key": "settings", "value": {"grade": "C50", "covers": [30, 35.5]}})).unwrap();
        let dir = std::env::temp_dir().join(format!("solidcraft-ext-data-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let design = dir.join("part.solidcraft");
        s.execute("file.save_as", &json!({"path": design.to_string_lossy()})).unwrap();
        assert!(!s.is_dirty());
        let mut reopened = Session::default();
        reopened.execute("doc.open", &json!({"path": design.to_string_lossy()})).unwrap();
        assert_eq!(reopened.doc.extensions, s.doc.extensions);
        s.execute("test_ext.data_put", &json!({"key": "settings", "value": {"grade": "C40"}})).unwrap();
        assert!(s.is_dirty(), "a data change is an unsaved change");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
