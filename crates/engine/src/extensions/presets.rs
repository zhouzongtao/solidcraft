//! Parameter packs: named sets of a form's values, kept per user by extension and feature kind
//! (one file per extension, `<ext>.json`, in the packs folder). A pack records the form's
//! version: an older pack fills the fields it has, a newer one is not applied. An extension may
//! register an importer for another program's parameter files ([`register_importer`]); importing
//! adds the packs it reads, never replacing one already kept.
//!
//! The packs are the user's, not the design's: changing them is not an undo step and never
//! marks a design changed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::forms::{FieldType, Form, form_for};
use crate::{EngineError, Result};

/// The packs file's own format (a newer one is refused, not overwritten).
const FORMAT: u64 = 1;
/// Most packs of one kind, longest name (bytes), largest file and text value.
pub const MAX_PACKS: usize = 200;
pub const MAX_NAME: usize = 64;
const MAX_FILE: usize = 4 << 20;
const MAX_TEXT: usize = 4096;

/// One named set of a form's values.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    /// The form's version when it was saved.
    pub version: u64,
    /// By field id, as the form's command takes them.
    pub values: Map<String, Value>,
}

/// An extension's packs file: feature kind -> its packs, in the order they were saved.
#[derive(Debug, Serialize, Deserialize)]
struct Store {
    format: u64,
    #[serde(default)]
    kinds: BTreeMap<String, Vec<Preset>>,
}

/// Where the packs are kept, when not the default ([`set_dir`]).
static DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// One writer at a time: a save, delete or import reads the file, changes it and writes it back,
/// and two at once would lose one's change.
static WRITER: Mutex<()> = Mutex::new(());

fn one_writer() -> std::sync::MutexGuard<'static, ()> {
    // A writer that panicked left the file whole (writes are atomic): carry on.
    WRITER.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Keep the packs in `dir` (`None`: the default folder) from now on, in this process.
pub fn set_dir(dir: Option<PathBuf>) {
    if let Ok(mut d) = DIR.lock() {
        *d = dir;
    }
}

/// The packs folder: [`set_dir`]'s, else `SOLIDCRAFT_PRESETS_DIR`, else `presets` next to the
/// recovery folder (so `SOLIDCRAFT_RECOVERY_DIR` moves it too).
pub fn default_dir() -> Option<PathBuf> {
    if let Some(d) = DIR.lock().ok().and_then(|d| d.clone()) {
        return Some(d);
    }
    // Tests never touch the user's real packs.
    if cfg!(test) {
        return Some(solidcraft_io::vfs::temp_dir().join(format!("solidcraft-test-presets-{}", std::process::id())));
    }
    if let Some(d) = std::env::var_os("SOLIDCRAFT_PRESETS_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    crate::recovery::default_dir().and_then(|d| d.parent().map(|p| p.join("presets")))
}

fn dir() -> Result<PathBuf> {
    default_dir().ok_or_else(|| EngineError::Other("there is no folder to keep parameter packs in".into()))
}

fn file(dir: &Path, ext: &str) -> Result<PathBuf> {
    if !solidcraft_doc::extension_data::is_extension_id(ext) {
        return Err(EngineError::Other(format!("`{}` is not an extension id", shown(ext))));
    }
    Ok(dir.join(format!("{ext}.json")))
}

/// The extension's packs as kept: none when there is no file yet; an error when it is damaged
/// or newer, so it is never overwritten.
fn load(dir: &Path, ext: &str) -> Result<Store> {
    let path = file(dir, ext)?;
    let shown_path = path.to_string_lossy().to_string();
    // The app's own folder: reached whatever an agent's file scope is.
    let bytes = match solidcraft_io::vfs::scoped(None, || solidcraft_io::vfs::read(&shown_path, MAX_FILE)) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Store { format: FORMAT, kinds: BTreeMap::new() }),
        Err(e) => return Err(EngineError::Other(format!("{shown_path}: {e}"))),
    };
    let store: Store =
        serde_json::from_slice(&bytes).map_err(|e| EngineError::Other(format!("{shown_path} is damaged ({e}); it was left as it is")))?;
    if store.format > FORMAT {
        return Err(EngineError::Other(format!(
            "{shown_path} was written by a newer version (format {}): update the program to change it",
            store.format
        )));
    }
    Ok(store)
}

