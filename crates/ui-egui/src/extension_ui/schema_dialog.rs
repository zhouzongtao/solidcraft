//! The dialog of an extension's feature command, drawn from its registered form
//! ([`solidcraft_engine::extensions::Form`]): tabs of groups of fields, the parameter figure
//! lighting the focused field's dimensions, a live preview, and each field's error beside it. The
//! timeline opens it filled from the feature to edit it in place.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use solidcraft_engine::doc::FeatureKind;
use solidcraft_engine::extensions::presets::{self, Preset};
use solidcraft_engine::extensions::{Field, FieldType, Form};

use crate::SolveApp;
use crate::dialogs::{Kind, check, combo, row_label};
use crate::selection::SelInput;
use crate::theme::Tokens;

/// The dialog's state.
#[derive(Clone, Debug)]
pub struct Sd {
    pub form: &'static Form,
    pub tab: usize,
    /// The feature's name (new features only).
    pub name: String,
    /// Text fields (numbers and lists), choices (option index) and switches, by field id.
    pub text: BTreeMap<String, String>,
    pub choice: BTreeMap<String, usize>,
    pub switch: BTreeMap<String, bool>,
    /// The edited feature's parameters, so the ones the form doesn't show keep their values.
    base: Map<String, Value>,
    /// The packs of the form's kind (read when the dialog first draws, again after a change),
    /// the one last picked, the name to save under, and what the last pack action did.
    packs: Option<Vec<Preset>>,
    pub pack: Option<usize>,
    pub pack_name: String,
    pub pack_note: Option<(String, bool)>,
}

fn fields(form: &Form) -> impl Iterator<Item = &Field> {
    form.fields()
}

impl Sd {
    /// The form's fields at `base`'s values, else their defaults.
    fn new(form: &'static Form, base: Map<String, Value>) -> Sd {
        let (mut text, mut choice, mut switch) = (BTreeMap::new(), BTreeMap::new(), BTreeMap::new());
        for f in fields(form) {
            let given = base.get(&f.id);
            match f.ty {
                FieldType::Choice => {
                    let id = given.and_then(Value::as_str).or(f.default.as_str()).unwrap_or_default();
                    choice.insert(f.id.clone(), f.options.iter().position(|(o, _)| o == id).unwrap_or(0));
                }
                FieldType::Switch => {
                    switch.insert(f.id.clone(), given.and_then(Value::as_bool).or(f.default.as_bool()).unwrap_or(false));
                }
                _ => {
                    let given = given.and_then(|v| v.as_str().map(str::to_owned).or_else(|| v.as_number().map(|n| n.to_string())));
                    text.insert(f.id.clone(), given.or_else(|| f.default_text()).unwrap_or_default());
                }
            }
        }
        Sd { form, tab: 0, name: String::new(), text, choice, switch, base, packs: None, pack: None, pack_name: String::new(), pack_note: None }
    }

    /// The command's parameters: the edited feature's, overwritten by the form's fields.
    fn params(&self, editing: bool) -> Value {
        let mut p = if editing { self.base.clone() } else { Map::new() };
        for f in fields(self.form) {
            let v = match f.ty {
                FieldType::Choice => {
                    let i = self.choice.get(&f.id).copied().unwrap_or(0);
                    f.options.get(i).map_or(Value::Null, |(o, _)| json!(o))
                }
                FieldType::Switch => json!(self.switch.get(&f.id).copied().unwrap_or(false)),
                _ => json!(self.text.get(&f.id).map_or("", |s| s.trim())),
            };
            p.insert(f.id.clone(), v);
        }
        if !editing && !self.name.trim().is_empty() {
            p.insert("name".into(), json!(self.name.trim()));
        }
        Value::Object(p)
    }
}

/// The dialog of command `id`, when it has a form.
pub fn start(_app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    let form = solidcraft_engine::extensions::form(id)?;
    Some((Kind::Schema(Box::new(Sd::new(form, Map::new()))), Vec::new()))
}

