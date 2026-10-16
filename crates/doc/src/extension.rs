//! Features of a product built on SolidCraft (an extension): `FeatureKind::Extension` keeps the
//! extension's name, the kind of feature, its version and its parameters. Its numeric inputs are
//! expressions like any feature input: they become model parameters (`dN`), may name user
//! parameters and configurations, and are edited with `timeline.edit`. The other parameters
//! (switches, choices, lists) are kept as JSON for the extension to read.

use std::collections::BTreeSet;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use crate::expr::Kind;
use crate::walk::{ExprItem, ExprItemMut, ExprRole, Walk};

/// The most inputs an extension feature may have, and the longest label.
pub const MAX_INPUTS: usize = 256;
const MAX_LABEL: usize = 200;
/// Labels kept for the parameters table; past this, inputs are labelled `input`.
const MAX_LABELS: usize = 4096;

/// One numeric input: its label (the parameter's name in the extension), its expression and
/// what it measures.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExtInput {
    pub label: String,
    pub expr: crate::expr::Expr,
    pub kind: Kind,
}

/// An extension feature's numeric inputs, in the order the extension gave them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ExtInputs(pub Vec<ExtInput>);

impl ExtInputs {
    /// The expression of the input labelled `label`.
    pub fn get(&self, label: &str) -> Option<&str> {
        self.0.iter().find(|i| i.label == label).map(|i| i.expr.as_str())
    }
}

/// Labels seen so far, for the parameters table (whose labels live as long as the program).
static LABELS: RwLock<BTreeSet<&'static str>> = RwLock::new(BTreeSet::new());

/// `label` as a label that lives as long as the program: kept once per distinct label.
fn label(label: &str) -> &'static str {
    if label.is_empty() || label.len() > MAX_LABEL {
        return "input";
    }
    if let Some(l) = LABELS.read().ok().and_then(|s| s.get(label).copied()) {
        return l;
    }
    let Ok(mut s) = LABELS.write() else { return "input" };
    if let Some(l) = s.get(label).copied() {
        return l;
    }
    if s.len() >= MAX_LABELS {
        return "input";
    }
    let l: &'static str = Box::leak(label.to_owned().into_boxed_str());
    s.insert(l);
    l
}

impl Walk for ExtInputs {
    fn walk_exprs<'a>(&'a self, f: &mut dyn FnMut(ExprItem<'a>)) {
        for i in self.0.iter().take(MAX_INPUTS) {
            f((label(&i.label), &i.expr, i.kind, ExprRole::Input));
        }
    }
    fn walk_exprs_mut<'a>(&'a mut self, f: &mut dyn FnMut(ExprItemMut<'a>)) {
        for i in self.0.iter_mut().take(MAX_INPUTS) {
            f((label(&i.label), &mut i.expr, i.kind, ExprRole::Input));
        }
    }
}

impl Walk for serde_json::Value {}

/// What a generator is given: the feature (its id, name and version), its numeric inputs
/// evaluated (lengths in mm, angles in radians, by label) and its other parameters.
pub struct GenCtx<'a> {
    pub feature: u64,
    pub name: &'a str,
    pub version: u64,
    pub inputs: &'a std::collections::BTreeMap<String, f64>,
    pub params: &'a serde_json::Value,
}

impl GenCtx<'_> {
    /// Input `label`, or `def` when the feature has none of that name.
    pub fn num(&self, label: &str, def: f64) -> f64 {
        self.inputs.get(label).copied().unwrap_or(def)
    }
}

/// A body a generator built: a key that stays the same when parameters change (`beam.middle[2]`),
/// its name in the model, and the role of each of its faces in face order (the first is its name,
/// the others aliases). Roles are unique within the body.
pub struct GeneratedBody {
    pub key: String,
    pub name: String,
    pub body: crate::kernel::Body,
    pub roles: Vec<Vec<String>>,
}

/// Builds the bodies of one kind of extension feature. It must not panic: a refusal or failure is
/// an error, which the feature shows.
pub trait FeatureGenerator: Send + Sync {
    /// The numeric inputs a new feature of this kind gets: (label, what it measures, default).
    fn inputs(&self) -> &'static [(&'static str, Kind, &'static str)];
    /// The bodies for these inputs and parameters.
    fn generate(&self, ctx: &GenCtx) -> std::result::Result<Vec<GeneratedBody>, String>;
}

