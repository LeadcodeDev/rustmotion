use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde_json::Value;

use rustmotion::components::Component;
use rustmotion::core::css::CssStyle;

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

#[derive(Debug, Clone, PartialEq)]
pub enum PropKind {
    Integer,
    Float,
    Bool,
    String,
    Enum(Vec<String>),
    Color,
    ColorList,
    Fill,
    Unit,
    Object(Vec<PropSpec>),
    NumberList,
    StringList,
    Complex,
}

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

pub fn next_entries_on_add(
    current: &[String],
    add_value: &str,
    prefill: Option<&'static [&'static str]>,
) -> Vec<String> {
    match prefill {
        Some(palette) if current.is_empty() => palette.iter().map(|c| c.to_string()).collect(),
        _ => {
            let mut v = current.to_vec();
            v.push(add_value.to_string());
            v
        }
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillMode {
    Single,
    Linear,
    Radial,
}

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

pub fn effective_element(raw: &Value) -> Value {
    serde_json::from_value::<Component>(raw.clone())
        .ok()
        .and_then(|c| serde_json::to_value(&c).ok())
        .unwrap_or_else(|| raw.clone())
}

pub fn is_multiline(name: &str, value: &str) -> bool {
    matches!(name, "code" | "content" | "message") || value.contains('\n')
}

pub fn engine_placeholder(name: &str) -> Option<&'static str> {
    match name {
        "font-family" => Some("Inter"),
        "background" => Some("none"),
        "width" | "height" | "min-width" | "min-height" | "max-width" | "max-height" => {
            Some("auto")
        }
        "line-height" | "letter-spacing" => Some("–"),
        _ => None,
    }
}

pub fn css_effective_default(prop: &str) -> Option<Value> {
    let v = match prop {
        "opacity" => Value::from(1.0),
        "position" => Value::from("static"),
        "top" | "right" | "bottom" | "left" => Value::from(0),
        "z-index" => Value::from(0),
        "padding" | "margin" | "gap" => Value::from(0),
        "border-radius" => Value::from(0),
        "overflow" | "overflow-x" | "overflow-y" => Value::from("visible"),
        "visibility" => Value::from("visible"),
        "flex-direction" => Value::from("column"),
        "align-items" => Value::from("start"),
        "justify-content" => Value::from("start"),
        "flex-grow" => Value::from(0),
        "flex-shrink" => Value::from(1),
        "font-weight" => Value::from("400"),
        _ => return None,
    };
    Some(v)
}

pub fn text_default_font_size(tag: &str) -> Option<f64> {
    match tag {
        "text" | "caption" | "gradient_text" => Some(48.0),
        "terminal" | "codeblock" => Some(14.0),
        _ => None,
    }
}

pub fn css_display_default(tag: &str, prop: &str) -> Option<Value> {
    match prop {
        "font-size" => text_default_font_size(tag).map(Value::from),
        "color" if css_family(tag) == CssFamily::TextLike => Some(Value::from("#FFFFFF")),
        _ => css_effective_default(prop),
    }
}

pub fn css_row_value(raw: String, tag: &str, prop: &str) -> (String, bool) {
    if !raw.is_empty() {
        return (raw, false);
    }
    match css_display_default(tag, prop) {
        Some(Value::String(s)) => (s, true),
        Some(other) => (display_number(&other.to_string()), true),
        None => (String::new(), false),
    }
}

pub fn component_props(tag: &str) -> Option<&'static Vec<PropSpec>> {
    component_registry().get(tag)
}

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

fn kind_of_schema(name: &str, schema: &Value, defs: &Value, depth: u8) -> PropKind {
    kind_of_schema_inner(name, schema, defs, depth, 0)
}

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
    if let Some(r) = schema.get("$ref").and_then(|r| r.as_str()) {
        let key = r.rsplit('/').next().unwrap_or_default();
        return match defs.get(key) {
            Some(target) => kind_of_schema_inner(name, target, defs, depth + 1, obj_level),
            None => PropKind::Complex,
        };
    }
    if let Some(all) = schema.get("allOf").and_then(|a| a.as_array()) {
        if all.len() == 1 {
            return kind_of_schema_inner(name, &all[0], defs, depth + 1, obj_level);
        }
    }
    if let Some(variants) = string_enum(schema) {
        return PropKind::Enum(variants);
    }
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

#[derive(Default)]
struct UnionInfo {
    variants: Vec<String>,
    has_number: bool,
    has_string: bool,
    has_gradient_object: bool,
}

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

fn primary_type(schema: &Value) -> Option<&str> {
    match schema.get("type") {
        Some(Value::String(s)) => Some(s.as_str()),
        Some(Value::Array(a)) => a.iter().filter_map(|v| v.as_str()).find(|s| *s != "null"),
        _ => None,
    }
}

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

pub fn section_for(prop: &str) -> CssSection {
    use CssSection::*;
    match prop {
        "font-family" | "font-size" | "font-weight" | "font-style" | "line-height"
        | "letter-spacing" | "text-align" | "color" | "white-space" | "text-shadow" => Typography,
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
        "width" | "height" | "min-width" | "min-height" | "max-width" | "max-height"
        | "aspect-ratio" | "box-sizing" => Sizing,
        "padding" | "margin" => Spacing,
        "position" | "top" | "right" | "bottom" | "left" | "z-index" => Position,
        "background" | "border" | "border-radius" | "box-shadow" | "opacity" | "mix-blend-mode"
        | "visibility" | "clip-path" => Visual,
        "filter" | "backdrop-filter" | "transform" | "transform-origin" | "perspective"
        | "perspective-origin" | "transition" | "audio-reactive" => Effects,
        "overflow" | "overflow-x" | "overflow-y" | "text-overflow" | "overflow-wrap" => Overflow,
        _ => Advanced,
    }
}

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

