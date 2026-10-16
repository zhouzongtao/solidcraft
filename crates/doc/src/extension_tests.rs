//! Extension features: evaluated by their registered generator, their faces named by role and
//! kept when inputs change, their inputs parameters like any feature's.

use crate::expr::Kind;
use crate::extension::{ExtInput, ExtInputs, FeatureGenerator, GenCtx, GeneratedBody, register_generator};
use crate::{Document, FeatureKind, Model};
use solidcraft_geom::Vec3;

/// Two cubes of side `size`, `gap` apart along x, each face named by the side it faces.
struct Pair;
impl FeatureGenerator for Pair {
    fn inputs(&self) -> &'static [(&'static str, Kind, &'static str)] {
        &[("size", Kind::Length, "10 mm"), ("gap", Kind::Length, "5 mm")]
    }
    fn generate(&self, ctx: &GenCtx) -> std::result::Result<Vec<GeneratedBody>, String> {
        let (a, gap) = (ctx.num("size", 10.0), ctx.num("gap", 5.0));
        let mut out = Vec::new();
        for (i, x) in [0.0, a + gap].into_iter().enumerate() {
            let body = crate::kernel::box_solid(Vec3::new(x, 0.0, 0.0), Vec3::new(x + a, a, a)).map_err(|e| e.to_string())?;
            let roles: Vec<Vec<String>> = body
                .face_sources()
                .ok_or("no face sources")?
                .iter()
                .map(|s| match s {
                    Some(crate::kernel::FaceSource::Box { axis, high, .. }) => {
                        let a = ["x", "y", "z"].get(usize::from(*axis)).copied().unwrap_or("?");
                        vec![format!("{}{a}", if *high { "+" } else { "-" })]
                    }
                    _ => vec!["?".into()],
                })
                .collect();
            out.push(GeneratedBody { key: format!("cube[{}]", i + 1), name: format!("{}-{}", ctx.name, i + 1), body, roles });
        }
        Ok(out)
    }
}
static PAIR: Pair = Pair;

fn doc_with_pair() -> (Document, u64) {
    let _ = register_generator("test_doc_ext", "pair", &PAIR);
    let mut doc = Document::default();
    doc.set_param("w", "10 mm", None, None).unwrap();
    let size = ExtInput { label: "size".into(), expr: "w".into(), kind: Kind::Length };
    let gap = ExtInput { label: "gap".into(), expr: "5 mm".into(), kind: Kind::Length };
    let kind = FeatureKind::Extension {
        ext: "test_doc_ext".into(),
        kind: "pair".into(),
        version: 1,
        inputs: ExtInputs(vec![size, gap]),
        params: serde_json::json!({}),
    };
    let id = doc.add_feature(kind, Some("Pair")).unwrap();
    doc.name_feature_inputs(id);
    (doc, id)
}

#[test]
fn extension_feature_faces_keep_their_names_when_inputs_change() {
    let (mut doc, id) = doc_with_pair();
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.results.iter().all(|r| r.error.is_none()), "{:?}", m.results);
    let st = m.state();
    let names: Vec<String> = st.bodies.iter().map(|b| b.name.clone()).collect();
    assert_eq!(names, ["Pair-1", "Pair-2"]);
    let faces = |st: &crate::ModelState, b: &str| {
        let mut v = (*crate::naming::face_names(st.body(b).unwrap())).clone();
        v.sort();
        v
    };
    let before = faces(&st, "Pair-2");
    let want: Vec<String> = ["+x", "+y", "+z", "-x", "-y", "-z"].iter().map(|r| format!("F{id}:cube[2]/{r}")).collect();
    assert_eq!(before, want);
    // The inputs are parameters (dN) like any feature's.
    assert!(doc.all_param_exprs().iter().any(|(_, e, k)| e == "w" && *k == Kind::Length), "{:?}", doc.all_param_exprs());
    doc.set_param("w", "20 mm", None, None).unwrap();
    m.evaluate(&doc);
    let st = m.state();
    assert!((crate::kernel::measure(&st.body("Pair-2").unwrap().body).unwrap().volume - 8000.0).abs() < 1e-6);
    assert_eq!(faces(&st, "Pair-2"), before);
}

#[test]
fn an_extension_feature_without_its_extension_fails_and_keeps_its_data() {
    let mut doc = Document::default();
    let params = serde_json::json!({"a": [1, 2]});
    let kind = FeatureKind::Extension { ext: "nobody".into(), kind: "nothing".into(), version: 3, inputs: ExtInputs::default(), params };
    doc.add_feature(kind.clone(), Some("Lost")).unwrap();
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.results.iter().any(|r| r.error.as_deref().is_some_and(|e| e.contains("nobody"))), "{:?}", m.results);
    let text = serde_json::to_string(&doc).unwrap();
    let back: Document = serde_json::from_str(&text).unwrap();
    assert_eq!(back.features.last().map(|f| &f.kind), Some(&kind));
}
