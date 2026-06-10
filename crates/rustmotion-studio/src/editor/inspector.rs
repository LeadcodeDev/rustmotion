use std::collections::HashSet;

use dioxus::prelude::*;
use palette::{encoding, FromColor, Hsv, IntoColor, Srgb};

use crate::components::color_picker::ColorPicker;
use crate::components::select::{Select, SelectOption};
use crate::components::slider::Slider;
use crate::components::switch::Switch;
use crate::scenario::{set_style, Shared};

use super::annotations::AnnotationBox;

// ── Property schema ──────────────────────────────────────────────────────

/// How a property is edited in the inspector.
#[derive(Clone, PartialEq)]
enum Control {
    Text,
    Number,
    Color,
    Select(&'static [&'static str]),
    /// (min, max, step)
    Slider(f64, f64, f64),
    /// (on_value, off_value) — a two-state toggle.
    Switch(&'static str, &'static str),
}

struct PropSpec {
    name: &'static str,
    control: Control,
}

struct Group {
    title: &'static str,
    props: &'static [PropSpec],
}

/// All supported style properties, grouped and mapped to a typed control.
/// Select options are the kebab-case variants of the matching `CssStyle` enum.
const GROUPS: &[Group] = &[
    Group {
        title: "Layout",
        props: &[
            PropSpec {
                name: "display",
                control: Control::Select(&[
                    "block",
                    "flex",
                    "grid",
                    "inline-block",
                    "none",
                    "contents",
                ]),
            },
            PropSpec {
                name: "position",
                control: Control::Select(&["static", "relative", "absolute"]),
            },
            PropSpec {
                name: "top",
                control: Control::Text,
            },
            PropSpec {
                name: "right",
                control: Control::Text,
            },
            PropSpec {
                name: "bottom",
                control: Control::Text,
            },
            PropSpec {
                name: "left",
                control: Control::Text,
            },
            PropSpec {
                name: "z-index",
                control: Control::Number,
            },
            PropSpec {
                name: "visibility",
                control: Control::Switch("hidden", "visible"),
            },
            PropSpec {
                name: "overflow",
                control: Control::Select(&["visible", "hidden", "auto", "scroll", "clip"]),
            },
            PropSpec {
                name: "overflow-x",
                control: Control::Select(&["visible", "hidden", "auto", "scroll", "clip"]),
            },
            PropSpec {
                name: "overflow-y",
                control: Control::Select(&["visible", "hidden", "auto", "scroll", "clip"]),
            },
            PropSpec {
                name: "box-sizing",
                control: Control::Select(&["content-box", "border-box"]),
            },
            PropSpec {
                name: "aspect-ratio",
                control: Control::Text,
            },
        ],
    },
    Group {
        title: "Sizing",
        props: &[
            PropSpec {
                name: "width",
                control: Control::Text,
            },
            PropSpec {
                name: "height",
                control: Control::Text,
            },
            PropSpec {
                name: "min-width",
                control: Control::Text,
            },
            PropSpec {
                name: "min-height",
                control: Control::Text,
            },
            PropSpec {
                name: "max-width",
                control: Control::Text,
            },
            PropSpec {
                name: "max-height",
                control: Control::Text,
            },
        ],
    },
    Group {
        title: "Spacing",
        props: &[
            PropSpec {
                name: "margin",
                control: Control::Text,
            },
            PropSpec {
                name: "padding",
                control: Control::Text,
            },
            PropSpec {
                name: "gap",
                control: Control::Text,
            },
        ],
    },
    Group {
        title: "Flex",
        props: &[
            PropSpec {
                name: "flex-direction",
                control: Control::Select(&["row", "row-reverse", "column", "column-reverse"]),
            },
            PropSpec {
                name: "flex-wrap",
                control: Control::Select(&["nowrap", "wrap", "wrap-reverse"]),
            },
            PropSpec {
                name: "justify-content",
                control: Control::Select(&[
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
            PropSpec {
                name: "align-items",
                control: Control::Select(&[
                    "stretch",
                    "flex-start",
                    "flex-end",
                    "center",
                    "baseline",
                    "start",
                    "end",
                ]),
            },
            PropSpec {
                name: "align-content",
                control: Control::Select(&[
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
            PropSpec {
                name: "align-self",
                control: Control::Select(&[
                    "auto",
                    "stretch",
                    "flex-start",
                    "flex-end",
                    "center",
                    "baseline",
                    "start",
                    "end",
                ]),
            },
            PropSpec {
                name: "flex-grow",
                control: Control::Slider(0.0, 10.0, 1.0),
            },
            PropSpec {
                name: "flex-shrink",
                control: Control::Slider(0.0, 10.0, 1.0),
            },
            PropSpec {
                name: "flex-basis",
                control: Control::Text,
            },
            PropSpec {
                name: "order",
                control: Control::Number,
            },
        ],
    },
    Group {
        title: "Typography",
        props: &[
            PropSpec {
                name: "font-family",
                control: Control::Text,
            },
            PropSpec {
                name: "font-size",
                control: Control::Text,
            },
            PropSpec {
                name: "font-weight",
                control: Control::Select(&[
                    "normal", "bold", "100", "200", "300", "400", "500", "600", "700", "800", "900",
                ]),
            },
            PropSpec {
                name: "font-style",
                control: Control::Select(&["normal", "italic", "oblique"]),
            },
            PropSpec {
                name: "line-height",
                control: Control::Text,
            },
            PropSpec {
                name: "letter-spacing",
                control: Control::Text,
            },
            PropSpec {
                name: "text-align",
                control: Control::Select(&["left", "right", "center", "justify", "start", "end"]),
            },
            PropSpec {
                name: "color",
                control: Control::Color,
            },
            PropSpec {
                name: "white-space",
                control: Control::Select(&["normal", "nowrap", "pre", "pre-wrap", "pre-line"]),
            },
            PropSpec {
                name: "overflow-wrap",
                control: Control::Select(&["normal", "break-word", "anywhere"]),
            },
            PropSpec {
                name: "text-overflow",
                control: Control::Select(&["clip", "ellipsis"]),
            },
        ],
    },
    Group {
        title: "Appearance",
        props: &[
            PropSpec {
                name: "background",
                control: Control::Color,
            },
            PropSpec {
                name: "border-radius",
                control: Control::Text,
            },
            PropSpec {
                name: "opacity",
                control: Control::Slider(0.0, 1.0, 0.01),
            },
            PropSpec {
                name: "mix-blend-mode",
                control: Control::Select(&[
                    "normal",
                    "multiply",
                    "screen",
                    "overlay",
                    "darken",
                    "lighten",
                    "color-dodge",
                    "color-burn",
                    "hard-light",
                    "soft-light",
                    "difference",
                    "exclusion",
                    "hue",
                    "saturation",
                    "color",
                    "luminosity",
                ]),
            },
        ],
    },
];

/// Stringify a property's current value from the element's `style` object.
fn prop_str(style: &serde_json::Value, name: &str) -> String {
    match style.get(name) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

// ── Panel ────────────────────────────────────────────────────────────────

/// The fixed right-hand inspector panel for the selected element: a header, the
/// grouped style fields, and the "comment for the agent" box. Driven by the
/// element's `style` value (frame-independent), so it stays memoized and
/// doesn't re-render on playback frame changes.
#[component]
pub fn InspectorPanel(
    selected: Signal<Option<(u32, String, String)>>,
    pointer: String,
    kind: String,
    current: Signal<u32>,
    style: serde_json::Value,
) -> Element {
    rsx! {
        div { style: "width:300px; flex:none; min-height:0; background:var(--rm-surface); border-left:1px solid var(--rm-border); box-sizing:border-box; display:flex; flex-direction:column; gap:12px; overflow:auto;",
            div {
              style: "padding:16px;",
              div { style: "display:flex; justify-content:space-between; align-items:center;",
                  div { style: "color:var(--rm-accent); font-weight:600;", "Inspector" }
                  button {
                      style: "cursor:pointer; background:none; border:none; color:var(--rm-text-muted); font-size:16px; line-height:1;",
                      onclick: move |_| selected.set(None),
                      "✕"
                  }
              }
              if !kind.is_empty() {
                  div { style: "color:var(--rm-text-muted);", "{kind}" }
              }
            }
            InspectorFields { pointer: pointer.clone(), style }
            AnnotationBox { pointer, kind, current }
        }
    }
}

/// The grouped, typed style controls — a memoized child so it doesn't re-render
/// on playback frame changes (which would steal focus / reset inputs).
#[component]
fn InspectorFields(pointer: String, style: serde_json::Value) -> Element {
    let mut open = use_signal(|| {
        GROUPS
            .iter()
            .map(|g| g.title)
            .collect::<HashSet<&'static str>>()
    });

    rsx! {
        for group in GROUPS.iter() {
            div { key: "{group.title}", style: "flex:none; display:flex; flex-direction:column; border:1px solid var(--rm-border)",
                button {
                    style: "display:flex; justify-content:space-between; align-items:center; padding:8px 10px; cursor:pointer; background:var(--rm-surface-2); color:var(--rm-text-strong); border:none; font:inherit; font-weight:600;",
                    onclick: move |_| {
                        let mut s = open.write();
                        if s.contains(group.title) { s.remove(group.title); } else { s.insert(group.title); }
                    },
                    "{group.title}"
                    span { style: "color:var(--rm-text-muted);", if open().contains(group.title) { "−" } else { "+" } }
                }
                if open().contains(group.title) {
                    div { style: "display:flex; flex-direction:column; gap:10px; padding:10px;",
                        for spec in group.props.iter() {
                            Field {
                                key: "{pointer}-{spec.name}",
                                pointer: pointer.clone(),
                                name: spec.name,
                                control: spec.control.clone(),
                                value: prop_str(&style, spec.name),
                            }
                        }
                    }
                }
            }
        }
    }
}

/// One labelled property row with its typed control.
#[component]
fn Field(pointer: String, name: &'static str, control: Control, value: String) -> Element {
    let shared = use_context::<Shared>();
    let input_style = "padding:6px; background:var(--rm-bg); color:var(--rm-text); border:1px solid var(--rm-border-2); border-radius:6px; font:inherit; width:100%; box-sizing:border-box;";

    let field = match &control {
        Control::Text | Control::Number => {
            let kind = if matches!(control, Control::Number) {
                "number"
            } else {
                "text"
            };
            rsx! {
                input {
                    r#type: "{kind}",
                    style: "{input_style}",
                    value: "{value}",
                    oninput: {
                        let shared = shared.clone();
                        let p = pointer.clone();
                        move |e: FormEvent| write_prop(&shared, &p, name, &e.value())
                    }
                }
            }
        }
        Control::Select(opts) => {
            let mut items: Vec<String> = opts.iter().map(|s| s.to_string()).collect();
            if !value.is_empty() && !items.iter().any(|o| o == &value) {
                items.insert(0, value.clone());
            }
            let options = items.iter().enumerate().map(|(i, opt)| {
                rsx! {
                    SelectOption::<String> {
                        key: "{opt}",
                        index: i,
                        value: opt.clone(),
                        text_value: "{opt}",
                        "{opt}"
                    }
                }
            });
            rsx! {
                Select::<String> {
                    width: "100%",
                    default_value: if value.is_empty() { None } else { Some(value.clone()) },
                    on_value_change: {
                        let shared = shared.clone();
                        let p = pointer.clone();
                        move |v: Option<String>| {
                            if let Some(v) = v { write_prop(&shared, &p, name, &v); }
                        }
                    },
                    {options}
                }
            }
        }
        Control::Slider(min, max, step) => {
            let cur = value.parse::<f64>().unwrap_or(*min);
            let (min, max, step) = (*min, *max, *step);
            rsx! {
                div {
                  style: "display:flex; flex-direction:column; align-items:center;",
                  Slider {
                      default_value: cur,
                      min,
                      max,
                      step,
                      on_value_change: {
                          let shared = shared.clone();
                          let p = pointer.clone();
                          move |v: f64| {
                              let s = if step >= 1.0 { format!("{}", v as i64) } else { format!("{v:.2}") };
                              write_prop(&shared, &p, name, &s);
                          }
                      },
                  }
                  span { style: "color:var(--rm-text-muted); min-width:36px; text-align:right;", "{value}" }
                }
            }
        }
        Control::Switch(on, off) => {
            let (on, off) = (*on, *off);
            rsx! {
                Switch {
                    default_checked: value == on,
                    on_checked_change: {
                        let shared = shared.clone();
                        let p = pointer.clone();
                        move |checked: bool| {
                            write_prop(&shared, &p, name, if checked { on } else { off });
                        }
                    },
                }
            }
        }
        Control::Color => {
            let mut color = use_signal(|| parse_hsv(&value));

            rsx! {
              div { style: "display:flex; align-items:center; justify-content:space-between; gap:6px;",
                ColorPicker {
                    label: "Pick a color",
                    color: color(),
                    on_color_change: {
                      let shared = shared.clone();
                      let p = pointer.clone();
                      move |c: Hsv<encoding::Srgb, f64>| {
                          color.set(c);
                          write_prop(&shared, &p, name, &hsv_to_hex(c));
                      }
                    },
                }
                button {
                    style: "cursor:pointer; background:none; border:1px solid var(--rm-border-2); border-radius:6px; color:var(--rm-text-muted); padding:5px 8px; font:inherit; line-height:1;",
                    title: "Make transparent",
                    onclick: {
                        let shared = shared.clone();
                        let p = pointer.clone();
                        move |_| {
                            color.set(parse_hsv("#00000000"));
                            write_prop(&shared, &p, name, "#00000000");
                        }
                    },
                    "✕"
                }
              }
            }
        }
    };

    // A plain div, NOT a <label>: wrapping interactive widgets (color picker,
    // select, switch) in a <label> makes the browser re-dispatch clicks to the
    // label's first labelable control — for the color picker that's its popover
    // trigger button, so every click inside the open popover would toggle it shut.
    rsx! {
        div { style: "display:flex; flex-direction:column; gap:4px;",
            span { style: "color:var(--rm-text-muted); font-size:11px;", "{name}" }
            {field}
        }
    }
}

fn parse_hsv(s: &str) -> Hsv<encoding::Srgb, f64> {
    let hex = s.trim().trim_start_matches('#');
    if hex.len() == 6 {
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

/// Write a single style property back to the scenario file. The file watcher
/// then reloads the model and refreshes the preview. Empty values are ignored
/// so clearing a field mid-edit doesn't collapse the element.
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
        // HTML source: edit the element's inline `style` attribute in place.
        if let Ok(html) = std::fs::read_to_string(&path) {
            if let Some(updated) =
                rustmotion::loader::set_html_inline_style(&html, pointer, prop, value)
            {
                let _ = std::fs::write(&path, updated);
            }
        }
    } else if let Some(updated) = set_style(raw, pointer, prop, value) {
        if let Ok(text) = serde_json::to_string_pretty(&updated) {
            let _ = std::fs::write(&path, text);
        }
    }
}
