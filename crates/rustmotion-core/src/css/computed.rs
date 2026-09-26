//! Where a `"= ..."` expression on a style property actually lands once it
//! survives past `rustmotion::loader::fold_static_expressions` — i.e. once
//! it reads something only known per frame (`$t`, `$T`, `$beat`, a declared
//! `vars` name, or a `node(...)` reference) and therefore cannot be folded
//! to a literal at load time. See [`crate::expr`]'s module doc for the full
//! two-tier model this is the dynamic half of (issue #338).
//!
//! [`Computed<T>`](crate::expr::Computed) already solves "a JSON value is
//! either a literal `T` or an `"= ..."` expression" *per field*, but
//! retyping [`crate::css::CssStyle`]'s existing fields (`opacity: Option<f32>`,
//! ...) to `Computed<f32>` would change their Rust type for every one of the
//! many consumers across this workspace that read them directly — not this
//! workstream's call to make. [`ComputedStyle`] is the alternative: a
//! sibling, additive field on `CssStyle` that holds the *parsed* [`Expr`]
//! for whichever of a small, fixed set of properties carried one, leaving
//! every existing typed field exactly as it was (either a literal, or
//! simply unset when the author wrote an expression instead). See
//! [`extract`] for how a `"= ..."` string moves from the raw JSON into this
//! side channel, and `CssStyle`'s own hand-written `Deserialize` impl
//! (`style.rs`) for where that happens — exactly once, at load, never
//! per frame.

use serde_json::{Map, Value};

use crate::engine::animator::AnimatedProperties;
use crate::engine::deps::NodeRef;
use crate::expr::{Expr, ExprError, Scope};

/// Parsed, not-yet-evaluated `"= ..."` expressions pulled off a
/// [`crate::css::CssStyle`] at deserialize time — one [`Expr`] per style
/// property that accepts one. A property is either a literal (its normal
/// typed field on `CssStyle`) or an expression (a field here), never both:
/// [`extract`] removes the `"= ..."` string from the JSON before the typed
/// field is deserialized, so the typed field is simply absent when this one
/// is populated.
///
/// Every [`Expr`] here was parsed exactly once, by [`extract`], when the
/// scenario was loaded — [`Self::resolve`] only ever calls [`Expr::eval`]
/// on it afterwards, once per sampled frame. See [`crate::expr`]'s "Two
/// evaluation tiers" doc: this struct is that second tier's landing zone.
///
/// # Coverage
///
/// `opacity`, `width`, `height`, and — inside a `style.transform` array
/// entry — the `x`/`y` of `translate`/`translate-x`/`translate-y`/`scale`/
/// `scale-x`/`scale-y` and the `deg` of `rotate`. Every other `CssStyle`
/// property (colors, other lengths, enums, `z`/3d transform functions,
/// `skew*`, `perspective`, `matrix`/`matrix3d`, ...) does not accept an
/// expression yet — a `"= ..."` string there still fails deserialization
/// exactly as it did before this module existed.
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
    /// Every `node("id", "prop")` call found across this node's own
    /// populated expressions above — the `(String, Vec<NodeRef>)` shape
    /// [`crate::engine::deps::DepGraph::build`] wants, pre-computed once at
    /// [`extract`] time (issue #328's join to #338). [`Expr`] itself
    /// compiles the same information internally but does not expose it
    /// (see `engine::deps`'s module doc, "Why text metrics are a trait" —
    /// no, rather its `scan_node_refs` doc — for why this crate re-derives
    /// it from the raw `"= ..."` source text instead of reaching into
    /// `Expr`'s private fields), so [`extract`] runs
    /// [`crate::engine::deps::scan_node_refs`] on each expression's source
    /// the moment it has it in hand, right before that source is discarded.
    /// Empty for the overwhelming common case (no expression at all, or an
    /// expression that only reads `$name`s) — same zero-cost-when-unused
    /// shape as every other field here.
    pub node_refs: Vec<NodeRef>,
}

impl ComputedStyle {
    /// True when this node's style carries no expression at all — the
    /// common case. `box_builder.rs` checks this before building a
    /// [`Scope`] or calling [`Self::resolve`], so a scenario that never
    /// writes `"= ..."` on a style property pays nothing beyond this one
    /// field-count check, per node, per frame — no `Expr::eval`, no `Scope`
    /// construction, no [`AnimatedProperties`] built or applied.
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

