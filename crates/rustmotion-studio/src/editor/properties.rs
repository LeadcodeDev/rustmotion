//! Schema-driven property registry for the inspector.
//!
//! Derived lazily (OnceLock) from `schemars::schema_for!(Component)` — the
//! same source as `rustmotion validate`'s unknown-attribute check — and from
//! `schema_for!(CssStyle)` for the CSS sections. Two invariants:
//!
//! 1. Component root fields: every non-excluded property of every `oneOf`
//!    variant gets a typed [`PropSpec`]; the exclusion list below is the only
//!    curation.
//! 2. CSS completeness BY CONSTRUCTION: every property of the `CssStyle`
//!    schema lands in exactly one [`CssSection`] — unmapped ones fall into
//!    `Advanced` automatically, so schema evolution can never silently drop a
//!    property from the inspector (locked by a test).

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde_json::Value;

use rustmotion::components::Component;
use rustmotion::core::css::CssStyle;

/// Root component fields NEVER shown as generic controls:
/// - `type`: the component identity, not editable.
/// - `style`: the whole CSS block (has its own sections).
/// - `children`: structural.
/// - `position`, `x`, `y`, `z-index`: `ChildComponent` wrapper fields.
/// - `animation`, `timeline`: structured animation config (dedicated tooling
///   later; a generic control would invite corruption).
/// - Structured data arrays (chart `data`, rich_text `spans`, …): no sensible
///   generic control in v1.
pub const EXCLUDED_FIELDS: &[&str] = &[
    "type",
    "style",
    "children",
    "position",
    "x",
    "y",
    "z-index",
    "animation",
    "timeline",
    // structured data arrays
    "data",
    "radar_data",
    "spans",
    "words",
    "lines",
    "steps",
    "items",
    "avatars",
    "points",
    "headers",
    "rows",
    "tags",
    "keyframes",
    "cells",
    "columns",
    "series",
    "segments",
    "stops",
    "states",
];

/// How a schema field is edited by the generic control factory.
#[derive(Debug, Clone, PartialEq)]
pub enum PropKind {
    /// Schema `"type": "integer"` (u8/u32/i64… — schemars adds a
    /// `format: uint8/uint32/…` hint). Writes REAL JSON integers: a `12.0`
    /// written into a `u32` field fails the typed parse ("invalid type:
    /// floating point") and would drop the element at render.
    Integer,
    /// Schema `"type": "number"` (f32/f64 — `format: float/double`).
    Float,
    Bool,
    String,
    /// String enum with the exact variants from the schema.
    Enum(Vec<String>),
    /// String whose name contains "color" → color picker.
    Color,
    /// Array of color strings (gradient_text `colors`) → per-entry pickers.
    ColorList,
    /// Untagged string|gradient-object (shape `fill`) → mode-switched editor.
    Fill,
    /// Untagged number|string (Length, LengthPercentage, Size, …) → unit input.
    Unit,
    /// Object with known schema properties (stat `trend`, `stroke`, …) →
    /// indented sub-rows through the same control factory (depth ≤ 2).
    Object(Vec<PropSpec>),
    /// Array of numbers (stat `sparkline_data`, …) → per-entry number rows.
    NumberList,
    /// Array of non-color strings (chart `axes`/`categories`, …) → per-entry
    /// text rows.
    StringList,
    /// Objects/arrays without a known shape → JSON textarea.
    Complex,
}

/// Shortest display form of a numeric raw string: `"405.0"` → `"405"`,
/// `"1.40"` → `"1.4"`, `"100"` → `"100"` (Rust's f64 Display is
/// shortest-round-trip). Non-numeric input passes through unchanged.
pub fn display_number(raw: &str) -> String {
    raw.trim()
        .parse::<f64>()
        .map(|v| v.to_string())
        .unwrap_or_else(|_| raw.to_string())
}

#[derive(Debug, Clone, PartialEq)]
pub struct PropSpec {
    pub name: String,
    pub kind: PropKind,
}

/// Default-palette prefill for an EMPTY color list: for `chart.colors`,
/// "+ Add color" seeds the list with the engine's actual rendering palette so
/// the user edits what the canvas shows instead of starting from nothing.
/// Extend the map here when other components gain a palette.
pub fn palette_prefill(
    tag: &str,
    field: &str,
    list_is_empty: bool,
) -> Option<&'static [&'static str]> {
    if !list_is_empty {
        return None;
    }
    match (tag, field) {
        ("chart", "colors") => Some(rustmotion::components::chart::DEFAULT_PALETTE),
        _ => None,
    }
}

/// Mutate one sub-key of an object value: `Null` prunes the key; an object
/// left empty collapses to `Null` (the whole field gets removed).
pub fn mutate_object_field(current: &Value, key: &str, new: Value) -> Value {
    let mut map = current.as_object().cloned().unwrap_or_default();
    if new.is_null() {
        map.remove(key);
    } else {
        map.insert(key.to_string(), new);
    }
    if map.is_empty() {
        Value::Null
    } else {
        Value::Object(map)
    }
}

/// Display mode of a [`PropKind::Fill`] value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillMode {
    Single,
    Linear,
    Radial,
}