fn store(dir: &Path, ext: &str, s: &Store) -> Result<()> {
    let path = file(dir, ext)?;
    let bytes = serde_json::to_vec_pretty(s).map_err(|e| EngineError::Other(format!("parameter packs: {e}")))?;
    if bytes.len() > MAX_FILE {
        return Err(EngineError::Other(format!("the parameter packs of `{ext}` would pass {} MiB", MAX_FILE >> 20)));
    }
    #[cfg(not(target_arch = "wasm32"))]
    std::fs::create_dir_all(dir).map_err(|e| EngineError::Other(format!("{}: {e}", dir.display())))?;
    solidcraft_io::vfs::scoped(None, || solidcraft_io::safe_write::write_atomic(&path, &bytes, false))
        .map_err(|e| EngineError::Other(format!("{}: {e}", path.display())))
}

/// Text from outside, shortened and escaped for a message.
fn shown(s: &str) -> String {
    s.chars().take(64).collect::<String>().escape_debug().to_string()
}

/// The packs of `form`'s feature kind, in the order they were saved.
pub fn list(form: &Form) -> Result<Vec<Preset>> {
    list_in(&dir()?, form)
}

pub fn list_in(dir: &Path, form: &Form) -> Result<Vec<Preset>> {
    Ok(load(dir, &form.ext)?.kinds.remove(&form.kind).unwrap_or_default())
}

/// Keep `values` as the pack `name` of `form`'s kind, replacing one of that name. Refused for a
/// bad name, a value no field of the form takes ([`check_values`]), or past [`MAX_PACKS`].
/// Returns whether it replaced one.
pub fn save(form: &Form, name: &str, values: Map<String, Value>) -> Result<bool> {
    save_in(&dir()?, form, name, values)
}

pub fn save_in(dir: &Path, form: &Form, name: &str, values: Map<String, Value>) -> Result<bool> {
    let _one = one_writer();
    save_unlocked(dir, form, name, values)
}

fn save_unlocked(dir: &Path, form: &Form, name: &str, values: Map<String, Value>) -> Result<bool> {
    let name = check_name(name)?;
    check_values(form, &values)?;
    let mut s = load(dir, &form.ext)?;
    let packs = s.kinds.entry(form.kind.clone()).or_default();
    let pack = Preset { name: name.to_owned(), version: form.version, values };
    let replaced = match packs.iter().position(|p| p.name == name) {
        Some(i) => {
            if let Some(p) = packs.get_mut(i) {
                *p = pack;
            }
            true
        }
        None if packs.len() >= MAX_PACKS => return Err(EngineError::Other(format!("`{}` keeps at most {MAX_PACKS} parameter packs", form.kind))),
        None => {
            packs.push(pack);
            false
        }
    };
    store(dir, &form.ext, &s)?;
    Ok(replaced)
}

/// Remove the pack `name` of `form`'s kind. Returns whether it was there.
pub fn delete(form: &Form, name: &str) -> Result<bool> {
    delete_in(&dir()?, form, name)
}

pub fn delete_in(dir: &Path, form: &Form, name: &str) -> Result<bool> {
    let _one = one_writer();
    let mut s = load(dir, &form.ext)?;
    let Some(packs) = s.kinds.get_mut(&form.kind) else { return Ok(false) };
    let before = packs.len();
    packs.retain(|p| p.name != name.trim());
    if packs.len() == before {
        return Ok(false);
    }
    if packs.is_empty() {
        s.kinds.remove(&form.kind);
    }
    store(dir, &form.ext, &s)?;
    Ok(true)
}

/// `name` as a pack's name: trimmed, 1 to [`MAX_NAME`] bytes, no control characters.
fn check_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() || name.len() > MAX_NAME || name.chars().any(char::is_control) {
        return Err(EngineError::Other(format!("`{}` can't name a parameter pack (1 to {MAX_NAME} bytes, no control characters)", shown(name))));
    }
    Ok(name)
}

/// Every value a field of `form` takes as its command does: a number's expression (text or a
/// number), a list's text, a choice's option id, a switch's true or false.
pub fn check_values(form: &Form, values: &Map<String, Value>) -> Result<()> {
    for (id, v) in values {
        let field = form.field(id).ok_or_else(|| EngineError::Other(format!("`{}` has no field `{}`", form.command, shown(id))))?;
        let text = |s: &str| s.len() <= MAX_TEXT && !s.chars().any(char::is_control);
        let ok = match field.ty {
            FieldType::Switch => v.is_boolean(),
            FieldType::Choice => v.as_str().is_some_and(|o| field.options.iter().any(|(id, _)| id == o)),
            FieldType::Lengths => v.as_str().is_some_and(text),
            FieldType::Length | FieldType::Angle | FieldType::Number | FieldType::Integer => v.as_str().is_some_and(text) || v.is_number(),
        };
        if !ok {
            return Err(EngineError::Other(format!("field `{id}` of `{}` doesn't take {v}", form.command)));
        }
    }
    Ok(())
}