/// The dialog that edits an extension feature, filled from it.
pub fn for_feature(feature: &FeatureKind) -> Option<(Kind, Vec<SelInput>, Map<String, Value>)> {
    let FeatureKind::Extension { ext, kind, inputs, params, .. } = feature else { return None };
    let form = solidcraft_engine::extensions::form_for(ext, kind)?;
    let mut base = params.as_object().cloned().unwrap_or_default();
    for input in &inputs.0 {
        base.insert(input.label.to_string(), json!(input.expr));
    }
    Some((Kind::Schema(Box::new(Sd::new(form, base))), Vec::new(), Map::new()))
}

/// The figure's width, beside the label column.
const FIGURE_W: f32 = 236.0;

/// The dialog's rows. Returns true when Enter was pressed in a field.
pub fn rows(app: &mut SolveApp, ui: &mut egui::Ui, k: &mut Sd, editing: bool) -> bool {
    let t = Tokens::get();
    let mut enter = false;
    if !editing {
        row_label(ui, "Name");
        let hint = k.form.default_name.as_deref().unwrap_or_default();
        let r = ui.add(egui::TextEdit::singleline(&mut k.name).hint_text(hint));
        enter |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.end_row();
    }
    pack_rows(app, ui, k);
    let form = k.form;
    // The focused field's figure (else the form's), with that field's groups lit.
    let focused = ui.ctx().memory(|m| m.focused());
    let field = fields(form).find(|f| Some(field_id(form, f)) == focused);
    let shown = field.and_then(|f| f.figure.as_deref()).or(form.figure.as_deref()).and_then(solidcraft_engine::extensions::figure);
    // The room of the tallest figure the form shows, so the fields stay put as the figure changes.
    let tallest = std::iter::once(form.figure.as_deref())
        .chain(fields(form).map(|f| f.figure.as_deref()))
        .flatten()
        .filter_map(solidcraft_engine::extensions::figure)
        .map(|f| super::figure_view::size(f, FIGURE_W).y)
        .fold(0.0f32, f32::max);
    if tallest > 0.0 {
        row_label(ui, "");
        let (room, _) = ui.allocate_exact_size(egui::vec2(FIGURE_W, tallest), egui::Sense::hover());
        if let Some(figure) = shown {
            let rect = egui::Rect::from_center_size(room.center(), super::figure_view::size(figure, FIGURE_W));
            super::figure_view::paint(&ui.painter_at(room), rect, figure, &field.map(Field::lit).unwrap_or_default());
        }
        ui.end_row();
    }
    if form.tabs.len() > 1 {
        row_label(ui, "");
        ui.horizontal_wrapped(|ui| {
            for (i, tab) in form.tabs.iter().enumerate() {
                let r = ui.selectable_value(&mut k.tab, i, tab.label.as_str());
                crate::scenario::publish_handle(&format!("dialog:tab:{}", tab.label), r.rect.center());
            }
        });
        ui.end_row();
    }
    let groups = form.tabs.get(k.tab).or(form.tabs.first()).map_or(&[][..], |tab| &tab.groups[..]);
    for group in groups {
        if !group.label.is_empty() {
            row_label(ui, "");
            ui.label(egui::RichText::new(group.label.as_str()).strong().color(t.text));
            ui.end_row();
        }
        for f in &group.fields {
            enter |= field_row(app, ui, k, f);
        }
    }
    enter
}