/// Decompose an existing fill value into `(mode, colors, angle)`. A bare
/// string is a single color; objects pick Linear/Radial from their `type`.
pub fn parse_fill(v: &Value) -> (FillMode, Vec<String>, f64) {
    match v {
        Value::String(s) => (FillMode::Single, vec![s.clone()], 0.0),
        Value::Object(o) => {
            let colors: Vec<String> = o
                .get("colors")
                .and_then(|c| c.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|c| c.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let angle = o.get("angle").and_then(|a| a.as_f64()).unwrap_or(0.0);
            let mode = match o.get("type").and_then(|t| t.as_str()) {
                Some("radial") => FillMode::Radial,
                _ => FillMode::Linear,
            };
            (mode, colors, angle)
        }
        _ => (FillMode::Single, Vec::new(), 0.0),
    }
}

/// Serialize a fill editor state back to the scenario value: Single → the hex
/// string, Linear/Radial → the gradient object (angle only on Linear).
pub fn fill_to_value(mode: FillMode, colors: &[String], angle: f64) -> Value {
    match mode {
        FillMode::Single => Value::String(
            colors
                .first()
                .cloned()
                .unwrap_or_else(|| "#ffffff".to_string()),
        ),
        FillMode::Linear => serde_json::json!({
            "type": "linear",
            "colors": colors,
            "angle": angle,
        }),
        FillMode::Radial => serde_json::json!({
            "type": "radial",
            "colors": colors,
        }),
    }
}

/// The element as the ENGINE sees it: typed round-trip through `Component`
/// fills every `#[serde(default)]`. Falls back to the raw element when the
/// round-trip fails (invalid element).
pub fn effective_element(raw: &Value) -> Value {
    serde_json::from_value::<Component>(raw.clone())
        .ok()
        .and_then(|c| serde_json::to_value(&c).ok())
        .unwrap_or_else(|| raw.clone())
}

/// Multiline heuristic for String controls: known long-form field names, or a
/// current value that already contains a newline.
pub fn is_multiline(name: &str, value: &str) -> bool {
    matches!(name, "code" | "content" | "message") || value.contains('\n')
}

/// Engine-default placeholder for string inputs (cascade defaults the schema
/// can't know): `font-family` → "Inter".
pub fn engine_placeholder(name: &str) -> Option<&'static str> {
    match name {
        "font-family" => Some("Inter"),
        _ => None,
    }
}

/// Root fields (name + kind) for a component tag, schema order. `None` for
/// unknown tags.
pub fn component_props(tag: &str) -> Option<&'static Vec<PropSpec>> {
    component_registry().get(tag)
}

/// tag → root PropSpecs, from the `oneOf` variants of the Component schema
/// (same walk as the CLI's unknown-attribute check).
fn component_registry() -> &'static BTreeMap<String, Vec<PropSpec>> {
    static CACHE: OnceLock<BTreeMap<String, Vec<PropSpec>>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let schema = serde_json::to_value(schemars::schema_for!(Component))
            .expect("Component schema serializes");
        let defs = schema.get("definitions").cloned().unwrap_or(Value::Null);
        let mut map = BTreeMap::new();
        if let Some(one_of) = schema["oneOf"].as_array() {
            for variant in one_of {
                let Some(tag) = variant["properties"]["type"]["enum"][0].as_str() else {
                    continue;
                };
                let Some(props) = variant["properties"].as_object() else {
                    continue;
                };
                let specs: Vec<PropSpec> = props
                    .iter()
                    .filter(|(name, _)| !EXCLUDED_FIELDS.contains(&name.as_str()))
                    .map(|(name, prop_schema)| PropSpec {
                        name: name.clone(),
                        kind: kind_of_schema(name, prop_schema, &defs, 0),
                    })
                    .collect();
                map.insert(tag.to_string(), specs);
            }
        }
        map
    })
}

// ── Schema walking ───────────────────────────────────────────────────────────

/// Resolve one property schema to a [`PropKind`]. Handles `$ref` into
/// definitions, single-arm `allOf` wrappers, nullable `anyOf [T, null]`, and
/// untagged unions (`anyOf` of several arms).
fn kind_of_schema(name: &str, schema: &Value, defs: &Value, depth: u8) -> PropKind {
    kind_of_schema_inner(name, schema, defs, depth, 0)
}

