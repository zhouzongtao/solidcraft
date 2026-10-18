//! Extension forms (E5): the dialog an extension's feature command opens from the toolbar, as
//! data: tabs of groups of fields, each field one of the command's parameters, and named
//! parameter figures whose parts light up while their field has focus. The UI draws every form
//! the same way, with a live preview and errors beside their fields, and opens the same dialog
//! from the timeline, filled from the feature, to edit it in place (`timeline.redefine` keeps its
//! id, name and face names).
//!
//! A form's command makes one extension feature (`ext` / `kind`) from flat parameters: `name`
//! and one value per field id. Number fields are the generator's inputs (expressions, by label);
//! the others are the feature's `params` (lists as text, choices by option id, switches).
//! Registering checks a form against its command and generator, so a mismatch fails at
//! start-up rather than in a dialog.

use std::collections::BTreeSet;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcraft_doc::expr::Kind;

use crate::{EngineError, Result};

/// Longest id or figure name, label or figure text, and list text, in bytes.
const MAX_ID: usize = 64;
const MAX_LABEL: usize = 200;
const MAX_TEXT: usize = 4096;
/// Most tabs, groups per tab, fields per form and options per choice.
const MAX_TABS: usize = 16;
const MAX_GROUPS: usize = 32;
const MAX_FIELDS: usize = 512;
const MAX_OPTIONS: usize = 64;
/// Most groups and items per figure, and points per polyline; largest coordinate.
const MAX_FIGURE_GROUPS: usize = 4096;
const MAX_ITEMS: usize = 20_000;
const MAX_POINTS: usize = 10_000;
const MAX_COORD: f64 = 1e9;

/// The dialog of an extension's feature command.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Form {
    /// The command OK runs; the dialog's title is its label.
    pub command: String,
    /// The extension feature the command makes.
    pub ext: String,
    pub kind: String,
    /// The version of its fields, which parameter packs record: a pack saved by a newer version
    /// is not applied (raise it when a field changes meaning).
    #[serde(default = "first_version")]
    pub version: u64,
    /// The figure shown while no field with a figure of its own has focus.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub figure: Option<String>,
    /// The name the command gives a new feature left unnamed (the Name field's hint).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_name: Option<String>,
    pub tabs: Vec<Tab>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tab {
    pub label: String,
    pub groups: Vec<Group>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Group {
    /// Shown above the group's fields; empty for none.
    #[serde(default)]
    pub label: String,
    pub fields: Vec<Field>,
}

/// One parameter of the command.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Field {
    /// The parameter: a generator input's label for a number, else a `params` key.
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub ty: FieldType,
    /// An expression for a number (evaluated on its own: no design parameters), text for
    /// `lengths`, an option id for `choice`, true or false for `switch`.
    pub default: Value,
    /// Limits of a number, as expressions; the dialog flags a value outside them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<String>,
    /// A choice's options: (id, label).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<(String, String)>,
    /// The figure shown while this field has focus (else the form's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub figure: Option<String>,
    /// The figure groups lit while this field has focus (else the group named after the field).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub highlight: Vec<String>,
}

/// What a field holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    Length,
    Angle,
    Number,
    /// A whole number (a count).
    Integer,
    /// Lengths as text (`7800 mm, 7200 mm`), read by the extension.
    Lengths,
    Choice,
    Switch,
}

impl FieldType {
    /// A number field's expression kind (`None`: the field is a `params` value).
    pub fn expr_kind(self) -> Option<Kind> {
        match self {
            FieldType::Length => Some(Kind::Length),
            FieldType::Angle => Some(Kind::Angle),
            FieldType::Number | FieldType::Integer => Some(Kind::Unitless),
            FieldType::Lengths | FieldType::Choice | FieldType::Switch => None,
        }
    }
}

fn first_version() -> u64 {
    1
}

impl Form {
    /// Every field, tab by tab and group by group.
    pub fn fields(&self) -> impl Iterator<Item = &Field> {
        self.tabs.iter().flat_map(|t| &t.groups).flat_map(|g| &g.fields)
    }

    /// The field `id`.
    pub fn field(&self, id: &str) -> Option<&Field> {
        self.fields().find(|f| f.id == id)
    }
}