/// What an importer read from another program's parameter file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Imported {
    /// (feature kind, pack name, values by field id).
    pub packs: Vec<(String, String, Map<String, Value>)>,
    /// What it could not carry over, for the user.
    pub notes: Vec<String>,
}

/// Reads another program's parameter file (`file_stem`: its name, for a pack that has none).
pub type Importer = fn(file: &Value, file_stem: &str) -> std::result::Result<Imported, String>;

static IMPORTERS: RwLock<Vec<(String, Importer)>> = RwLock::new(Vec::new());

/// Register `ext`'s importer of another program's parameter files. Refused when `ext` is not
/// an extension id or already has one.
pub fn register_importer(ext: &str, importer: Importer) -> Result<()> {
    if !solidcraft_doc::extension_data::is_extension_id(ext) {
        return Err(EngineError::Other(format!("`{}` is not an extension id", shown(ext))));
    }
    let mut all = IMPORTERS.write().map_err(|_| EngineError::Other("the importer registry is unavailable".into()))?;
    if all.iter().any(|(e, _)| e == ext) {
        return Err(EngineError::Other(format!("`{ext}` already has an importer")));
    }
    all.push((ext.to_owned(), importer));
    Ok(())
}

/// `ext`'s importer, if it registered one.
pub fn importer(ext: &str) -> Option<Importer> {
    IMPORTERS.read().ok()?.iter().find(|(e, _)| e == ext).map(|(_, i)| *i)
}

/// Read another program's parameter file at `path` with `ext`'s importer and keep the packs it
/// holds, each under a name no pack of its kind has yet (`name (2)`, …). Returns the packs kept,
/// as (kind, name), and what could not be carried over.
pub fn import(ext: &str, path: &str) -> Result<(Vec<(String, String)>, Vec<String>)> {
    import_in(&dir()?, ext, path)
}