/// `obj_level` counts object nesting for [`PropKind::Object`] (capped at 2 —
/// deeper structures fall back to the JSON textarea).
fn kind_of_schema_inner(
    name: &str,
    schema: &Value,
    defs: &Value,
    depth: u8,
    obj_level: u8,
) -> PropKind {
    if depth > 8 {
        return PropKind::Complex;
    }
    // $ref → definitions lookup.
    if let Some(r) = schema.get("$ref").and_then(|r| r.as_str()) {
        let key = r.rsplit('/').next().unwrap_or_default();
        return match defs.get(key) {
            Some(target) => kind_of_schema_inner(name, target, defs, depth + 1, obj_level),
            None => PropKind::Complex,
        };
    }
    // allOf: [X] wrapper (schemars uses it to attach descriptions to refs).
    if let Some(all) = schema.get("allOf").and_then(|a| a.as_array()) {
        if all.len() == 1 {
            return kind_of_schema_inner(name, &all[0], defs, depth + 1, obj_level);
        }
    }
    // Direct string enum.
    if let Some(variants) = string_enum(schema) {
        return PropKind::Enum(variants);
    }
    // anyOf/oneOf: drop null arms; single arm → recurse; several → union,
    // flattened recursively (Size nests LengthPercentage which nests
    // number|string).
    for key in ["anyOf", "oneOf"] {
        if let Some(arms) = schema.get(key).and_then(|a| a.as_array()) {
            let non_null: Vec<&Value> = arms.iter().filter(|a| !is_null_schema(a)).collect();
            if non_null.len() == 1 {
                return kind_of_schema_inner(name, non_null[0], defs, depth + 1, obj_level);
            }
            let mut info = UnionInfo::default();
            for arm in &non_null {
                collect_union(arm, defs, depth + 1, &mut info);
            }
            // Decision order (see the doc comment on `UnionInfo`).
            if info.has_string && info.has_gradient_object {
                return PropKind::Fill;
            }
            if info.has_string && (name.contains("color") || name.contains("colour")) {
                return PropKind::Color;
            }
            if info.has_number && (info.has_string || !info.variants.is_empty()) {
                return PropKind::Unit;
            }
            if !info.variants.is_empty() && !info.has_string && !info.has_number {
                return PropKind::Enum(info.variants);
            }
            if info.has_string {
                return PropKind::String;
            }
            return PropKind::Complex;
        }
    }
    match primary_type(schema) {
        Some("integer") => PropKind::Integer,
        Some("number") => PropKind::Float,
        Some("object") if obj_level < 2 => match object_specs(schema, defs, depth, obj_level) {
            Some(specs) => PropKind::Object(specs),
            None => PropKind::Complex,
        },
        Some("boolean") => PropKind::Bool,
        Some("string") if name.contains("color") || name.contains("colour") => PropKind::Color,
        Some("string") => PropKind::String,
        Some("array") if is_color_string_array(name, schema, defs, depth) => PropKind::ColorList,
        Some("array") if is_number_array(schema, defs, depth) => PropKind::NumberList,
        Some("array") if is_string_array(schema, defs, depth) => PropKind::StringList,
        _ => PropKind::Complex,
    }
}

/// Sub-specs of an object schema with KNOWN properties (None for map-like /
/// empty objects → JSON textarea fallback).
fn object_specs(schema: &Value, defs: &Value, depth: u8, obj_level: u8) -> Option<Vec<PropSpec>> {
    let props = schema.get("properties")?.as_object()?;
    if props.is_empty() {
        return None;
    }
    Some(
        props
            .iter()
            .map(|(n, ps)| PropSpec {
                name: n.clone(),
                kind: kind_of_schema_inner(n, ps, defs, depth + 1, obj_level + 1),
            })
            .collect(),
    )
}

/// Array whose items resolve to plain strings (labels, axes, categories) —
/// color-string arrays are caught earlier by `is_color_string_array`.
fn is_string_array(schema: &Value, defs: &Value, depth: u8) -> bool {
    let Some(items) = schema.get("items") else {
        return false;
    };
    let resolved = resolve_arm(items, defs, depth + 1);
    if primary_type(&resolved) == Some("string") {
        return true;
    }
    let mut info = UnionInfo::default();
    collect_union(items, defs, depth + 1, &mut info);
    info.has_string && !info.has_number
}

/// Array whose items resolve to numbers (`Vec<f64>`, dash patterns, …).
fn is_number_array(schema: &Value, defs: &Value, depth: u8) -> bool {
    let Some(items) = schema.get("items") else {
        return false;
    };
    let resolved = resolve_arm(items, defs, depth + 1);
    if matches!(primary_type(&resolved), Some("number") | Some("integer")) {
        return true;
    }
    let mut info = UnionInfo::default();
    collect_union(items, defs, depth + 1, &mut info);
    info.has_number && !info.has_string && info.variants.is_empty()
}

/// Array-of-color-strings detection: the items resolve to strings (or a
/// Color-typed union with a string arm) AND either the field name contains
/// "color" or the items are the `Color` schema type.
fn is_color_string_array(name: &str, schema: &Value, defs: &Value, depth: u8) -> bool {
    let Some(items) = schema.get("items") else {
        return false;
    };
    let items_ref_is_color = items
        .get("$ref")
        .and_then(|r| r.as_str())
        .is_some_and(|r| r.rsplit('/').next().unwrap_or_default().contains("Color"));
    let resolved = resolve_arm(items, defs, depth + 1);
    let mut info = UnionInfo::default();
    collect_union(items, defs, depth + 1, &mut info);
    let string_items = primary_type(&resolved) == Some("string") || info.has_string;
    string_items && (name.contains("color") || name.contains("colour") || items_ref_is_color)
}

