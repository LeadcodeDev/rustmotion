use serde_json::{Map, Value};

use crate::engine::animator::AnimatedProperties;
use crate::engine::deps::NodeRef;
use crate::expr::{Expr, ExprError, Scope};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ComputedStyle {
    pub opacity: Option<Expr>,
    pub width: Option<Expr>,
    pub height: Option<Expr>,
    pub translate_x: Option<Expr>,
    pub translate_y: Option<Expr>,
    pub scale_x: Option<Expr>,
    pub scale_y: Option<Expr>,
    pub rotate: Option<Expr>,
    pub node_refs: Vec<NodeRef>,
}

impl ComputedStyle {
    pub fn is_empty(&self) -> bool {
        self.opacity.is_none()
            && self.width.is_none()
            && self.height.is_none()
            && self.translate_x.is_none()
            && self.translate_y.is_none()
            && self.scale_x.is_none()
            && self.scale_y.is_none()
            && self.rotate.is_none()
    }

    pub fn prefer(self, base: ComputedStyle) -> ComputedStyle {
        let mut node_refs = self.node_refs;
        node_refs.extend(base.node_refs.iter().cloned());
        ComputedStyle {
            opacity: self.opacity.or(base.opacity),
            width: self.width.or(base.width),
            height: self.height.or(base.height),
            translate_x: self.translate_x.or(base.translate_x),
            translate_y: self.translate_y.or(base.translate_y),
            scale_x: self.scale_x.or(base.scale_x),
            scale_y: self.scale_y.or(base.scale_y),
            rotate: self.rotate.or(base.rotate),
            node_refs,
        }
    }

    pub fn resolve(&self, scope: &dyn Scope) -> Result<AnimatedProperties, ComputedError> {
        let mut props = AnimatedProperties::default();
        if let Some(e) = &self.opacity {
            props.opacity = eval_named(e, scope, "opacity")? as f32;
        }
        if let Some(e) = &self.width {
            props.width = eval_named(e, scope, "width")? as f32;
        }
        if let Some(e) = &self.height {
            props.height = eval_named(e, scope, "height")? as f32;
        }
        if let Some(e) = &self.translate_x {
            props.translate_x = eval_named(e, scope, "transform.translate-x.x")? as f32;
        }
        if let Some(e) = &self.translate_y {
            props.translate_y = eval_named(e, scope, "transform.translate-y.y")? as f32;
        }
        if let Some(e) = &self.scale_x {
            props.scale_x = eval_named(e, scope, "transform.scale.x")? as f32;
        }
        if let Some(e) = &self.scale_y {
            props.scale_y = eval_named(e, scope, "transform.scale.y")? as f32;
        }
        if let Some(e) = &self.rotate {
            props.rotation = eval_named(e, scope, "transform.rotate.deg")? as f32;
        }
        Ok(props)
    }
}

fn eval_named(expr: &Expr, scope: &dyn Scope, property: &str) -> Result<f64, ComputedError> {
    expr.eval(scope).map_err(|source| ComputedError {
        property: property.to_string(),
        source,
    })
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("style.{property}: {source}")]
pub struct ComputedError {
    pub property: String,
    #[source]
    pub source: ExprError,
}

pub fn extract(obj: &mut Map<String, Value>) -> Result<ComputedStyle, ComputedError> {
    let mut node_refs = Vec::new();
    let mut out = ComputedStyle {
        opacity: take_scalar(obj, "opacity", &mut node_refs)?,
        width: take_scalar(obj, "width", &mut node_refs)?,
        height: take_scalar(obj, "height", &mut node_refs)?,
        ..Default::default()
    };
    if let Some(Value::Array(items)) = obj.get_mut("transform") {
        for item in items.iter_mut() {
            extract_transform_leaf(item, &mut out, &mut node_refs)?;
        }
    }
    out.node_refs = node_refs;
    Ok(out)
}

fn as_expr_source(v: &Value) -> Option<&str> {
    match v {
        Value::String(s) if s.trim_start().starts_with('=') => Some(s.as_str()),
        _ => None,
    }
}

