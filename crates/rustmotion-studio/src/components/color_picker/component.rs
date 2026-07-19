use std::sync::atomic::{AtomicU64, Ordering};

use dioxus::prelude::*;
use dioxus_primitives::color_picker::{self, Color, ColorAreaProps, ColorPickerContext};
use dioxus_primitives::label::Label;
use dioxus_primitives::popover;
use dioxus_primitives::slider::*;
use dioxus_primitives::use_controlled;
use palette::{encoding, FromColor, Hsv, IntoColor, RgbHue, Srgb};

use crate::components::input::Input;

// ADOPTION BOUNDARY — this file mirrors the OFFICIAL DioxusLabs/components
// color picker (preview/src/components/color_picker/component.rs at the
// pinned checkout), composed from the `dioxus_primitives::color_picker` and
// `popover` primitives. Popover behavior (open state, outside-click dismiss,
// global Escape) and positioning (CSS `absolute` under the trigger) are
// UPSTREAM's — we deliberately implement no positioning of our own.
//
// Our divergences, kept to the minimum:
// 1. `--rm-*` theming in style.css (values only; selectors upstream).
// 2. A local HSV mirror in `ColorPickerRoot` (live drag preview + guaranteed
//    `on_color_change` propagation).
// 3. Panel-wide exclusivity: the `ColorPicker` wrapper drives the primitive's
//    CONTROLLED `open` from the shared [`OpenPicker`] state (one picker open
//    at a time); upstream's dismiss paths flow through `on_open_change` and
//    compose cleanly with it.

fn format_color_hex(color: Color) -> String {
    format!("#{color:X}")
}

/// Which color picker is currently expanded, shared across the inspector
/// panel so only ONE picker is open at a time. The wrapper feeds this into
/// the primitive's controlled `open`; without the context the picker is
/// uncontrolled (upstream default behavior).
#[derive(Clone, Copy)]
pub struct OpenPicker(pub Signal<Option<u64>>);

/// Exclusive-state transition for a primitive `on_open_change(open)` event
/// (pure): opening claims the slot (closing any other picker); closing only
/// releases it if this picker holds it.
pub fn apply_open_change(current: Option<u64>, id: u64, open: bool) -> Option<u64> {
    if open {
        Some(id)
    } else if current == Some(id) {
        None
    } else {
        current
    }
}

/// Stable per-instance picker id.
fn next_picker_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone, Copy)]
struct ColorPickerRootContext {
    open: Memo<bool>,
    disabled: ReadSignal<bool>,
    color: ReadSignal<Hsv<encoding::Srgb, f64>>,
}

/// The props for the [`ColorPickerRoot`] component.
#[derive(Props, Clone, PartialEq)]
pub struct ColorPickerRootProps {
    /// The selected color
    #[props(default)]
    pub color: ReadSignal<Hsv<encoding::Srgb, f64>>,

    /// Callback when color changes
    #[props(default)]
    pub on_color_change: Callback<Hsv<encoding::Srgb, f64>>,

    /// Whether the color picker is disabled
    #[props(default)]
    pub disabled: ReadSignal<bool>,

    /// The controlled open state of the popover.
    pub open: ReadSignal<Option<bool>>,

    /// The default open state when uncontrolled.
    #[props(default)]
    pub default_open: bool,

    /// Callback fired when the open state changes.
    #[props(default)]
    pub on_open_change: Callback<bool>,

    /// Additional attributes to extend the color picker element
    #[props(extends = GlobalAttributes)]
    pub attributes: Vec<Attribute>,

    /// The children of the color picker element
    pub children: Element,
}

#[component]
pub fn ColorPickerRoot(props: ColorPickerRootProps) -> Element {
    let (open, set_open) = use_controlled(props.open, props.default_open, props.on_open_change);

    // OUR divergence: local mirror of the color. The picker internals (area
    // drag, hue slider, hex field) read/write THIS state, so every selection
    // — including mid-drag moves — previews live AND propagates through
    // `on_color_change` immediately, independent of whether the parent echoes
    // the new color back through the `color` prop.
    let mut local = use_signal(|| (props.color)());
    use_effect(move || {
        let external = (props.color)();
        if external != *local.peek() {
            local.set(external);
        }
    });
    let forward = props.on_color_change;
    let on_change = move |c: Hsv<encoding::Srgb, f64>| {
        local.set(c);
        forward.call(c);
    };

    use_context_provider(|| ColorPickerRootContext {
        open,
        disabled: props.disabled,
        color: local.into(),
    });

    rsx! {
        color_picker::ColorPicker {
            class: "dx-color-picker",
            color: local(),
            on_color_change: on_change,
            disabled: props.disabled,
            attributes: props.attributes,
            popover::PopoverRoot {
                is_modal: false,
                open: Some(open()),
                on_open_change: move |v| set_open.call(v),
                {props.children}
            }
        }
    }
}

