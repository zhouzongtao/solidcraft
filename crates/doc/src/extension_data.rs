//! The extension data area (`extensions` in the design file): model data a product built on
//! SolidCraft keeps with the design, such as concrete cover per face keyed by the faces'
//! persistent names. Each extension keeps its values under its own id, one value per key, and
//! every value records the version of the extension's data layout that wrote it, so a later
//! version can still read it and an earlier one never misreads it.
//!
//! The area is part of the design: it is saved with it, undone and redone with every command,
//! and kept as it is through open and save when the extension that wrote it is not part of the
//! program. Unlike `metadata`, which is informational only, it is model data: opening checks
//! its numbers like every other number in the design, [`ExtensionData::set`] checks ids, keys,
//! nesting, numbers and size, and a design holding data written by a newer version of a
//! registered extension is refused instead of being misread ([`register_data_version`]).
//!
//! Values are shared between undo steps (`Arc`), so a change copies only the keys it touches.
//! Feature generators don't see the area, so changing it never recomputes a feature.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{DocError, Result};

/// Longest extension id, in bytes.
pub const MAX_ID_BYTES: usize = 64;
/// Longest key, in bytes.
pub const MAX_KEY_BYTES: usize = 256;
/// Largest value, as compact JSON.
pub const MAX_VALUE_BYTES: usize = 4 << 20;
/// Deepest nesting of a value.
pub const MAX_VALUE_DEPTH: usize = 32;
/// Most keys (over all extensions), and largest area as compact JSON, that a change may grow
/// the area to.
pub const MAX_KEYS: usize = 65_536;
pub const MAX_TOTAL_BYTES: usize = 32 << 20;

/// The design's extension data: extension id → key → value.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ExtensionData(BTreeMap<String, BTreeMap<String, Entry>>);

/// A value and the version of its extension's data layout that wrote it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Entry {
    version: u64,
    value: Arc<Value>,
}

/// The data version each extension writes, registered at start-up.
static VERSIONS: RwLock<BTreeMap<String, u64>> = RwLock::new(BTreeMap::new());