pub fn import_in(dir: &Path, ext: &str, path: &str) -> Result<(Vec<(String, String)>, Vec<String>)> {
    let importer = importer(ext).ok_or_else(|| EngineError::Other(format!("`{}` has no importer of parameter files", shown(ext))))?;
    // The user's file: read within an agent's file scope.
    let text = solidcraft_io::vfs::read_to_string(path, MAX_FILE).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    let value: Value = serde_json::from_str(&text).map_err(|e| EngineError::Other(format!("{path} is not JSON: {e}")))?;
    let stem = Path::new(path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let Imported { packs, mut notes } = importer(&value, &stem).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    // The names taken and the packs saved, without another writer in between.
    let _one = one_writer();
    let mut kept = Vec::new();
    for (kind, name, values) in packs {
        let Some(form) = form_for(ext, &kind) else {
            notes.push(format!("{name}: no form makes `{kind}`"));
            continue;
        };
        let taken: Vec<String> = list_in(dir, form)?.into_iter().map(|p| p.name).collect();
        let base = name.trim();
        let unique = (1..=MAX_PACKS + 1)
            .map(|i| if i == 1 { base.to_owned() } else { format!("{base} ({i})") })
            .find(|n| !taken.contains(n))
            .unwrap_or_default();
        match save_unlocked(dir, form, &unique, values) {
            Ok(_) => kept.push((kind, unique)),
            Err(e) => notes.push(format!("{name}: {e}")),
        }
    }
    Ok((kept, notes))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::extensions::forms::form;
    use crate::extensions::forms::tests::{EXT, setup};

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("solidcraft-packs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn values(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap_or_default()
    }

    #[test]
    fn writers_at_once_lose_no_pack() {
        setup();
        let (d, f) = (tmp("writers"), form("test_forms.slab").unwrap());
        std::thread::scope(|scope| {
            for t in 0..8 {
                let d = &d;
                scope.spawn(move || {
                    for i in 0..10 {
                        assert!(save_in(d, f, &format!("t{t} p{i}"), values(json!({"w": "10 mm"}))).is_ok());
                        if i % 2 == 1 {
                            assert!(delete_in(d, f, &format!("t{t} p{i}")).unwrap(), "t{t} p{i} was saved");
                        }
                    }
                });
            }
        });
        assert_eq!(list_in(&d, f).unwrap().len(), 8 * 5);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn packs_are_kept_replaced_and_removed_by_name() {
        setup();
        let (d, f) = (tmp("keep"), form("test_forms.slab").unwrap());
        assert!(list_in(&d, f).unwrap().is_empty(), "no file yet: no packs");
        assert!(!save_in(&d, f, " Wide ", values(json!({"w": "40 mm", "n": 2, "thick": true}))).unwrap());
        assert!(!save_in(&d, f, "Narrow", values(json!({"w": "10 mm"}))).unwrap());
        assert!(save_in(&d, f, "Wide", values(json!({"w": "50 mm"}))).unwrap(), "the same name replaces");
        let packs = list_in(&d, f).unwrap();
        assert_eq!(packs.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Wide", "Narrow"]);
        assert_eq!(packs[0].values["w"], "50 mm");
        assert_eq!(packs[0].version, f.version);
        // Another kind's packs are its own; the file is the extension's.
        assert!(list_in(&d, form("test_forms.other").unwrap()).unwrap().is_empty());
        assert!(d.join("test_forms.json").is_file());
        let long = "x".repeat(MAX_NAME + 1);
        for bad in ["", "   ", "a\u{7}b", long.as_str()] {
            assert!(save_in(&d, f, bad, Map::new()).is_err(), "{bad:?}");
        }
        for v in [json!({"nothing": 1}), json!({"w": true}), json!({"thick": "yes"}), json!({"spans": 3}), json!({"w": "a\u{0}b"})] {
            assert!(save_in(&d, f, "Bad", values(v.clone())).is_err(), "{v}");
        }
        assert!(delete_in(&d, f, "Narrow").unwrap());
        assert!(!delete_in(&d, f, "Narrow").unwrap());
        assert_eq!(list_in(&d, f).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_damaged_or_newer_file_is_refused_and_left_as_it_is() {
        setup();
        let (d, f) = (tmp("damaged"), form("test_forms.slab").unwrap());
        std::fs::create_dir_all(&d).unwrap();
        let path = d.join("test_forms.json");
        for text in ["{not json", r#"{"format": 99, "kinds": {}}"#] {
            std::fs::write(&path, text).unwrap();
            assert!(list_in(&d, f).is_err(), "{text}");
            assert!(save_in(&d, f, "Mine", Map::new()).is_err(), "{text}");
            assert!(delete_in(&d, f, "Mine").is_err(), "{text}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), text, "left as it was");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_kind_keeps_at_most_max_packs() {
        setup();
        let (d, f) = (tmp("cap"), form("test_forms.slab").unwrap());
        for i in 0..MAX_PACKS {
            save_in(&d, f, &format!("p{i}"), Map::new()).unwrap();
        }
        assert!(save_in(&d, f, "one more", Map::new()).is_err());
        assert!(save_in(&d, f, "p0", values(json!({"n": 4}))).unwrap(), "replacing still works");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Reads `{"packs": [{"name"?, "width"?}]}`: the `slab` kind, or one no form makes for "far".
    fn other_program(file: &Value, stem: &str) -> std::result::Result<Imported, String> {
        let packs = file.get("packs").and_then(Value::as_array).ok_or("no packs")?;
        let mut out = Imported::default();
        for p in packs {
            let name = p.get("name").and_then(Value::as_str).unwrap_or(stem).to_owned();
            let kind = if name == "far" { "nothing" } else { "slab" };
            let width = p.get("width").and_then(Value::as_f64).unwrap_or(1.0);
            out.packs.push((kind.into(), name, values(json!({"w": format!("{width} mm")}))));
        }
        out.notes.push("width only".into());
        Ok(out)
    }

    #[test]
    fn imported_packs_are_added_under_names_of_their_own() {
        setup();
        let _ = register_importer(EXT, other_program);
        assert!(register_importer(EXT, other_program).is_err(), "one importer per extension");
        assert!(register_importer("Not An Id", other_program).is_err());
        let d = tmp("import");
        std::fs::create_dir_all(&d).unwrap();
        let f = form("test_forms.slab").unwrap();
        save_in(&d, f, "Mine", values(json!({"w": "5 mm"}))).unwrap();
        let path = d.join("old.json");
        std::fs::write(&path, json!({"packs": [{"name": "Mine", "width": 30}, {"width": 40}, {"name": "far"}]}).to_string()).unwrap();
        let (kept, notes) = import_in(&d, EXT, &path.to_string_lossy()).unwrap();
        assert_eq!(kept, [("slab".to_string(), "Mine (2)".to_string()), ("slab".to_string(), "old".to_string())]);
        assert!(notes.iter().any(|n| n.contains("width only")) && notes.iter().any(|n| n.contains("far")), "{notes:?}");
        let packs = list_in(&d, f).unwrap();
        assert_eq!(packs.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Mine", "Mine (2)", "old"]);
        assert_eq!(packs[0].values["w"], "5 mm", "a pack already kept is never replaced");
        assert_eq!(packs[1].values["w"], "30 mm");
        assert!(import_in(&d, "test_unknown", &path.to_string_lossy()).is_err(), "no importer");
        std::fs::write(&path, "not json").unwrap();
        assert!(import_in(&d, EXT, &path.to_string_lossy()).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
