use std::sync::atomic::{AtomicU64, Ordering};

use dioxus::prelude::*;
use dioxus_primitives::color_picker::{self, Color, ColorAreaProps, ColorPickerContext};
use dioxus_primitives::label::Label;
use dioxus_primitives::slider::*;
use palette::{encoding, FromColor, Hsv, IntoColor, RgbHue, Srgb};

use crate::components::input::Input;

fn format_color_hex(color: Color) -> String {
    format!("#{color:X}")
}

/// Which color picker is currently expanded, shared across the inspector
/// panel so only ONE picker is open at a time: opening a swatch closes any
/// other. Provided by the panel; pickers fall back to a local open state when
/// the context is absent.
#[derive(Clone, Copy)]
pub struct OpenPicker(pub Signal<Option<u64>>);

/// Toggle decision for the exclusive open-picker state (pure): clicking the
/// open picker closes it, clicking any other opens that one.
pub fn toggle_picker(current: Option<u64>, id: u64) -> Option<u64> {
    if current == Some(id) {
        None
    } else {
        Some(id)
    }
}

/// Stable per-instance picker id.
fn next_picker_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
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

    /// Unused (kept for call-site compatibility): the picker expands inline
    /// and manages its own exclusive open state.
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

    /// Additional content to append to the expanded editor
    pub children: Element,
}

/// # ColorPicker — inline expansion
///
/// The open picker renders NO floating popover: it expands IN FLOW directly
/// below its row (`flex-basis:100%` wraps it to the next line of the
/// `flex-wrap` row), pushing the content below. No fixed/absolute
/// positioning, no measurement, no clamping — clipping is impossible by
/// construction and it scrolls naturally with the panel.
///
/// Close: re-click on the swatch, Escape inside the picker subtree, or
/// opening another swatch (exclusive [`OpenPicker`] state).
///
/// The local HSV mirror (live drag preview + guaranteed propagation) is kept
/// unchanged from the optimistic-edit round.
#[component]
pub fn ColorPicker(props: ColorPickerProps) -> Element {
    // Local mirror of the color: the picker internals (area drag, hue slider,
    // hex field) read/write THIS state, so every selection — including
    // mid-drag moves — previews live AND propagates through
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

    // Exclusive open state (panel-wide when the context is provided).
    let id = use_hook(next_picker_id);
    let shared_open = try_consume_context::<OpenPicker>();
    let local_open = use_signal(|| false);
    let is_open = match shared_open {
        Some(s) => (s.0)() == Some(id),
        None => local_open(),
    };

    let on_open_change = props.on_open_change;
    let toggle = move |_| {
        let now_open = match shared_open {
            Some(s) => {
                let mut sig = s.0;
                let next = toggle_picker(sig(), id);
                sig.set(next);
                next == Some(id)
            }
            None => {
                let mut l = local_open;
                let v = !l();
                l.set(v);
                v
            }
        };
        on_open_change.call(now_open);
    };
    let close = move || {
        match shared_open {
            Some(s) => {
                let mut sig = s.0;
                if sig() == Some(id) {
                    sig.set(None);
                }
            }
            None => {
                let mut l = local_open;
                l.set(false);
            }
        }
        on_open_change.call(false);
    };

    let aria_hex = use_memo(move || {
        let rgb: Color = Srgb::<f64>::from_color(local()).into_format();
        format_color_hex(rgb)
    });

    rsx! {
        color_picker::ColorPicker {
            class: "dx-color-picker",
            // `display:contents`: the trigger button and the inline editor
            // become direct items of the surrounding `flex-wrap` row, so the
            // editor wraps below the row at full row width.
            style: "display:contents;",
            color: local(),
            on_color_change: on_change,
            disabled: props.disabled,
            attributes: props.attributes,
            button {
                class: "dx-color-picker-button",
                disabled: (props.disabled)(),
                aria_label: "Color picker {aria_hex}",
                aria_expanded: is_open,
                onclick: toggle,
                ColorSwatch { color: local }
                if let Some(label) = props.label {
                    span { {label} }
                }
            }
            if is_open {
                div {
                    class: "dx-color-picker-inline",
                    // Escape closes THIS picker; handled locally because the
                    // panel wrapper stops keydown propagation to the app root.
                    onkeydown: move |evt: KeyboardEvent| {
                        if evt.key() == Key::Escape {
                            evt.stop_propagation();
                            close();
                        }
                    },
                    ColorPickerSelect {}
                    {props.children}
                }
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
    use super::toggle_picker;

    #[test]
    fn only_one_picker_open_at_a_time() {
        // Opening from nothing.
        assert_eq!(toggle_picker(None, 7), Some(7));
        // Re-click on the open picker closes it.
        assert_eq!(toggle_picker(Some(7), 7), None);
        // Clicking ANOTHER swatch switches to it (previous one closes).
        assert_eq!(toggle_picker(Some(7), 9), Some(9));
    }
}
