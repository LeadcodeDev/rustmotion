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
use crate::scenario::{set_field, set_style, Shared};

use super::annotations::AnnotationBox;

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
    Field { name: "font-size", label: "Size", ctrl: Ctrl::UnitSlider { min: 8.0, max: 200.0, step: 1.0, unit: "PX" } },
    Field { name: "font-weight", label: "Weight", ctrl: Ctrl::Weight },
    Field { name: "style", label: "Style", ctrl: Ctrl::StyleToggles },
    Field { name: "line-height", label: "Line height", ctrl: Ctrl::UnitSlider { min: 0.8, max: 3.0, step: 0.1, unit: "" } },
    Field { name: "letter-spacing", label: "Tracking", ctrl: Ctrl::UnitSlider { min: -5.0, max: 20.0, step: 0.5, unit: "PX" } },
    Field { name: "text-align", label: "Align", ctrl: Ctrl::Align },
];

const F_COLOR: &[Field] = &[
    Field { name: "color", label: "Text", ctrl: Ctrl::Color { clearable: false } },
    Field { name: "background", label: "Background", ctrl: Ctrl::Color { clearable: true } },
];

const F_LAYOUT: &[Field] = &[
    Field { name: "display", label: "Display", ctrl: Ctrl::Select(&["block", "flex", "grid", "inline-block", "none", "contents"]) },
    Field { name: "position", label: "Position", ctrl: Ctrl::Select(&["static", "relative", "absolute"]) },
    Field { name: "top", label: "Top", ctrl: Ctrl::Text },
    Field { name: "right", label: "Right", ctrl: Ctrl::Text },
    Field { name: "bottom", label: "Bottom", ctrl: Ctrl::Text },
    Field { name: "left", label: "Left", ctrl: Ctrl::Text },
    Field { name: "z-index", label: "Z-index", ctrl: Ctrl::Number },
    Field { name: "overflow", label: "Overflow", ctrl: Ctrl::Select(&["visible", "hidden", "auto", "scroll", "clip"]) },
    Field { name: "visibility", label: "Visible", ctrl: Ctrl::Switch("hidden", "visible") },
];

const F_POSITION: &[Field] = &[
    Field { name: "position", label: "Position", ctrl: Ctrl::Select(&["static", "relative", "absolute"]) },
    Field { name: "top", label: "Top", ctrl: Ctrl::Text },
    Field { name: "left", label: "Left", ctrl: Ctrl::Text },
    Field { name: "z-index", label: "Z-index", ctrl: Ctrl::Number },
];

const F_FLEX: &[Field] = &[
    Field { name: "flex-direction", label: "Direction", ctrl: Ctrl::Select(&["row", "row-reverse", "column", "column-reverse"]) },
    Field { name: "flex-wrap", label: "Wrap", ctrl: Ctrl::Select(&["nowrap", "wrap", "wrap-reverse"]) },
    Field { name: "justify-content", label: "Justify", ctrl: Ctrl::Select(&["flex-start", "flex-end", "center", "space-between", "space-around", "space-evenly", "start", "end"]) },
    Field { name: "align-items", label: "Align", ctrl: Ctrl::Select(&["stretch", "flex-start", "flex-end", "center", "baseline", "start", "end"]) },
    Field { name: "align-content", label: "Align content", ctrl: Ctrl::Select(&["stretch", "flex-start", "flex-end", "center", "space-between", "space-around", "space-evenly", "start", "end"]) },
    Field { name: "gap", label: "Gap", ctrl: Ctrl::Text },
];

const F_SPACING: &[Field] = &[
    Field { name: "padding", label: "Padding", ctrl: Ctrl::Text },
    Field { name: "margin", label: "Margin", ctrl: Ctrl::Text },
];

const F_MARGIN: &[Field] = &[Field { name: "margin", label: "Margin", ctrl: Ctrl::Text }];

const F_SIZING: &[Field] = &[
    Field { name: "width", label: "Width", ctrl: Ctrl::Text },
    Field { name: "height", label: "Height", ctrl: Ctrl::Text },
    Field { name: "min-width", label: "Min W", ctrl: Ctrl::Text },
    Field { name: "min-height", label: "Min H", ctrl: Ctrl::Text },
    Field { name: "max-width", label: "Max W", ctrl: Ctrl::Text },
    Field { name: "max-height", label: "Max H", ctrl: Ctrl::Text },
];

const F_SIZING_WH: &[Field] = &[
    Field { name: "width", label: "Width", ctrl: Ctrl::Text },
    Field { name: "height", label: "Height", ctrl: Ctrl::Text },
];

const F_TEXT_SIZING: &[Field] = &[
    Field { name: "width", label: "Width", ctrl: Ctrl::Text },
    Field { name: "max-width", label: "Max W", ctrl: Ctrl::Text },
];

const F_APPEARANCE: &[Field] = &[
    Field { name: "background", label: "Background", ctrl: Ctrl::Color { clearable: true } },
    Field { name: "border-radius", label: "Radius", ctrl: Ctrl::Text },
    Field { name: "opacity", label: "Opacity", ctrl: Ctrl::Slider { min: 0.0, max: 1.0, step: 0.01 } },
];

const F_OPACITY: &[Field] = &[Field { name: "opacity", label: "Opacity", ctrl: Ctrl::Slider { min: 0.0, max: 1.0, step: 0.01 } }];

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

