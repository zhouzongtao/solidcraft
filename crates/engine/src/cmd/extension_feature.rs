//! Adding an extension feature (`FeatureKind::Extension`) from an extension's own command, the
//! way built-in commands add theirs: in the active component, its inputs named as parameters
//! (`dN`), its bodies named from `name` / `body_names` in `p`, the model brought up to date.

use serde_json::Value;
use solidcraft_doc::FeatureKind;
use solidcraft_doc::extension::{ExtInput, ExtInputs};

use crate::{Result, Session};

/// Add an extension feature of `ext` / `kind` with `inputs` (expressions; give lengths with their
/// unit, `"875 mm"`, as a bare number is in the design's units) and `params`. `p` may give
/// `name` and `body_names`. Returns what the command adding a feature returns.
pub fn add_extension_feature(s: &mut Session, p: &Value, ext: &str, kind: &str, version: u64, inputs: Vec<ExtInput>, params: Value) -> Result<Value> {
    let kind = FeatureKind::Extension { ext: ext.to_owned(), kind: kind.to_owned(), version, inputs: ExtInputs(inputs), params };
    super::features::add_feature(s, p, kind)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use solidcraft_doc::expr::Kind;
    use solidcraft_doc::extension::{FeatureGenerator, GenCtx, GeneratedBody, register_generator};

    use super::*;

    struct Cube;
    impl FeatureGenerator for Cube {
        fn inputs(&self) -> &'static [(&'static str, Kind, &'static str)] {
            &[("a", Kind::Length, "10 mm")]
        }
        fn generate(&self, ctx: &GenCtx) -> std::result::Result<Vec<GeneratedBody>, String> {
            let a = ctx.num("a", 10.0);
            let v = crate::geom::Vec3::new(a, a, a);
            let body = crate::kernel::box_solid(crate::geom::Vec3::ZERO, v).map_err(|e| e.to_string())?;
            let roles = (0..body.face_count()).map(|k| vec![format!("f{k}")]).collect();
            Ok(vec![GeneratedBody { key: "cube".into(), name: format!("{}-cube", ctx.name), body, roles }])
        }
    }
    static CUBE: Cube = Cube;

    #[test]
    fn an_extension_feature_is_added_and_edited_like_any_feature() {
        let _ = register_generator("test_engine_ext", "cube", &CUBE);
        let mut s = Session::default();
        let inputs = vec![ExtInput { label: "a".into(), expr: "10 mm".into(), kind: Kind::Length }];
        let r = add_extension_feature(&mut s, &json!({"name": "Block"}), "test_engine_ext", "cube", 1, inputs, json!({})).unwrap();
        let id = r["feature"].as_u64().unwrap();
        let st = s.model.state();
        let b = st.body("Block-cube").unwrap();
        assert_eq!(b.body.face_count(), 6);
        let names = solidcraft_doc::naming::face_names(b);
        assert!(names.iter().all(|n| n.starts_with(&format!("F{id}:cube/f"))), "{names:?}");
        let set = json!({"inputs": [{"label": "a", "expr": "20 mm", "kind": "length"}]});
        s.execute("timeline.edit", &json!({"feature": id, "set": set})).unwrap();
        let st = s.model.state();
        let b = st.body("Block-cube").unwrap();
        assert!((crate::kernel::measure(&b.body).unwrap().volume - 8000.0).abs() < 1e-6);
        assert_eq!(*solidcraft_doc::naming::face_names(b), *names);
    }
}