pub fn css_section_props(section: CssSection) -> Vec<&'static PropSpec> {
    css_props()
        .iter()
        .filter(|p| section_for(&p.name) == section)
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssFamily {
    TextLike,
    Container,
    Plain,
}

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

    #[test]
    fn counter_exposes_its_root_fields_with_kinds() {
        let props = component_props("counter").expect("counter in schema");
        assert_eq!(kind_of(props, "from"), Some(&PropKind::Float));
        assert_eq!(kind_of(props, "to"), Some(&PropKind::Float));
        assert_eq!(kind_of(props, "decimals"), Some(&PropKind::Integer));
        assert_eq!(kind_of(props, "separator"), Some(&PropKind::String));
        assert_eq!(kind_of(props, "prefix"), Some(&PropKind::String));
        assert_eq!(kind_of(props, "suffix"), Some(&PropKind::String));
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
        assert!(component_props("chart").is_some());
        assert!(component_props("rich_text").is_some());
    }

    #[test]
    fn unknown_tag_has_no_props() {
        assert!(component_props("definitely_not_a_component").is_none());
    }

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

    #[test]
    fn chart_axes_and_categories_are_string_lists() {
        let props = component_props("chart").expect("chart in schema");
        assert_eq!(kind_of(props, "axes"), Some(&PropKind::StringList));
        assert_eq!(kind_of(props, "categories"), Some(&PropKind::StringList));
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
        assert_eq!(palette_prefill("chart", "colors", false), None);
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

    #[test]
    fn css_effective_default_table_entries() {
        assert_eq!(css_effective_default("opacity"), Some(Value::from(1.0)));
        assert_eq!(
            css_effective_default("position"),
            Some(Value::from("static"))
        );
        assert_eq!(css_effective_default("padding"), Some(Value::from(0)));
        assert_eq!(
            css_effective_default("overflow"),
            Some(Value::from("visible"))
        );
        assert_eq!(css_effective_default("flex-shrink"), Some(Value::from(1)));
        assert_eq!(css_effective_default("background"), None);
        assert_eq!(css_effective_default("width"), None);
        assert_eq!(engine_placeholder("background"), Some("none"));
        assert_eq!(engine_placeholder("width"), Some("auto"));
    }

    #[test]
    fn font_size_default_is_per_component() {
        assert_eq!(text_default_font_size("terminal"), Some(14.0));
        assert_eq!(text_default_font_size("codeblock"), Some(14.0));
        assert_eq!(text_default_font_size("text"), Some(48.0));
        assert_eq!(text_default_font_size("made_up_tag"), None);
        assert_eq!(
            css_display_default("terminal", "font-size"),
            Some(Value::from(14.0))
        );
        assert_eq!(
            css_display_default("text", "color"),
            Some(Value::from("#FFFFFF"))
        );
        assert_eq!(css_display_default("shape", "color"), None);
    }

    #[test]
    fn css_row_value_prefers_raw_and_marks_defaults() {
        assert_eq!(
            css_row_value("12px".into(), "text", "opacity"),
            ("12px".to_string(), false)
        );
        assert_eq!(
            css_row_value(String::new(), "text", "opacity"),
            ("1".to_string(), true)
        );
        assert_eq!(
            css_row_value(String::new(), "card", "position"),
            ("static".to_string(), true)
        );
        assert_eq!(
            css_row_value(String::new(), "card", "background"),
            (String::new(), false)
        );
    }

    #[test]
    fn add_click_seeds_palette_or_appends() {
        let palette = palette_prefill("chart", "colors", true).unwrap();
        let seeded = next_entries_on_add(&[], "#ffffff", Some(palette));
        assert_eq!(seeded.len(), 8);
        assert_eq!(seeded[0], "#3B82F6");
        let appended = next_entries_on_add(&seeded, "#ffffff", Some(palette));
        assert_eq!(appended.len(), 9);
        assert_eq!(appended[8], "#ffffff");
        assert_eq!(next_entries_on_add(&[], "0", None), vec!["0".to_string()]);
        assert_eq!(next_entries_on_add(&[], "", None), vec![String::new()]);
    }

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
        let gt = component_props("gradient_text").unwrap();
        assert_eq!(kind_of(gt, "colors"), Some(&PropKind::ColorList));
        let strings = serde_json::json!({"type": "array", "items": {"type": "string"}});
        assert_eq!(
            kind_of_schema("labels", &strings, &Value::Null, 0),
            PropKind::StringList
        );
    }

    #[test]
    fn mutate_object_field_sets_prunes_and_collapses() {
        let trend = serde_json::json!({"value": "+340%", "direction": "up"});
        let out = mutate_object_field(&trend, "direction", serde_json::json!("down"));
        assert_eq!(
            out,
            serde_json::json!({"value": "+340%", "direction": "down"})
        );
        let out = mutate_object_field(&out, "direction", Value::Null);
        assert_eq!(out, serde_json::json!({"value": "+340%"}));
        let out = mutate_object_field(&out, "value", Value::Null);
        assert!(out.is_null());
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

    #[test]
    fn integer_and_float_split_follows_the_schema() {
        let gauge = component_props("gauge").expect("gauge in schema");
        assert_eq!(kind_of(gauge, "value"), Some(&PropKind::Float));
        let badge = component_props("badge").expect("badge in schema");
        assert_eq!(kind_of(badge, "count"), Some(&PropKind::Integer));
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
        let raw = serde_json::json!({"type": "gauge", "value": 50.0});
        let eff = effective_element(&raw);
        assert_eq!(eff["show_value"], serde_json::json!(true));

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