type Generators = Vec<(String, String, &'static dyn FeatureGenerator)>;
static GENERATORS: RwLock<Generators> = RwLock::new(Vec::new());

/// Register the generator of extension features `ext` / `kind`. Refused when one is registered.
pub fn register_generator(ext: &str, kind: &str, g: &'static dyn FeatureGenerator) -> crate::Result<()> {
    let mut all = GENERATORS.write().map_err(|_| crate::DocError::Invalid("generator registry is unavailable".into()))?;
    if all.iter().any(|(e, k, _)| e == ext && k == kind) {
        return Err(crate::DocError::Invalid(format!("a generator for `{ext}` / `{kind}` is already registered")));
    }
    all.push((ext.to_owned(), kind.to_owned(), g));
    Ok(())
}

/// The generator registered for `ext` / `kind`.
pub fn generator(ext: &str, kind: &str) -> Option<&'static dyn FeatureGenerator> {
    GENERATORS.read().ok()?.iter().find(|(e, k, _)| e == ext && k == kind).map(|(_, _, g)| *g)
}

/// Evaluate extension feature `f`: its generator's bodies become model bodies of the feature,
/// each face named `F<id>:<key>/<role>`.
pub(crate) fn evaluate(
    vals: &std::collections::BTreeMap<String, crate::expr::Value>,
    f: &crate::Feature,
    st: &mut crate::ModelState,
) -> crate::Result<()> {
    let crate::FeatureKind::Extension { ext, kind, version, inputs, params } = &f.kind else { return Ok(()) };
    let g = generator(ext, kind)
        .ok_or_else(|| crate::DocError::Unknown(format!("extension feature `{ext}` / `{kind}`: that extension is not part of this program")))?;
    if inputs.0.len() > MAX_INPUTS {
        return Err(crate::DocError::Invalid(format!("an extension feature takes at most {MAX_INPUTS} inputs")));
    }
    let mut values = std::collections::BTreeMap::new();
    for i in &inputs.0 {
        values.insert(i.label.clone(), crate::eval::val(vals, &i.expr, i.kind)?);
    }
    let ctx = GenCtx { feature: f.id, name: &f.name, version: *version, inputs: &values, params };
    let bodies = g.generate(&ctx).map_err(|e| crate::DocError::Invalid(format!("{}: {e}", f.name)))?;
    let mut keys = BTreeSet::new();
    for (k, gb) in bodies.into_iter().enumerate() {
        if gb.key.is_empty() || !keys.insert(gb.key.clone()) {
            return Err(crate::DocError::Invalid(format!("{}: body key `{}` is empty or repeated", f.name, gb.key)));
        }
        if gb.roles.len() != gb.body.face_count() {
            return Err(crate::DocError::Invalid(format!(
                "{}: {} roles for the {} faces of `{}`",
                f.name,
                gb.roles.len(),
                gb.body.face_count(),
                gb.key
            )));
        }
        let mut seen = BTreeSet::new();
        let mut roles = Vec::with_capacity(gb.roles.len());
        for r in gb.roles {
            let Some(first) = r.first().filter(|x| !x.is_empty()) else {
                return Err(crate::DocError::Invalid(format!("{}: a face of `{}` has no role", f.name, gb.key)));
            };
            if !seen.insert(first.clone()) {
                return Err(crate::DocError::Invalid(format!("{}: role `{first}` repeats in `{}`", f.name, gb.key)));
            }
            roles.push(r.iter().map(|x| format!("{}/{x}", gb.key)).collect());
        }
        let name = match f.body_names.get(k).filter(|n| !n.trim().is_empty() && st.body(n).is_none()) {
            Some(n) => n.clone(),
            None => free_name(st, &gb.name),
        };
        let traced = crate::lineage::Traced::made(gb.body, Some(roles));
        st.bodies.push(crate::ModelBody::traced_new(name, traced, f.id));
    }
    Ok(())
}

/// `base`, or `base (n)` with the first free n.
fn free_name(st: &crate::ModelState, base: &str) -> String {
    let base = if base.trim().is_empty() { "Body" } else { base };
    if st.body(base).is_none() {
        return base.to_owned();
    }
    (2..10_000u32).map(|i| format!("{base} ({i})")).find(|n| st.body(n).is_none()).unwrap_or_else(|| format!("{base} (*)"))
}