impl Field {
    /// The text a text field starts with: a number's default expression or a list's text.
    pub fn default_text(&self) -> Option<String> {
        match &self.default {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) if self.ty.expr_kind().is_some() => Some(n.to_string()),
            _ => None,
        }
    }

    /// The figure groups this field lights.
    pub fn lit(&self) -> Vec<&str> {
        if self.highlight.is_empty() { vec![self.id.as_str()] } else { self.highlight.iter().map(String::as_str).collect() }
    }
}

/// A parameter figure: vector drawing in its own units, Y up, in named groups that fields light.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Figure {
    pub name: String,
    /// The drawing's extent: [x0, y0, x1, y1].
    pub bounds: [f64; 4],
    pub groups: Vec<FigureGroup>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FigureGroup {
    /// The name fields light it by; empty for the outline that is never lit.
    #[serde(default)]
    pub name: String,
    pub items: Vec<Item>,
}

/// One primitive of a figure.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Item {
    Line {
        a: [f64; 2],
        b: [f64; 2],
    },
    Polyline {
        points: Vec<[f64; 2]>,
        #[serde(default)]
        closed: bool,
        /// Filled (an arrowhead), else stroked.
        #[serde(default)]
        filled: bool,
    },
    /// Counter-clockwise from `start` to `end` (radians).
    Arc {
        center: [f64; 2],
        radius: f64,
        start: f64,
        end: f64,
    },
    Circle {
        center: [f64; 2],
        radius: f64,
    },
    Text {
        at: [f64; 2],
        text: String,
        /// Cap height, in the figure's units.
        height: f64,
        /// Counter-clockwise, radians.
        #[serde(default)]
        angle: f64,
        #[serde(default)]
        h: HAlign,
        #[serde(default)]
        v: VAlign,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HAlign {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VAlign {
    #[default]
    Bottom,
    Middle,
    Top,
}

/// Registered forms and figures, never removed (so they are handed out as `&'static`).
static FORMS: RwLock<Vec<&'static Form>> = RwLock::new(Vec::new());
static FIGURES: RwLock<Vec<&'static Figure>> = RwLock::new(Vec::new());

fn registry() -> EngineError {
    EngineError::Other("the extension form registry is unavailable".into())
}

/// Register a parameter figure. Refused when its name is taken or it is malformed.
pub fn register_figure(figure: Figure) -> Result<()> {
    check_figure(&figure)?;
    let mut all = FIGURES.write().map_err(|_| registry())?;
    if all.iter().any(|f| f.name == figure.name) {
        return Err(EngineError::Other(format!("a figure `{}` is already registered", figure.name)));
    }
    all.push(Box::leak(Box::new(figure)));
    Ok(())
}

/// The registered figure called `name`.
pub fn figure(name: &str) -> Option<&'static Figure> {
    FIGURES.read().ok()?.iter().copied().find(|f| f.name == name)
}

/// Register the form of an extension command, after its command, generator and figures. Refused
/// when it doesn't match them ([`Form`]) or its command already has one.
pub fn register_form(form: Form) -> Result<()> {
    check_form(&form)?;
    let mut all = FORMS.write().map_err(|_| registry())?;
    if all.iter().any(|f| f.command == form.command) {
        return Err(EngineError::Other(format!("`{}` already has a form", form.command)));
    }
    all.push(Box::leak(Box::new(form)));
    Ok(())
}

/// The form of command `id`.
pub fn form(id: &str) -> Option<&'static Form> {
    FORMS.read().ok()?.iter().copied().find(|f| f.command == id)
}

/// The form that edits an `ext` / `kind` feature: the first registered for it (when several
/// commands make the kind, each feature keeps its own switches in its `params`).
pub fn form_for(ext: &str, kind: &str) -> Option<&'static Form> {
    FORMS.read().ok()?.iter().copied().find(|f| f.ext == ext && f.kind == kind)
}

fn bad(what: &str, msg: String) -> EngineError {
    EngineError::Other(format!("{what}: {msg}"))
}

/// Text from outside, shortened and escaped for a message.
fn shown(s: &str) -> String {
    s.chars().take(64).collect::<String>().escape_debug().to_string()
}