    /// `self`'s own populated fields win; `base`'s fill in whatever `self`
    /// left `None`. Used by `box_builder.rs::apply_style_states`, whose
    /// timeline-style-state merge round-trips `CssStyle` through
    /// `Serialize`/`Deserialize` — `expr` is `#[serde(skip)]` on that round
    /// trip (see `style.rs`), so the node's original expressions would
    /// otherwise vanish the instant it has any `timeline` style state. A
    /// state's own `style` block can itself carry a fresh expression on the
    /// same property (rare, but not disallowed) — that one is `self` here,
    /// and takes precedence.
    pub fn prefer(self, base: ComputedStyle) -> ComputedStyle {
        // `node_refs` is a per-field-agnostic union rather than a
        // per-field "self wins" pick like every field above: knowing
        // *which* of the 8 slots each `NodeRef` came from would need
        // per-field storage this struct doesn't keep (see the field's own
        // doc). A union can only ever add a dependency-graph edge that a
        // precise per-field pick wouldn't have — never drop a real one —
        // so the worst case is an unnecessary ordering constraint, never a
        // reference silently failing to resolve. Duplicate `NodeRef`s
        // across `self`/`base` are harmless: `DepGraph::build` counts an
        // edge once per occurrence on both sides of Kahn's algorithm, so a
        // duplicate self-corrects instead of leaving a dangling count.
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

    /// Evaluate every populated expression against `scope` and return the
    /// result shaped as an [`AnimatedProperties`] — ready for
    /// [`crate::css::apply_animated_props`], the exact same per-frame
    /// override path a resolved animation already goes through (see
    /// `css::animation`'s module doc and issue #338). `AnimatedProperties`'s
    /// own `Default` already carries the correct neutral/identity value for
    /// every field this touches (opacity 1.0, scale 1.0, translate/rotation
    /// 0.0, width/height the `-1.0` "unset" sentinel — see that type's own
    /// `Default` impl), so an unpopulated field here is simply left at that
    /// default rather than needing its own sentinel logic.
    ///
    /// Stops at the first property whose expression fails to evaluate,
    /// naming which one in [`ComputedError::property`] — never a silent
    /// zero, never a panic (issue #338's deliverable #4).
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

/// A named, located expression failure — either at extraction (a `"= ..."`
/// string that doesn't parse) or at per-frame evaluation (a `Scope` that
/// can't answer one of the expression's free variables or `node(...)`
/// calls). Always says which style property it was on and why
/// ([`ExprError`]'s own message) — issue #338's deliverable #4: never a
/// silent zero, never a panic.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("style.{property}: {source}")]
pub struct ComputedError {
    pub property: String,
    #[source]
    pub source: ExprError,
}

/// Pull every `"= ..."` expression out of a raw style JSON object, in
/// place, replacing each with a value its normal typed field can still
/// deserialize successfully:
/// - `opacity`/`width`/`height`: the key is removed outright, so the typed
///   field simply deserializes as unset (`None`) — [`ComputedStyle::resolve`]
///   supplies the per-frame value later, through the same override path a
///   literal-then-overridden-by-animation value already goes through.
/// - A covered `transform` leaf (see this module's doc, "Coverage"): the
///   key's value is replaced with that function's neutral/identity literal
///   (`0` for a translate/rotate component, `1` for a scale component).
///   This is what lets [`ComputedStyle::resolve`]'s result be *appended* as
///   a fresh `TransformFn` (via `apply_animated_props`) rather than needing
///   to patch the original array entry in place: composing an identity
///   transform with the expression's per-frame value is the same net
///   transform as if the original entry had held that value directly, for
///   every one of these single-axis functions.
///
/// Called exactly once per node, from `CssStyle`'s own `Deserialize` impl
/// (`style.rs`) — never from the per-frame path, which only ever calls
/// [`ComputedStyle::resolve`] on the result.
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

/// A minimal, self-contained [`Scope`] answering exactly the reserved
/// scenario-clock names [`crate::expr::eval`]'s own `is_dynamic_var_name`
/// treats specially (`t`, `T`, `duration` — see [`crate::expr`]'s "Two
/// evaluation tiers" doc) plus `W`/`H`/`fps`, all derivable from the same
/// per-frame context `box_builder.rs` already threads through the box tree
/// (`BuildAnimationCtx` plus the viewport size) — no `vars` table, no
/// `node(...)` dependency graph. `box_builder.rs` builds one of these for
/// every frame that has any expression to resolve at all, with zero new
/// parameters on any of its existing, publicly-called functions — see that
/// file's own doc for why that constraint mattered.
///
/// `beat` is deliberately not answered here: nothing reaching this scope
/// knows the scenario's BPM. An expression naming `$beat` (or a declared
/// `vars` name, or a `node(...)` call) gets a real
/// [`crate::expr::ExprError::UnknownIdent`] back from [`Scope::var`]/
/// [`Scope::node_prop`]'s default `None` — the same "not defined in this
/// scope" contract every [`Scope`] impl in this codebase already uses, not
/// a special case. A caller with a richer context (a `vars::VarScope`, a
/// `node(...)` dependency graph) answers those by composing its own `Scope`
/// impl instead of this one — see `vars::scope`'s module doc for exactly
/// this composition pattern.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameClock {
    /// Scene-local time in seconds — resets to 0 at the start of each scene.
    pub t: f64,
    /// Scenario-absolute time in seconds — never resets.
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

/// Composes [`FrameClock`] with a caller-supplied outer [`Scope`] — the
/// join issue #326's decomposition left open: a declared `vars` name and a
/// `node(...)` reference both answer through *some* richer `Scope`, but
/// nothing upstream of `box_builder.rs`'s per-node [`ComputedStyle::resolve`]
/// call had one to hand it. [`vars::scope`](crate::vars::scope)'s own module
/// doc explains why this is a struct holding both rather than one `Scope`
/// wrapping another: a `Scope` is only ever consumed behind `&dyn Scope`,
/// and trait objects don't nest.
///
/// # Order
///
/// [`Scope::var`] tries `clock` first, `outer` second. The six names
/// [`FrameClock`] answers (`t`, `T`, `duration`, `W`, `H`, `fps`) are
/// reserved — see that type's own doc — and must always win over a
/// same-named declared variable rather than being shadowable by one; trying
/// `clock` first is what makes that true regardless of what `outer`
/// happens to answer. [`Scope::node_prop`] only ever reaches `outer` —
/// `FrameClock` has no notion of another node's state and never will (it is
/// built fresh, per node, from data with no dependency-graph position of
/// its own).
///
/// `outer` is `None` for a build that has no richer context to offer (no
/// declared `vars`, no node ids in the scene) — [`Scope::var`] then behaves
/// exactly as a bare [`FrameClock`] would, byte for byte, which is what
/// keeps an expression-free-of-`vars`-and-`node(...)` scenario's render
/// unaffected by this type existing at all.
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
                "t" => Some(999.0), // must never win: `clock` is reserved.
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
