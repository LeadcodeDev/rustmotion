use dioxus::prelude::*;
use dioxus_primitives::color_picker::{self, Color, ColorAreaProps, ColorPickerContext};
use dioxus_primitives::label::Label;
use dioxus_primitives::popover;
use dioxus_primitives::slider::*;
use dioxus_primitives::use_controlled;
use palette::{encoding, FromColor, Hsv, IntoColor, RgbHue, Srgb};

use crate::components::input::Input;

fn format_color_hex(color: Color) -> String {
    format!("#{color:X}")
}

/// Compute the popover's fixed position from the anchor (trigger swatch)
/// rect, the popover size and the window viewport, all in logical pixels.
/// Preference: below the anchor, right edges aligned (opens leftward — the
/// inspector hugs the right window edge); flips above when the bottom would
/// clip; then a HARD clamp of both axes into `[margin, viewport - size -
/// margin]` — the clamp always wins, even after the flip.
pub fn popover_position(
    anchor: (f64, f64, f64, f64), // x, y, w, h
    popover: (f64, f64),          // w, h
    viewport: (f64, f64),         // w, h
    margin: f64,
) -> (f64, f64) {
    const GAP: f64 = 4.0;
    let (ax, ay, aw, ah) = anchor;
    let (pw, ph) = popover;
    let (vw, vh) = viewport;

    // Right-aligned under the anchor.
    let x = ax + aw - pw;
    let mut y = ay + ah + GAP;
    // Flip above when the bottom clips.
    if y + ph > vh - margin {
        y = ay - ph - GAP;
    }
    // Hard clamp (min first so the margin wins on tiny viewports).
    let cx = x.min(vw - pw - margin).max(margin);
    let cy = y.min(vh - ph - margin).max(margin);
    (cx, cy)
}

#[derive(Clone, Copy)]
struct ColorPickerRootContext {
    open: Memo<bool>,
    disabled: ReadSignal<bool>,
    color: ReadSignal<Hsv<encoding::Srgb, f64>>,
    /// The trigger's mounted node — the popover anchors its fixed position on
    /// this rect at open time.
    trigger: Signal<Option<std::rc::Rc<MountedData>>>,
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

    // Local mirror of the color: the picker internals (area drag, hue slider,
    // hex field) read/write THIS state, so every selection — including
    // mid-drag moves — previews live in the popover AND propagates through
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

    let trigger = use_signal(|| None);
    use_context_provider(|| ColorPickerRootContext {
        open,
        disabled: props.disabled,
        color: local.into(),
        trigger,
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

    /// Additional content to append to the default color picker popover
    pub children: Element,
}

#[component]
pub fn ColorPicker(props: ColorPickerProps) -> Element {
    rsx! {
        ColorPickerRoot {
            color: props.color,
            on_color_change: props.on_color_change,
            disabled: props.disabled,
            open: props.open,
            default_open: props.default_open,
            on_open_change: props.on_open_change,
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

    let mut trigger = ctx.trigger;
    rsx! {
        div {
            style: "display:inline-flex;",
            onmounted: move |e| trigger.set(Some(e.data())),
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

#[component]
pub fn ColorPickerPopover(props: ColorPickerPopoverProps) -> Element {
    let ctx = use_context::<ColorPickerRootContext>();
    // Fixed positioning computed at open time by [`popover_position`]: below
    // the swatch opening leftward, flipped above near the window bottom, and
    // hard-clamped into the viewport on both axes. `position:fixed` escapes
    // every scrollable/clipping container of the panel.
    let mut node = use_signal(|| None::<std::rc::Rc<MountedData>>);
    let mut pos = use_signal(|| None::<(f64, f64)>);
    use_effect(move || {
        if (ctx.open)() {
            if let (Some(t), Some(n)) = ((ctx.trigger)(), node()) {
                spawn(async move {
                    let (Ok(anchor), Ok(content)) =
                        (t.get_client_rect().await, n.get_client_rect().await)
                    else {
                        return;
                    };
                    let win = dioxus::desktop::window();
                    let vs = win.inner_size().to_logical::<f64>(win.scale_factor());
                    // The measured node is the inner content: compensate for
                    // the popover's own padding (12px each side).
                    const PAD: f64 = 24.0;
                    pos.set(Some(popover_position(
                        (
                            anchor.origin.x,
                            anchor.origin.y,
                            anchor.size.width,
                            anchor.size.height,
                        ),
                        (content.size.width + PAD, content.size.height + PAD),
                        (vs.width, vs.height),
                        8.0,
                    )));
                });
            }
        }
    });
    let style = match pos() {
        Some((x, y)) => {
            format!("position:fixed; left:{x}px; top:{y}px; right:auto; bottom:auto; margin:0;")
        }
        // Not measured yet (first open): keep it invisible for one frame.
        None => "visibility:hidden;".to_string(),
    };

    rsx! {
        popover::PopoverContent {
            class: "dx-color-picker-popover".to_string(),
            style: "{style}",
            attributes: props.attributes,
            div {
                onmounted: move |e| node.set(Some(e.data())),
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
    use super::popover_position;

    #[test]
    fn anchor_top_right_opens_below_clamped_into_viewport() {
        // Swatch near the top-right corner of a 1200x800 window.
        let (x, y) = popover_position(
            (1150.0, 60.0, 24.0, 24.0),
            (260.0, 320.0),
            (1200.0, 800.0),
            8.0,
        );
        // Below the anchor…
        assert_eq!(y, 60.0 + 24.0 + 4.0);
        // …right-aligned would be 1174-260=914, fits; but never past the right margin.
        assert_eq!(x, 914.0);
        assert!(x + 260.0 <= 1200.0 - 8.0);
    }

    #[test]
    fn anchor_near_bottom_flips_above() {
        let (x, y) = popover_position(
            (900.0, 700.0, 24.0, 24.0),
            (260.0, 320.0),
            (1200.0, 800.0),
            8.0,
        );
        // 700+24+4+320 > 792 → flip above: 700-320-4 = 376.
        assert_eq!(y, 376.0);
        assert_eq!(x, 900.0 + 24.0 - 260.0);
    }

    #[test]
    fn anchor_top_left_never_clips_left_or_top() {
        // Right-aligned x would be negative → clamped to the margin.
        let (x, y) = popover_position(
            (10.0, 10.0, 24.0, 24.0),
            (260.0, 320.0),
            (1200.0, 800.0),
            8.0,
        );
        assert_eq!(x, 8.0);
        assert_eq!(y, 10.0 + 24.0 + 4.0);
    }

    #[test]
    fn tiny_viewport_clamps_to_margins() {
        // Popover larger than the window: both axes pinned at the margin
        // (the clamp always wins, even after the flip).
        let (x, y) = popover_position(
            (50.0, 90.0, 24.0, 24.0),
            (260.0, 320.0),
            (200.0, 150.0),
            8.0,
        );
        assert_eq!((x, y), (8.0, 8.0));
    }
}
