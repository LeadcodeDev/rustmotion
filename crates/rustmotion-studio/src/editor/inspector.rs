use std::cell::RefCell;
use std::rc::Rc;

use dioxus::prelude::*;
use dioxus_icons::lucide::{
    Bold, Italic, TextAlignCenter, TextAlignEnd, TextAlignJustify, TextAlignStart, X,
};
use palette::{encoding, FromColor, Hsv, IntoColor, Srgb};

use crate::components::button::{Button, ButtonSize, ButtonVariant};
use crate::components::color_picker::ColorPicker;
use crate::components::select::{Select, SelectOption};
use crate::components::slider::Slider;
use crate::components::switch::Switch;
use crate::scenario::{
    apply_optimistic, scene_duration_for_pointer, set_field, set_field_value, set_style,
    set_style_value, Mutation, Shared,
};

use super::view::RevSignal;

use super::annotations::AnnotationBox;
use super::properties::{
    component_props, css_family, css_section_props, display_number, effective_element,
    engine_placeholder, fill_to_value, is_multiline, parse_fill, slider_range, visible_sections,
    FillMode, PropKind,
};

// ── Debounce context ─────────────────────────────────────────────────────────

/// Holds the Dioxus [`dioxus_core::Task`] handle of the last scheduled write
/// so the next keystroke can cancel it before it fires. Provided via Dioxus
/// context by [`InspectorPanel`] and consumed by the `write_*` helpers.
///
/// Uses `Rc<RefCell<>>` because Dioxus's `Task` is `!Send`; all access is
/// single-threaded (Dioxus desktop runs on one thread).
#[derive(Clone)]
pub struct WriteDebounce(Rc<RefCell<Option<dioxus_core::Task>>>);

// ── Schema ───────────────────────────────────────────────────────────────

/// How one property is edited. The variant decides the widget; the field's
/// `name` is the CSS property it writes (except `StyleToggles`, which writes
/// `font-weight`/`font-style` itself).
#[derive(Clone, Copy, PartialEq)]
enum Ctrl {
    Text,
    Number,
    /// Slider + numeric input + unit chip (e.g. font-size → `26 PX`).
    UnitSlider {
        min: f64,
        max: f64,
        step: f64,
        unit: &'static str,
    },
    /// Bare slider + value readout (opacity, flex-grow…).
    Slider {
        min: f64,
        max: f64,
        step: f64,
    },
    Select(&'static [&'static str]),
    /// font-weight with labelled options ("Semibold · 600").
    Weight,
    /// text-align as a segmented icon group.
    Align,
    /// Bold / Italic toggles (reads + writes font-weight & font-style).
    StyleToggles,
    /// Swatch + hex input; `clearable` adds a "make transparent" button.
    Color {
        clearable: bool,
    },
    /// Two-state toggle (on_value, off_value).
    Switch(&'static str, &'static str),
}

#[derive(Clone, Copy, PartialEq)]
struct Field {
    name: &'static str,
    label: &'static str,
    ctrl: Ctrl,
}

#[derive(Clone, Copy, PartialEq)]
struct Section {
    title: &'static str,
    fields: &'static [Field],
}

const WEIGHTS: &[(&str, &str)] = &[
    ("100", "Thin · 100"),
    ("200", "Extra Light · 200"),
    ("300", "Light · 300"),
    ("400", "Regular · 400"),
    ("500", "Medium · 500"),
    ("600", "Semibold · 600"),
    ("700", "Bold · 700"),
    ("800", "Extra Bold · 800"),
    ("900", "Black · 900"),
];

// ── Field lists ────────────────────────────────────────────────────────────

const F_TYPO: &[Field] = &[
    Field {
        name: "font-size",
        label: "Size",
        ctrl: Ctrl::UnitSlider {
            min: 8.0,
            max: 200.0,
            step: 1.0,
            unit: "PX",
        },
    },
    Field {
        name: "font-weight",
        label: "Weight",
        ctrl: Ctrl::Weight,
    },
    Field {
        name: "style",
        label: "Style",
        ctrl: Ctrl::StyleToggles,
    },
    Field {
        name: "line-height",
        label: "Line height",
        ctrl: Ctrl::UnitSlider {
            min: 0.8,
            max: 3.0,
            step: 0.1,
            unit: "",
        },
    },
    Field {
        name: "letter-spacing",
        label: "Tracking",
        ctrl: Ctrl::UnitSlider {
            min: -5.0,
            max: 20.0,
            step: 0.5,
            unit: "PX",
        },
    },
    Field {
        name: "text-align",
        label: "Align",
        ctrl: Ctrl::Align,
    },
];

const F_COLOR: &[Field] = &[
    Field {
        name: "color",
        label: "Text",
        ctrl: Ctrl::Color { clearable: false },
    },
    Field {
        name: "background",
        label: "Background",
        ctrl: Ctrl::Color { clearable: true },
    },
];

const F_LAYOUT: &[Field] = &[
    Field {
        name: "display",
        label: "Display",
        ctrl: Ctrl::Select(&["block", "flex", "grid", "inline-block", "none", "contents"]),
    },
    Field {
        name: "position",
        label: "Position",
        ctrl: Ctrl::Select(&["static", "relative", "absolute"]),
    },
    Field {
        name: "top",
        label: "Top",
        ctrl: Ctrl::Text,
    },
    Field {
        name: "right",
        label: "Right",
        ctrl: Ctrl::Text,
    },
    Field {
        name: "bottom",
        label: "Bottom",
        ctrl: Ctrl::Text,
    },
    Field {
        name: "left",
        label: "Left",
        ctrl: Ctrl::Text,
    },
    Field {
        name: "z-index",
        label: "Z-index",
        ctrl: Ctrl::Number,
    },
    Field {
        name: "overflow",
        label: "Overflow",
        ctrl: Ctrl::Select(&["visible", "hidden", "auto", "scroll", "clip"]),
    },
    Field {
        name: "visibility",
        label: "Visible",
        ctrl: Ctrl::Switch("hidden", "visible"),
    },
];

const F_POSITION: &[Field] = &[
    Field {
        name: "position",
        label: "Position",
        ctrl: Ctrl::Select(&["static", "relative", "absolute"]),
    },
    Field {
        name: "top",
        label: "Top",
        ctrl: Ctrl::Text,
    },
    Field {
        name: "left",
        label: "Left",
        ctrl: Ctrl::Text,
    },
    Field {
        name: "z-index",
        label: "Z-index",
        ctrl: Ctrl::Number,
    },
];

const F_FLEX: &[Field] = &[
    Field {
        name: "flex-direction",
        label: "Direction",
        ctrl: Ctrl::Select(&["row", "row-reverse", "column", "column-reverse"]),
    },
    Field {
        name: "flex-wrap",
        label: "Wrap",
        ctrl: Ctrl::Select(&["nowrap", "wrap", "wrap-reverse"]),
    },
    Field {
        name: "justify-content",
        label: "Justify",
        ctrl: Ctrl::Select(&[
            "flex-start",
            "flex-end",
            "center",
            "space-between",
            "space-around",
            "space-evenly",
            "start",
            "end",
        ]),
    },
    Field {
        name: "align-items",
        label: "Align",
        ctrl: Ctrl::Select(&[
            "stretch",
            "flex-start",
            "flex-end",
            "center",
            "baseline",
            "start",
            "end",
        ]),
    },
    Field {
        name: "align-content",
        label: "Align content",
        ctrl: Ctrl::Select(&[
            "stretch",
            "flex-start",
            "flex-end",
            "center",
            "space-between",
            "space-around",
            "space-evenly",
            "start",
            "end",
        ]),
    },
    Field {
        name: "gap",
        label: "Gap",
        ctrl: Ctrl::Text,
    },
];

const F_SPACING: &[Field] = &[
    Field {
        name: "padding",
        label: "Padding",
        ctrl: Ctrl::Text,
    },
    Field {
        name: "margin",
        label: "Margin",
        ctrl: Ctrl::Text,
    },
];

const F_MARGIN: &[Field] = &[Field {
    name: "margin",
    label: "Margin",
    ctrl: Ctrl::Text,
}];

const F_SIZING: &[Field] = &[
    Field {
        name: "width",
        label: "Width",
        ctrl: Ctrl::Text,
    },
    Field {
        name: "height",
        label: "Height",
        ctrl: Ctrl::Text,
    },
    Field {
        name: "min-width",
        label: "Min W",
        ctrl: Ctrl::Text,
    },
    Field {
        name: "min-height",
        label: "Min H",
        ctrl: Ctrl::Text,
    },
    Field {
        name: "max-width",
        label: "Max W",
        ctrl: Ctrl::Text,
    },
    Field {
        name: "max-height",
        label: "Max H",
        ctrl: Ctrl::Text,
    },
];