/// What a (recursively flattened) union offers. Decision order in
/// `kind_of_schema`:
/// 1. string arm + color-ish name → `Color` (untagged string|rgba).
/// 2. number + (string or keywords) → `Unit` (Length/Size: free text accepts
///    numbers, "12px" and keywords alike).
/// 3. keywords only → `Enum` (an unreachable object arm like cubic-bezier is
///    accepted collateral — the picker covers the keyword variants).
/// 4. any string arm → `String` (free text is always a valid input).
/// 5. otherwise `Complex` (JSON textarea).
#[derive(Default)]
struct UnionInfo {
    variants: Vec<String>,
    has_number: bool,
    has_string: bool,
    /// An object arm with a `colors` property (the gradient side of `Fill`).
    has_gradient_object: bool,
}

/// Recursively flatten a union arm into `UnionInfo`.
fn collect_union(arm: &Value, defs: &Value, depth: u8, info: &mut UnionInfo) {
    if depth > 8 {
        return;
    }
    let s = resolve_arm(arm, defs, depth);
    if let Some(v) = string_enum(&s) {
        info.variants.extend(v);
        return;
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(arms) = s.get(key).and_then(|a| a.as_array()) {
            for nested in arms.iter().filter(|a| !is_null_schema(a)) {
                collect_union(nested, defs, depth + 1, info);
            }
            return;
        }
    }
    match primary_type(&s) {
        Some("number") | Some("integer") => info.has_number = true,
        Some("string") => info.has_string = true,
        Some("object")
            if s.get("properties")
                .and_then(|p| p.as_object())
                .is_some_and(|p| p.contains_key("colors")) =>
        {
            info.has_gradient_object = true;
        }
        _ => {}
    }
}

/// Follow refs/allOf for one union arm (no kind decision).
fn resolve_arm(arm: &Value, defs: &Value, depth: u8) -> Value {
    if depth > 8 {
        return arm.clone();
    }
    if let Some(r) = arm.get("$ref").and_then(|r| r.as_str()) {
        let key = r.rsplit('/').next().unwrap_or_default();
        if let Some(target) = defs.get(key) {
            return resolve_arm(target, defs, depth + 1);
        }
    }
    if let Some(all) = arm.get("allOf").and_then(|a| a.as_array()) {
        if all.len() == 1 {
            return resolve_arm(&all[0], defs, depth + 1);
        }
    }
    arm.clone()
}

/// The string variants of `{"enum": ["a", "b"]}` schemas, if all-string.
fn string_enum(schema: &Value) -> Option<Vec<String>> {
    let arr = schema.get("enum")?.as_array()?;
    let variants: Vec<String> = arr
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    if variants.len() == arr.len() && !variants.is_empty() {
        Some(variants)
    } else {
        None
    }
}

fn is_null_schema(schema: &Value) -> bool {
    schema.get("type").and_then(|t| t.as_str()) == Some("null")
}

/// The non-null primary type of a schema (`"type": "x"` or `["x", "null"]`).
fn primary_type(schema: &Value) -> Option<&str> {
    match schema.get("type") {
        Some(Value::String(s)) => Some(s.as_str()),
        Some(Value::Array(a)) => a.iter().filter_map(|v| v.as_str()).find(|s| *s != "null"),
        _ => None,
    }
}

// ── CSS sections ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssSection {
    Typography,
    Layout,
    Sizing,
    Spacing,
    Position,
    Visual,
    Effects,
    Overflow,
    Advanced,
}

impl CssSection {
    pub fn label(self) -> &'static str {
        match self {
            CssSection::Typography => "Typography",
            CssSection::Layout => "Layout",
            CssSection::Sizing => "Sizing",
            CssSection::Spacing => "Spacing",
            CssSection::Position => "Position",
            CssSection::Visual => "Visual",
            CssSection::Effects => "Effects",
            CssSection::Overflow => "Overflow",
            CssSection::Advanced => "Advanced",
        }
    }
}

/// Section for a CSS property name. Every KNOWN property is mapped explicitly;
/// anything else (including future schema additions) lands in `Advanced`.
pub fn section_for(prop: &str) -> CssSection {
    use CssSection::*;
    match prop {
        // Typography
        "font-family" | "font-size" | "font-weight" | "font-style" | "line-height"
        | "letter-spacing" | "text-align" | "color" | "white-space" | "text-decoration"
        | "text-shadow" => Typography,
        // Layout (flex + grid)
        "display"
        | "flex-direction"
        | "flex-wrap"
        | "justify-content"
        | "align-items"
        | "align-self"
        | "align-content"
        | "gap"
        | "flex-grow"
        | "flex-shrink"
        | "flex-basis"
        | "order"
        | "grid-template-columns"
        | "grid-template-rows"
        | "grid-column"
        | "grid-row"
        | "grid-auto-flow"
        | "justify-items"
        | "justify-self" => Layout,
        // Sizing
        "width" | "height" | "min-width" | "min-height" | "max-width" | "max-height"
        | "aspect-ratio" | "box-sizing" => Sizing,
        // Spacing
        "padding" | "margin" => Spacing,
        // Position
        "position" | "top" | "right" | "bottom" | "left" | "z-index" => Position,
        // Visual
        "background" | "border" | "border-radius" | "box-shadow" | "opacity" | "mix-blend-mode"
        | "visibility" | "clip-path" => Visual,
        // Effects
        "filter" | "backdrop-filter" | "transform" | "transform-origin" | "perspective"
        | "perspective-origin" | "transition" | "audio-reactive" => Effects,
        // Overflow
        "overflow" | "overflow-x" | "overflow-y" | "text-overflow" | "overflow-wrap" => Overflow,
        // Everything else — including future schema additions — by construction.
        _ => Advanced,
    }
}