const TEXT_SECTIONS: &[Section] = &[
    Section { title: "Typography", fields: F_TYPO },
    Section { title: "Color", fields: F_COLOR },
    Section { title: "Sizing", fields: F_TEXT_SIZING },
    Section { title: "Spacing", fields: F_SPACING },
    Section { title: "Appearance", fields: F_OPACITY },
];

const CONTAINER_SECTIONS: &[Section] = &[
    Section { title: "Layout", fields: F_LAYOUT },
    Section { title: "Flex", fields: F_FLEX },
    Section { title: "Spacing", fields: F_SPACING },
    Section { title: "Sizing", fields: F_SIZING },
    Section { title: "Appearance", fields: F_APPEARANCE },
];

const SHAPE_SECTIONS: &[Section] = &[
    Section { title: "Layout", fields: F_POSITION },
    Section { title: "Sizing", fields: F_SIZING },
    Section { title: "Spacing", fields: F_MARGIN },
    Section { title: "Appearance", fields: F_APPEARANCE },
];

const FALLBACK_SECTIONS: &[Section] = &[
    Section { title: "Layout", fields: F_POSITION },
    Section { title: "Sizing", fields: F_SIZING_WH },
    Section { title: "Spacing", fields: F_SPACING },
    Section { title: "Appearance", fields: F_APPEARANCE },
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
    if active { "var(--rm-on-accent)" } else { "var(--rm-text-muted)" }
}

// ── Value helpers ────────────────────────────────────────────────────────────

/// Stringify a property's current value from the element's `style` object.
fn prop_str(style: &serde_json::Value, name: &str) -> String {
    match style.get(name) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
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
        format!("{v:.2}")
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

/// The right-hand inspector for the selected element: header, the content
/// editor (text family only), the per-type style sections, and the
/// "comment for the agent" box. Driven by frame-independent values, so it stays
/// memoized and doesn't re-render on playback.
#[component]
pub fn InspectorPanel(
    selected: Signal<Option<(u32, String, String)>>,
    pointer: String,
    kind: String,
    current: Signal<u32>,
    content: Option<String>,
    style: serde_json::Value,
) -> Element {
    let fam = family(&kind);
    rsx! {
        div { style: "width:300px; flex:none; min-height:0; background:var(--rm-surface); border-left:1px solid var(--rm-border); box-sizing:border-box; display:flex; flex-direction:column; overflow:auto;",
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
            if fam == Family::Text {
                ContentEditor { pointer: pointer.clone(), content: content.clone().unwrap_or_default() }
            }
            for section in sections(fam) {
                SectionView {
                    key: "{section.title}",
                    section: *section,
                    pointer: pointer.clone(),
                    style: style.clone(),
                }
            }
            AnnotationBox { pointer, kind, current }
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
            let kind = if matches!(field.ctrl, Ctrl::Number) { "number" } else { "text" };
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
        Ctrl::UnitSlider { min, max, step, unit } => {
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
                    default_value: if value.is_empty() { None } else { Some(value.clone()) },
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
            let vr = |v: &str| if value == v { ButtonVariant::Primary } else { ButtonVariant::Ghost };
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
            let is_bold = weight == "bold" || weight.parse::<i32>().map(|w| w >= 600).unwrap_or(false);
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
                div { style: "display:flex; align-items:center; gap:6px; width:100%;",
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

/// Write a single style property back to the scenario file. The watcher reloads
/// and refreshes the preview. Empty values are ignored so clearing a field
/// mid-edit doesn't collapse the element.
fn write_prop(shared: &Shared, pointer: &str, prop: &str, value: &str) {
    if value.trim().is_empty() {
        return;
    }
    let (path, raw) = {
        let m = shared.lock().unwrap();
        (m.path.clone(), m.raw.clone())
    };
    let Some(path) = path else {
        return;
    };
    if rustmotion::loader::is_html_path(&path) {
        if let Ok(html) = std::fs::read_to_string(&path) {
            if let Some(updated) = rustmotion::loader::set_html_inline_style(&html, pointer, prop, value) {
                let _ = std::fs::write(&path, updated);
            }
        }
    } else if let Some(updated) = set_style(raw, pointer, prop, value) {
        if let Ok(text) = serde_json::to_string_pretty(&updated) {
            let _ = std::fs::write(&path, text);
        }
    }
}

/// Write the element's text `content` back to the scenario file. Unlike
/// [`write_prop`], an empty value is allowed (clearing the text is valid).
fn write_content(shared: &Shared, pointer: &str, text: &str) {
    let (path, raw) = {
        let m = shared.lock().unwrap();
        (m.path.clone(), m.raw.clone())
    };
    let Some(path) = path else {
        return;
    };
    if rustmotion::loader::is_html_path(&path) {
        if let Ok(html) = std::fs::read_to_string(&path) {
            if let Some(updated) = rustmotion::loader::set_html_text_content(&html, pointer, text) {
                let _ = std::fs::write(&path, updated);
            }
        }
    } else if let Some(updated) = set_field(raw, pointer, "content", text) {
        if let Ok(s) = serde_json::to_string_pretty(&updated) {
            let _ = std::fs::write(&path, s);
        }
    }
}
