//! SolidCraft's environment variables, read in one place.
//!
//! Every variable is `SOLIDCRAFT_<NAME>`, or the program's own prefix when it is a product built
//! on SolidCraft ([`crate::brand`]). Compatibility shim (see docs/compatibility.md): when it is
//! unset, SolidCraft reads the same variable under the product's former prefix instead, so
//! scripts and service files written for earlier releases keep working.

use std::ffi::OsString;

/// The current prefix.
pub const PREFIX: &str = "SOLIDCRAFT_";
/// The former prefix, read only as a fallback.
pub const LEGACY_PREFIX: &str = "SOLVECRAFT_";

/// `SOLIDCRAFT_<name>` (the brand's prefix), else the legacy `SOLVECRAFT_<name>`. `name` is the
/// part after the prefix, e.g. `"RECOVERY_DIR"`.
pub fn var_os(name: &str) -> Option<OsString> {
    lookup(name, |k| std::env::var_os(k))
}

/// [`var_os`] as UTF-8; a value that is not valid UTF-8 counts as unset.
pub fn var(name: &str) -> Option<String> {
    var_os(name).and_then(|v| v.into_string().ok())
}

/// The variable's full name under the brand's prefix (`SOLIDCRAFT_RENDERER`), for setting it.
pub fn name(name: &str) -> String {
    format!("{}{name}", crate::brand::get().env_prefix)
}

fn lookup(name: &str, get: impl Fn(&str) -> Option<OsString>) -> Option<OsString> {
    lookup_in(crate::brand::get(), name, get)
}

/// A product built on SolidCraft reads its own prefix only.
fn lookup_in(brand: &crate::brand::Brand, name: &str, get: impl Fn(&str) -> Option<OsString>) -> Option<OsString> {
    let own = get(&format!("{}{name}", brand.env_prefix));
    if own.is_some() || !brand.is_solidcraft() {
        return own;
    }
    get(&format!("{LEGACY_PREFIX}{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let m: HashMap<String, OsString> = pairs.iter().map(|(k, v)| ((*k).to_string(), OsString::from(*v))).collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn new_prefix_wins_over_legacy() {
        let get = env(&[("SOLIDCRAFT_RECOVERY_DIR", "/new"), ("SOLVECRAFT_RECOVERY_DIR", "/old")]);
        assert_eq!(lookup("RECOVERY_DIR", get), Some(OsString::from("/new")));
    }

    #[test]
    fn legacy_prefix_is_the_fallback() {
        let get = env(&[("SOLVECRAFT_CONTROL_PORT", "4000")]);
        assert_eq!(lookup("CONTROL_PORT", get), Some(OsString::from("4000")));
    }

    #[test]
    fn unset_in_both_is_none() {
        assert_eq!(lookup("WAYLAND", env(&[("OTHER", "1")])), None);
    }

    #[test]
    fn a_product_reads_its_own_prefix_only() {
        let product = crate::brand::Brand { env_prefix: "REBAR3D_", app_id: "ai.tonghao.rebar3d", ..crate::brand::SOLIDCRAFT };
        let get = env(&[("REBAR3D_CONTROL_PORT", "4100"), ("SOLIDCRAFT_RECOVERY_DIR", "/sc"), ("SOLVECRAFT_RECOVERY_DIR", "/old")]);
        assert_eq!(lookup_in(&product, "CONTROL_PORT", &get), Some(OsString::from("4100")));
        assert_eq!(lookup_in(&product, "RECOVERY_DIR", &get), None);
        assert_eq!(name("RENDERER"), "SOLIDCRAFT_RENDERER");
    }
}
