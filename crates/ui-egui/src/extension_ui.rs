//! Toolbar tabs and icons of a product built on SolidCraft (an extension), registered once at
//! start-up. A tab lists its panels in order; its commands come from the registry by (tab,
//! panel) as on any tab, and it sits before UTILITIES. An icon is drawn in code, as every
//! SolidCraft icon is, by a function the extension gives under a name with a dot in it
//! (`bridge.t_beam`), which no built-in icon has. An extension command with a registered form
//! opens its dialog ([`schema_dialog`]) with its parameter figures ([`figure_view`]).

use std::sync::RwLock;

pub mod figure_view;
pub mod schema_dialog;

pub use egui;

/// Draws an icon in `rect` with the ink, fill and accent colours it is given.
pub type IconFn = fn(&egui::Painter, egui::Rect, egui::Color32, egui::Color32, egui::Color32);

/// Draws a product's mark in the app bar, 20 px tall, centred on the point.
pub type MarkFn = fn(&egui::Painter, egui::Pos2);

static MARK: std::sync::OnceLock<MarkFn> = std::sync::OnceLock::new();

/// The app bar's mark for a product built on SolidCraft (SolidCraft's logo is SolidCraft's
/// alone). Refused once one is registered.
pub fn register_mark(draw: MarkFn) -> Result<(), String> {
    MARK.set(draw).map_err(|_| "the app bar mark may be registered once".to_string())
}

/// The app bar's mark: the registered one, else SolidCraft's logo for SolidCraft itself and
/// none for a product that registered none.
pub(crate) fn brand_mark(painter: &egui::Painter, centre: egui::Pos2) {
    if let Some(draw) = MARK.get() {
        draw(painter, centre);
    } else if solidcraft_engine::geom::brand::is_default() {
        crate::icons::glyphs::brand_mark(painter, centre);
    }
}

struct Tab {
    name: &'static str,
    panels: &'static [&'static str],
    buttons: usize,
}

static TABS: RwLock<Vec<Tab>> = RwLock::new(Vec::new());
static ICONS: RwLock<Vec<(&'static str, IconFn)>> = RwLock::new(Vec::new());

/// Add tab `name` with `panels` (in order), each showing `buttons` commands as buttons on a fresh
/// toolbar (the rest in its menu). Refused for an empty name or one already a tab.
pub fn register_tab(name: &'static str, panels: &'static [&'static str], buttons: usize) -> Result<(), String> {
    let mut all = TABS.write().map_err(|_| "toolbar registry is unavailable".to_string())?;
    if name.trim().is_empty() || crate::toolbar::TABS.contains(&name) || ["SKETCH", "RENDER"].contains(&name) || all.iter().any(|t| t.name == name) {
        return Err(format!("toolbar tab `{name}` is empty or already a tab"));
    }
    all.push(Tab { name, panels, buttons: buttons.clamp(1, 12) });
    Ok(())
}

/// Add icon `name` (with a dot in it). Refused when it has none or is taken.
pub fn register_icon(name: &'static str, draw: IconFn) -> Result<(), String> {
    let mut all = ICONS.write().map_err(|_| "icon registry is unavailable".to_string())?;
    if !name.contains('.') || all.iter().any(|(n, _)| *n == name) {
        return Err(format!("icon `{name}` needs a dot in its name and may be registered once"));
    }
    all.push((name, draw));
    Ok(())
}

/// The toolbar's tabs: the built-in ones with the extensions' before UTILITIES (else at the end).
pub fn tabs() -> Vec<&'static str> {
    let extra: Vec<&'static str> = TABS.read().map(|t| t.iter().map(|t| t.name).collect()).unwrap_or_default();
    ordered(crate::toolbar::TABS, &extra)
}

fn ordered(builtin: &[&'static str], extra: &[&'static str]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = builtin.iter().copied().filter(|t| *t != "UTILITIES").collect();
    out.extend(extra);
    if builtin.contains(&"UTILITIES") {
        out.push("UTILITIES");
    }
    out
}

/// An extension tab's panels and how many buttons each shows.
pub(crate) fn tab(name: &str) -> Option<(&'static [&'static str], usize)> {
    TABS.read().ok()?.iter().find(|t| t.name == name).map(|t| (t.panels, t.buttons))
}

/// The function drawing extension icon `name`.
pub(crate) fn icon(name: &str) -> Option<IconFn> {
    if !name.contains('.') {
        return None;
    }
    ICONS.read().ok()?.iter().find(|(n, _)| *n == name).map(|(_, f)| *f)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dot(_: &egui::Painter, _: egui::Rect, _: egui::Color32, _: egui::Color32, _: egui::Color32) {}

    #[test]
    fn extension_tabs_go_before_utilities() {
        assert_eq!(ordered(&["SOLID", "UTILITIES"], &["桥梁"]), ["SOLID", "桥梁", "UTILITIES"]);
        assert_eq!(ordered(&["SOLID"], &["桥梁"]), ["SOLID", "桥梁"]);
    }

    #[test]
    fn taken_tabs_and_icons_without_a_dot_are_refused() {
        assert!(register_tab("SOLID", &["CREATE"], 2).is_err());
        assert!(register_tab("  ", &["CREATE"], 2).is_err());
        assert!(register_icon("extrude", dot).is_err());
        assert!(icon("extrude").is_none());
        let _ = register_icon("test_ext.dot", dot);
        assert!(register_icon("test_ext.dot", dot).is_err());
        assert!(icon("test_ext.dot").is_some());
    }
}
