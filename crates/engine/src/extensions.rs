//! Commands added by a product built on SolidCraft (an extension): registered once at start-up,
//! they join the built-in commands, so the toolbar, palette, scripts, CLI, MCP and control
//! channel all reach them as they reach any command. An id may be registered once; a second
//! registration, or an id a built-in command has, is refused.

use std::sync::{Mutex, RwLock};

use crate::cmd::CommandSpec;
use crate::{EngineError, Result};

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

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::Session;

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
}