const F_SIZING_WH: &[Field] = &[
    Field {
        name: "width",
        label: "Width",
        ctrl: Ctrl::Text,
    },
    Field {
        name: "height",
        label: "Height",
        ctrl: Ctrl::Text,
    },
];

const F_TEXT_SIZING: &[Field] = &[
    Field {
        name: "width",
        label: "Width",
        ctrl: Ctrl::Text,
    },
    Field {
        name: "max-width",
        label: "Max W",
        ctrl: Ctrl::Text,
    },
];

const F_APPEARANCE: &[Field] = &[
    Field {
        name: "background",
        label: "Background",
        ctrl: Ctrl::Color { clearable: true },
    },
    Field {
        name: "border-radius",
        label: "Radius",
        ctrl: Ctrl::Text,
    },
    Field {
        name: "opacity",
        label: "Opacity",
        ctrl: Ctrl::Slider {
            min: 0.0,
            max: 1.0,
            step: 0.01,
        },
    },
];

const F_OPACITY: &[Field] = &[Field {
    name: "opacity",
    label: "Opacity",
    ctrl: Ctrl::Slider {
        min: 0.0,
        max: 1.0,
        step: 0.01,
    },
}];

// ── Per-family sections (strict: only valid props per element type) ──────────

#[derive(Clone, Copy, PartialEq)]
enum Family {
    Text,
    Container,
    Shape,
    Other,
}

/// Map a component `kind` to a family that decides which sections render.
/// `div` arrives here as `container`.
fn family(kind: &str) -> Family {
    match kind {
        "text" | "caption" | "gradient_text" => Family::Text,
        "container" | "card" | "flex" | "grid" | "positioned" => Family::Container,
        "shape" | "image" | "icon" | "svg" | "video" | "gif" | "lottie" | "qrcode" | "mockup"
        | "divider" | "line" | "arrow" | "connector" => Family::Shape,
        _ => Family::Other,
    }
}

/// Text-family elements edit their content first: the CONTENT section renders
/// ABOVE Properties. Other families keep Properties on top (and have no
/// content editor).
fn content_before_properties(kind: &str) -> bool {
    family(kind) == Family::Text
}

const TEXT_SECTIONS: &[Section] = &[
    Section {
        title: "Typography",
        fields: F_TYPO,
    },
    Section {
        title: "Color",
        fields: F_COLOR,
    },
    Section {
        title: "Sizing",
        fields: F_TEXT_SIZING,
    },
    Section {
        title: "Spacing",
        fields: F_SPACING,
    },
    Section {
        title: "Appearance",
        fields: F_OPACITY,
    },
];

const CONTAINER_SECTIONS: &[Section] = &[
    Section {
        title: "Layout",
        fields: F_LAYOUT,
    },
    Section {
        title: "Flex",
        fields: F_FLEX,
    },
    Section {
        title: "Spacing",
        fields: F_SPACING,
    },
    Section {
        title: "Sizing",
        fields: F_SIZING,
    },
    Section {
        title: "Appearance",
        fields: F_APPEARANCE,
    },
];

const SHAPE_SECTIONS: &[Section] = &[
    Section {
        title: "Layout",
        fields: F_POSITION,
    },
    Section {
        title: "Sizing",
        fields: F_SIZING,
    },
    Section {
        title: "Spacing",
        fields: F_MARGIN,
    },
    Section {
        title: "Appearance",
        fields: F_APPEARANCE,
    },
];

const FALLBACK_SECTIONS: &[Section] = &[
    Section {
        title: "Layout",
        fields: F_POSITION,
    },
    Section {
        title: "Sizing",
        fields: F_SIZING_WH,
    },
    Section {
        title: "Spacing",
        fields: F_SPACING,
    },
    Section {
        title: "Appearance",
        fields: F_APPEARANCE,
    },
];

fn sections(f: Family) -> &'static [Section] {
    match f {
        Family::Text => TEXT_SECTIONS,
        Family::Container => CONTAINER_SECTIONS,
        Family::Shape => SHAPE_SECTIONS,
        Family::Other => FALLBACK_SECTIONS,
    }
}

// ── Styling constants ────────────────────────────────────────────────────────

const INPUT_STYLE: &str = "width:100%; box-sizing:border-box; padding:5px 7px; background:var(--rm-bg); color:var(--rm-text); border:1px solid var(--rm-border-2); border-radius:6px; font:inherit;";
const NUM_STYLE: &str = "width:46px; flex:none; box-sizing:border-box; text-align:right; padding:5px 5px; background:var(--rm-bg); color:var(--rm-text); border:1px solid var(--rm-border-2); border-radius:6px; font:inherit;";
const HEX_STYLE: &str = "flex:1; min-width:0; box-sizing:border-box; padding:5px 7px; background:var(--rm-bg); color:var(--rm-text); border:1px solid var(--rm-border-2); border-radius:6px; font:inherit;";
const SECTION_HEADER: &str = "color:var(--rm-text-muted); font-size:10px; font-weight:600; letter-spacing:0.06em; text-transform:uppercase;";
const TEXTAREA_STYLE: &str = "width:100%; box-sizing:border-box; min-height:58px; resize:vertical; padding:7px 9px; background:var(--rm-bg); color:var(--rm-text); border:1px solid var(--rm-border-2); border-radius:7px; font:inherit;";

/// Stroke color for a segmented-button icon (`.rm-seg-btn`), keyed on active.
/// The button chrome (bg/hover/active) is themed via `INSPECTOR_CSS` classes —
/// inline styles can't override the WKWebView native button `appearance`.
fn seg_icon(active: bool) -> &'static str {
    if active {
        "var(--rm-on-accent)"
    } else {
        "var(--rm-text-muted)"
    }
}

// ── Value helpers ────────────────────────────────────────────────────────────