/// `s` as a label: 1 to `max` bytes, no control characters (`lines`: line breaks allowed).
fn check_text(what: &str, s: &str, max: usize, lines: bool) -> Result<()> {
    if s.is_empty() || s.len() > max || s.chars().any(|c| c.is_control() && !(lines && c == '\n')) {
        return Err(bad(what, format!("`{}` must be 1 to {max} bytes without control characters", shown(s))));
    }
    Ok(())
}

/// `form` against its command, its generator's inputs and the registered figures.
fn check_form(form: &Form) -> Result<()> {
    let what = format!("form of `{}`", shown(&form.command));
    let what = what.as_str();
    if !crate::cmd::find_command(&form.command).is_some_and(|c| c.id == form.command) {
        return Err(bad(what, "no such command".into()));
    }
    let generator = solidcraft_doc::extension::generator(&form.ext, &form.kind)
        .ok_or_else(|| bad(what, format!("no generator for `{}` / `{}`", shown(&form.ext), shown(&form.kind))))?;
    let inputs = generator.inputs();
    if let Some(name) = &form.figure
        && figure(name).is_none()
    {
        return Err(bad(what, format!("no figure `{}` (register figures first)", shown(name))));
    }
    if let Some(name) = &form.default_name {
        check_text(what, name, MAX_LABEL, false)?;
    }
    if form.tabs.is_empty() || form.tabs.len() > MAX_TABS {
        return Err(bad(what, format!("a form has 1 to {MAX_TABS} tabs")));
    }
    let empty = solidcraft_doc::Document::default();
    let mut ids = BTreeSet::new();
    for tab in &form.tabs {
        check_text(what, &tab.label, MAX_LABEL, false)?;
        if tab.groups.is_empty() || tab.groups.len() > MAX_GROUPS {
            return Err(bad(what, format!("tab `{}` needs 1 to {MAX_GROUPS} groups", tab.label)));
        }
        for group in &tab.groups {
            if !group.label.is_empty() {
                check_text(what, &group.label, MAX_LABEL, false)?;
            }
            if group.fields.is_empty() {
                return Err(bad(what, format!("group `{}` of tab `{}` has no fields", group.label, tab.label)));
            }
            for field in &group.fields {
                check_field(what, form, field, inputs, &empty)?;
                if !ids.insert(field.id.as_str()) || ids.len() > MAX_FIELDS {
                    return Err(bad(what, format!("field `{}` is repeated (or there are more than {MAX_FIELDS})", shown(&field.id))));
                }
            }
        }
    }
    Ok(())
}