/// All `CssStyle` schema properties with kinds, in schema order.
pub fn css_props() -> &'static Vec<PropSpec> {
    static CACHE: OnceLock<Vec<PropSpec>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let schema = serde_json::to_value(schemars::schema_for!(CssStyle))
            .expect("CssStyle schema serializes");
        let defs = schema.get("definitions").cloned().unwrap_or(Value::Null);
        schema["properties"]
            .as_object()
            .map(|props| {
                props
                    .iter()
                    .map(|(name, prop_schema)| PropSpec {
                        name: name.clone(),
                        kind: kind_of_schema(name, prop_schema, &defs, 0),
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// The properties of one section, schema order.
pub fn css_section_props(section: CssSection) -> Vec<&'static PropSpec> {
    css_props()
        .iter()
        .filter(|p| section_for(&p.name) == section)
        .collect()
}

// ── Family → visible sections ────────────────────────────────────────────────

/// Which CSS sections a component family gets: Typography for text-likes,
/// Layout for containers, the common trunk for everyone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssFamily {
    TextLike,
    Container,
    Plain,
}

/// Classify a component tag for CSS-section visibility (independent from the
/// inspector's curated-section `Family`).
pub fn css_family(tag: &str) -> CssFamily {
    match tag {
        "text" | "caption" | "gradient_text" | "rich_text" | "counter" | "kbd" | "badge"
        | "marquee" | "callout" | "tooltip" | "codeblock" | "terminal" | "list" | "tag_cloud" => {
            CssFamily::TextLike
        }
        "container" | "div" | "card" | "flex" | "grid" | "positioned" => CssFamily::Container,
        _ => CssFamily::Plain,
    }
}

/// The CSS sections shown for a family, display order: the family-specific
/// section first, then the common trunk.
pub fn visible_sections(family: CssFamily) -> Vec<CssSection> {
    use CssSection::*;
    let mut out = Vec::new();
    match family {
        CssFamily::TextLike => out.push(Typography),
        CssFamily::Container => out.push(Layout),
        CssFamily::Plain => {}
    }
    out.extend([
        Sizing, Spacing, Position, Visual, Effects, Overflow, Advanced,
    ]);
    out
}

/// Heuristic slider ranges by property name: `(min, max, step)`. Anything not
/// listed gets a bare input.
pub fn slider_range(prop: &str) -> Option<(f64, f64, f64)> {
    match prop {
        "opacity" => Some((0.0, 1.0, 0.01)),
        "font-size" => Some((8.0, 300.0, 1.0)),
        "line-height" => Some((0.8, 3.0, 0.1)),
        "letter-spacing" => Some((-5.0, 20.0, 0.5)),
        "flex-grow" | "flex-shrink" => Some((0.0, 10.0, 0.1)),
        "aspect-ratio" => Some((0.1, 4.0, 0.05)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind_of<'a>(props: &'a [PropSpec], name: &str) -> Option<&'a PropKind> {
        props.iter().find(|p| p.name == name).map(|p| &p.kind)
    }

    // ── Component registry ──────────────────────────────────────────────

    #[test]
    fn counter_exposes_its_root_fields_with_kinds() {
        let props = component_props("counter").expect("counter in schema");
        assert_eq!(kind_of(props, "from"), Some(&PropKind::Float));
        assert_eq!(kind_of(props, "to"), Some(&PropKind::Float));
        assert_eq!(kind_of(props, "decimals"), Some(&PropKind::Integer));
        assert_eq!(kind_of(props, "separator"), Some(&PropKind::String));
        assert_eq!(kind_of(props, "prefix"), Some(&PropKind::String));
        assert_eq!(kind_of(props, "suffix"), Some(&PropKind::String));
        // The exact enum variants come from the schema (snake_case).
        match kind_of(props, "easing") {
            Some(PropKind::Enum(variants)) => {
                assert!(variants.contains(&"linear".to_string()), "{variants:?}");
                assert!(
                    variants.contains(&"ease_in_out".to_string()),
                    "{variants:?}"
                );
            }
            other => panic!("easing should be Enum, got {other:?}"),
        }
    }

    #[test]
    fn excluded_fields_never_appear() {
        for tag in ["text", "counter", "card", "chart", "rich_text"] {
            let Some(props) = component_props(tag) else {
                panic!("{tag} missing from schema registry");
            };
            for excluded in EXCLUDED_FIELDS {
                assert!(
                    !props.iter().any(|p| &p.name == excluded),
                    "{tag} must not expose '{excluded}'"
                );
            }
        }
        // Spot-check the data arrays actually exist on their components and
        // are excluded (chart.data, rich_text.spans).
        assert!(component_props("chart").is_some());
        assert!(component_props("rich_text").is_some());
    }

    #[test]
    fn unknown_tag_has_no_props() {
        assert!(component_props("definitely_not_a_component").is_none());
    }

    // ── CSS bucketing: completeness by construction ─────────────────────

    #[test]
    fn every_css_schema_property_is_in_exactly_one_section() {
        let all = css_props();
        assert!(
            all.len() > 50,
            "CssStyle schema should be large: {}",
            all.len()
        );
        let sections = [
            CssSection::Typography,
            CssSection::Layout,
            CssSection::Sizing,
            CssSection::Spacing,
            CssSection::Position,
            CssSection::Visual,
            CssSection::Effects,
            CssSection::Overflow,
            CssSection::Advanced,
        ];
        let mut seen = std::collections::BTreeMap::new();
        for s in sections {
            for p in css_section_props(s) {
                *seen.entry(p.name.clone()).or_insert(0usize) += 1;
            }
        }
        for p in all {
            assert_eq!(
                seen.get(&p.name),
                Some(&1),
                "property '{}' must be in exactly one section",
                p.name
            );
        }
        let total: usize = seen.values().sum();
        assert_eq!(total, all.len(), "no extra properties invented");
    }

    #[test]
    fn css_spot_checks() {
        assert_eq!(section_for("font-size"), CssSection::Typography);
        assert_eq!(section_for("backdrop-filter"), CssSection::Effects);
        assert_eq!(section_for("display"), CssSection::Layout);
        assert_eq!(section_for("padding"), CssSection::Spacing);
        assert_eq!(section_for("z-index"), CssSection::Position);
        assert_eq!(section_for("box-shadow"), CssSection::Visual);
        assert_eq!(section_for("overflow-wrap"), CssSection::Overflow);
        assert_eq!(section_for("width"), CssSection::Sizing);
    }

    #[test]
    fn future_unknown_property_lands_in_advanced() {
        assert_eq!(section_for("grid-magic-2030"), CssSection::Advanced);
        assert_eq!(section_for("scroll-timeline"), CssSection::Advanced);
    }

    #[test]
    fn css_kinds_are_usable() {
        let all = css_props();
        // color is a Color control, opacity a Number, display an Enum,
        // width a Unit (untagged number|string), box-shadow Complex.
        assert_eq!(kind_of(all, "color"), Some(&PropKind::Color));
        assert_eq!(kind_of(all, "opacity"), Some(&PropKind::Float));
        assert!(matches!(kind_of(all, "display"), Some(PropKind::Enum(_))));
        assert!(matches!(
            kind_of(all, "width"),
            Some(PropKind::Unit) | Some(PropKind::Complex)
        ));
        assert!(matches!(
            kind_of(all, "box-shadow"),
            Some(PropKind::Complex)
        ));
    }

    // ── Family visibility ───────────────────────────────────────────────

    #[test]
    fn counter_is_text_like_and_gets_typography() {
        assert_eq!(css_family("counter"), CssFamily::TextLike);
        assert!(visible_sections(CssFamily::TextLike).contains(&CssSection::Typography));
        assert!(!visible_sections(CssFamily::TextLike).contains(&CssSection::Layout));
    }

    #[test]
    fn card_is_container_and_gets_layout() {
        assert_eq!(css_family("card"), CssFamily::Container);
        assert!(visible_sections(CssFamily::Container).contains(&CssSection::Layout));
        assert!(!visible_sections(CssFamily::Container).contains(&CssSection::Typography));
    }

    #[test]
    fn plain_family_gets_common_trunk_only() {
        assert_eq!(css_family("shape"), CssFamily::Plain);
        let v = visible_sections(CssFamily::Plain);
        assert!(!v.contains(&CssSection::Typography));
        assert!(!v.contains(&CssSection::Layout));
        for s in [
            CssSection::Sizing,
            CssSection::Spacing,
            CssSection::Position,
            CssSection::Visual,
            CssSection::Effects,
            CssSection::Overflow,
            CssSection::Advanced,
        ] {
            assert!(v.contains(&s), "{s:?} missing from common trunk");
        }
    }

    // ── Round 6: string lists / palette prefill / typed enums ───────────

    #[test]
    fn chart_axes_and_categories_are_string_lists() {
        let props = component_props("chart").expect("chart in schema");
        assert_eq!(kind_of(props, "axes"), Some(&PropKind::StringList));
        assert_eq!(kind_of(props, "categories"), Some(&PropKind::StringList));
        // colors keeps the color editor, sparkline stays numeric (stat).
        assert_eq!(kind_of(props, "colors"), Some(&PropKind::ColorList));
        let stat = component_props("stat").unwrap();
        assert_eq!(kind_of(stat, "sparkline_data"), Some(&PropKind::NumberList));
    }

    #[test]
    fn radar_data_is_excluded_from_properties() {
        let props = component_props("chart").expect("chart in schema");
        assert!(
            !props.iter().any(|p| p.name == "radar_data"),
            "radar_data is chart data — excluded from the generic editor"
        );
    }

    #[test]
    fn palette_prefill_only_for_empty_chart_colors() {
        let palette = palette_prefill("chart", "colors", true).expect("chart palette");
        assert_eq!(palette.len(), 8);
        assert_eq!(palette[0], "#3B82F6");
        // Non-empty list → no prefill (don't clobber user colors).
        assert_eq!(palette_prefill("chart", "colors", false), None);
        // Other tags/fields → None.
        assert_eq!(palette_prefill("gradient_text", "colors", true), None);
        assert_eq!(palette_prefill("chart", "axes", true), None);
    }

    #[test]
    fn converted_enum_fields_expose_exact_variants() {
        let stepper = component_props("stepper").expect("stepper in schema");
        match kind_of(stepper, "orientation") {
            Some(PropKind::Enum(v)) => {
                assert!(v.contains(&"horizontal".to_string()), "{v:?}");
                assert!(v.contains(&"vertical".to_string()), "{v:?}");
            }
            other => panic!("orientation should be Enum, got {other:?}"),
        }
        let chart = component_props("chart").expect("chart in schema");
        match kind_of(chart, "direction") {
            Some(PropKind::Enum(v)) => {
                assert!(v.contains(&"vertical".to_string()), "{v:?}");
                assert!(v.contains(&"horizontal".to_string()), "{v:?}");
            }
            other => panic!("direction should be Enum, got {other:?}"),
        }
    }

    // ── Round 5: object / number-list editors ───────────────────────────

    #[test]
    fn stat_trend_is_an_object_with_known_sub_kinds() {
        let props = component_props("stat").expect("stat in schema");
        match kind_of(props, "trend") {
            Some(PropKind::Object(specs)) => {
                let sub = |n: &str| specs.iter().find(|p| p.name == n).map(|p| &p.kind);
                assert_eq!(sub("value"), Some(&PropKind::String));
                match sub("direction") {
                    Some(PropKind::Enum(v)) => {
                        assert!(v.contains(&"up".to_string()), "{v:?}");
                        assert!(v.contains(&"down".to_string()), "{v:?}");
                        assert!(v.contains(&"neutral".to_string()), "{v:?}");
                    }
                    other => panic!("direction should be Enum, got {other:?}"),
                }
                assert_eq!(sub("color"), Some(&PropKind::Color));
            }
            other => panic!("trend should be Object, got {other:?}"),
        }
    }

    #[test]
    fn shapeless_objects_stay_json_areas() {
        // An object without known properties (map-like) must NOT become an
        // Object control.
        let map_schema = serde_json::json!({
            "type": "object",
            "additionalProperties": { "type": "string" }
        });
        assert_eq!(
            kind_of_schema("anything", &map_schema, &Value::Null, 0),
            PropKind::Complex
        );
    }

    #[test]
    fn number_arrays_become_number_lists() {
        let props = component_props("stat").expect("stat in schema");
        assert_eq!(
            kind_of(props, "sparkline_data"),
            Some(&PropKind::NumberList)
        );
        // Color arrays keep their dedicated editor.
        let gt = component_props("gradient_text").unwrap();
        assert_eq!(kind_of(gt, "colors"), Some(&PropKind::ColorList));
        // Plain string arrays get their own editor (round 6).
        let strings = serde_json::json!({"type": "array", "items": {"type": "string"}});
        assert_eq!(
            kind_of_schema("labels", &strings, &Value::Null, 0),
            PropKind::StringList
        );
    }

    #[test]
    fn mutate_object_field_sets_prunes_and_collapses() {
        let trend = serde_json::json!({"value": "+340%", "direction": "up"});
        // Set a sub-key → whole object with the new value.
        let out = mutate_object_field(&trend, "direction", serde_json::json!("down"));
        assert_eq!(
            out,
            serde_json::json!({"value": "+340%", "direction": "down"})
        );
        // Null prunes the key.
        let out = mutate_object_field(&out, "direction", Value::Null);
        assert_eq!(out, serde_json::json!({"value": "+340%"}));
        // Last key removed → the whole field collapses to Null.
        let out = mutate_object_field(&out, "value", Value::Null);
        assert!(out.is_null());
        // Mutating a null/absent object starts a fresh one.
        let out = mutate_object_field(&Value::Null, "value", serde_json::json!("+1%"));
        assert_eq!(out, serde_json::json!({"value": "+1%"}));
    }

    #[test]
    fn mutated_trend_round_trips_through_the_typed_parse() {
        let raw = serde_json::json!({
            "type": "stat", "value": "8.4M",
            "trend": {"value": "+340%", "direction": "up"}
        });
        let mutated_trend =
            mutate_object_field(&raw["trend"], "direction", serde_json::json!("down"));
        let mut updated = raw.clone();
        updated["trend"] = mutated_trend;
        assert!(
            serde_json::from_value::<Component>(updated).is_ok(),
            "mutated stat parses"
        );
    }

    // ── Round 3: Integer/Float split + display ──────────────────────────

    #[test]
    fn integer_and_float_split_follows_the_schema() {
        // f64 → "type": "number" → Float.
        let gauge = component_props("gauge").expect("gauge in schema");
        assert_eq!(kind_of(gauge, "value"), Some(&PropKind::Float));
        // Option<u32> → "type": "integer" (format uint32) → Integer.
        let badge = component_props("badge").expect("badge in schema");
        assert_eq!(kind_of(badge, "count"), Some(&PropKind::Integer));
        // u8 → "type": "integer" (format uint8) → Integer.
        let counter = component_props("counter").expect("counter in schema");
        assert_eq!(kind_of(counter, "decimals"), Some(&PropKind::Integer));
    }

    #[test]
    fn display_number_trims_trailing_zeros() {
        assert_eq!(display_number("100"), "100");
        assert_eq!(display_number("405.0"), "405");
        assert_eq!(display_number("1.40"), "1.4");
        assert_eq!(display_number("0.5"), "0.5");
        assert_eq!(display_number("72.0"), "72");
        assert_eq!(display_number("not-a-number"), "not-a-number");
        assert_eq!(display_number(""), "");
    }

    #[test]
    fn integer_write_round_trips_through_the_typed_parse() {
        // Writing 12 into badge.count must produce a REAL JSON integer: a
        // 12.0 float makes from_value::<Component> fail ("invalid type:
        // floating point") and the element would be dropped at render.
        let raw = serde_json::json!({"type": "badge", "text": "New", "count": 12});
        assert!(
            serde_json::from_value::<Component>(raw).is_ok(),
            "integer count parses"
        );
        let bad = serde_json::json!({"type": "badge", "text": "New", "count": 12.0});
        assert!(
            serde_json::from_value::<Component>(bad).is_err(),
            "float in u32 field must fail — this is why Integer never writes floats"
        );
    }

    // ── Round 2: color editors / effective view / multiline ─────────────

    #[test]
    fn gradient_text_colors_is_a_color_list() {
        let props = component_props("gradient_text").expect("gradient_text in schema");
        assert_eq!(kind_of(props, "colors"), Some(&PropKind::ColorList));
    }

    #[test]
    fn plain_string_arrays_are_not_color_lists() {
        let arr = serde_json::json!({"type": "array", "items": {"type": "string"}});
        assert!(!is_color_string_array("headers", &arr, &Value::Null, 0));
        assert!(is_color_string_array("colors", &arr, &Value::Null, 0));
    }

    #[test]
    fn shape_fill_is_fill_kind() {
        let props = component_props("shape").expect("shape in schema");
        assert_eq!(kind_of(props, "fill"), Some(&PropKind::Fill));
    }

    #[test]
    fn parse_fill_detects_modes() {
        let (m, c, _) = parse_fill(&serde_json::json!("#ff0000"));
        assert_eq!(m, FillMode::Single);
        assert_eq!(c, vec!["#ff0000".to_string()]);

        let (m, c, a) =
            parse_fill(&serde_json::json!({"type":"linear","colors":["#a","#b"],"angle":45}));
        assert_eq!(m, FillMode::Linear);
        assert_eq!(c, vec!["#a".to_string(), "#b".to_string()]);
        assert_eq!(a, 45.0);

        let (m, _, _) = parse_fill(&serde_json::json!({"type":"radial","colors":["#a"]}));
        assert_eq!(m, FillMode::Radial);
    }

    #[test]
    fn fill_to_value_serializes_by_mode() {
        assert_eq!(
            fill_to_value(FillMode::Single, &["#fff".to_string()], 0.0),
            serde_json::json!("#fff")
        );
        assert_eq!(
            fill_to_value(
                FillMode::Linear,
                &["#a".to_string(), "#b".to_string()],
                90.0
            ),
            serde_json::json!({"type":"linear","colors":["#a","#b"],"angle":90.0})
        );
        assert_eq!(
            fill_to_value(FillMode::Radial, &["#a".to_string()], 45.0),
            serde_json::json!({"type":"radial","colors":["#a"]})
        );
    }

    #[test]
    fn effective_element_fills_serde_defaults() {
        // User bug: gauge without show_value renders the value (default true),
        // but the inspector showed the switch off.
        let raw = serde_json::json!({"type": "gauge", "value": 50.0});
        let eff = effective_element(&raw);
        assert_eq!(eff["show_value"], serde_json::json!(true));

        // Counter without easing → the exact default variant, serialized.
        let raw = serde_json::json!({"type": "counter", "from": 0, "to": 10});
        let eff = effective_element(&raw);
        assert_eq!(eff["easing"], serde_json::json!("linear"));
        assert_eq!(eff["decimals"], serde_json::json!(0));
    }

    #[test]
    fn effective_element_falls_back_to_raw_when_invalid() {
        let raw = serde_json::json!({"type": "counter", "note": "missing from/to"});
        assert_eq!(effective_element(&raw), raw);
    }

    #[test]
    fn multiline_heuristic() {
        assert!(is_multiline("code", "let x = 1;"));
        assert!(is_multiline("content", ""));
        assert!(is_multiline("message", ""));
        assert!(!is_multiline("title", "Hello"));
        assert!(is_multiline("title", "line\nbreak"));
    }

    #[test]
    fn slider_ranges_known_and_unknown() {
        assert_eq!(slider_range("opacity"), Some((0.0, 1.0, 0.01)));
        assert_eq!(slider_range("font-size"), Some((8.0, 300.0, 1.0)));
        assert_eq!(slider_range("background"), None);
    }
}
