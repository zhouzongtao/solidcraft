//! Parameter packs of an extension command's form (`crate::extensions::presets`): list, save,
//! delete and import them. The packs are the user's, kept beside the recovery folder, so these
//! commands never change the design or its undo history.

use serde_json::{Value, json};

use super::CommandSpec;
use crate::extensions::{Form, form, presets};
use crate::params::{bad, str_};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("preset.list", "Parameter Packs", list)
        .icon("settings")
        .noundo()
        .params("command: an extension command with a form → packs: [{name, version, values}], in the order they were saved"),
    CommandSpec::new("preset.save", "Save Parameter Pack", save)
        .icon("settings")
        .noundo()
        .params("command, name, values: {field id: value} (as the command takes them) → replaced: whether one of that name was there"),
    CommandSpec::new("preset.delete", "Delete Parameter Pack", delete).icon("settings").noundo().params("command, name → deleted"),
    CommandSpec::new("preset.import", "Import Parameter Packs", import)
        .icon("settings")
        .noundo()
        .params("command, path: another program's parameter file, read by the command's extension → kept: [{kind, name}], notes"),
];

/// The form of the command `p` names.
fn form_of(p: &Value, cmd: &str) -> Result<&'static Form> {
    let id = str_(p, "command").ok_or_else(|| bad(cmd, "`command` must name an extension command"))?;
    form(id).ok_or_else(|| bad(cmd, format!("`{}` has no form", id.chars().take(64).collect::<String>())))
}

fn list(_: &mut Session, p: &Value) -> Result<Value> {
    let f = form_of(p, "preset.list")?;
    Ok(json!({"packs": presets::list(f)?}))
}

fn save(_: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "preset.save";
    let f = form_of(p, cmd)?;
    let name = str_(p, "name").ok_or_else(|| bad(cmd, "`name` must be text"))?;
    let values = p.get("values").and_then(Value::as_object).cloned().ok_or_else(|| bad(cmd, "`values` must be an object"))?;
    Ok(json!({"replaced": presets::save(f, name, values)?}))
}

fn delete(_: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "preset.delete";
    let f = form_of(p, cmd)?;
    let name = str_(p, "name").ok_or_else(|| bad(cmd, "`name` must be text"))?;
    Ok(json!({"deleted": presets::delete(f, name)?}))
}

fn import(_: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "preset.import";
    let f = form_of(p, cmd)?;
    let path = str_(p, "path").ok_or_else(|| bad(cmd, "`path` must be a file"))?;
    let (kept, notes) = presets::import(&f.ext, path)?;
    let kept: Vec<Value> = kept.into_iter().map(|(kind, name)| json!({"kind": kind, "name": name})).collect();
    Ok(json!({"kept": kept, "notes": notes}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::forms::tests::setup;

    #[test]
    fn packs_are_saved_listed_and_deleted_without_touching_the_design() {
        setup();
        let mut s = Session::default();
        let run = |s: &mut Session, id: &str, p: Value| s.execute(id, &p);
        let pack = json!({"command": "test_forms.slab", "name": "Cmd pack", "values": {"w": "33 mm", "thick": true}});
        assert_eq!(run(&mut s, "preset.save", pack.clone()).unwrap()["replaced"], false);
        assert_eq!(run(&mut s, "preset.save", pack).unwrap()["replaced"], true);
        let packs = run(&mut s, "preset.list", json!({"command": "test_forms.slab"})).unwrap();
        let mine: Vec<&Value> = packs["packs"].as_array().unwrap().iter().filter(|p| p["name"] == "Cmd pack").collect();
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0]["values"]["w"], "33 mm");
        for bad in [
            json!({"command": "test_forms.bare", "name": "x", "values": {}}),
            json!({"command": "test_forms.slab", "name": "x", "values": {"nothing": 1}}),
            json!({"command": "test_forms.slab", "values": {}}),
            json!({"name": "x", "values": {}}),
        ] {
            assert!(run(&mut s, "preset.save", bad.clone()).is_err(), "{bad}");
        }
        assert_eq!(run(&mut s, "preset.delete", json!({"command": "test_forms.slab", "name": "Cmd pack"})).unwrap()["deleted"], true);
        assert!(s.undo.is_empty() && !s.is_dirty(), "packs are not the design's");
    }
    /// Hostile values never panic the pack commands, alone or beside a form's real command.
    #[test]
    fn hostile_parameters_are_refused_without_a_panic() {
        setup();
        let mut s = Session::default();
        for spec in COMMANDS {
            for v in crate::tests::hostile_values() {
                for key in ["command", "name", "values", "path"] {
                    for base in [json!({}), json!({"command": "test_forms.slab", "name": "hostile", "values": {}})] {
                        let mut p = base;
                        p[key] = v.clone();
                        if let Err(crate::EngineError::Internal(id, msg)) = s.execute(spec.id, &p) {
                            panic!("{id} panicked on {p}: {msg}");
                        }
                    }
                }
            }
        }
    }
}