fn check_field(what: &str, form: &Form, field: &Field, inputs: &[(&str, Kind, &str)], empty: &solidcraft_doc::Document) -> Result<()> {
    let id = shown(&field.id);
    if field.id.is_empty() || field.id.len() > MAX_ID || field.id == "name" {
        return Err(bad(what, format!("`{id}` can't name a field (1 to {MAX_ID} bytes; `name` is the feature's)")));
    }
    check_text(what, &field.label, MAX_LABEL, false)?;
    let input = inputs.iter().find(|(label, _, _)| *label == field.id);
    match field.ty.expr_kind() {
        Some(kind) => {
            if !input.is_some_and(|(_, k, _)| *k == kind) {
                return Err(bad(what, format!("field `{id}` needs an input of that name and kind ({kind:?}) in `{}` / `{}`", form.ext, form.kind)));
            }
            let text = field.default_text().ok_or_else(|| bad(what, format!("field `{id}` needs an expression as its default")))?;
            let value = empty.eval(&text, kind).map_err(|e| bad(what, format!("field `{id}`'s default: {e}")))?;
            let limit = |e: &Option<String>| {
                e.as_ref().map(|e| empty.eval(e, kind).map_err(|err| bad(what, format!("field `{id}`'s limit: {err}")))).transpose()
            };
            let (min, max) = (limit(&field.min)?, limit(&field.max)?);
            if min.is_some_and(|m| value < m) || max.is_some_and(|m| value > m) || (field.ty == FieldType::Integer && value.fract() != 0.0) {
                return Err(bad(what, format!("field `{id}`'s default is outside its limits or not whole")));
            }
        }
        None => {
            if input.is_some() {
                return Err(bad(what, format!("field `{id}` is an input of the generator: give it a number type")));
            }
            if field.min.is_some() || field.max.is_some() {
                return Err(bad(what, format!("field `{id}`: only numbers have limits")));
            }
            let ok = match field.ty {
                FieldType::Lengths => field.default.as_str().is_some_and(|s| s.len() <= MAX_TEXT && !s.chars().any(char::is_control)),
                FieldType::Choice => {
                    let mut seen = BTreeSet::new();
                    !field.options.is_empty()
                        && field.options.len() <= MAX_OPTIONS
                        && field.options.iter().all(|(o, label)| {
                            !o.is_empty() && o.len() <= MAX_ID && seen.insert(o) && check_text(what, label, MAX_LABEL, false).is_ok()
                        })
                        && field.default.as_str().is_some_and(|d| field.options.iter().any(|(o, _)| o == d))
                }
                _ => field.default.is_boolean(),
            };
            if !ok {
                return Err(bad(what, format!("field `{id}`'s default or options don't fit its type")));
            }
        }
    }
    if field.ty != FieldType::Choice && !field.options.is_empty() {
        return Err(bad(what, format!("field `{id}`: only a choice has options")));
    }
    let shows = field.figure.as_ref().or(form.figure.as_ref());
    if let Some(name) = &field.figure
        && figure(name).is_none()
    {
        return Err(bad(what, format!("field `{id}`: no figure `{}`", shown(name))));
    }
    for group in &field.highlight {
        let found = shows.and_then(|name| figure(name)).is_some_and(|f| f.groups.iter().any(|g| &g.name == group));
        if !found {
            return Err(bad(what, format!("field `{id}` lights `{}`, which its figure doesn't have", shown(group))));
        }
    }
    Ok(())
}

/// Points within the figure's range.
fn in_range(what: &str, points: &[[f64; 2]]) -> Result<()> {
    if points.iter().flatten().all(|v| v.is_finite() && v.abs() <= MAX_COORD) {
        Ok(())
    } else {
        Err(bad(what, format!("a coordinate is not finite or is beyond ±{MAX_COORD:e}")))
    }
}