fn take_scalar(
    obj: &mut Map<String, Value>,
    key: &str,
    node_refs: &mut Vec<NodeRef>,
) -> Result<Option<Expr>, ComputedError> {
    let Some(src) = obj.get(key).and_then(as_expr_source) else {
        return Ok(None);
    };
    let expr = Expr::parse(src).map_err(|source| ComputedError {
        property: key.to_string(),
        source,
    })?;
    node_refs.extend(crate::engine::deps::scan_node_refs(src));
    obj.remove(key);
    Ok(Some(expr))
}

fn extract_transform_leaf(
    item: &mut Value,
    out: &mut ComputedStyle,
    node_refs: &mut Vec<NodeRef>,
) -> Result<(), ComputedError> {
    let Some(map) = item.as_object_mut() else {
        return Ok(());
    };
    let Some(tag) = map.get("fn").and_then(Value::as_str).map(str::to_string) else {
        return Ok(());
    };
    match tag.as_str() {
        "translate" => {
            take_leaf(map, "x", 0.0, &tag, &mut out.translate_x, node_refs)?;
            take_leaf(map, "y", 0.0, &tag, &mut out.translate_y, node_refs)?;
        }
        "translate-x" => take_leaf(map, "x", 0.0, &tag, &mut out.translate_x, node_refs)?,
        "translate-y" => take_leaf(map, "y", 0.0, &tag, &mut out.translate_y, node_refs)?,
        "scale" => {
            take_leaf(map, "x", 1.0, &tag, &mut out.scale_x, node_refs)?;
            take_leaf(map, "y", 1.0, &tag, &mut out.scale_y, node_refs)?;
        }
        "scale-x" => take_leaf(map, "x", 1.0, &tag, &mut out.scale_x, node_refs)?,
        "scale-y" => take_leaf(map, "y", 1.0, &tag, &mut out.scale_y, node_refs)?,
        "rotate" => take_leaf(map, "deg", 0.0, &tag, &mut out.rotate, node_refs)?,
        _ => {}
    }
    Ok(())
}

fn take_leaf(
    map: &mut Map<String, Value>,
    field: &str,
    neutral: f64,
    tag: &str,
    slot: &mut Option<Expr>,
    node_refs: &mut Vec<NodeRef>,
) -> Result<(), ComputedError> {
    let Some(src) = map.get(field).and_then(as_expr_source) else {
        return Ok(());
    };
    let expr = Expr::parse(src).map_err(|source| ComputedError {
        property: format!("transform.{tag}.{field}"),
        source,
    })?;
    node_refs.extend(crate::engine::deps::scan_node_refs(src));
    *slot = Some(expr);
    map.insert(field.to_string(), serde_json::json!(neutral));
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameClock {
    pub t: f64,
    pub t_abs: f64,
    pub duration: f64,
    pub width: f64,
    pub height: f64,
    pub fps: f64,
}

impl Scope for FrameClock {
    fn var(&self, name: &str) -> Option<f64> {
        match name {
            "t" => Some(self.t),
            "T" => Some(self.t_abs),
            "duration" => Some(self.duration),
            "W" => Some(self.width),
            "H" => Some(self.height),
            "fps" => Some(self.fps),
            _ => None,
        }
    }
}

pub struct ComposedScope<'a> {
    pub clock: FrameClock,
    pub outer: Option<&'a dyn Scope>,
}