/// The pack rows: pick a saved pack (its values fill the fields), save the current values
/// under a name, delete the picked pack, import another program's parameter file.
fn pack_rows(app: &mut SolveApp, ui: &mut egui::Ui, k: &mut Sd) {
    let t = Tokens::get();
    if k.packs.is_none() {
        k.packs = Some(presets::list(k.form).unwrap_or_else(|e| {
            k.pack_note = Some((e.to_string(), true));
            Vec::new()
        }));
    }
    let names: Vec<String> = k.packs.iter().flatten().map(|p| p.name.clone()).collect();
    row_label(ui, "ui.parameter_pack");
    let mut picked = None;
    let mut delete = false;
    ui.horizontal(|ui| {
        let empty = if names.is_empty() { "ui.pack_none" } else { "ui.pack_choose" };
        let shown = k.pack.and_then(|i| names.get(i)).cloned().unwrap_or_else(|| crate::i18n::tr(empty).to_string());
        let r = egui::ComboBox::from_id_salt(("extension_form_pack", k.form.command.as_str()))
            .selected_text(shown)
            .width(crate::dialogs::FIELD_W)
            .show_ui(ui, |ui| {
                for (i, name) in names.iter().enumerate() {
                    let item = ui.selectable_label(k.pack == Some(i), name.as_str());
                    crate::scenario::publish_handle(&format!("dialog:pack:{name}"), item.rect.center());
                    if item.clicked() {
                        picked = Some(i);
                    }
                }
            });
        crate::scenario::publish_handle("dialog:pack", r.response.rect.center());
        let d = ui.add_enabled(k.pack.is_some(), egui::Button::new(crate::i18n::tr("ui.delete")));
        crate::scenario::publish_handle("dialog:pack_delete", d.rect.center());
        delete = d.clicked();
    });
    ui.end_row();
    if let Some(i) = picked {
        apply_pack(k, i);
    }
    if delete {
        delete_pack(app, k);
    }
    row_label(ui, "");
    ui.horizontal(|ui| {
        let r = ui.add(
            egui::TextEdit::singleline(&mut k.pack_name).hint_text(crate::i18n::tr("ui.pack_name")).desired_width(crate::dialogs::FIELD_W - 50.0),
        );
        crate::scenario::publish_handle("dialog:pack_name", r.rect.center());
        let taken = names.iter().any(|n| n == k.pack_name.trim());
        let label = crate::i18n::tr(if taken { "ui.replace" } else { "ui.save" });
        let b = ui.add_enabled(!k.pack_name.trim().is_empty(), egui::Button::new(label));
        crate::scenario::publish_handle("dialog:pack_save", b.rect.center());
        if b.clicked() {
            save_pack(app, k);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if presets::importer(&k.form.ext).is_some() {
            let b = ui.button(crate::i18n::tr("ui.import_ellipsis"));
            crate::scenario::publish_handle("dialog:pack_import", b.rect.center());
            if b.clicked() {
                import_packs(app, k.form.command.clone());
            }
        }
    });
    ui.end_row();
    if let Some((note, error)) = &k.pack_note {
        row_label(ui, "");
        ui.label(egui::RichText::new(note.as_str()).small().color(if *error { t.error } else { t.text_dim }));
        ui.end_row();
    }
}

/// Fill the fields from pack `i` (the fields it has; a pack saved by a newer form is refused).
fn apply_pack(k: &mut Sd, i: usize) {
    let Some(pack) = k.packs.as_ref().and_then(|p| p.get(i)).cloned() else { return };
    k.pack = Some(i);
    k.pack_name = pack.name.clone();
    if pack.version > k.form.version {
        k.pack_note = Some((crate::i18n::format("ui.pack_newer", &[("name", &pack.name)]), true));
        return;
    }
    for f in fields(k.form) {
        let Some(v) = pack.values.get(&f.id) else { continue };
        match f.ty {
            FieldType::Choice => {
                if let Some(o) = v.as_str().and_then(|o| f.options.iter().position(|(id, _)| id == o)) {
                    k.choice.insert(f.id.clone(), o);
                }
            }
            FieldType::Switch => {
                if let Some(on) = v.as_bool() {
                    k.switch.insert(f.id.clone(), on);
                }
            }
            _ => {
                if let Some(text) = v.as_str().map(str::to_owned).or_else(|| v.as_number().map(|n| n.to_string())) {
                    k.text.insert(f.id.clone(), text);
                }
            }
        }
    }
    k.pack_note = Some((crate::i18n::format("ui.pack_applied", &[("name", &pack.name)]), false));
}

/// Save the fields' values as the pack named in the name field (replacing one of that name).
fn save_pack(app: &mut SolveApp, k: &mut Sd) {
    let name = k.pack_name.trim().to_owned();
    let mut values = k.params(false).as_object().cloned().unwrap_or_default();
    values.remove("name");
    let r = app.run("preset.save", json!({"command": k.form.command, "name": name, "values": values}));
    k.packs = presets::list(k.form).ok();
    k.pack = k.packs.iter().flatten().position(|p| p.name == name);
    k.pack_note = Some(match r {
        Ok(_) => (crate::i18n::format("ui.pack_saved", &[("name", &name)]), false),
        Err(e) => (e, true),
    });
}

/// Delete the picked pack.
fn delete_pack(app: &mut SolveApp, k: &mut Sd) {
    let Some(name) = k.pack.and_then(|i| k.packs.as_ref()?.get(i)).map(|p| p.name.clone()) else { return };
    let r = app.run("preset.delete", json!({"command": k.form.command, "name": name}));
    k.packs = presets::list(k.form).ok();
    k.pack = None;
    k.pack_note = Some(match r {
        Ok(_) => (crate::i18n::format("ui.pack_deleted", &[("name", &name)]), false),
        Err(e) => (e, true),
    });
}

/// Import another program's parameter file, picked by the user, with the extension's importer.
#[cfg(not(target_arch = "wasm32"))]
fn import_packs(app: &mut SolveApp, command: String) {
    app.pick_open(move |app, path| {
        let Some(path) = path else { return };
        let r = app.run("preset.import", json!({"command": command, "path": path}));
        let Some(crate::dialogs::Dialog { kind: Kind::Schema(k), .. }) = app.dialog.as_mut() else { return };
        if k.form.command != command {
            return;
        }
        k.packs = presets::list(k.form).ok();
        k.pack_note = Some(match r {
            Ok(v) => {
                let kept = v["kept"].as_array().map_or(0, Vec::len);
                let mut note = crate::i18n::format("ui.pack_imported", &[("count", &kept.to_string())]);
                for n in v["notes"].as_array().into_iter().flatten().filter_map(Value::as_str).take(4) {
                    note.push('\n');
                    note.push_str(n);
                }
                (note, false)
            }
            Err(e) => (e, true),
        });
    });
}

/// The text edit of field `f`, by which the figure knows the focused field.
fn field_id(form: &Form, f: &Field) -> egui::Id {
    egui::Id::new(("extension_form", form.command.as_str(), f.id.as_str()))
}

/// One field's row, with its error under it. Returns true when Enter was pressed in it.
fn field_row(app: &SolveApp, ui: &mut egui::Ui, k: &mut Sd, f: &Field) -> bool {
    row_label(ui, &f.label);
    let mut enter = false;
    match f.ty {
        FieldType::Choice => {
            let labels: Vec<&str> = f.options.iter().map(|(_, label)| label.as_str()).collect();
            if let Some(sel) = k.choice.get_mut(&f.id) {
                combo(ui, &format!("{}:{}", k.form.command, f.id), &labels, sel);
            }
        }
        FieldType::Switch => {
            if let Some(on) = k.switch.get_mut(&f.id) {
                check(ui, on);
            }
        }
        _ => {
            let error = k.text.get(&f.id).and_then(|s| field_error(app, f, s));
            let id = field_id(k.form, f);
            ui.vertical(|ui| {
                if let Some(s) = k.text.get_mut(&f.id) {
                    let r = ui.add(egui::TextEdit::singleline(s).id(id).desired_width(crate::dialogs::FIELD_W));
                    crate::scenario::publish_handle(&format!("dialog:{}", f.label), r.rect.center());
                    crate::params_dialog::complete(ui, &r, s);
                    enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                }
                if let Some(e) = error {
                    ui.label(egui::RichText::new(e).small().color(Tokens::get().error));
                }
            });
        }
    }
    ui.end_row();
    enter
}

/// What is wrong with a number field's text, if anything (lists are the extension's to read).
fn field_error(app: &SolveApp, f: &Field, text: &str) -> Option<String> {
    let kind = f.ty.expr_kind()?;
    let doc = &app.session.doc;
    let v = match doc.eval(text, kind) {
        Ok(v) => v,
        Err(e) => return Some(e.to_string()),
    };
    let limit = |e: Option<&str>| e.and_then(|e| doc.eval(e, kind).ok());
    if let (Some(e), Some(min)) = (f.min.as_deref(), limit(f.min.as_deref()))
        && v < min
    {
        return Some(format!("≥ {e}"));
    }
    if let (Some(e), Some(max)) = (f.max.as_deref(), limit(f.max.as_deref()))
        && v > max
    {
        return Some(format!("≤ {e}"));
    }
    (f.ty == FieldType::Integer && v.fract() != 0.0).then(|| crate::i18n::tr_label("a whole number").to_string())
}

/// The command OK runs, with the form's values; refused while a field is wrong.
pub fn commands(app: &SolveApp, k: &Sd, editing: bool) -> Result<Vec<(String, Value)>, String> {
    for f in fields(k.form) {
        if let Some(e) = k.text.get(&f.id).and_then(|s| field_error(app, f, s)) {
            return Err(format!("{}: {e}", f.label));
        }
    }
    Ok(vec![(k.form.command.clone(), k.params(editing))])
}

/// The dialog's title: its command's label.
pub fn title(k: &Sd) -> &'static str {
    solidcraft_engine::find_command(&k.form.command).map_or("", |c| c.label)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use solidcraft_engine::Session;
    use solidcraft_engine::cmd::CommandSpec;
    use solidcraft_engine::doc::expr::Kind as ExprKind;
    use solidcraft_engine::doc::extension::{ExtInput, FeatureGenerator, GenCtx, GeneratedBody, register_generator};

    use super::*;
    use crate::scenario::Harness;

    const EXT: &str = "test_ui_forms";

    /// A plate `w` × 10 mm, `n` × 5 mm thick.
    struct Plate;
    impl FeatureGenerator for Plate {
        fn inputs(&self) -> &'static [(&'static str, ExprKind, &'static str)] {
            &[("w", ExprKind::Length, "20 mm"), ("n", ExprKind::Unitless, "1")]
        }
        fn generate(&self, ctx: &GenCtx) -> std::result::Result<Vec<GeneratedBody>, String> {
            let v = solidcraft_engine::geom::Vec3::new(ctx.num("w", 20.0), 10.0, ctx.num("n", 1.0) * 5.0);
            let body = solidcraft_engine::doc::kernel::box_solid(solidcraft_engine::geom::Vec3::ZERO, v).map_err(|e| e.to_string())?;
            let roles = (0..body.face_count()).map(|k| vec![format!("f{k}")]).collect();
            Ok(vec![GeneratedBody { key: "plate".into(), name: format!("{}-plate", ctx.name), body, roles }])
        }
    }
    static PLATE: Plate = Plate;

    fn plate(s: &mut Session, p: &Value) -> solidcraft_engine::Result<Value> {
        let inputs = PLATE
            .inputs()
            .iter()
            .map(|(label, kind, def)| ExtInput {
                label: (*label).into(),
                expr: p.get(*label).and_then(Value::as_str).unwrap_or(def).into(),
                kind: *kind,
            })
            .collect();
        let params = json!({"keep": p.get("keep").cloned().unwrap_or(json!("default"))});
        let mut p = if p.is_object() { p.clone() } else { json!({}) };
        if let Some(m) = p.as_object_mut() {
            m.entry("name").or_insert_with(|| json!("Plate"));
        }
        solidcraft_engine::extensions::add_extension_feature(s, &p, EXT, "plate", 1, inputs, params)
    }
    static COMMANDS: [CommandSpec; 1] = [CommandSpec::new("test_ui_forms.plate", "Plate", plate).params("name?, w, n, keep")];

    fn setup() {
        let _ = register_generator(EXT, "plate", &PLATE);
        let _ = solidcraft_engine::extensions::register_commands(&COMMANDS);
        let figure = json!({"name": "test_ui_forms.plan", "bounds": [0, 0, 40, 20], "groups": [
            {"items": [{"type": "polyline", "points": [[0, 5], [40, 5], [40, 15], [0, 15]], "closed": true}]},
            {"name": "w", "items": [{"type": "line", "a": [0, 2], "b": [40, 2]}, {"type": "text", "at": [20, 1], "text": "w", "height": 2, "h": "center", "v": "top"}]},
        ]});
        // A tall one for the count, so showing it would push the fields down.
        let tall = json!({"name": "test_ui_forms.count", "bounds": [0, 0, 10, 40], "groups": [{"name": "n", "items": [{"type": "line", "a": [5, 0], "b": [5, 40]}]}]});
        for f in [figure, tall] {
            if let Ok(f) = serde_json::from_value(f) {
                let _ = solidcraft_engine::extensions::register_figure(f);
            }
        }
        let form = json!({"command": "test_ui_forms.plate", "ext": EXT, "kind": "plate", "figure": "test_ui_forms.plan", "default_name": "Plate", "tabs": [
            {"label": "Size", "groups": [{"label": "Plate", "fields": [
                {"id": "w", "label": "Width w", "type": "length", "default": "20 mm", "min": "1 mm"},
                {"id": "n", "label": "Count n", "type": "integer", "default": 1, "min": "1", "figure": "test_ui_forms.count"},
            ]}]},
        ]});
        if let Ok(f) = serde_json::from_value(form) {
            let _ = solidcraft_engine::extensions::register_form(f);
        }
    }

    /// The packs folder of these tests: one for the process (`presets::set_dir` is global).
    fn packs_dir() -> std::path::PathBuf {
        static DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
        let d = DIR.get_or_init(|| std::env::temp_dir().join(format!("solidcraft-ui-packs-{}", std::process::id()))).clone();
        presets::set_dir(Some(d.clone()));
        d
    }

    /// Replace a field's text by typing, as a person does.
    fn type_into(h: &mut Harness, label: &str, text: &str) {
        h.step(&json!({"click": {"handle": format!("dialog:{label}")}})).unwrap();
        h.step(&json!({"key": "A", "cmd": true})).unwrap();
        h.step(&json!({"text": text})).unwrap();
    }

    fn ok(h: &mut Harness) {
        h.step(&json!({"click": {"handle": "dialog:OK"}})).unwrap();
        h.frames(2);
    }

    fn volume(h: &Harness) -> f64 {
        h.app.session.world_state().bodies.iter().map(|b| solidcraft_engine::doc::kernel::measure(&b.body).map_or(0.0, |m| m.volume)).sum()
    }

    fn face_names(h: &Harness, body: &str) -> Vec<String> {
        let st = h.app.session.world_state();
        st.body(body).map(|b| solidcraft_engine::doc::naming::face_names(b).iter().map(|n| n.to_string()).collect()).unwrap_or_default()
    }

    #[test]
    fn a_form_opens_from_the_toolbar_and_edits_its_feature_in_place() {
        setup();
        packs_dir();
        let mut h = Harness::new();
        h.app.home.open = false;
        h.frames(2);
        h.step(&json!({"start": "test_ui_forms.plate"})).unwrap();
        h.frames(2);
        assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::Schema(_))), "the form's dialog opens");
        type_into(&mut h, "Width w", "30 mm");
        // The count's taller figure takes the figure's place without moving the fields.
        let before = crate::scenario::handle_at("dialog:Width w");
        type_into(&mut h, "Count n", "2");
        h.frames(2);
        assert_eq!(crate::scenario::handle_at("dialog:Width w"), before, "the fields stay put");
        ok(&mut h);
        assert!(h.app.dialog.is_none(), "OK closes the dialog: {:?}", h.app.dialog.as_ref().and_then(|d| d.error.clone()));
        assert!((volume(&h) - 30.0 * 10.0 * 10.0).abs() < 1e-6, "{}", volume(&h));
        // Edited from the timeline: filled from the feature, rewritten in place, and a value the
        // form doesn't show (set by a script) is kept.
        h.app.run("test_ui_forms.plate", json!({"name": "Second", "w": "10 mm", "n": "1", "keep": "mine"})).unwrap();
        let id = h.app.session.doc.find_feature("Second").map(|f| f.id).unwrap();
        let names = face_names(&h, "Second-plate");
        h.step(&json!({"call": "ui.editFeature", "params": {"feature": "Second"}})).unwrap();
        h.frames(4);
        let Some(Kind::Schema(sd)) = h.app.dialog.as_ref().map(|d| &d.kind) else { panic!("no form dialog") };
        assert_eq!(sd.text.get("w").map(String::as_str), Some("10 mm"), "filled from the feature");
        type_into(&mut h, "Count n", "3");
        h.frames(2);
        ok(&mut h);
        assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().and_then(|d| d.error.clone()));
        let f = h.app.session.doc.find_feature("Second").unwrap();
        assert_eq!(f.id, id);
        let FeatureKind::Extension { inputs, params, .. } = &f.kind else { panic!("extension feature") };
        assert_eq!(inputs.get("n"), Some("3"));
        assert_eq!(params["keep"], "mine");
        assert_eq!(face_names(&h, "Second-plate"), names, "the faces keep their names");
        assert!((volume(&h) - (3000.0 + 10.0 * 10.0 * 15.0)).abs() < 1e-6, "{}", volume(&h));
        // A value outside its limits is flagged beside its field and holds OK back; nothing changes.
        h.step(&json!({"call": "ui.editFeature", "params": {"feature": "Second"}})).unwrap();
        h.frames(4);
        type_into(&mut h, "Width w", "0.5 mm");
        h.frames(2);
        assert!(h.widget("≥ 1 mm").is_some(), "the error is shown beside the field");
        ok(&mut h);
        assert!(h.app.dialog.is_some(), "OK is held back while a field is wrong");
        h.step(&json!({"key": "Escape"})).unwrap();
        h.frames(2);
        assert!(h.app.dialog.is_none());
        let f = h.app.session.doc.find_feature("Second").unwrap();
        let FeatureKind::Extension { inputs, .. } = &f.kind else { panic!("extension feature") };
        assert_eq!(inputs.get("w"), Some("10 mm"));
    }
    #[test]
    fn a_pack_keeps_the_fields_and_fills_them_again() {
        setup();
        let dir = packs_dir();
        let mut h = Harness::new();
        h.app.home.open = false;
        h.frames(2);
        h.step(&json!({"start": "test_ui_forms.plate"})).unwrap();
        h.frames(2);
        type_into(&mut h, "Width w", "37 mm");
        type_into(&mut h, "pack_name", "UI pack");
        h.frames(1);
        h.step(&json!({"click": {"handle": "dialog:pack_save"}})).unwrap();
        h.frames(2);
        let form = solidcraft_engine::extensions::form("test_ui_forms.plate").unwrap();
        let kept = presets::list(form).unwrap();
        let pack = kept.iter().find(|p| p.name == "UI pack").unwrap();
        assert_eq!(pack.values["w"], "37 mm");
        assert!(pack.values.get("name").is_none(), "the feature's name is not a pack value");
        // Another width, then the pack: its width comes back.
        type_into(&mut h, "Width w", "12 mm");
        h.frames(1);
        h.step(&json!({"click": {"handle": "dialog:pack"}})).unwrap();
        h.frames(2);
        h.step(&json!({"click": {"handle": "dialog:pack:UI pack"}})).unwrap();
        h.frames(2);
        let Some(Kind::Schema(sd)) = h.app.dialog.as_ref().map(|d| &d.kind) else { panic!("no form dialog") };
        assert_eq!(sd.text.get("w").map(String::as_str), Some("37 mm"));
        h.step(&json!({"click": {"handle": "dialog:pack_delete"}})).unwrap();
        h.frames(2);
        assert!(presets::list(form).unwrap().iter().all(|p| p.name != "UI pack"));
        assert!(h.app.session.doc.features.is_empty() && !h.app.session.is_dirty(), "packs are not the design's");
        let _ = std::fs::remove_dir_all(dir);
    }
}
