#[derive(Clone, Copy, PartialEq)]
pub enum Ctrl {
    Text,
    Number,
    UnitSlider {
        min: f64,
        max: f64,
        step: f64,
        unit: &'static str,
    },
    Slider {
        min: f64,
        max: f64,
        step: f64,
    },
    Select(&'static [&'static str]),
    Weight,
    Align,
    StyleToggles,
    Color {
        clearable: bool,
    },
    Switch(&'static str, &'static str),
}

#[derive(Clone, Copy, PartialEq)]
pub struct Field {
    pub name: &'static str,
    pub label: &'static str,
    pub ctrl: Ctrl,
}

#[derive(Clone, Copy, PartialEq)]
pub struct Section {
    pub title: &'static str,
    pub fields: &'static [Field],
}

pub const WEIGHTS: &[(&str, &str)] = &[
    ("100", "Thin \u{b7} 100"),
    ("200", "Extra Light \u{b7} 200"),
    ("300", "Light \u{b7} 300"),
    ("400", "Regular \u{b7} 400"),
    ("500", "Medium \u{b7} 500"),
    ("600", "Semibold \u{b7} 600"),
    ("700", "Bold \u{b7} 700"),
    ("800", "Extra Bold \u{b7} 800"),
    ("900", "Black \u{b7} 900"),
];

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

#[derive(Clone, Copy, PartialEq)]
pub enum Family {
    Text,
    Container,
    Shape,
    Other,
}

pub fn family(kind: &str) -> Family {
    match kind {
        "text" | "caption" | "gradient_text" => Family::Text,
        "container" | "card" | "flex" | "grid" | "positioned" => Family::Container,
        "shape" | "image" | "icon" | "svg" | "video" | "gif" | "lottie" | "qrcode" | "mockup"
        | "divider" | "line" | "arrow" | "connector" => Family::Shape,
        _ => Family::Other,
    }
}

pub fn content_before_properties(kind: &str) -> bool {
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

pub fn sections(f: Family) -> &'static [Section] {
    match f {
        Family::Text => TEXT_SECTIONS,
        Family::Container => CONTAINER_SECTIONS,
        Family::Shape => SHAPE_SECTIONS,
        Family::Other => FALLBACK_SECTIONS,
    }
}

pub fn curated_names(fam: Family) -> std::collections::BTreeSet<&'static str> {
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

pub fn prop_str(style: &serde_json::Value, name: &str) -> String {
    match style.get(name) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

pub fn parse_num(s: &str) -> Option<f64> {
    let t: String = s
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    t.parse::<f64>().ok()
}

pub fn fmt_num(v: f64, step: f64) -> String {
    if step >= 1.0 {
        format!("{}", v.round() as i64)
    } else {
        ((v * 100.0).round() / 100.0).to_string()
    }
}

pub fn fmt_unit(v: f64, step: f64, unit: &str) -> String {
    let n = fmt_num(v, step);
    if unit.is_empty() {
        n
    } else {
        format!("{n}{}", unit.to_ascii_lowercase())
    }
}

pub fn num_display(value: &str, step: f64) -> String {
    parse_num(value)
        .map(|v| fmt_num(v, step))
        .unwrap_or_else(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_family_shows_content_before_properties() {
        assert!(content_before_properties("text"));
        assert!(content_before_properties("caption"));
        assert!(!content_before_properties("gauge"));
        assert!(!content_before_properties("card"));
    }

    #[test]
    fn curated_field_entries_match_the_original_transcription() {
        let total: usize = [
            Family::Text,
            Family::Container,
            Family::Shape,
            Family::Other,
        ]
        .iter()
        .map(|f| sections(*f).iter().map(|s| s.fields.len()).sum::<usize>())
        .sum();
        assert_eq!(
            total, 64,
            "sum of every Field row across the four family section lists"
        );
        let distinct_lists = [
            F_TYPO.len(),
            F_COLOR.len(),
            F_LAYOUT.len(),
            F_POSITION.len(),
            F_FLEX.len(),
            F_SPACING.len(),
            F_MARGIN.len(),
            F_SIZING.len(),
            F_SIZING_WH.len(),
            F_TEXT_SIZING.len(),
            F_APPEARANCE.len(),
            F_OPACITY.len(),
        ]
        .iter()
        .sum::<usize>();
        assert_eq!(
            distinct_lists, 44,
            "sum of every distinct Field literal declared once (F_* constants)"
        );
    }

    #[test]
    fn curated_names_cover_style_toggles_as_two_css_props() {
        let names = curated_names(Family::Text);
        assert!(names.contains("font-weight"));
        assert!(names.contains("font-style"));
        assert!(names.contains("font-size"));
        assert!(names.contains("text-align"));
    }

    #[test]
    fn container_curated_names_do_not_leak_text_fields() {
        let names = curated_names(Family::Container);
        assert!(!names.contains("text-align"));
        assert!(names.contains("display"));
        assert!(names.contains("flex-direction"));
    }

    #[test]
    fn parse_num_reads_the_leading_numeric_run() {
        assert_eq!(parse_num("26px"), Some(26.0));
        assert_eq!(parse_num("-0.5px"), Some(-0.5));
        assert_eq!(parse_num("auto"), None);
    }

    #[test]
    fn fmt_num_rounds_by_step_granularity() {
        assert_eq!(fmt_num(26.4, 1.0), "26");
        assert_eq!(fmt_num(1.006, 0.01), "1.01");
        assert_eq!(fmt_num(0.5, 0.01), "0.5");
    }

    #[test]
    fn fmt_unit_lowercases_the_unit_suffix() {
        assert_eq!(fmt_unit(26.0, 1.0, "PX"), "26px");
        assert_eq!(fmt_unit(1.5, 0.1, ""), "1.5");
    }

    #[test]
    fn prop_str_treats_null_and_missing_as_unset() {
        let style = serde_json::json!({"opacity": serde_json::Value::Null, "color": "#fff"});
        assert_eq!(prop_str(&style, "opacity"), "");
        assert_eq!(prop_str(&style, "color"), "#fff");
        assert_eq!(prop_str(&style, "missing"), "");
    }
}