/// Stringify a property's current value from the element's `style` object.
fn prop_str(style: &serde_json::Value, name: &str) -> String {
    match style.get(name) {
        Some(serde_json::Value::String(s)) => s.clone(),
        // Null = unset (effective documents serialize unset Options as null).
        Some(serde_json::Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

/// Leading numeric part of a CSS value: `"26px"` → `26`, `"-0.5px"` → `-0.5`.
fn parse_num(s: &str) -> Option<f64> {
    let t: String = s
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    t.parse::<f64>().ok()
}

fn fmt_num(v: f64, step: f64) -> String {
    if step >= 1.0 {
        format!("{}", v.round() as i64)
    } else {
        // Round to 2 decimals, then shortest display: 1.40 → "1.4", 405.0 → "405".
        ((v * 100.0).round() / 100.0).to_string()
    }
}

fn fmt_unit(v: f64, step: f64, unit: &str) -> String {
    let n = fmt_num(v, step);
    if unit.is_empty() {
        n
    } else {
        format!("{n}{}", unit.to_ascii_lowercase())
    }
}

fn num_display(value: &str, step: f64) -> String {
    parse_num(value)
        .map(|v| fmt_num(v, step))
        .unwrap_or_else(|| value.to_string())
}

// ── Panel ────────────────────────────────────────────────────────────────────

/// The right-hand inspector for the selected element: header, the schema-driven
/// Properties section (component root fields), the content editor (text family
/// only), the curated per-type style sections, the generic (schema-complete)
/// CSS sections, and the "comment for the agent" box. Driven by
/// frame-independent values, so it stays memoized and doesn't re-render on
/// playback.
#[component]
pub fn InspectorPanel(
    selected: Signal<Option<(u32, String, String)>>,
    pointer: String,
    kind: String,
    current: Signal<u32>,
    content: Option<String>,
    style: serde_json::Value,
    element: serde_json::Value,
) -> Element {
    // Provide the debounce handle so all child write helpers share one slot.
    use_context_provider(|| WriteDebounce(Rc::new(RefCell::new(None))));
    // One expanded color picker at a time across the whole panel.
    let mut open_picker = use_signal(|| None::<u64>);
    use_context_provider(|| crate::components::color_picker::OpenPicker(open_picker));
    let fam = family(&kind);
    rsx! {
        div {
            // --picker-anchor-right: popover right-edge offset so it hugs the
            // swatch's left edge. Standard rows: 300 − (14 section padding +
            // 76 label + 8 gap) + 8 gap = 210px. Object sub-rows override it.
            style: "width:300px; flex:none; min-height:0; background:var(--rm-surface); border-left:1px solid var(--rm-border); box-sizing:border-box; display:flex; flex-direction:column; overflow:auto; --picker-anchor-right:210px;",
            // The open picker popover is `position:fixed` at its trigger's
            // static position: it doesn't follow the panel scroll, so close
            // it when the panel scrolls (standard dropdown behavior).
            onscroll: move |_| open_picker.set(None),
            div {
                style: "padding:14px; display:flex; justify-content:space-between; align-items:center;",
                div {
                    div { style: "color:var(--rm-accent); font-weight:600;", "Inspector" }
                    if !kind.is_empty() {
                        div { style: "color:var(--rm-text-muted); font-size:11px;", "{kind}" }
                    }
                }
                Button {
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::IconXs,
                    onclick: move |_| selected.set(None),
                    "✕"
                }
            }
            if content_before_properties(&kind) {
                ContentEditor { pointer: pointer.clone(), content: content.clone().unwrap_or_default() }
            }
            RootPropsSection { pointer: pointer.clone(), kind: kind.clone(), element }
            for section in sections(fam) {
                SectionView {
                    key: "{section.title}",
                    section: *section,
                    pointer: pointer.clone(),
                    style: style.clone(),
                }
            }
            GenericCssSections { pointer: pointer.clone(), kind: kind.clone(), style: style.clone() }
            AnnotationBox { pointer, kind, current }
        }
    }
}

/// Names already handled by the curated controls for a family — the generic
/// sections skip these so no property appears twice.
fn curated_names(fam: Family) -> std::collections::BTreeSet<&'static str> {
    let mut set = std::collections::BTreeSet::new();
    for section in sections(fam) {
        for field in section.fields {
            set.insert(field.name);
            if matches!(field.ctrl, Ctrl::StyleToggles) {
                set.insert("font-weight");
                set.insert("font-style");
            }
        }
    }
    set
}

/// Schema-driven "Properties" section: the component's editable root fields
/// (counter `from`/`to`/`decimals`…, badge `label`…), typed writes. `content`
/// is skipped for the text family (the Content editor owns it); start_at /
/// end_at move to the dedicated Timing section below.
///
/// Controls read the EFFECTIVE element (typed round-trip fills serde
/// defaults), so a gauge without `show_value` shows the switch ON like the
/// engine renders it; fields absent from the raw get a dimmed "default"
/// marker. Writes keep targeting the raw document.
#[component]
fn RootPropsSection(pointer: String, kind: String, element: serde_json::Value) -> Element {
    let Some(props) = component_props(&kind) else {
        return rsx! {};
    };
    let effective = effective_element(&element);
    let skip_content = family(&kind) == Family::Text;
    let has_timing = props
        .iter()
        .any(|p| p.name == "start_at" || p.name == "end_at");
    let rows: Vec<_> = props
        .iter()
        .filter(|p| !(skip_content && p.name == "content"))
        .filter(|p| p.name != "start_at" && p.name != "end_at")
        .collect();
    if rows.is_empty() && !has_timing {
        return rsx! {};
    }
    rsx! {
        if !rows.is_empty() {
            CollapsibleSection { title: "Properties".to_string(), start_open: true,
                for spec in rows {
                    GenericRow {
                        key: "{pointer}-root-{spec.name}",
                        pointer: pointer.clone(),
                        name: spec.name.clone(),
                        prop_kind: spec.kind.clone(),
                        value: prop_str(&effective, &spec.name),
                        is_style: false,
                        is_default: element.get(&spec.name).is_none()
                            && effective.get(&spec.name).map(|v| !v.is_null()).unwrap_or(false),
                    }
                }
            }
        }
        if has_timing {
            TimingSection { pointer: pointer.clone(), effective: effective.clone() }
        }
    }
}

/// Dedicated visibility-window editor (start_at / end_at), bounded by the
/// containing scene's duration.
#[component]
fn TimingSection(pointer: String, effective: serde_json::Value) -> Element {
    let shared = use_context::<Shared>();
    let max = {
        let m = shared.lock().unwrap_or_else(|e| e.into_inner());
        scene_duration_for_pointer(&m.raw, &pointer)
    };
    rsx! {
        CollapsibleSection { title: "Timing".to_string(), start_open: true,
            TimingRow {
                pointer: pointer.clone(),
                field: "start_at".to_string(),
                label: "Visible from (s)".to_string(),
                placeholder: "scene start".to_string(),
                value: prop_str(&effective, "start_at"),
                max,
            }
            TimingRow {
                pointer: pointer.clone(),
                field: "end_at".to_string(),
                label: "Visible until (s)".to_string(),
                placeholder: "end of scene".to_string(),
                value: prop_str(&effective, "end_at"),
                max,
            }
            div { style: "color:var(--rm-text-muted); font-size:10px;",
                "Visibility window — the element keeps its layout slot outside it."
            }
        }
    }
}

/// One Timing row: numeric input (step 0.1) writing the typed field; empty
/// unsets it.
#[component]
fn TimingRow(
    pointer: String,
    field: String,
    label: String,
    placeholder: String,
    value: String,
    max: Option<f64>,
) -> Element {
    let shared = use_context::<Shared>();
    // An empty max attribute is ignored by the browser (no constraint).
    let max_attr = max.map(|m| m.to_string()).unwrap_or_default();
    rsx! {
        div { style: "display:flex; align-items:center; gap:8px; min-height:26px;",
            span { style: "width:96px; flex:none; color:var(--rm-text-muted); font-size:11px;", "{label}" }
            input {
                r#type: "number",
                style: "{INPUT_STYLE}",
                step: "0.1",
                min: "0",
                max: "{max_attr}",
                value: "{value}",
                placeholder: "{placeholder}",
                title: "Visibility window — the element keeps its layout slot outside it.",
                oninput: {
                    let shared = shared.clone();
                    let p = pointer.clone();
                    let f = field.clone();
                    move |e: FormEvent| {
                        if let Ok(v) = parse_root_value(&PropKind::Float, &e.value()) {
                            write_root_field(&shared, &p, &f, v);
                        }
                    }
                },
            }
        }
    }
}

/// The schema-complete CSS sections for the component's family, minus every
/// property the curated controls already expose. Collapsed by default.
#[component]
fn GenericCssSections(pointer: String, kind: String, style: serde_json::Value) -> Element {
    let curated = curated_names(family(&kind));
    let sections_for_family = visible_sections(css_family(&kind));
    rsx! {
        for section in sections_for_family {
            {
                let props: Vec<_> = css_section_props(section)
                    .into_iter()
                    .filter(|p| !curated.contains(p.name.as_str()))
                    .collect();
                rsx! {
                    if !props.is_empty() {
                        CollapsibleSection {
                            key: "{pointer}-{section.label()}",
                            title: section.label().to_string(),
                            start_open: false,
                            for spec in props {
                                GenericRow {
                                    key: "{pointer}-css-{spec.name}",
                                    pointer: pointer.clone(),
                                    name: spec.name.clone(),
                                    prop_kind: spec.kind.clone(),
                                    value: prop_str(&style, &spec.name),
                                    is_style: true,
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A section with a clickable header that folds its rows.
#[component]
fn CollapsibleSection(title: String, start_open: bool, children: Element) -> Element {
    let mut open = use_signal(|| start_open);
    let chevron = if open() { "▾" } else { "▸" };
    rsx! {
        div { style: "display:flex; flex-direction:column; gap:8px; padding:12px 14px; border-top:1px solid var(--rm-border);",
            div {
                style: "{SECTION_HEADER} cursor:pointer; user-select:none;",
                onclick: move |_| open.set(!open()),
                "{chevron} {title}"
            }
            if open() {
                {children}
            }
        }
    }
}

/// Parse a generic control's text into the TYPED JSON value written to a root
/// field. Empty → `Null` (remove). Numbers stay numbers (integral floats
/// become JSON integers so `u8`-style fields deserialize), bools bools,
/// Complex must be valid JSON.
fn parse_root_value(kind: &PropKind, text: &str) -> Result<serde_json::Value, ()> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(serde_json::Value::Null);
    }
    Ok(match kind {
        PropKind::Integer => {
            // NEVER write a float into an integer field: `12.0` fails the
            // typed parse ("invalid type: floating point") and would drop the
            // element at render.
            match t.parse::<i64>() {
                Ok(i) => serde_json::Value::from(i),
                Err(_) => {
                    let f: f64 = t.parse().map_err(|_| ())?;
                    if f.fract() == 0.0 && f.abs() < 9_007_199_254_740_992.0 {
                        serde_json::Value::from(f as i64)
                    } else {
                        return Err(());
                    }
                }
            }
        }
        PropKind::Float => {
            let f: f64 = t.parse().map_err(|_| ())?;
            // Integral floats are written as JSON integers (clean source; f64
            // fields deserialize integers fine).
            if f.fract() == 0.0 && f.abs() < 9_007_199_254_740_992.0 {
                serde_json::Value::from(f as i64)
            } else {
                serde_json::Value::from(f)
            }
        }
        PropKind::Bool => serde_json::Value::Bool(t == "true"),
        PropKind::Complex => serde_json::from_str(t).map_err(|_| ())?,
        _ => serde_json::Value::String(text.to_string()),
    })
}

/// One generic (schema-driven) property row. `is_style` picks the write path:
/// style properties write CSS strings (an emptied control REMOVES the
/// declaration); root fields write typed JSON (empty removes the field /
/// HTML attribute).
#[component]
fn GenericRow(
    pointer: String,
    name: String,
    prop_kind: PropKind,
    value: String,
    is_style: bool,
    #[props(default = false)] is_default: bool,
    /// When set (object sub-rows), the parsed TYPED value is handed to this
    /// callback instead of being written as a root field — the parent object
    /// control folds it into the whole-object write.
    #[props(default)]
    custom_commit: Option<Callback<serde_json::Value>>,
) -> Element {
    let shared = use_context::<Shared>();

    // Single write route shared by every control variant.
    let commit: Rc<dyn Fn(&str)> = {
        let shared = shared.clone();
        let p = pointer.clone();
        let n = name.clone();
        let k = prop_kind.clone();
        Rc::new(move |text: &str| {
            if let Some(cb) = &custom_commit {
                if let Ok(v) = parse_root_value(&k, text) {
                    cb.call(v);
                }
            } else if is_style {
                if text.trim().is_empty() {
                    write_style_removal(&shared, &p, &n);
                } else {
                    write_prop(&shared, &p, &n, text);
                }
            } else if let Ok(v) = parse_root_value(&k, text) {
                write_root_field(&shared, &p, &n, v);
            }
        })
    };

    let control = match prop_kind.clone() {
        PropKind::Bool => {
            let commit = commit.clone();
            rsx! {
                Switch {
                    default_checked: value == "true",
                    on_checked_change: move |checked: bool| {
                        commit(if checked { "true" } else { "false" })
                    },
                }
            }
        }
        PropKind::Enum(variants) => {
            let mut items = variants;
            if !value.is_empty() && !items.iter().any(|o| o == &value) {
                items.insert(0, value.clone());
            }
            let options = items.iter().enumerate().map(|(i, opt)| {
                rsx! {
                    SelectOption::<String> { key: "{opt}", index: i, value: opt.clone(), text_value: "{opt}", "{opt}" }
                }
            });
            let commit = commit.clone();
            rsx! {
                Select::<String> {
                    width: "100%",
                    default_value: if value.is_empty() { None } else { Some(value.clone()) },
                    on_value_change: move |v: Option<String>| {
                        if let Some(v) = v {
                            commit(&v);
                        }
                    },
                    {options}
                }
            }
        }
        PropKind::Color => {
            let mut color = use_signal(|| parse_hsv(&value));
            let pick_commit = commit.clone();
            let hex_commit = commit.clone();
            rsx! {
                div { style: "display:flex; flex-wrap:wrap; align-items:center; gap:6px; width:100%;",
                    ColorPicker {
                        color: color(),
                        on_color_change: move |c: Hsv<encoding::Srgb, f64>| {
                            color.set(c);
                            pick_commit(&hsv_to_hex(c));
                        },
                    }
                    input {
                        r#type: "text",
                        style: "{HEX_STYLE}",
                        value: "{value}",
                        oninput: move |e: FormEvent| {
                            let v = e.value();
                            if v.trim_start_matches('#').len() >= 6 {
                                color.set(parse_hsv(&v));
                            }
                            hex_commit(&v);
                        },
                    }
                }
            }
        }
        PropKind::Float => {
            if let Some((min, max, step)) = slider_range(&name) {
                let mut num = use_signal(|| parse_num(&value).unwrap_or(min));
                let mut txt = use_signal(|| num_display(&value, step));
                let slide_commit = commit.clone();
                let type_commit = commit.clone();
                rsx! {
                    div { style: "display:flex; align-items:center; gap:8px; width:100%;",
                        div { style: "flex:1; min-width:0;",
                            Slider {
                                value: Some(num()),
                                min,
                                max,
                                step,
                                on_value_change: move |v: f64| {
                                    num.set(v);
                                    txt.set(fmt_num(v, step));
                                    slide_commit(&fmt_num(v, step));
                                },
                            }
                        }
                        input {
                            r#type: "text",
                            style: "{NUM_STYLE}",
                            value: "{txt}",
                            oninput: move |e: FormEvent| {
                                let raw = e.value();
                                txt.set(raw.clone());
                                if let Some(v) = parse_num(&raw) {
                                    num.set(v);
                                }
                                type_commit(&raw);
                            },
                        }
                    }
                }
            } else {
                let commit = commit.clone();
                rsx! {
                    input {
                        r#type: "number",
                        step: "0.1",
                        style: "{INPUT_STYLE}",
                        value: "{display_number(&value)}",
                        oninput: move |e: FormEvent| commit(&e.value()),
                    }
                }
            }
        }
        PropKind::Integer => {
            let commit = commit.clone();
            rsx! {
                input {
                    r#type: "number",
                    step: "1",
                    style: "{INPUT_STYLE}",
                    value: "{display_number(&value)}",
                    oninput: move |e: FormEvent| commit(&e.value()),
                }
            }
        }
        PropKind::String if is_multiline(&name, &value) => {
            // Long-form text (codeblock `code`, notification `message`, …).
            let commit = commit.clone();
            let font = if name == "code" {
                "font-family:monospace; font-size:11px;"
            } else {
                "font:inherit;"
            };
            rsx! {
                textarea {
                    style: "width:100%; box-sizing:border-box; min-height:88px; resize:vertical; padding:7px 9px; background:var(--rm-bg); color:var(--rm-text); border:1px solid var(--rm-border-2); border-radius:7px; {font}",
                    value: "{value}",
                    oninput: move |e: FormEvent| commit(&e.value()),
                }
            }
        }
        PropKind::Unit | PropKind::String => {
            let commit = commit.clone();
            let placeholder = engine_placeholder(&name).unwrap_or_default();
            rsx! {
                input {
                    r#type: "text",
                    style: "{INPUT_STYLE}",
                    value: "{value}",
                    placeholder: "{placeholder}",
                    oninput: move |e: FormEvent| commit(&e.value()),
                }
            }
        }
        PropKind::ColorList => {
            rsx! {
                ColorListControl { pointer: pointer.clone(), name: name.clone(), value: value.clone() }
            }
        }
        PropKind::Fill => {
            rsx! {
                FillControl { pointer: pointer.clone(), name: name.clone(), value: value.clone() }
            }
        }
        // Object/number-list editors are root-field only: style writes are
        // string-based, so object-shaped style props keep the JSON area.
        PropKind::Object(specs) if !is_style => {
            rsx! {
                ObjectControl {
                    pointer: pointer.clone(),
                    name: name.clone(),
                    specs,
                    value: value.clone(),
                    custom_commit,
                }
            }
        }
        PropKind::NumberList if !is_style => {
            rsx! {
                NumberListControl { pointer: pointer.clone(), name: name.clone(), value: value.clone() }
            }
        }
        PropKind::Complex | PropKind::Object(_) | PropKind::NumberList => {
            let commit = commit.clone();
            rsx! {
                JsonArea {
                    initial: value.clone(),
                    on_commit: move |text: String| commit(&text),
                }
            }
        }
    };

    rsx! {
        div { style: "display:flex; align-items:center; gap:8px; min-height:26px;",
            span {
                style: "width:76px; flex:none; color:var(--rm-text-muted); font-size:11px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;",
                title: if is_default { "{name} — engine default (not set in the source)" } else { "{name}" },
                "{name}"
                if is_default {
                    // Discreet default marker; the row title explains it.
                    span { style: "margin-left:4px; color:var(--rm-text-muted); opacity:0.6;", "•" }
                }
            }
            div { style: "flex:1; min-width:0; display:flex; justify-content:flex-end;", {control} }
        }
    }
}

/// Folded JSON editor for Complex values: parse-checked on blur; invalid input
/// shows an error border and never writes.
#[component]
fn JsonArea(initial: String, on_commit: EventHandler<String>) -> Element {
    let mut txt = use_signal(|| initial.clone());
    let mut bad = use_signal(|| false);
    let border = if bad() {
        "var(--rm-error)"
    } else {
        "var(--rm-border-2)"
    };
    rsx! {
        textarea {
            style: "width:100%; box-sizing:border-box; min-height:44px; resize:vertical; padding:6px 8px; background:var(--rm-bg); color:var(--rm-text); border:1px solid {border}; border-radius:6px; font-size:11px; font-family:monospace;",
            value: "{txt}",
            oninput: move |e: FormEvent| {
                txt.set(e.value());
                bad.set(false);
            },
            onblur: move |_| {
                let t = txt();
                let trimmed = t.trim().to_string();
                if trimmed.is_empty() {
                    bad.set(false);
                    on_commit.call(String::new());
                } else if serde_json::from_str::<serde_json::Value>(&trimmed).is_ok() {
                    bad.set(false);
                    on_commit.call(trimmed);
                } else {
                    bad.set(true);
                }
            },
        }
    }
}

/// Editable text content for text-bearing elements (writes the `content` field).
#[component]
fn ContentEditor(pointer: String, content: String) -> Element {
    let shared = use_context::<Shared>();
    rsx! {
        div { style: "display:flex; flex-direction:column; gap:7px; padding:12px 14px; border-top:1px solid var(--rm-border);",
            div { style: "{SECTION_HEADER}", "Content" }
            textarea {
                style: "{TEXTAREA_STYLE}",
                value: "{content}",
                oninput: {
                    let shared = shared.clone();
                    let p = pointer.clone();
                    move |e: FormEvent| write_content(&shared, &p, &e.value())
                },
            }
        }
    }
}

/// One labelled, uppercase-headed section with its rows.
#[component]
fn SectionView(section: Section, pointer: String, style: serde_json::Value) -> Element {
    rsx! {
        div { style: "display:flex; flex-direction:column; gap:8px; padding:12px 14px; border-top:1px solid var(--rm-border);",
            div { style: "{SECTION_HEADER}", "{section.title}" }
            for field in section.fields {
                FieldRow {
                    key: "{pointer}-{field.name}",
                    field: *field,
                    pointer: pointer.clone(),
                    style: style.clone(),
                }
            }
        }
    }
}

/// One property row: label on the left, the typed control on the right.
#[component]
fn FieldRow(field: Field, pointer: String, style: serde_json::Value) -> Element {
    let shared = use_context::<Shared>();
    let value = prop_str(&style, field.name);

    let control = match field.ctrl {
        Ctrl::Text | Ctrl::Number => {
            let kind = if matches!(field.ctrl, Ctrl::Number) {
                "number"
            } else {
                "text"
            };
            rsx! {
                input {
                    r#type: "{kind}",
                    style: "{INPUT_STYLE}",
                    value: "{value}",
                    oninput: {
                        let shared = shared.clone();
                        let p = pointer.clone();
                        move |e: FormEvent| write_prop(&shared, &p, field.name, &e.value())
                    },
                }
            }
        }
        Ctrl::UnitSlider {
            min,
            max,
            step,
            unit,
        } => {
            // Shared live state so the slider and the number field track each other
            // (the panel itself doesn't re-render on the file reload). `txt` holds the
            // field's raw text so typing decimals isn't reformatted mid-keystroke.
            let mut num = use_signal(|| parse_num(&value).unwrap_or(min));
            let mut txt = use_signal(|| num_display(&value, step));
            rsx! {
                div { style: "display:flex; align-items:center; gap:8px; width:100%;",
                    div { style: "flex:1; min-width:0;",
                        Slider {
                            value: Some(num()),
                            min,
                            max,
                            step,
                            on_value_change: {
                                let shared = shared.clone();
                                let p = pointer.clone();
                                move |v: f64| {
                                    num.set(v);
                                    txt.set(fmt_num(v, step));
                                    write_prop(&shared, &p, field.name, &fmt_unit(v, step, unit));
                                }
                            },
                        }
                    }
                    input {
                        r#type: "text",
                        style: "{NUM_STYLE}",
                        value: "{txt}",
                        oninput: {
                            let shared = shared.clone();
                            let p = pointer.clone();
                            move |e: FormEvent| {
                                let raw = e.value();
                                txt.set(raw.clone());
                                match parse_num(&raw) {
                                    Some(v) => {
                                        num.set(v);
                                        write_prop(&shared, &p, field.name, &fmt_unit(v, step, unit));
                                    }
                                    None => write_prop(&shared, &p, field.name, &raw),
                                }
                            }
                        },
                    }
                    if !unit.is_empty() {
                        span { style: "flex:none; color:var(--rm-text-muted); font-size:10px;", "{unit}" }
                    }
                }
            }
        }
        Ctrl::Slider { min, max, step } => {
            let mut num = use_signal(|| parse_num(&value).unwrap_or(min));
            rsx! {
                div { style: "display:flex; align-items:center; gap:8px; width:100%;",
                    div { style: "flex:1; min-width:0;",
                        Slider {
                            value: Some(num()),
                            min,
                            max,
                            step,
                            on_value_change: {
                                let shared = shared.clone();
                                let p = pointer.clone();
                                move |v: f64| {
                                    num.set(v);
                                    write_prop(&shared, &p, field.name, &fmt_num(v, step));
                                }
                            },
                        }
                    }
                    span { style: "flex:none; width:32px; text-align:right; color:var(--rm-text-muted); font-size:11px;", "{fmt_num(num(), step)}" }
                }
            }
        }
        Ctrl::Select(opts) => {
            let mut items: Vec<String> = opts.iter().map(|s| s.to_string()).collect();
            if !value.is_empty() && !items.iter().any(|o| o == &value) {
                items.insert(0, value.clone());
            }
            let options = items.iter().enumerate().map(|(i, opt)| {
                rsx! {
                    SelectOption::<String> { key: "{opt}", index: i, value: opt.clone(), text_value: "{opt}", "{opt}" }
                }
            });
            rsx! {
                Select::<String> {
                    width: "100%",
                    default_value: if value.is_empty() { None } else { Some(value.clone()) },
                    on_value_change: {
                        let shared = shared.clone();
                        let p = pointer.clone();
                        move |v: Option<String>| if let Some(v) = v { write_prop(&shared, &p, field.name, &v); }
                    },
                    {options}
                }
            }
        }
        Ctrl::Weight => {
            let options = WEIGHTS.iter().enumerate().map(|(i, (val, label))| {
                rsx! {
                    SelectOption::<String> { key: "{val}", index: i, value: val.to_string(), text_value: "{label}", "{label}" }
                }
            });
            rsx! {
                Select::<String> {
                    width: "100%",
                    // Unset → show the engine's cascade default (Regular · 400)
                    // instead of an empty "Select an option" placeholder.
                    default_value: Some(if value.is_empty() { "400".to_string() } else { value.clone() }),
                    on_value_change: {
                        let shared = shared.clone();
                        let p = pointer.clone();
                        move |v: Option<String>| if let Some(v) = v { write_prop(&shared, &p, "font-weight", &v); }
                    },
                    {options}
                }
            }
        }
        Ctrl::Align => {
            let vr = |v: &str| {
                if value == v {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Ghost
                }
            };
            rsx! {
                div { class: "rm-seg",
                    Button {
                        variant: vr("left"), size: ButtonSize::IconSm, style: "flex:1;",
                        onclick: { let s = shared.clone(); let p = pointer.clone(); move |_| write_prop(&s, &p, "text-align", "left") },
                        TextAlignStart { size: "15px", stroke: seg_icon(value == "left") }
                    }
                    Button {
                        variant: vr("center"), size: ButtonSize::IconSm, style: "flex:1;",
                        onclick: { let s = shared.clone(); let p = pointer.clone(); move |_| write_prop(&s, &p, "text-align", "center") },
                        TextAlignCenter { size: "15px", stroke: seg_icon(value == "center") }
                    }
                    Button {
                        variant: vr("right"), size: ButtonSize::IconSm, style: "flex:1;",
                        onclick: { let s = shared.clone(); let p = pointer.clone(); move |_| write_prop(&s, &p, "text-align", "right") },
                        TextAlignEnd { size: "15px", stroke: seg_icon(value == "right") }
                    }
                    Button {
                        variant: vr("justify"), size: ButtonSize::IconSm, style: "flex:1;",
                        onclick: { let s = shared.clone(); let p = pointer.clone(); move |_| write_prop(&s, &p, "text-align", "justify") },
                        TextAlignJustify { size: "15px", stroke: seg_icon(value == "justify") }
                    }
                }
            }
        }
        Ctrl::StyleToggles => {
            let weight = prop_str(&style, "font-weight");
            let is_bold =
                weight == "bold" || weight.parse::<i32>().map(|w| w >= 600).unwrap_or(false);
            let fstyle = prop_str(&style, "font-style");
            let is_italic = fstyle == "italic" || fstyle == "oblique";
            rsx! {
                div { class: "rm-seg",
                    Button {
                        variant: if is_bold { ButtonVariant::Primary } else { ButtonVariant::Ghost },
                        size: ButtonSize::IconSm,
                        style: "flex:1;",
                        onclick: { let s = shared.clone(); let p = pointer.clone(); move |_| write_prop(&s, &p, "font-weight", if is_bold { "400" } else { "700" }) },
                        Bold { size: "15px", stroke: seg_icon(is_bold) }
                    }
                    Button {
                        variant: if is_italic { ButtonVariant::Primary } else { ButtonVariant::Ghost },
                        size: ButtonSize::IconSm,
                        style: "flex:1;",
                        onclick: { let s = shared.clone(); let p = pointer.clone(); move |_| write_prop(&s, &p, "font-style", if is_italic { "normal" } else { "italic" }) },
                        Italic { size: "15px", stroke: seg_icon(is_italic) }
                    }
                }
            }
        }
        Ctrl::Color { clearable } => {
            let mut color = use_signal(|| parse_hsv(&value));
            rsx! {
                div { style: "display:flex; flex-wrap:wrap; align-items:center; gap:6px; width:100%;",
                    ColorPicker {
                        color: color(),
                        on_color_change: {
                            let shared = shared.clone();
                            let p = pointer.clone();
                            move |c: Hsv<encoding::Srgb, f64>| {
                                color.set(c);
                                write_prop(&shared, &p, field.name, &hsv_to_hex(c));
                            }
                        },
                    }
                    input {
                        r#type: "text",
                        style: "{HEX_STYLE}",
                        value: "{value}",
                        oninput: {
                            let shared = shared.clone();
                            let p = pointer.clone();
                            move |e: FormEvent| {
                                let v = e.value();
                                if v.trim_start_matches('#').len() >= 6 {
                                    color.set(parse_hsv(&v));
                                }
                                write_prop(&shared, &p, field.name, &v);
                            }
                        },
                    }
                    if clearable {
                        Button {
                            variant: ButtonVariant::Outline,
                            size: ButtonSize::IconSm,
                            title: "Make transparent",
                            onclick: {
                                let shared = shared.clone();
                                let p = pointer.clone();
                                move |_| {
                                    color.set(parse_hsv("#00000000"));
                                    write_prop(&shared, &p, field.name, "#00000000");
                                }
                            },
                            X { size: "13px", stroke: "var(--rm-text-muted)" }
                        }
                    }
                }
            }
        }
        Ctrl::Switch(on, off) => {
            rsx! {
                Switch {
                    default_checked: value == on,
                    on_checked_change: {
                        let shared = shared.clone();
                        let p = pointer.clone();
                        move |checked: bool| write_prop(&shared, &p, field.name, if checked { on } else { off })
                    },
                }
            }
        }
    };

    rsx! {
        div { style: "display:flex; align-items:center; gap:8px; min-height:26px;",
            span { style: "width:76px; flex:none; color:var(--rm-text-muted); font-size:11px;", "{field.label}" }
            div { style: "flex:1; min-width:0; display:flex; justify-content:flex-end;", {control} }
        }
    }
}

// ── Object & number-list editors ─────────────────────────────────────────────

/// Structured editor for object fields with known schema properties (stat
/// `trend`, `stroke`, …): one indented sub-row per property through the same
/// control factory. Sub-edits fold into the WHOLE object (typed) via the
/// existing root-field write — optimistic + debounce for free. A pruned last
/// key collapses to `Null` (field removed). Nested objects thread their
/// writes through `custom_commit` (registry depth cap: 2).
#[component]
fn ObjectControl(
    pointer: String,
    name: String,
    specs: Vec<crate::editor::properties::PropSpec>,
    value: String,
    #[props(default)] custom_commit: Option<Callback<serde_json::Value>>,
) -> Element {
    let shared = use_context::<Shared>();
    // Local object state so successive sub-edits accumulate (the panel is
    // memoized per selection and won't re-render between keystrokes).
    let obj = use_signal(|| {
        serde_json::from_str::<serde_json::Value>(&value).unwrap_or(serde_json::Value::Null)
    });

    let commit_whole = {
        let shared = shared.clone();
        let p = pointer.clone();
        let n = name.clone();
        move |whole: serde_json::Value| {
            if let Some(cb) = &custom_commit {
                cb.call(whole);
            } else {
                write_root_field(&shared, &p, &n, whole);
            }
        }
    };

    rsx! {
        // Sub-rows are indented (10px padding + 2px border) and re-nest a
        // 76px label + 8px gap: swatch sits at 98 + 12 + 84 = 194 from the
        // panel's left → --picker-anchor-right: 300 − 194 + 8 = 114px.
        div { style: "display:flex; flex-direction:column; gap:6px; width:100%; padding-left:10px; border-left:2px solid var(--rm-border-2); --picker-anchor-right:114px;",
            for spec in specs {
                GenericRow {
                    key: "{pointer}-{name}-{spec.name}",
                    pointer: pointer.clone(),
                    name: spec.name.clone(),
                    prop_kind: spec.kind.clone(),
                    value: prop_str(&obj(), &spec.name),
                    is_style: false,
                    custom_commit: {
                        let commit_whole = commit_whole.clone();
                        let key = spec.name.clone();
                        let mut obj = obj;
                        Callback::new(move |sub: serde_json::Value| {
                            let next = crate::editor::properties::mutate_object_field(
                                &obj(),
                                &key,
                                sub,
                            );
                            obj.set(next.clone());
                            commit_whole(next);
                        })
                    },
                }
            }
        }
    }
}

/// Per-entry editor for arrays of numbers (`sparkline_data`, dash patterns…):
/// number input + remove per entry, "+ Add". Writes the whole array (typed)
/// once every entry parses; an emptied list removes the field. Kept separate
/// from `ColorRows` — the item controls share almost nothing.
#[component]
fn NumberListControl(pointer: String, name: String, value: String) -> Element {
    let shared = use_context::<Shared>();
    let entries = use_signal(|| {
        serde_json::from_str::<Vec<serde_json::Value>>(&value)
            .map(|v| {
                v.into_iter()
                    .map(|n| display_number(&n.to_string()))
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default()
    });

    let write = {
        let shared = shared.clone();
        let p = pointer.clone();
        let n = name.clone();
        move |_| {
            let parsed: Option<Vec<serde_json::Value>> = entries
                .read()
                .iter()
                .map(|e| {
                    parse_root_value(&PropKind::Float, e)
                        .ok()
                        .filter(|v| !v.is_null())
                })
                .collect();
            if let Some(nums) = parsed {
                let value = if nums.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::Value::Array(nums)
                };
                write_root_field(&shared, &p, &n, value);
            }
        }
    };

    rsx! {
        div { style: "display:flex; flex-direction:column; gap:6px; width:100%;",
            for (i, e) in entries().iter().cloned().enumerate() {
                div { key: "{i}", style: "display:flex; flex-wrap:wrap; align-items:center; gap:6px;",
                    input {
                        r#type: "number",
                        step: "0.1",
                        style: "{INPUT_STYLE}",
                        value: "{e}",
                        oninput: {
                            let mut entries = entries;
                            let write = write.clone();
                            move |ev: FormEvent| {
                                if let Some(slot) = entries.write().get_mut(i) {
                                    *slot = ev.value();
                                }
                                write(());
                            }
                        },
                    }
                    Button {
                        variant: ButtonVariant::Ghost,
                        size: ButtonSize::IconSm,
                        title: "Remove entry",
                        onclick: {
                            let mut entries = entries;
                            let write = write.clone();
                            move |_| {
                                if entries.read().len() > i {
                                    entries.write().remove(i);
                                }
                                write(());
                            }
                        },
                        X { size: 13, stroke: "var(--rm-text-muted)" }
                    }
                }
            }
            Button {
                variant: ButtonVariant::Outline,
                size: ButtonSize::Xs,
                onclick: {
                    let mut entries = entries;
                    let write = write.clone();
                    move |_| {
                        entries.write().push("0".to_string());
                        write(());
                    }
                },
                "+ Add"
            }
        }
    }
}

// ── Color-list & fill editors ────────────────────────────────────────────────

/// Editable list of color rows (swatch + hex + delete, plus "+ Add color").
/// Operates on a shared signal so add/remove render immediately (the panel
/// itself is memoized per selection). Reordering: v1 = none.
#[component]
fn ColorRows(colors: Signal<Vec<String>>, on_change: EventHandler<()>) -> Element {
    rsx! {
        div { style: "display:flex; flex-direction:column; gap:6px; width:100%;",
            for (i, c) in colors().iter().cloned().enumerate() {
                div { key: "{i}", style: "display:flex; flex-wrap:wrap; align-items:center; gap:6px;",
                    ColorPicker {
                        color: parse_hsv(&c),
                        on_color_change: {
                            let mut colors = colors;
                            move |hsv: Hsv<encoding::Srgb, f64>| {
                                if let Some(slot) = colors.write().get_mut(i) {
                                    *slot = hsv_to_hex(hsv);
                                }
                                on_change.call(());
                            }
                        },
                    }
                    input {
                        r#type: "text",
                        style: "{HEX_STYLE}",
                        value: "{c}",
                        oninput: {
                            let mut colors = colors;
                            move |e: FormEvent| {
                                if let Some(slot) = colors.write().get_mut(i) {
                                    *slot = e.value();
                                }
                                on_change.call(());
                            }
                        },
                    }
                    Button {
                        variant: ButtonVariant::Ghost,
                        size: ButtonSize::IconSm,
                        title: "Remove color",
                        onclick: {
                            let mut colors = colors;
                            move |_| {
                                if colors.read().len() > i {
                                    colors.write().remove(i);
                                }
                                on_change.call(());
                            }
                        },
                        X { size: 13, stroke: "var(--rm-text-muted)" }
                    }
                }
            }
            Button {
                variant: ButtonVariant::Outline,
                size: ButtonSize::Xs,
                onclick: {
                    let mut colors = colors;
                    move |_| {
                        colors.write().push("#ffffff".to_string());
                        on_change.call(());
                    }
                },
                "+ Add color"
            }
        }
    }
}

/// Control for [`PropKind::ColorList`] fields (gradient_text `colors`):
/// per-entry pickers writing the whole array (typed).
#[component]
fn ColorListControl(pointer: String, name: String, value: String) -> Element {
    let shared = use_context::<Shared>();
    let colors = use_signal(|| serde_json::from_str::<Vec<String>>(&value).unwrap_or_default());
    let write = {
        let shared = shared.clone();
        let p = pointer.clone();
        let n = name.clone();
        move |_| {
            let list: Vec<serde_json::Value> = colors
                .read()
                .iter()
                .cloned()
                .map(serde_json::Value::String)
                .collect();
            write_root_field(&shared, &p, &n, serde_json::Value::Array(list));
        }
    };
    rsx! {
        ColorRows { colors, on_change: write }
    }
}

/// Control for [`PropKind::Fill`] fields (shape `fill`): segmented
/// Single | Linear | Radial, a single picker or a stop list + angle, writing
/// the hex string or the gradient object.
#[component]
fn FillControl(pointer: String, name: String, value: String) -> Element {
    let shared = use_context::<Shared>();
    let parsed: serde_json::Value =
        serde_json::from_str(&value).unwrap_or(serde_json::Value::String(value.clone()));
    let (m0, c0, a0) = parse_fill(&parsed);
    let mode = use_signal(|| m0);
    let colors = use_signal(|| {
        if c0.is_empty() {
            vec!["#ffffff".to_string()]
        } else {
            c0
        }
    });
    let angle = use_signal(|| a0);

    let write = {
        let shared = shared.clone();
        let p = pointer.clone();
        let n = name.clone();
        Rc::new(move || {
            write_root_field(
                &shared,
                &p,
                &n,
                fill_to_value(mode(), &colors.read(), angle()),
            );
        })
    };

    let seg = |m: FillMode, label: &'static str| {
        let mut mode = mode;
        let write = write.clone();
        rsx! {
            Button {
                variant: if mode() == m { ButtonVariant::Primary } else { ButtonVariant::Ghost },
                size: ButtonSize::Xs,
                style: "flex:1;",
                onclick: move |_| {
                    mode.set(m);
                    write();
                },
                "{label}"
            }
        }
    };

    rsx! {
        div { style: "display:flex; flex-direction:column; gap:8px; width:100%;",
            div { class: "rm-seg",
                {seg(FillMode::Single, "Single")}
                {seg(FillMode::Linear, "Linear")}
                {seg(FillMode::Radial, "Radial")}
            }
            if mode() == FillMode::Single {
                div { style: "display:flex; flex-wrap:wrap; align-items:center; gap:6px;",
                    ColorPicker {
                        color: parse_hsv(colors.read().first().map(String::as_str).unwrap_or("#ffffff")),
                        on_color_change: {
                            let mut colors = colors;
                            let write = write.clone();
                            move |hsv: Hsv<encoding::Srgb, f64>| {
                                let hex = hsv_to_hex(hsv);
                                if colors.read().is_empty() {
                                    colors.write().push(hex);
                                } else {
                                    colors.write()[0] = hex;
                                }
                                write();
                            }
                        },
                    }
                    input {
                        r#type: "text",
                        style: "{HEX_STYLE}",
                        value: "{colors.read().first().cloned().unwrap_or_default()}",
                        oninput: {
                            let mut colors = colors;
                            let write = write.clone();
                            move |e: FormEvent| {
                                let v = e.value();
                                if colors.read().is_empty() {
                                    colors.write().push(v);
                                } else {
                                    colors.write()[0] = v;
                                }
                                write();
                            }
                        },
                    }
                }
            } else {
                ColorRows {
                    colors,
                    on_change: {
                        let write = write.clone();
                        move |_| write()
                    },
                }
                if mode() == FillMode::Linear {
                    div { style: "display:flex; align-items:center; gap:8px;",
                        span { style: "color:var(--rm-text-muted); font-size:11px;", "Angle" }
                        input {
                            r#type: "number",
                            style: "{NUM_STYLE}",
                            value: "{angle}",
                            oninput: {
                                let mut angle = angle;
                                let write = write.clone();
                                move |e: FormEvent| {
                                    if let Ok(v) = e.value().parse::<f64>() {
                                        angle.set(v);
                                        write();
                                    }
                                }
                            },
                        }
                        span { style: "color:var(--rm-text-muted); font-size:10px;", "°" }
                    }
                }
            }
        }
    }
}

// ── Color helpers ────────────────────────────────────────────────────────────

fn parse_hsv(s: &str) -> Hsv<encoding::Srgb, f64> {
    let hex = s.trim().trim_start_matches('#');
    if hex.len() >= 6 {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&hex[0..2], 16),
            u8::from_str_radix(&hex[2..4], 16),
            u8::from_str_radix(&hex[4..6], 16),
        ) {
            let srgb: Srgb<f64> = Srgb::new(r, g, b).into_format();
            return srgb.into_color();
        }
    }
    Hsv::new(0.0, 0.0, 0.0)
}

fn hsv_to_hex(c: Hsv<encoding::Srgb, f64>) -> String {
    let srgb: Srgb<f64> = Srgb::from_color(c);
    let b = srgb.into_format::<u8>();
    format!("#{:02x}{:02x}{:02x}", b.red, b.green, b.blue)
}

// ── Persistence ──────────────────────────────────────────────────────────────

/// The payload for a deferred disk write. Carries everything needed to perform
/// the write so it can be captured by the spawned task without borrowing.
enum WritePayload {
    Prop {
        path: std::path::PathBuf,
        raw: serde_json::Value,
        pointer: String,
        prop: String,
        value: String,
    },
    Content {
        path: std::path::PathBuf,
        raw: serde_json::Value,
        pointer: String,
        text: String,
    },
    /// Typed root-field write (`Value::Null` removes the field / attribute).
    RootField {
        path: std::path::PathBuf,
        raw: serde_json::Value,
        pointer: String,
        field: String,
        value: serde_json::Value,
    },
    /// Remove one style property (emptied generic control).
    StyleRemove {
        path: std::path::PathBuf,
        raw: serde_json::Value,
        pointer: String,
        prop: String,
    },
}

impl WritePayload {
    fn path(&self) -> &std::path::Path {
        match self {
            WritePayload::Prop { path, .. }
            | WritePayload::Content { path, .. }
            | WritePayload::RootField { path, .. }
            | WritePayload::StyleRemove { path, .. } => path,
        }
    }
}

/// A root-field JSON value as an HTML attribute string. `Null` → empty (which
/// [`rustmotion::loader::set_html_attribute`] treats as "remove"); complex
/// values are compact JSON (attributes are strings; the transpiler coerces).
fn root_value_to_attr(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Schedule a debounced disk write (~250 ms). Any previously scheduled write is
/// cancelled first so only the last value in a burst reaches the disk.
///
/// On success the model's `write_error` is cleared and the pre-write file state
/// is pushed onto the undo history; on failure `write_error` is set to the OS
/// error message and `generation` is bumped so the hot-reload loop picks it up
/// and shows the topbar indicator. The pending window is surfaced as the
/// "Saving…" indicator via the history slot.
fn schedule_write(debounce: &WriteDebounce, shared: Shared, payload: WritePayload) {
    // Cancel the previous pending write (if any). `Task::cancel` is safe to
    // call on an already-completed task (it's a no-op).
    {
        let mut slot = debounce.0.borrow_mut();
        if let Some(prev) = slot.take() {
            prev.cancel();
        }
    }
    crate::scenario::set_saving(&crate::scenario::history_slot(), true);

    let debounce_slot = debounce.0.clone();
    let task = spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;

        // Clear the slot so the next write doesn't try to cancel this one.
        *debounce_slot.borrow_mut() = None;

        // Capture the file state BEFORE the write: one history entry per
        // effective disk write.
        let snapshot = std::fs::read_to_string(payload.path()).ok();
        let result = perform_write(&payload);
        crate::scenario::set_saving(&crate::scenario::history_slot(), false);
        if let (Ok(true), Some(snapshot)) = (&result, snapshot) {
            crate::scenario::record_edit(
                &crate::scenario::history_slot(),
                payload.path(),
                snapshot,
            );
        }
        let mut m = shared.lock().unwrap_or_else(|e| e.into_inner());
        match result {
            Ok(_) => {
                // Clear any previous write error on success.
                m.write_error = None;
            }
            Err(e) => {
                m.write_error = Some(e);
                m.generation = m.generation.wrapping_add(1);
            }
        }
    });

    // Store the new handle for the next cancellation.
    *debounce.0.borrow_mut() = Some(task);
}

/// Write scenario-file content and record it in the self-write ledger so the
/// watcher skips the resulting event (the in-memory model is already ahead).
fn write_and_note(path: &std::path::Path, content: &str) -> Result<(), String> {
    std::fs::write(path, content).map_err(|e| format!("write: {e}"))?;
    crate::scenario::note_self_write(&crate::scenario::self_write_slot(), path, content);
    Ok(())
}

/// Execute the actual file write. Returns `Ok(true)` when the file was
/// written, `Ok(false)` when the edit was a no-op (nothing to record in the
/// undo history), or an error message.
fn perform_write(payload: &WritePayload) -> Result<bool, String> {
    match payload {
        WritePayload::Prop {
            path,
            raw,
            pointer,
            prop,
            value,
        } => {
            if rustmotion::loader::is_html_path(path) {
                let html = std::fs::read_to_string(path).map_err(|e| format!("read: {e}"))?;
                if let Some(updated) =
                    rustmotion::loader::set_html_inline_style(&html, pointer, prop, value)
                {
                    write_and_note(path, &updated)?;
                    return Ok(true);
                }
            } else if let Some(updated) = set_style(raw.clone(), pointer, prop, value) {
                let text =
                    serde_json::to_string_pretty(&updated).map_err(|e| format!("json: {e}"))?;
                write_and_note(path, &text)?;
                return Ok(true);
            }
            Ok(false)
        }
        WritePayload::Content {
            path,
            raw,
            pointer,
            text,
        } => {
            if rustmotion::loader::is_html_path(path) {
                let html = std::fs::read_to_string(path).map_err(|e| format!("read: {e}"))?;
                if let Some(updated) =
                    rustmotion::loader::set_html_text_content(&html, pointer, text)
                {
                    write_and_note(path, &updated)?;
                    return Ok(true);
                }
            } else if let Some(updated) = set_field(raw.clone(), pointer, "content", text) {
                let s = serde_json::to_string_pretty(&updated).map_err(|e| format!("json: {e}"))?;
                write_and_note(path, &s)?;
                return Ok(true);
            }
            Ok(false)
        }
        WritePayload::RootField {
            path,
            raw,
            pointer,
            field,
            value,
        } => {
            if rustmotion::loader::is_html_path(path) {
                let html = std::fs::read_to_string(path).map_err(|e| format!("read: {e}"))?;
                let attr = root_value_to_attr(value);
                if let Some(updated) =
                    rustmotion::loader::set_html_attribute(&html, pointer, field, &attr)
                {
                    write_and_note(path, &updated)?;
                    return Ok(true);
                }
            } else if let Some(updated) =
                set_field_value(raw.clone(), pointer, field, value.clone())
            {
                let s = serde_json::to_string_pretty(&updated).map_err(|e| format!("json: {e}"))?;
                write_and_note(path, &s)?;
                return Ok(true);
            }
            Ok(false)
        }
        WritePayload::StyleRemove {
            path,
            raw,
            pointer,
            prop,
        } => {
            if rustmotion::loader::is_html_path(path) {
                let html = std::fs::read_to_string(path).map_err(|e| format!("read: {e}"))?;
                if let Some(updated) =
                    rustmotion::loader::remove_html_inline_style(&html, pointer, prop)
                {
                    write_and_note(path, &updated)?;
                    return Ok(true);
                }
            } else if let Some(updated) =
                set_style_value(raw.clone(), pointer, prop, serde_json::Value::Null)
            {
                let s = serde_json::to_string_pretty(&updated).map_err(|e| format!("json: {e}"))?;
                write_and_note(path, &s)?;
                return Ok(true);
            }
            Ok(false)
        }
    }
}

/// Apply an edit to the in-memory model immediately (canvas refreshes in ~one
/// render) and nudge the hot-reload signal. Rebuild failures are transient
/// (mid-typing) and silently keep the previous model — the disk write path
/// has its own guards.
fn optimistic(shared: &Shared, mutation: Mutation) {
    if apply_optimistic(shared, &mutation).is_ok() {
        if let Some(rev) = try_consume_context::<RevSignal>() {
            let mut r = rev.0;
            r.set(r() + 1);
        }
    }
}

/// Write a single style property back to the scenario file. Schedules a debounced
/// write (~250 ms) so rapid slider drags and keystrokes coalesce into one flush.
/// Empty values are ignored so clearing a field mid-edit doesn't collapse the
/// element (generic controls route empties through [`write_style_removal`]
/// instead). Write errors are stored in the model and surfaced in the topbar.
fn write_prop(shared: &Shared, pointer: &str, prop: &str, value: &str) {
    if value.trim().is_empty() {
        return;
    }
    optimistic(
        shared,
        Mutation::Style {
            pointer: pointer.to_string(),
            prop: prop.to_string(),
            value: serde_json::Value::String(value.to_string()),
        },
    );
    let (path, raw) = {
        let m = shared.lock().unwrap_or_else(|e| e.into_inner());
        (m.path.clone(), m.raw.clone())
    };
    let Some(path) = path else {
        return;
    };
    let debounce = consume_context::<WriteDebounce>();
    schedule_write(
        &debounce,
        shared.clone(),
        WritePayload::Prop {
            path,
            raw,
            pointer: pointer.to_string(),
            prop: prop.to_string(),
            value: value.to_string(),
        },
    );
}

/// Typed write of a component root field (schema-driven Properties section).
/// `Value::Null` removes the field (JSON) / the attribute (HTML). Debounced
/// with the same guarantees as [`write_prop`].
fn write_root_field(shared: &Shared, pointer: &str, field: &str, value: serde_json::Value) {
    optimistic(
        shared,
        Mutation::Field {
            pointer: pointer.to_string(),
            field: field.to_string(),
            value: value.clone(),
        },
    );
    let (path, raw) = {
        let m = shared.lock().unwrap_or_else(|e| e.into_inner());
        (m.path.clone(), m.raw.clone())
    };
    let Some(path) = path else {
        return;
    };
    let debounce = consume_context::<WriteDebounce>();
    schedule_write(
        &debounce,
        shared.clone(),
        WritePayload::RootField {
            path,
            raw,
            pointer: pointer.to_string(),
            field: field.to_string(),
            value,
        },
    );
}

/// Remove one style property (an emptied generic control unsets the key /
/// declaration rather than writing an empty string). Debounced.
fn write_style_removal(shared: &Shared, pointer: &str, prop: &str) {
    optimistic(
        shared,
        Mutation::Style {
            pointer: pointer.to_string(),
            prop: prop.to_string(),
            value: serde_json::Value::Null,
        },
    );
    let (path, raw) = {
        let m = shared.lock().unwrap_or_else(|e| e.into_inner());
        (m.path.clone(), m.raw.clone())
    };
    let Some(path) = path else {
        return;
    };
    let debounce = consume_context::<WriteDebounce>();
    schedule_write(
        &debounce,
        shared.clone(),
        WritePayload::StyleRemove {
            path,
            raw,
            pointer: pointer.to_string(),
            prop: prop.to_string(),
        },
    );
}

/// Write the element's text `content` back to the scenario file. Unlike
/// [`write_prop`], an empty value is allowed (clearing the text is valid).
/// Schedules a debounced write (~250 ms); errors are surfaced in the topbar.
fn write_content(shared: &Shared, pointer: &str, text: &str) {
    optimistic(
        shared,
        Mutation::Field {
            pointer: pointer.to_string(),
            field: "content".to_string(),
            value: serde_json::Value::String(text.to_string()),
        },
    );
    let (path, raw) = {
        let m = shared.lock().unwrap_or_else(|e| e.into_inner());
        (m.path.clone(), m.raw.clone())
    };
    let Some(path) = path else {
        return;
    };
    let debounce = consume_context::<WriteDebounce>();
    schedule_write(
        &debounce,
        shared.clone(),
        WritePayload::Content {
            path,
            raw,
            pointer: pointer.to_string(),
            text: text.to_string(),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_family_shows_content_before_properties() {
        assert!(content_before_properties("text"));
        assert!(content_before_properties("caption"));
        // Non-text components keep the current order (no content editor).
        assert!(!content_before_properties("gauge"));
        assert!(!content_before_properties("card"));
    }
}