/// The props for the [`ColorPickerTrigger`] component.
#[derive(Props, Clone, PartialEq)]
pub struct ColorPickerTriggerProps {
    /// Optional label on the trigger button
    #[props(default)]
    pub label: Option<String>,

    /// Additional attributes to extend the trigger button
    #[props(extends = GlobalAttributes)]
    pub attributes: Vec<Attribute>,

    /// Additional content to render inside the trigger button
    pub children: Element,
}

#[component]
pub fn ColorPickerTrigger(props: ColorPickerTriggerProps) -> Element {
    let ctx = use_context::<ColorPickerRootContext>();
    let aria_hex = use_memo(move || {
        let rgb: Color = Srgb::<f64>::from_color((ctx.color)()).into_format();
        format_color_hex(rgb)
    });

    rsx! {
        popover::PopoverTrigger {
            class: "dx-color-picker-button",
            disabled: if (ctx.disabled)() { true },
            aria_label: format!("Color picker {aria_hex}"),
            aria_expanded: (ctx.open)(),
            attributes: props.attributes,
            ColorSwatch { color: ctx.color }
            if let Some(label) = props.label { span { {label} } }
            {props.children}
        }
    }
}

/// The props for the [`ColorPickerPopover`] component.
#[derive(Props, Clone, PartialEq)]
pub struct ColorPickerPopoverProps {
    /// Additional attributes to extend the popover content
    #[props(extends = GlobalAttributes)]
    pub attributes: Vec<Attribute>,

    /// The children of the color picker popover
    pub children: Element,
}

/// Upstream verbatim: the popover primitive owns open/dismiss (outside click,
/// global Escape) and the position comes from the stylesheet — no runtime
/// positioning on our side.
#[component]
pub fn ColorPickerPopover(props: ColorPickerPopoverProps) -> Element {
    rsx! {
        popover::PopoverContent {
            class: "dx-color-picker-popover".to_string(),
            attributes: props.attributes,
            {props.children}
        }
    }
}

/// The props for the [`ColorPicker`] component.
#[derive(Props, Clone, PartialEq)]
pub struct ColorPickerProps {
    /// The selected color
    #[props(default)]
    pub color: ReadSignal<Hsv<encoding::Srgb, f64>>,

    /// Callback when color changes
    #[props(default)]
    pub on_color_change: Callback<Hsv<encoding::Srgb, f64>>,

    /// Whether the color picker is disabled
    #[props(default)]
    pub disabled: ReadSignal<bool>,

    /// Optional label on the trigger button
    #[props(default)]
    pub label: Option<String>,

    /// Callback fired when the open state changes.
    #[props(default)]
    pub on_open_change: Callback<bool>,

    /// Additional attributes to extend the color picker element
    #[props(extends = GlobalAttributes)]
    pub attributes: Vec<Attribute>,

    /// Additional content to append to the default color picker popover
    pub children: Element,
}

/// The styled wrapper every inspector row uses (swatch trigger + popover with
/// the full editor). Adds panel-wide exclusivity on top of the upstream
/// primitives by CONTROLLING their `open`.
#[component]
pub fn ColorPicker(props: ColorPickerProps) -> Element {
    let id = use_hook(next_picker_id);
    let shared = try_consume_context::<OpenPicker>();
    // Controlled when the panel provides the exclusivity context, otherwise
    // uncontrolled (upstream default).
    let controlled_open: Option<bool> = shared.map(|s| (s.0)() == Some(id));

    let forward_open = props.on_open_change;
    let on_open_change = move |v: bool| {
        if let Some(s) = shared {
            let mut sig = s.0;
            let next = apply_open_change(sig(), id, v);
            sig.set(next);
        }
        forward_open.call(v);
    };

    rsx! {
        ColorPickerRoot {
            color: props.color,
            on_color_change: props.on_color_change,
            disabled: props.disabled,
            open: controlled_open,
            on_open_change,
            attributes: props.attributes,
            ColorPickerTrigger {
                label: props.label,
            }
            ColorPickerPopover {
                ColorPickerSelect {}
                {props.children}
            }
        }
    }
}