impl Scope for ComposedScope<'_> {
    fn var(&self, name: &str) -> Option<f64> {
        self.clock
            .var(name)
            .or_else(|| self.outer.and_then(|o| o.var(name)))
    }

    fn node_prop(&self, id: &str, prop: &str) -> Option<f64> {
        self.outer.and_then(|o| o.node_prop(id, prop))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(json: &str) -> Map<String, Value> {
        match serde_json::from_str(json).unwrap() {
            Value::Object(m) => m,
            _ => panic!("not an object"),
        }
    }

    #[test]
    fn extracts_opacity_and_removes_the_key() {
        let mut o = obj(r#"{ "opacity": "= $t * 2", "z-index": 3 }"#);
        let computed = extract(&mut o).unwrap();
        assert!(computed.opacity.is_some());
        assert!(!o.contains_key("opacity"));
        assert_eq!(o.get("z-index"), Some(&Value::from(3)));
    }

    #[test]
    fn leaves_literal_opacity_untouched() {
        let mut o = obj(r#"{ "opacity": 0.5 }"#);
        let computed = extract(&mut o).unwrap();
        assert!(computed.opacity.is_none());
        assert_eq!(o.get("opacity"), Some(&Value::from(0.5)));
    }

    #[test]
    fn width_and_height_survive_as_expressions() {
        let mut o = obj(r#"{ "width": "= $W / 2", "height": "= $H / 2" }"#);
        let computed = extract(&mut o).unwrap();
        assert!(computed.width.is_some());
        assert!(computed.height.is_some());
        assert!(!o.contains_key("width"));
        assert!(!o.contains_key("height"));
    }

    #[test]
    fn transform_translate_x_extracts_and_neutralizes() {
        let mut o = obj(r#"{ "transform": [ { "fn": "translate-x", "x": "= $t * 100" } ] }"#);
        let computed = extract(&mut o).unwrap();
        assert!(computed.translate_x.is_some());
        let arr = o.get("transform").unwrap().as_array().unwrap();
        assert_eq!(arr[0]["x"], Value::from(0.0));
    }

    #[test]
    fn transform_scale_extracts_both_axes_and_neutralizes_to_one() {
        let mut o =
            obj(r#"{ "transform": [ { "fn": "scale", "x": "= 1 + $t", "y": "= 1 + $t * 2" } ] }"#);
        let computed = extract(&mut o).unwrap();
        assert!(computed.scale_x.is_some());
        assert!(computed.scale_y.is_some());
        let arr = o.get("transform").unwrap().as_array().unwrap();
        assert_eq!(arr[0]["x"], Value::from(1.0));
        assert_eq!(arr[0]["y"], Value::from(1.0));
    }

    #[test]
    fn transform_rotate_extracts_deg_and_neutralizes_to_zero() {
        let mut o = obj(r#"{ "transform": [ { "fn": "rotate", "deg": "= $t * 90" } ] }"#);
        let computed = extract(&mut o).unwrap();
        assert!(computed.rotate.is_some());
        let arr = o.get("transform").unwrap().as_array().unwrap();
        assert_eq!(arr[0]["deg"], Value::from(0.0));
    }

    #[test]
    fn uncovered_transform_fn_is_left_completely_alone() {
        let mut o = obj(r#"{ "transform": [ { "fn": "skew-x", "x": 12.0 } ] }"#);
        let computed = extract(&mut o).unwrap();
        assert!(computed.is_empty());
        let arr = o.get("transform").unwrap().as_array().unwrap();
        assert_eq!(arr[0]["x"], Value::from(12.0));
    }

    #[test]
    fn a_bad_expression_is_a_named_located_parse_error() {
        let mut o = obj(r#"{ "opacity": "= $t +" }"#);
        let err = extract(&mut o).unwrap_err();
        assert_eq!(err.property, "opacity");
        assert!(matches!(err.source, ExprError::Parse { .. }));
    }

    #[test]
    fn is_empty_is_true_for_the_default() {
        assert!(ComputedStyle::default().is_empty());
    }

    #[test]
    fn resolve_applies_frame_clock_and_yields_animated_properties() {
        let mut o = obj(r#"{ "opacity": "= 0.5 + $t / 10" }"#);
        let computed = extract(&mut o).unwrap();
        let clock = FrameClock {
            t: 2.0,
            t_abs: 2.0,
            duration: 5.0,
            width: 1920.0,
            height: 1080.0,
            fps: 30.0,
        };
        let props = computed.resolve(&clock).unwrap();
        assert!((props.opacity - 0.7).abs() < 1e-6);
    }

    #[test]
    fn resolve_reports_a_named_located_error_for_an_unresolvable_scope_reference() {
        let mut o = obj(r#"{ "opacity": "= $keyDraw" }"#);
        let computed = extract(&mut o).unwrap();
        let clock = FrameClock {
            t: 0.0,
            t_abs: 0.0,
            duration: 1.0,
            width: 1.0,
            height: 1.0,
            fps: 30.0,
        };
        let err = computed.resolve(&clock).unwrap_err();
        assert_eq!(err.property, "opacity");
        assert_eq!(err.source, ExprError::UnknownIdent("keyDraw".to_string()));
    }

    #[test]
    fn prefer_keeps_self_over_base_per_field() {
        let a = ComputedStyle {
            opacity: Some(Expr::parse("= 1").unwrap()),
            ..Default::default()
        };
        let b = ComputedStyle {
            opacity: Some(Expr::parse("= 2").unwrap()),
            width: Some(Expr::parse("= 3").unwrap()),
            ..Default::default()
        };
        let merged = a.prefer(b);
        assert_eq!(merged.opacity, Some(Expr::parse("= 1").unwrap()));
        assert_eq!(merged.width, Some(Expr::parse("= 3").unwrap()));
    }

    #[test]
    fn extract_collects_a_node_ref_from_a_covered_property() {
        let mut o = obj(r#"{ "opacity": "= node(\"badge\", \"tx\")" }"#);
        let computed = extract(&mut o).unwrap();
        assert_eq!(
            computed.node_refs,
            vec![NodeRef {
                id: "badge".to_string(),
                prop: "tx".to_string()
            }]
        );
    }

    #[test]
    fn extract_collects_node_refs_from_a_transform_leaf() {
        let mut o =
            obj(r#"{ "transform": [ { "fn": "translate-x", "x": "= node(\"chip\", \"tx\")" } ] }"#);
        let computed = extract(&mut o).unwrap();
        assert_eq!(
            computed.node_refs,
            vec![NodeRef {
                id: "chip".to_string(),
                prop: "tx".to_string()
            }]
        );
    }

    #[test]
    fn extract_finds_no_node_refs_in_a_var_only_expression() {
        let mut o = obj(r#"{ "opacity": "= $fade" }"#);
        let computed = extract(&mut o).unwrap();
        assert!(computed.node_refs.is_empty());
    }

    #[test]
    fn prefer_unions_node_refs_from_both_sides() {
        let mut a_obj = obj(r#"{ "opacity": "= node(\"a\", \"tx\")" }"#);
        let a = extract(&mut a_obj).unwrap();
        let mut b_obj = obj(r#"{ "width": "= node(\"b\", \"width\")" }"#);
        let b = extract(&mut b_obj).unwrap();
        let merged = a.prefer(b);
        assert_eq!(merged.node_refs.len(), 2);
        assert!(merged
            .node_refs
            .iter()
            .any(|r| r.id == "a" && r.prop == "tx"));
        assert!(merged
            .node_refs
            .iter()
            .any(|r| r.id == "b" && r.prop == "width"));
    }

    struct StaticOuter;
    impl Scope for StaticOuter {
        fn var(&self, name: &str) -> Option<f64> {
            match name {
                "fade" => Some(0.25),
                "t" => Some(999.0),
                _ => None,
            }
        }
        fn node_prop(&self, id: &str, prop: &str) -> Option<f64> {
            match (id, prop) {
                ("badge", "tx") => Some(42.0),
                _ => None,
            }
        }
    }

    #[test]
    fn composed_scope_prefers_the_clocks_reserved_names() {
        let clock = FrameClock {
            t: 1.0,
            t_abs: 1.0,
            duration: 1.0,
            width: 1.0,
            height: 1.0,
            fps: 30.0,
        };
        let composed = ComposedScope {
            clock,
            outer: Some(&StaticOuter),
        };
        assert_eq!(composed.var("t"), Some(1.0));
    }

    #[test]
    fn composed_scope_falls_back_to_the_outer_scope_for_vars_and_node_refs() {
        let clock = FrameClock {
            t: 1.0,
            t_abs: 1.0,
            duration: 1.0,
            width: 1.0,
            height: 1.0,
            fps: 30.0,
        };
        let composed = ComposedScope {
            clock,
            outer: Some(&StaticOuter),
        };
        assert_eq!(composed.var("fade"), Some(0.25));
        assert_eq!(composed.node_prop("badge", "tx"), Some(42.0));
        assert_eq!(composed.node_prop("nope", "tx"), None);
    }

    #[test]
    fn composed_scope_with_no_outer_behaves_exactly_like_a_bare_frame_clock() {
        let clock = FrameClock {
            t: 3.0,
            t_abs: 4.0,
            duration: 5.0,
            width: 6.0,
            height: 7.0,
            fps: 30.0,
        };
        let composed = ComposedScope { clock, outer: None };
        assert_eq!(composed.var("t"), clock.var("t"));
        assert_eq!(composed.var("W"), clock.var("W"));
        assert_eq!(composed.var("unknown"), clock.var("unknown"));
        assert_eq!(composed.node_prop("x", "y"), None);
    }
}