/// An extension id: dot-separated labels of lower-case ASCII letters, digits, `_` and `-`, the
/// first starting with a letter (`bridge`, `com.example.tool`).
pub fn is_extension_id(id: &str) -> bool {
    id.len() <= MAX_ID_BYTES
        && id.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && id.split('.').all(|l| !l.is_empty() && l.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-'))
}

/// Register `version` as the version of `ext`'s data layout: [`ExtensionData::set`] stamps it on
/// every value `ext` writes, and opening refuses a design holding `ext` data of a later version.
/// Refused when `ext` is not an extension id or already has a version.
pub fn register_data_version(ext: &str, version: u64) -> Result<()> {
    if !is_extension_id(ext) {
        return Err(invalid(format!("`{}` is not an extension id (lower-case letters, digits, `_`, `-` and dots)", shown(ext))));
    }
    let mut all = VERSIONS.write().map_err(|_| invalid("the extension data registry is unavailable".into()))?;
    if all.contains_key(ext) {
        return Err(invalid(format!("extension `{ext}` already has a data version")));
    }
    all.insert(ext.to_owned(), version);
    Ok(())
}

/// The data version registered for `ext`.
pub fn data_version(ext: &str) -> Option<u64> {
    VERSIONS.read().ok()?.get(ext).copied()
}

impl ExtensionData {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The extensions holding data, including ones that are not part of this program.
    pub fn extensions(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }

    /// `ext`'s value under `key`, with the data version that wrote it.
    pub fn get(&self, ext: &str, key: &str) -> Option<(u64, &Value)> {
        self.0.get(ext)?.get(key).map(|e| (e.version, e.value.as_ref()))
    }

    /// `ext`'s keys, with each value and the data version that wrote it, in key order.
    pub fn entries(&self, ext: &str) -> impl Iterator<Item = (&str, u64, &Value)> {
        self.0.get(ext).into_iter().flatten().map(|(k, e)| (k.as_str(), e.version, e.value.as_ref()))
    }

    /// Set one value; see [`ExtensionData::set_many`].
    pub fn set(&mut self, ext: &str, key: &str, value: Value) -> Result<bool> {
        self.set_many(ext, vec![(key.to_owned(), value)])
    }

    /// Set `ext`'s values under `entries`' keys, stamped with `ext`'s data version: all of them,
    /// or none when one is refused. Refused when `ext` has no registered data version; a key is
    /// empty, longer than [`MAX_KEY_BYTES`] or has a control character; a value is nested more
    /// than [`MAX_VALUE_DEPTH`] levels, is larger than [`MAX_VALUE_BYTES`] or holds a number a
    /// design can't (a non-integer past ±1e9); or the area would grow past [`MAX_KEYS`] keys or
    /// [`MAX_TOTAL_BYTES`]. Returns whether anything changed.
    pub fn set_many(&mut self, ext: &str, entries: Vec<(String, Value)>) -> Result<bool> {
        let version = data_version(ext).ok_or_else(|| invalid(format!("`{}` has no registered data version", shown(ext))))?;
        let mut next = self.0.get(ext).cloned().unwrap_or_default();
        let mut changed = false;
        for (key, value) in entries {
            check_key(&key)?;
            check_value(&key, &value)?;
            let entry = Entry { version, value: Arc::new(value) };
            if next.get(&key) != Some(&entry) {
                next.insert(key, entry);
                changed = true;
            }
        }
        if !changed {
            return Ok(false);
        }
        let before = self.0.insert(ext.to_owned(), next);
        let (keys, bytes) = (self.key_count(), json_len(&self.0));
        if keys <= MAX_KEYS && bytes <= MAX_TOTAL_BYTES {
            return Ok(true);
        }
        // Growing past a limit is refused; a design already past one may still shrink.
        let after = match before {
            Some(before) => self.0.insert(ext.to_owned(), before),
            None => self.0.remove(ext),
        };
        if (keys > MAX_KEYS && keys > self.key_count()) || (bytes > MAX_TOTAL_BYTES && bytes > json_len(&self.0)) {
            return Err(invalid(format!("the extension data is limited to {MAX_KEYS} keys and {} MiB", MAX_TOTAL_BYTES >> 20)));
        }
        if let Some(after) = after {
            self.0.insert(ext.to_owned(), after);
        }
        Ok(true)
    }

    /// Remove `ext`'s value under `key`; an extension's last value takes the extension with it.
    /// Never refused, so data can always be cut down. Returns whether it was there.
    pub fn remove(&mut self, ext: &str, key: &str) -> bool {
        let Some(values) = self.0.get_mut(ext) else { return false };
        let removed = values.remove(key).is_some();
        if removed && values.is_empty() {
            self.0.remove(ext);
        }
        removed
    }

    /// Refuse data written by a newer version of a registered extension than this program's: it
    /// would be misread, and saving would lose what this version doesn't know. Data of an
    /// extension that is not part of this program is kept as it is.
    pub(crate) fn check_versions(&self) -> Result<()> {
        for (ext, values) in &self.0 {
            let Some(known) = data_version(ext) else { continue };
            if let Some(newest) = values.values().map(|e| e.version).max().filter(|&v| v > known) {
                return Err(invalid(format!(
                    "this design holds `{ext}` data of version {newest}, newer than this program reads (up to {known}): update the program to open it"
                )));
            }
        }
        Ok(())
    }

    fn key_count(&self) -> usize {
        self.0.values().map(BTreeMap::len).sum()
    }
}

/// `key` as a key: 1 to [`MAX_KEY_BYTES`] bytes, no control characters.
fn check_key(key: &str) -> Result<()> {
    if key.is_empty() || key.len() > MAX_KEY_BYTES || key.chars().any(char::is_control) {
        return Err(invalid(format!("`{}` is not an extension data key (1 to {MAX_KEY_BYTES} bytes, no control characters)", shown(key))));
    }
    Ok(())
}

/// `value` as a design may hold it. Iterative, so a hostile value can't overflow the stack, and
/// nesting is checked before the value is measured.
fn check_value(key: &str, value: &Value) -> Result<()> {
    let mut stack = vec![(value, 1usize)];
    while let Some((v, depth)) = stack.pop() {
        if depth > MAX_VALUE_DEPTH {
            return Err(invalid(format!("`{}` is nested more than {MAX_VALUE_DEPTH} levels deep", shown(key))));
        }
        match v {
            Value::Number(n) if !(n.is_u64() || n.is_i64()) => {
                let x = n.as_f64().unwrap_or(f64::NAN);
                if !(x.is_finite() && x.abs() <= crate::format::MAX_REAL) {
                    return Err(invalid(format!("`{}` holds the number {n}, outside ±{:e}", shown(key), crate::format::MAX_REAL)));
                }
            }
            Value::Array(a) => stack.extend(a.iter().map(|x| (x, depth + 1))),
            Value::Object(m) => stack.extend(m.values().map(|x| (x, depth + 1))),
            _ => {}
        }
    }
    if json_len(value) > MAX_VALUE_BYTES {
        return Err(invalid(format!("`{}` is larger than {} MiB", shown(key), MAX_VALUE_BYTES >> 20)));
    }
    Ok(())
}

/// Bytes of `v` as compact JSON, counted without building the text.
fn json_len(v: &impl Serialize) -> usize {
    struct Count(usize);
    impl std::io::Write for Count {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.saturating_add(b.len());
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut count = Count(0);
    serde_json::to_writer(&mut count, v).map_or(usize::MAX, |()| count.0)
}

fn invalid(msg: String) -> DocError {
    DocError::Invalid(msg)
}

/// Text from outside, shortened and escaped for a message.
fn shown(s: &str) -> String {
    s.chars().take(64).collect::<String>().escape_debug().to_string()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::Document;

    /// `ext`, registered at `version` (once: the registry is shared by every test in the process).
    fn registered(ext: &str, version: u64) -> &str {
        let _ = register_data_version(ext, version);
        assert_eq!(data_version(ext), Some(version));
        ext
    }

    #[test]
    fn set_checks_ids_keys_nesting_numbers_and_sizes() {
        let ext = registered("test_data.checks", 2);
        let mut d = ExtensionData::default();
        assert!(d.set(ext, "covers/Body1", json!({"F3:block/top": 35.0})).unwrap());
        assert!(!d.set(ext, "covers/Body1", json!({"F3:block/top": 35.0})).unwrap(), "the same value is no change");
        assert_eq!(d.get(ext, "covers/Body1"), Some((2, &json!({"F3:block/top": 35.0}))));
        assert!(d.set("test_data.unregistered", "k", json!(1)).is_err(), "an extension writes once it has a data version");
        for bad in ["", "a\u{7}b", "line\nbreak"] {
            assert!(d.set(ext, bad, json!(1)).is_err(), "{bad:?}");
        }
        assert!(d.set(ext, &"k".repeat(MAX_KEY_BYTES + 1), json!(1)).is_err());
        assert!(d.set(ext, "big", json!("x".repeat(MAX_VALUE_BYTES))).is_err());
        let mut deep = json!(1);
        for _ in 1..MAX_VALUE_DEPTH {
            deep = json!([deep]);
        }
        assert!(d.set(ext, "deep", deep.clone()).unwrap(), "{MAX_VALUE_DEPTH} levels");
        assert!(d.set(ext, "deeper", json!([deep])).is_err());
        assert!(d.set(ext, "far", json!(1.5e9)).is_err(), "past the design's number range");
        assert!(d.set(ext, "count", json!(5_000_000_000u64)).unwrap(), "integers are exact, any size");
        // A refused batch leaves everything as it was.
        let kept = d.clone();
        assert!(d.set_many(ext, vec![("one".into(), json!(1)), ("two".into(), json!(f64::MAX))]).is_err());
        assert_eq!(d, kept);
        for bad in ["", "Bridge", "1x", "a..b", "a.", ".a", "a b", "é"] {
            assert!(register_data_version(bad, 1).is_err(), "{bad:?}");
        }
        assert!(register_data_version(ext, 3).is_err(), "one data version per extension");
        // Removing never fails; the last value takes the extension with it.
        for key in ["covers/Body1", "deep", "count"] {
            assert!(d.remove(ext, key));
        }
        assert!(!d.remove(ext, "covers/Body1"));
        assert!(d.is_empty());
    }

    #[test]
    fn the_area_is_saved_with_the_design_and_read_back() {
        let ext = registered("test_data.saved", 1);
        let mut doc = Document::new("Part");
        assert!(serde_json::to_value(&doc).unwrap().get("extensions").is_none(), "an empty area isn't written");
        doc.extensions.set(ext, "covers/Body1", json!({"F1:block/top": 30.0, "F1:block/side[1]": 35})).unwrap();
        doc.extensions.set(ext, "settings", json!({"units": "mm"})).unwrap();
        let text = doc.to_json();
        let back = Document::from_json(&text).unwrap();
        assert_eq!(back.extensions, doc.extensions);
        assert_eq!(back.extensions.entries(ext).map(|(k, v, _)| (k, v)).collect::<Vec<_>>(), [("covers/Body1", 1), ("settings", 1)]);
        let mut saved: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(saved["extensions"][ext]["settings"], json!({"version": 1, "value": {"units": "mm"}}));
        // A design from before the area opens with it empty.
        saved.as_object_mut().unwrap().remove("extensions");
        assert!(Document::from_json(&saved.to_string()).unwrap().extensions.is_empty());
    }

    #[test]
    fn data_from_a_newer_extension_is_refused_and_unknown_extensions_are_kept() {
        let ext = registered("test_data.versions", 2);
        let mut design = serde_json::to_value(Document::new("Part")).unwrap();
        design["extensions"] = json!({
            ext: {"a": {"version": 1, "value": [1, 2]}, "b": {"version": 2, "value": "two"}},
            "test_data.not_here": {"x": {"version": 9, "value": {"kept": true}}},
        });
        let doc = Document::from_json(&design.to_string()).unwrap();
        assert_eq!(doc.extensions.get(ext, "a"), Some((1, &json!([1, 2]))), "older data opens for the extension to read");
        assert_eq!(doc.extensions.get("test_data.not_here", "x"), Some((9, &json!({"kept": true}))));
        assert_eq!(serde_json::to_value(&doc).unwrap()["extensions"], design["extensions"], "saved as it was read");
        design["extensions"][ext]["c"] = json!({"version": 3, "value": null});
        let e = Document::from_json(&design.to_string()).unwrap_err().to_string();
        assert!(e.contains("newer") && e.contains(ext), "{e}");
    }

    #[test]
    fn opening_checks_the_areas_numbers_as_set_does() {
        let ext = registered("test_data.numbers", 1);
        let mut doc = Document::new("Part");
        doc.extensions.set(ext, "edge", json!(crate::format::MAX_REAL)).unwrap();
        assert_eq!(Document::from_json(&doc.to_json()).unwrap().extensions, doc.extensions);
        let mut design = serde_json::to_value(&doc).unwrap();
        design["extensions"][ext]["edge"]["value"] = json!(1.5e9);
        assert!(Document::from_json(&design.to_string()).is_err());
        assert!(doc.extensions.set(ext, "edge", json!(1.5e9)).is_err());
    }

    #[test]
    fn the_limits_refuse_growth_but_let_an_oversized_area_shrink() {
        let ext = registered("test_data.limits", 1);
        let mut d = ExtensionData::default();
        d.set_many(ext, (0..MAX_KEYS).map(|i| (format!("k{i}"), json!(i))).collect()).unwrap();
        assert!(d.set(ext, "one_more", json!(0)).is_err());
        assert!(d.set(ext, "k0", json!("replaced")).unwrap(), "the count doesn't grow");
        assert!(d.get(ext, "one_more").is_none());
        // Size: a value just under the cap each, until the area is full.
        let chunk = json!("x".repeat(MAX_TOTAL_BYTES / 9));
        let mut d = ExtensionData::default();
        for i in 0..8 {
            d.set(ext, &format!("c{i}"), chunk.clone()).unwrap();
        }
        assert!(d.set(ext, "c8", chunk.clone()).is_err());
        // An area written past the cap by another program still opens and can be cut down.
        let past: BTreeMap<String, Value> = (0..10).map(|i| (format!("c{i}"), json!({"version": 1, "value": chunk.clone()}))).collect();
        let mut big: ExtensionData = serde_json::from_value(json!({ ext: past })).unwrap();
        assert!(big.set(ext, "c0", json!("short")).unwrap(), "smaller is allowed");
        assert!(big.set(ext, "c1", json!(chunk.as_str().map(|s| format!("{s}y")))).is_err(), "still over: no growth");
        assert!(big.remove(ext, "c2"));
    }
}