/// The props for the [`ColorField`] component.
#[derive(Props, Clone, PartialEq)]
pub struct ColorFieldProps {
    /// Optional label above the input field
    #[props(default)]
    pub label: Option<String>,

    /// Optional props for the text description element
    #[props(default)]
    pub description: Option<String>,

    /// Additional attributes to extend the color field element
    #[props(extends = GlobalAttributes)]
    pub attributes: Vec<Attribute>,

    /// The children of the color field element
    pub children: Element,
}

/// # ColorField
///
/// The [`ColorField`] allows users to edit a hex color. Reads and writes the
/// current color through the surrounding [`ColorPickerContext`].
#[component]
fn ColorField(props: ColorFieldProps) -> Element {
    let ctx = use_context::<ColorPickerContext>();
    let hex_from_hsv = |hsv: Hsv<encoding::Srgb, f64>| {
        let rgb: Color = Srgb::<f64>::from_color(hsv).into_format();
        format_color_hex(rgb)
    };
    let emit_rgb = move |rgb: Color| {
        let hsv: Hsv<encoding::Srgb, f64> = rgb.into_format::<f64>().into_color();
        ctx.set_color(hsv);
    };

    let mut value = use_signal(|| hex_from_hsv(ctx.color()));

    // Synchronize local text with external color changes. Only overwrite
    // when the field already holds a parseable hex — otherwise the user is
    // mid-edit and replacing their text would clobber the input.
    use_effect(move || {
        let external = ctx.color();
        let current = value();
        if let Ok(parsed) = current.parse::<Color>() {
            let external_rgb: Color = Srgb::<f64>::from_color(external).into_format();
            if parsed != external_rgb {
                value.set(hex_from_hsv(external));
            }
        } else if current.is_empty() {
            value.set(hex_from_hsv(external));
        }
    });

    rsx! {
        div {
            class: "dx-color-field-container",
            ..props.attributes,
            if let Some(label) = props.label {
                Label {
                    html_for: "color_field",
                    class: "dx-color-slider-title",
                    {label}
                }
            }
            Input {
                id: "color_field",
                placeholder: "Enter a color",
                value: "{value}",
                oninput: move |e: FormEvent| {
                    let mut input = e.value();

                    // Sanitize input: allow only '#' and hex digits, length limit.
                    input.retain(|c| c == '#' || c.is_ascii_hexdigit());

                    // Automatically prepend '#' if missing.
                    if !input.starts_with('#') && !input.is_empty() {
                        input.insert(0, '#');
                    }

                    input.truncate(7);
                    value.set(input.to_uppercase());

                    if let Ok(parsed) = input.parse::<Color>() {
                        emit_rgb(parsed);
                    }
                },
            }
            if let Some(text) = props.description {
                span { class: "dx-color-field-description", {text} }
            }
            {props.children}
        }
    }
}

/// The props for the [`ColorSwatch`] component.
#[derive(Props, Clone, PartialEq)]
pub struct ColorSwatchProps {
    /// The selected color
    #[props(default)]
    pub color: ReadSignal<Hsv<encoding::Srgb, f64>>,

    /// Additional attributes to extend the color swatch element
    #[props(extends = GlobalAttributes)]
    pub attributes: Vec<Attribute>,

    /// The children of the color swatch element
    pub children: Element,
}

/// # ColorSwatch
///
/// The [`ColorSwatch`] displays a preview of a selected color.
#[component]
fn ColorSwatch(props: ColorSwatchProps) -> Element {
    let hex_color = use_memo(move || {
        let rgb: Color = Srgb::<f64>::from_color((props.color)()).into_format();
        format_color_hex(rgb)
    });

    rsx! {
        div {
            role: "img",
            aria_label: format!("Selected color {hex_color}"),
            class: "dx-color-swatch",
            style: "--swatch-color: {hex_color}",
            ..props.attributes,
            {props.children}
        }
    }
}

/// The props for the [`ColorSlider`] component.
#[derive(Props, Clone, PartialEq)]
pub struct ColorSliderProps {
    pub title: ReadSignal<String>,