fn check_figure(f: &Figure) -> Result<()> {
    let what = format!("figure `{}`", shown(&f.name));
    let what = what.as_str();
    if f.name.is_empty() || f.name.len() > MAX_ID || f.name.chars().any(char::is_control) {
        return Err(bad(what, format!("a figure's name is 1 to {MAX_ID} bytes")));
    }
    let [x0, y0, x1, y1] = f.bounds;
    in_range(what, &[[x0, y0], [x1, y1]])?;
    if x1 <= x0 || y1 <= y0 {
        return Err(bad(what, "its bounds are empty".into()));
    }
    if f.groups.len() > MAX_FIGURE_GROUPS || f.groups.iter().map(|g| g.items.len()).sum::<usize>() > MAX_ITEMS {
        return Err(bad(what, format!("a figure has at most {MAX_FIGURE_GROUPS} groups and {MAX_ITEMS} items")));
    }
    let mut names = BTreeSet::new();
    for g in &f.groups {
        if !g.name.is_empty() && (g.name.len() > MAX_ID || g.name.chars().any(char::is_control) || !names.insert(g.name.as_str())) {
            return Err(bad(what, format!("group `{}` is repeated or misnamed", shown(&g.name))));
        }
        for item in &g.items {
            match item {
                Item::Line { a, b } => in_range(what, &[*a, *b])?,
                Item::Polyline { points, .. } => {
                    if points.len() < 2 || points.len() > MAX_POINTS {
                        return Err(bad(what, format!("a polyline has 2 to {MAX_POINTS} points")));
                    }
                    in_range(what, points)?;
                }
                Item::Arc { center, radius, start, end } => {
                    in_range(what, &[*center, [*radius, *start], [*end, 0.0]])?;
                    if *radius <= 0.0 {
                        return Err(bad(what, "an arc's radius must be positive".into()));
                    }
                }
                Item::Circle { center, radius } => {
                    in_range(what, &[*center, [*radius, 0.0]])?;
                    if *radius <= 0.0 {
                        return Err(bad(what, "a circle's radius must be positive".into()));
                    }
                }
                Item::Text { at, text, height, angle, .. } => {
                    in_range(what, &[*at, [*height, *angle]])?;
                    check_text(what, text, MAX_LABEL, true)?;
                    if *height <= 0.0 {
                        return Err(bad(what, "a text's height must be positive".into()));
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use serde_json::json;
    use solidcraft_doc::extension::{ExtInput, FeatureGenerator, GenCtx, GeneratedBody, register_generator};

    use super::*;
    use crate::Session;
    use crate::cmd::CommandSpec;

    pub(crate) const EXT: &str = "test_forms";

    /// A slab `w` wide and `n` × 10 mm long, 5 mm thick (thicker with `thick`).
    struct Slab;
    impl FeatureGenerator for Slab {
        fn inputs(&self) -> &'static [(&'static str, Kind, &'static str)] {
            &[("w", Kind::Length, "20 mm"), ("n", Kind::Unitless, "3")]
        }
        fn generate(&self, ctx: &GenCtx) -> std::result::Result<Vec<GeneratedBody>, String> {
            let t = if ctx.params.get("thick").and_then(Value::as_bool) == Some(true) { 10.0 } else { 5.0 };
            let v = crate::geom::Vec3::new(ctx.num("w", 20.0), ctx.num("n", 3.0) * 10.0, t);
            let body = crate::kernel::box_solid(crate::geom::Vec3::ZERO, v).map_err(|e| e.to_string())?;
            let roles = (0..body.face_count()).map(|k| vec![format!("f{k}")]).collect();
            Ok(vec![GeneratedBody { key: "slab".into(), name: format!("{}-slab", ctx.name), body, roles }])
        }
    }
    static SLAB: Slab = Slab;

    fn slab(s: &mut Session, p: &Value, kind: &str) -> Result<Value> {
        let inputs = SLAB
            .inputs()
            .iter()
            .map(|(label, k, def)| ExtInput { label: (*label).into(), expr: p.get(*label).and_then(Value::as_str).unwrap_or(def).into(), kind: *k })
            .collect();
        let params =
            json!({"thick": p.get("thick").and_then(Value::as_bool).unwrap_or(false), "spans": p.get("spans").cloned().unwrap_or(json!(""))});
        let mut p = if p.is_object() { p.clone() } else { json!({}) };
        if let Some(m) = p.as_object_mut() {
            m.entry("name").or_insert_with(|| json!("Slab"));
        }
        crate::extensions::add_extension_feature(s, &p, EXT, kind, 1, inputs, params)
    }
    fn make_slab(s: &mut Session, p: &Value) -> Result<Value> {
        slab(s, p, "slab")
    }
    fn make_other(s: &mut Session, p: &Value) -> Result<Value> {
        slab(s, p, "other")
    }

    static COMMANDS: [CommandSpec; 3] = [
        CommandSpec::new("test_forms.slab", "Slab", make_slab).params("w, n, thick, spans"),
        CommandSpec::new("test_forms.other", "Other slab", make_other).params("as test_forms.slab"),
        CommandSpec::new("test_forms.bare", "Slab without a form", make_slab).params("as test_forms.slab"),
    ];

    fn slab_figure() -> Value {
        json!({"name": "test_forms.plan", "bounds": [0, 0, 100, 50], "groups": [
            {"items": [{"type": "polyline", "points": [[0, 0], [100, 0], [100, 50], [0, 50]], "closed": true}]},
            {"name": "w", "items": [{"type": "line", "a": [0, -5], "b": [100, -5]}, {"type": "text", "at": [50, -8], "text": "w", "height": 4, "h": "center", "v": "top"}]},
            {"name": "n", "items": [{"type": "arc", "center": [50, 25], "radius": 10, "start": 0, "end": 3.1}]},
        ]})
    }

    fn slab_form(command: &str, kind: &str) -> Value {
        json!({"command": command, "ext": EXT, "kind": kind, "figure": "test_forms.plan", "default_name": "Slab", "tabs": [
            {"label": "Size", "groups": [{"label": "Plan", "fields": [
                {"id": "w", "label": "Width w", "type": "length", "default": "20 mm", "min": "1 mm"},
                {"id": "n", "label": "Spans n", "type": "integer", "default": 3, "min": "1", "max": "50"},
            ]}]},
            {"label": "Options", "groups": [{"fields": [
                {"id": "thick", "label": "Thick", "type": "switch", "default": false},
                {"id": "spans", "label": "Span lengths", "type": "lengths", "default": "7800 mm, 7200 mm", "highlight": ["w", "n"]},
            ]}]},
        ]})
    }

    /// The generators, commands, figure and forms, registered once for every test.
    pub(crate) fn setup() {
        let _ = register_generator(EXT, "slab", &SLAB);
        let _ = register_generator(EXT, "other", &SLAB);
        let _ = crate::extensions::register_commands(&COMMANDS);
        if let Ok(f) = serde_json::from_value(slab_figure()) {
            let _ = register_figure(f);
        }
        for (command, kind) in [("test_forms.slab", "slab"), ("test_forms.other", "other")] {
            if let Ok(f) = serde_json::from_value(slab_form(command, kind)) {
                let _ = register_form(f);
            }
        }
    }

    #[test]
    fn forms_are_checked_against_their_command_generator_and_figures() {
        setup();
        let slab = form("test_forms.slab").unwrap();
        assert_eq!((slab.ext.as_str(), slab.kind.as_str()), (EXT, "slab"));
        assert_eq!(form_for(EXT, "slab").map(|f| f.command.as_str()), Some("test_forms.slab"));
        assert!(form("test_forms.bare").is_none());
        assert_eq!(slab.default_name.as_deref(), Some("Slab"));
        let refused = |change: &dyn Fn(&mut Value), why: &str| {
            let mut v = slab_form("test_forms.bare", "slab");
            change(&mut v);
            let e = register_form(serde_json::from_value(v).unwrap()).unwrap_err().to_string();
            assert!(e.contains(why), "{why}: {e}");
        };
        refused(&|v| v["command"] = json!("test_forms.nothing"), "no such command");
        refused(&|v| v["command"] = json!("TEST_FORMS.BARE"), "no such command");
        refused(&|v| v["kind"] = json!("nothing"), "no generator");
        refused(&|v| v["figure"] = json!("test_forms.none"), "no figure");
        refused(&|v| v["tabs"] = json!([]), "1 to");
        refused(&|v| v["tabs"][0]["groups"][0]["fields"][0]["id"] = json!("x"), "needs an input");
        refused(&|v| v["tabs"][0]["groups"][0]["fields"][0]["type"] = json!("angle"), "needs an input");
        refused(&|v| v["tabs"][0]["groups"][0]["fields"][0]["default"] = json!("10 deg"), "default");
        refused(&|v| v["tabs"][0]["groups"][0]["fields"][0]["default"] = json!("0.5 mm"), "outside its limits");
        refused(&|v| v["tabs"][0]["groups"][0]["fields"][1]["default"] = json!(2.5), "not whole");
        refused(&|v| v["tabs"][0]["groups"][0]["fields"][1] = json!({"id": "w", "label": "Again", "type": "length", "default": "5 mm"}), "repeated");
        refused(&|v| v["tabs"][0]["groups"][0]["fields"][1]["id"] = json!("name"), "can't name a field");
        refused(&|v| v["tabs"][1]["groups"][0]["fields"][0]["type"] = json!("length"), "needs an input");
        refused(&|v| v["tabs"][1]["groups"][0]["fields"][0]["default"] = json!("yes"), "don't fit its type");
        refused(&|v| v["tabs"][1]["groups"][0]["fields"][0]["min"] = json!("1"), "only numbers have limits");
        refused(&|v| v["tabs"][1]["groups"][0]["fields"][1]["highlight"] = json!(["nothing"]), "doesn't have");
        refused(
            &|v| {
                v["tabs"][1]["groups"][0]["fields"][0] =
                    json!({"id": "side", "label": "Side", "type": "choice", "default": "c", "options": [["a", "A"], ["b", "B"]]})
            },
            "don't fit its type",
        );
        refused(&|v| v["tabs"][0]["label"] = json!("bad\u{7}label"), "control characters");
        refused(&|v| v["default_name"] = json!(""), "1 to");
        let again: Form = serde_json::from_value(slab_form("test_forms.slab", "slab")).unwrap();
        assert!(register_form(again).unwrap_err().to_string().contains("already has a form"));
        assert!(form("test_forms.bare").is_none(), "nothing refused was registered");
    }

    #[test]
    fn figures_are_checked_and_named_once() {
        setup();
        let f = figure("test_forms.plan").unwrap();
        assert_eq!(f.groups.len(), 3);
        assert!(register_figure(serde_json::from_value(slab_figure()).unwrap()).unwrap_err().to_string().contains("already registered"));
        let refused = |change: &dyn Fn(&mut Value)| {
            let mut v = slab_figure();
            v["name"] = json!("test_forms.bad");
            change(&mut v);
            let figure: Figure = serde_json::from_value(v).unwrap();
            assert!(register_figure(figure).is_err());
        };
        refused(&|v| v["bounds"] = json!([0, 0, 0, 50]));
        refused(&|v| v["bounds"] = json!([0, 0, 2e9, 50]));
        refused(&|v| v["name"] = json!(""));
        refused(&|v| v["groups"][2]["name"] = json!("w"));
        refused(&|v| v["groups"][0]["items"][0]["points"] = json!([[0, 0]]));
        refused(&|v| v["groups"][2]["items"][0]["radius"] = json!(0));
        refused(&|v| v["groups"][1]["items"][1]["height"] = json!(-1));
        refused(&|v| v["groups"][1]["items"][1]["text"] = json!("a\u{1}b"));
        let mut nan: Figure = serde_json::from_value(slab_figure()).unwrap();
        nan.name = "test_forms.nan".into();
        nan.groups[1].items[0] = Item::Line { a: [f64::NAN, 0.0], b: [1.0, 1.0] };
        assert!(register_figure(nan).is_err());
        assert!(figure("test_forms.bad").is_none() && figure("test_forms.nan").is_none());
    }

    fn face_names(s: &Session, body: &str) -> Vec<String> {
        let st = s.model.state();
        st.body(body).map(|b| solidcraft_doc::naming::face_names(b).iter().map(|n| n.to_string()).collect()).unwrap_or_default()
    }

    fn volume(s: &Session, body: &str) -> f64 {
        let st = s.model.state();
        st.body(body).and_then(|b| crate::kernel::measure(&b.body).ok()).map_or(f64::NAN, |m| m.volume)
    }

    #[test]
    fn a_form_command_redefines_its_feature_in_place() {
        setup();
        let mut s = Session::default();
        let r = s.execute("test_forms.slab", &json!({"name": "Deck", "w": "20 mm", "n": "2"})).unwrap();
        let id = r["feature"].as_u64().unwrap();
        let names = face_names(&s, "Deck-slab");
        assert_eq!(names.len(), 6);
        assert!(names.iter().all(|n| n.starts_with(&format!("F{id}:slab/f"))), "{names:?}");
        assert!((volume(&s, "Deck-slab") - 20.0 * 20.0 * 5.0).abs() < 1e-6);
        let set = |w: &str| json!({"feature": id, "command": "test_forms.slab", "params": {"w": w, "n": "3", "thick": true}});
        s.execute("timeline.redefine", &set("30 mm")).unwrap();
        assert_eq!(s.doc.features.len(), 1);
        assert_eq!(s.doc.feature(id).map(|f| f.name.as_str()), Some("Deck"));
        assert!((volume(&s, "Deck-slab") - 30.0 * 30.0 * 10.0).abs() < 1e-6, "inputs and switches both changed");
        assert_eq!(face_names(&s, "Deck-slab"), names, "the faces keep their names");
        // Another kind's command, or a command without a form, can't rewrite it.
        let other = json!({"feature": id, "command": "test_forms.other", "params": {}});
        assert!(s.execute("timeline.redefine", &other).unwrap_err().to_string().contains("is not a"));
        let bare = json!({"feature": id, "command": "test_forms.bare", "params": {}});
        assert!(s.execute("timeline.redefine", &bare).unwrap_err().to_string().contains("does not make a feature"));
        s.execute("edit.undo", &json!({})).unwrap();
        assert!((volume(&s, "Deck-slab") - 20.0 * 20.0 * 5.0).abs() < 1e-6, "one undo takes the edit back");
    }
}