    /// Additional attributes to extend the color slider element
    #[props(extends = GlobalAttributes)]
    pub attributes: Vec<Attribute>,

    /// The children of the color slider element
    pub children: Element,
}

/// # ColorSlider
///
/// The [`ColorSlider`] allows users to adjust the hue of the color held by
/// the surrounding [`ColorPickerContext`].
#[component]
fn ColorSlider(props: ColorSliderProps) -> Element {
    let ctx = use_context::<ColorPickerContext>();
    let mut current_hue = use_signal(|| ctx.color().hue.into_positive_degrees());

    let thumb_color = use_memo(move || {
        Srgb::<f64>::from_color(Hsv::<encoding::Srgb, f64>::new(
            RgbHue::new(current_hue()),
            1.0,
            1.0,
        ))
        .into_format()
    });

    use_effect(move || {
        let value = ctx.color().hue.into_positive_degrees();
        let current = current_hue();

        let is_wrap_around = (value - current).abs() > 350.0;

        // Update the signal only if this is an actual new position,
        // and not a "flip" of the circle by the palette library.
        if !is_wrap_around && value != current {
            current_hue.set(value);
        }
    });

    let display_value = {
        let value = current_hue();
        format!("{value:.2}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
            + "°"
    };

    rsx! {

        div {
            class: "dx-color-slider-container",
            ..props.attributes,
            label { class: "dx-color-slider-title", {props.title} }
            output { class: "dx-color-slider-output", "{display_value}" }
            Slider {
                class: "dx-color-slider",
                label: "Color Slider",
                horizontal: true,
                max: 360.0,
                value: current_hue(),
                on_value_change: move |h: f64| {
                    // Allow the value to be exactly 360.0
                    // The palette will understand that 360.0 == 0.0, but the signal will remain 360.0 for the UI.
                    current_hue.set(h);
                    ctx.set_hue(h);
                },
                SliderTrack {
                    class: "dx-color-slider-track",
                    SliderThumb {
                        class: "dx-color-slider-thumb",
                        aria_label: "Hue",
                        aria_valuetext: format!("{:.0}°", current_hue()),
                        background_color: format_color_hex(thumb_color()),
                    }
                }
            }
            {props.children}
        }
    }
}

#[component]
fn ColorArea(props: ColorAreaProps) -> Element {
    rsx! {
        color_picker::ColorArea {
            class: "dx-color-area-container",
            step: props.step,
            attributes: props.attributes,
            color_picker::AreaTrack {
                class: "dx-color-area-track",
                color_picker::AreaThumb {
                    class: "dx-color-area-thumb",
                    color_picker::AreaThumbSaturationInput {
                        class: "dx-color-area-input",
                    }
                    color_picker::AreaThumbValueInput {
                        class: "dx-color-area-input",
                    }
                }
            }
            {props.children}
        }
    }
}

/// The props for the [`ColorPickerSelect`] component.
#[derive(Props, Clone, PartialEq)]
pub struct ColorPickerSelectProps {
    /// Additional attributes to extend the color picker select element
    #[props(extends = GlobalAttributes)]
    pub attributes: Vec<Attribute>,

    /// The children of the color picker select element
    pub children: Element,
}

#[component]
pub fn ColorPickerSelect(props: ColorPickerSelectProps) -> Element {
    let ctx = use_context::<ColorPickerContext>();

    rsx! {
        div {
            class: "dx-color-picker-dialog",
            ..props.attributes,
            ColorArea {}
            ColorSlider { title: "Hue" }
            div {
                class: "dx-color-picker-input",
                ColorField { label: "Hex" }
                ColorSwatch { color: ctx.color() }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::apply_open_change;

    #[test]
    fn only_one_picker_open_at_a_time() {
        // Opening claims the slot.
        assert_eq!(apply_open_change(None, 7, true), Some(7));
        // Opening ANOTHER picker takes the slot over (the first closes: its
        // controlled `open` becomes false).
        assert_eq!(apply_open_change(Some(7), 9, true), Some(9));
        // Closing releases only when this picker holds the slot…
        assert_eq!(apply_open_change(Some(7), 7, false), None);
        // …a stale close from a picker that lost the slot changes nothing.
        assert_eq!(apply_open_change(Some(9), 7, false), Some(9));
    }
}
