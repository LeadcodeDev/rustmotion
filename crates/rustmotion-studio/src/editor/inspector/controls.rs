use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
pub use gpui_component::select::SearchableVec;
use gpui_component::select::{Select, SelectEvent, SelectState};
use gpui_component::slider::{Slider, SliderEvent, SliderState};
use gpui_component::switch::Switch;
use gpui_component::{h_flex, v_flex, ActiveTheme, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, AppContext as _, Context, Entity, Hsla, IntoElement, ParentElement, Rgba,
    SharedString, Styled, Window,
};

use crate::editor::properties::{FillMode, PropKind, PropSpec};

use super::sections::{fmt_num, fmt_unit, parse_num};
use super::write::Target;
use super::InspectorPanel;

use std::sync::atomic::{AtomicU64, Ordering};

pub fn apply_open_change(current: Option<u64>, id: u64, open: bool) -> Option<u64> {
    if open {
        Some(id)
    } else if current == Some(id) {
        None
    } else {
        current
    }
}

pub fn next_picker_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

pub fn parse_hex(value: &str) -> Option<Hsla> {
    let value = value.trim();
    let value = value.strip_prefix('#').unwrap_or(value);
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let (width, has_alpha) = match value.len() {
        3 => (1, false),
        4 => (1, true),
        6 => (2, false),
        8 => (2, true),
        _ => return None,
    };
    let component = |index: usize| -> Option<f32> {
        let start = index * width;
        let raw = u8::from_str_radix(&value[start..start + width], 16).ok()?;
        let raw = if width == 1 { raw * 0x11 } else { raw };
        Some(raw as f32 / 255.0)
    };
    Some(
        Rgba {
            r: component(0)?,
            g: component(1)?,
            b: component(2)?,
            a: if has_alpha { component(3)? } else { 1.0 },
        }
        .into(),
    )
}

pub fn hex_string(color: Hsla) -> String {
    let rgba = Rgba::from(color);
    let channel = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u32;
    if rgba.a < 1.0 {
        format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            channel(rgba.r),
            channel(rgba.g),
            channel(rgba.b),
            channel(rgba.a)
        )
    } else {
        format!(
            "#{:02x}{:02x}{:02x}",
            channel(rgba.r),
            channel(rgba.g),
            channel(rgba.b)
        )
    }
}

#[allow(dead_code)]
pub struct ColorWidget {
    pub picker: Entity<ColorPickerState>,
    pub hex: Entity<InputState>,
    pub id: u64,
}

#[allow(dead_code)]
pub enum ScalarWidget {
    Switch {
        checked: bool,
        target: Target,
    },
    Select(Entity<SelectState<SearchableVec<SharedString>>>),
    Color(ColorWidget),
    SliderNum {
        slider: Entity<SliderState>,
        text: Entity<InputState>,
        unit: &'static str,
        step: f64,
    },
    Number(Entity<InputState>),
    Multiline {
        state: Entity<TextareaState>,
        monospace: bool,
    },
    Text(Entity<InputState>),
    Json(Entity<TextareaState>),
}

pub enum RowWidget {
    Scalar(ScalarWidget),
    ColorList(Vec<ColorWidget>),
    Fill {
        mode: FillMode,
        colors: Vec<ColorWidget>,
        angle: Entity<InputState>,
    },
    Object(Vec<(String, PropKind, ScalarWidget)>),
    NumberList(Vec<Entity<InputState>>),
    StringList(Vec<Entity<InputState>>),
}

pub struct Row {
    pub key: String,
    pub label: String,
    pub is_default: bool,
    pub widget: RowWidget,
    pub prefill: Option<&'static [&'static str]>,
}

impl InspectorPanel {
    pub(super) fn build_text(
        &mut self,
        value: &str,
        placeholder: Option<&'static str>,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let state = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            if let Some(ph) = placeholder {
                s = s.placeholder(ph);
            }
            s.set_value(value, window, cx);
            s
        });
        let target_for_sub = target.clone();
        self.subscriptions.push(cx.subscribe_in(
            &state,
            window,
            move |this, input, event: &InputEvent, window, cx| {
                if let InputEvent::Change = event {
                    let text = input.read(cx).value().to_string();
                    this.commit_text(&target_for_sub, &text, window, cx);
                }
            },
        ));
        state
    }

    pub(super) fn build_multiline(
        &mut self,
        value: &str,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TextareaState> {
        let state = cx.new(|cx| {
            let mut s = TextareaState::new(window, cx).auto_grow(3, 12);
            s.set_value(value, window, cx);
            s
        });
        let target_for_sub = target.clone();
        self.subscriptions.push(cx.subscribe_in(
            &state,
            window,
            move |this, input, event: &InputEvent, window, cx| {
                if let InputEvent::Change = event {
                    let text = input.read(cx).value().to_string();
                    this.commit_text(&target_for_sub, &text, window, cx);
                }
            },
        ));
        state
    }

    pub(super) fn build_json(
        &mut self,
        value: &str,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TextareaState> {
        let state = cx.new(|cx| {
            let mut s = TextareaState::new(window, cx).auto_grow(2, 10);
            s.set_value(value, window, cx);
            s
        });
        let target_for_sub = target.clone();
        self.subscriptions.push(cx.subscribe_in(
            &state,
            window,
            move |this, input, event: &InputEvent, window, cx| {
                if let InputEvent::Blur = event {
                    let text = input.read(cx).value().to_string();
                    let trimmed = text.trim();
                    if trimmed.is_empty()
                        || serde_json::from_str::<serde_json::Value>(trimmed).is_ok()
                    {
                        this.commit_text(&target_for_sub, trimmed, window, cx);
                    }
                }
            },
        ));
        state
    }

    pub(super) fn build_slider(
        &mut self,
        value: &str,
        min: f64,
        max: f64,
        step: f64,
        unit: &'static str,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<SliderState>, Entity<InputState>) {
        let start = parse_num(value).unwrap_or(min).clamp(min, max);
        let slider = cx.new(|_| {
            SliderState::new()
                .min(min as f32)
                .max(max as f32)
                .step(step as f32)
                .default_value(start as f32)
        });
        let text = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_value(fmt_num(start, step), window, cx);
            s
        });

        let slider_target = target.clone();
        let slider_text = text.clone();
        self.subscriptions.push(cx.subscribe_in(
            &slider,
            window,
            move |this, slider, event: &SliderEvent, window, cx| {
                let SliderEvent::Change(v) = event else {
                    return;
                };
                let v = v.start() as f64;
                slider_text.update(cx, |s, cx| {
                    s.set_value(fmt_num(v, step), window, cx);
                });
                let _ = slider;
                this.commit_text(&slider_target, &fmt_unit(v, step, unit), window, cx);
            },
        ));

        let text_target = target;
        let text_slider = slider.clone();
        self.subscriptions.push(cx.subscribe_in(
            &text,
            window,
            move |this, input, event: &InputEvent, window, cx| {
                if let InputEvent::Change = event {
                    let raw = input.read(cx).value().to_string();
                    if let Some(v) = parse_num(&raw) {
                        text_slider.update(cx, |s, cx| {
                            s.set_value(v.clamp(min, max) as f32, window, cx);
                        });
                        this.commit_text(&text_target, &fmt_unit(v, step, unit), window, cx);
                    } else {
                        this.commit_text(&text_target, &raw, window, cx);
                    }
                }
            },
        ));

        (slider, text)
    }

    fn new_color_widget(
        &mut self,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ColorWidget {
        let id = next_picker_id();
        let initial = parse_hex(value).unwrap_or(gpui_kit::hsla(0.0, 0.0, 0.0, 0.0));
        let picker = cx.new(|cx| ColorPickerState::new(window, cx).default_value(initial));
        let hex = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_value(value, window, cx);
            s
        });
        self.color_pickers.push((id, picker.clone()));

        self.subscriptions.push(cx.observe_in(
            &picker,
            window,
            move |this, picker_handle, _window, cx| {
                let open = picker_handle.read(cx).is_open();
                let next = apply_open_change(this.open_picker, id, open);
                if next != this.open_picker {
                    this.open_picker = next;
                    let others: Vec<_> = this
                        .color_pickers
                        .iter()
                        .filter(|(pid, _)| *pid != id)
                        .map(|(_, p)| p.clone())
                        .collect();
                    for other in others {
                        other.update(cx, |s, cx| s.set_open(false, cx));
                    }
                    cx.notify();
                }
            },
        ));

        ColorWidget { picker, hex, id }
    }

    pub(super) fn build_color(
        &mut self,
        value: &str,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ColorWidget {
        let widget = self.new_color_widget(value, window, cx);

        let picker_target = target.clone();
        let picker_hex = widget.hex.clone();
        self.subscriptions.push(cx.subscribe_in(
            &widget.picker,
            window,
            move |this, _picker, event: &ColorPickerEvent, window, cx| {
                let ColorPickerEvent::Change(Some(color)) = event else {
                    return;
                };
                let text = hex_string(*color);
                picker_hex.update(cx, |s, cx| s.set_value(text.clone(), window, cx));
                this.commit_text(&picker_target, &text, window, cx);
            },
        ));

        let hex_target = target;
        let hex_picker = widget.picker.clone();
        self.subscriptions.push(cx.subscribe_in(
            &widget.hex,
            window,
            move |this, input, event: &InputEvent, window, cx| {
                if let InputEvent::Change = event {
                    let text = input.read(cx).value().to_string();
                    if let Some(color) = parse_hex(&text) {
                        hex_picker.update(cx, |s, cx| s.set_value(color, window, cx));
                    }
                    this.commit_text(&hex_target, &text, window, cx);
                }
            },
        ));

        widget
    }

    pub(super) fn build_color_list_entry(
        &mut self,
        value: &str,
        field: String,
        siblings: std::rc::Rc<std::cell::RefCell<Vec<Entity<InputState>>>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ColorWidget {
        let widget = self.new_color_widget(value, window, cx);
        siblings.borrow_mut().push(widget.hex.clone());

        let commit_all = {
            let field = field.clone();
            let siblings = siblings.clone();
            move |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                let colors: Vec<serde_json::Value> = siblings
                    .borrow()
                    .iter()
                    .map(|e| serde_json::Value::String(e.read(cx).value().to_string()))
                    .collect();
                let value = if colors.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::Value::Array(colors)
                };
                this.write_root_field(&field, value, window, cx);
            }
        };

        let picker_hex = widget.hex.clone();
        let picker_commit = commit_all.clone();
        self.subscriptions.push(cx.subscribe_in(
            &widget.picker,
            window,
            move |this, _picker, event: &ColorPickerEvent, window, cx| {
                let ColorPickerEvent::Change(Some(color)) = event else {
                    return;
                };
                picker_hex.update(cx, |s, cx| s.set_value(hex_string(*color), window, cx));
                picker_commit(this, window, cx);
            },
        ));

        let hex_picker = widget.picker.clone();
        self.subscriptions.push(cx.subscribe_in(
            &widget.hex,
            window,
            move |this, input, event: &InputEvent, window, cx| {
                if let InputEvent::Change = event {
                    let text = input.read(cx).value().to_string();
                    if let Some(color) = parse_hex(&text) {
                        hex_picker.update(cx, |s, cx| s.set_value(color, window, cx));
                    }
                    commit_all(this, window, cx);
                }
            },
        ));

        widget
    }

    pub(super) fn build_select(
        &mut self,
        options: Vec<String>,
        value: &str,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SelectState<SearchableVec<SharedString>>> {
        let mut items: Vec<SharedString> = options.into_iter().map(SharedString::from).collect();
        if !value.is_empty() && !items.iter().any(|o| o.as_ref() == value) {
            items.insert(0, SharedString::from(value.to_string()));
        }
        let selected_index = items
            .iter()
            .position(|o| o.as_ref() == value)
            .map(gpui_component::IndexPath::new);
        let delegate = SearchableVec::new(items);
        let state = cx.new(|cx| SelectState::new(delegate, selected_index, window, cx));

        let select_target = target;
        self.subscriptions.push(cx.subscribe_in(
            &state,
            window,
            move |this, _select, event: &SelectEvent<SearchableVec<SharedString>>, window, cx| {
                let SelectEvent::Confirm(Some(value)) = event else {
                    return;
                };
                this.commit_text(&select_target, value.as_ref(), window, cx);
            },
        ));
        state
    }

    pub(super) fn build_scalar(
        &mut self,
        kind: &PropKind,
        value: &str,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ScalarWidget {
        match kind {
            PropKind::Bool => ScalarWidget::Switch {
                checked: value == "true",
                target,
            },
            PropKind::Enum(variants) => {
                ScalarWidget::Select(self.build_select(variants.clone(), value, target, window, cx))
            }
            PropKind::Color => ScalarWidget::Color(self.build_color(value, target, window, cx)),
            PropKind::Float => {
                if let Some((min, max, step)) =
                    crate::editor::properties::slider_range(row_prop_name(&target))
                {
                    let (slider, text) =
                        self.build_slider(value, min, max, step, "", target, window, cx);
                    ScalarWidget::SliderNum {
                        slider,
                        text,
                        unit: "",
                        step,
                    }
                } else {
                    ScalarWidget::Number(self.build_text(value, None, target, window, cx))
                }
            }
            PropKind::Integer => {
                ScalarWidget::Number(self.build_text(value, None, target, window, cx))
            }
            PropKind::String
                if crate::editor::properties::is_multiline(row_prop_name(&target), value) =>
            {
                let monospace = row_prop_name(&target) == "code";
                let state = self.build_multiline(value, target, window, cx);
                ScalarWidget::Multiline { state, monospace }
            }
            PropKind::Unit | PropKind::String => {
                let placeholder =
                    crate::editor::properties::engine_placeholder(row_prop_name(&target));
                ScalarWidget::Text(self.build_text(value, placeholder, target, window, cx))
            }
            _ => ScalarWidget::Json(self.build_json(value, target, window, cx)),
        }
    }
}

impl InspectorPanel {
    pub(super) fn build_number_list(
        &mut self,
        name: &str,
        values: &[f64],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Entity<InputState>> {
        let entities: Vec<Entity<InputState>> = values
            .iter()
            .map(|v| {
                cx.new(|cx| {
                    let mut s = InputState::new(window, cx);
                    s.set_value(fmt_num(*v, 0.01), window, cx);
                    s
                })
            })
            .collect();
        for entity in &entities {
            let siblings = entities.clone();
            let field = name.to_string();
            self.subscriptions.push(cx.subscribe_in(
                entity,
                window,
                move |this, _input, event: &InputEvent, window, cx| {
                    if let InputEvent::Change = event {
                        let nums: Option<Vec<serde_json::Value>> = siblings
                            .iter()
                            .map(|e| {
                                let text = e.read(cx).value().to_string();
                                super::write::parse_root_value(&PropKind::Float, &text)
                                    .ok()
                                    .filter(|v| !v.is_null())
                            })
                            .collect();
                        if let Some(nums) = nums {
                            let value = if nums.is_empty() {
                                serde_json::Value::Null
                            } else {
                                serde_json::Value::Array(nums)
                            };
                            this.write_root_field(&field, value, window, cx);
                        }
                    }
                },
            ));
        }
        entities
    }

    pub(super) fn build_string_list(
        &mut self,
        name: &str,
        values: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Entity<InputState>> {
        let entities: Vec<Entity<InputState>> = values
            .iter()
            .map(|v| {
                cx.new(|cx| {
                    let mut s = InputState::new(window, cx);
                    s.set_value(v.clone(), window, cx);
                    s
                })
            })
            .collect();
        for entity in &entities {
            let siblings = entities.clone();
            let field = name.to_string();
            self.subscriptions.push(cx.subscribe_in(
                entity,
                window,
                move |this, _input, event: &InputEvent, window, cx| {
                    if let InputEvent::Change = event {
                        let strings: Vec<serde_json::Value> = siblings
                            .iter()
                            .map(|e| serde_json::Value::String(e.read(cx).value().to_string()))
                            .collect();
                        let value = if strings.is_empty() {
                            serde_json::Value::Null
                        } else {
                            serde_json::Value::Array(strings)
                        };
                        this.write_root_field(&field, value, window, cx);
                    }
                },
            ));
        }
        entities
    }

    pub(super) fn build_color_list(
        &mut self,
        field: &str,
        colors: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<ColorWidget> {
        let siblings = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        colors
            .iter()
            .map(|c| {
                self.build_color_list_entry(c, field.to_string(), siblings.clone(), window, cx)
            })
            .collect()
    }

    pub(super) fn build_object_row(
        &mut self,
        root_name: &str,
        specs: &[PropSpec],
        object_value: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<(String, PropKind, ScalarWidget)> {
        specs
            .iter()
            .map(|spec| {
                let value = super::sections::prop_str(object_value, &spec.name);
                let target =
                    Target::Nested(root_name.to_string(), spec.name.clone(), spec.kind.clone());
                let widget = self.build_scalar(&spec.kind, &value, target, window, cx);
                (spec.name.clone(), spec.kind.clone(), widget)
            })
            .collect()
    }

    pub(super) fn build_fill_row(
        &mut self,
        field: &str,
        mode: FillMode,
        colors: &[String],
        angle: f64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> RowWidget {
        let color_siblings = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let angle_state = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_value(fmt_num(angle, 1.0), window, cx);
            s
        });

        let commit_fill = {
            let field = field.to_string();
            let color_siblings = color_siblings.clone();
            let angle_state = angle_state.clone();
            move |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                let colors: Vec<String> = color_siblings
                    .borrow()
                    .iter()
                    .map(|e: &Entity<InputState>| e.read(cx).value().to_string())
                    .collect();
                let angle_text = angle_state.read(cx).value().to_string();
                let angle = parse_num(&angle_text).unwrap_or(0.0);
                let value = crate::editor::properties::fill_to_value(mode, &colors, angle);
                this.write_root_field(&field, value, window, cx);
            }
        };

        let colors_widgets: Vec<ColorWidget> = colors
            .iter()
            .map(|c| {
                let widget = self.new_color_widget(c, window, cx);
                color_siblings.borrow_mut().push(widget.hex.clone());

                let picker_hex = widget.hex.clone();
                let picker_commit = commit_fill.clone();
                self.subscriptions.push(cx.subscribe_in(
                    &widget.picker,
                    window,
                    move |this, _picker, event: &ColorPickerEvent, window, cx| {
                        let ColorPickerEvent::Change(Some(color)) = event else {
                            return;
                        };
                        picker_hex.update(cx, |s, cx| s.set_value(hex_string(*color), window, cx));
                        picker_commit(this, window, cx);
                    },
                ));
                let hex_picker = widget.picker.clone();
                let hex_commit = commit_fill.clone();
                self.subscriptions.push(cx.subscribe_in(
                    &widget.hex,
                    window,
                    move |this, input, event: &InputEvent, window, cx| {
                        if let InputEvent::Change = event {
                            let text = input.read(cx).value().to_string();
                            if let Some(color) = parse_hex(&text) {
                                hex_picker.update(cx, |s, cx| s.set_value(color, window, cx));
                            }
                            hex_commit(this, window, cx);
                        }
                    },
                ));
                widget
            })
            .collect();

        let angle_commit = commit_fill;
        self.subscriptions.push(cx.subscribe_in(
            &angle_state,
            window,
            move |this, _input, event: &InputEvent, window, cx| {
                if let InputEvent::Change = event {
                    angle_commit(this, window, cx);
                }
            },
        ));

        RowWidget::Fill {
            mode,
            colors: colors_widgets,
            angle: angle_state,
        }
    }

    fn current_value(&self, field: &str) -> serde_json::Value {
        let Some(pointer) = self.selection.as_ref().map(|s| s.pointer.clone()) else {
            return serde_json::Value::Null;
        };
        let m = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        m.raw
            .pointer(&pointer)
            .and_then(|el| el.get(field))
            .cloned()
            .unwrap_or(serde_json::Value::Null)
    }

    fn current_array(&self, field: &str) -> Vec<serde_json::Value> {
        self.current_value(field)
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    pub(super) fn list_add(
        &mut self,
        field: &str,
        add_value: serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut current = self.current_array(field);
        current.push(add_value);
        self.write_root_field(field, serde_json::Value::Array(current), window, cx);
        self.force_rebuild = true;
        cx.notify();
    }

    pub(super) fn color_list_add(
        &mut self,
        field: &str,
        prefill: Option<&'static [&'static str]>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current: Vec<String> = self
            .current_array(field)
            .into_iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        let next = crate::editor::properties::next_entries_on_add(&current, "#ffffff", prefill);
        let value: Vec<serde_json::Value> =
            next.into_iter().map(serde_json::Value::String).collect();
        self.write_root_field(field, serde_json::Value::Array(value), window, cx);
        self.force_rebuild = true;
        cx.notify();
    }

    pub(super) fn list_remove(
        &mut self,
        field: &str,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut current = self.current_array(field);
        if index < current.len() {
            current.remove(index);
        }
        let value = if current.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::Value::Array(current)
        };
        self.write_root_field(field, value, window, cx);
        self.force_rebuild = true;
        cx.notify();
    }

    pub(super) fn fill_set_mode(
        &mut self,
        field: &str,
        mode: FillMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let raw = self.current_value(field);
        let (_, mut colors, angle) = crate::editor::properties::parse_fill(&raw);
        if colors.is_empty() {
            colors.push("#ffffff".to_string());
        }
        let value = crate::editor::properties::fill_to_value(mode, &colors, angle);
        self.write_root_field(field, value, window, cx);
        self.force_rebuild = true;
        cx.notify();
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_property_row(
        &mut self,
        spec: &PropSpec,
        raw_value: &serde_json::Value,
        display_value: String,
        is_default: bool,
        is_style: bool,
        host_tag: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Row {
        let value_str = display_value;
        let prefill = crate::editor::properties::palette_prefill(host_tag, &spec.name, true);
        let widget = match &spec.kind {
            PropKind::ColorList => {
                let colors: Vec<String> = raw_value
                    .get(&spec.name)
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|c| c.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                RowWidget::ColorList(self.build_color_list(&spec.name, &colors, window, cx))
            }
            PropKind::Fill if !is_style => {
                let raw = raw_value
                    .get(&spec.name)
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let (mode, colors, angle) = crate::editor::properties::parse_fill(&raw);
                let colors = if colors.is_empty() {
                    vec!["#ffffff".to_string()]
                } else {
                    colors
                };
                self.build_fill_row(&spec.name, mode, &colors, angle, window, cx)
            }
            PropKind::Object(specs) if !is_style => {
                let object_value = raw_value
                    .get(&spec.name)
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                RowWidget::Object(self.build_object_row(
                    &spec.name,
                    specs,
                    &object_value,
                    window,
                    cx,
                ))
            }
            PropKind::NumberList if !is_style => {
                let nums: Vec<f64> = raw_value
                    .get(&spec.name)
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|n| n.as_f64()).collect())
                    .unwrap_or_default();
                RowWidget::NumberList(self.build_number_list(&spec.name, &nums, window, cx))
            }
            PropKind::StringList if !is_style => {
                let strings: Vec<String> = raw_value
                    .get(&spec.name)
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|s| s.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                RowWidget::StringList(self.build_string_list(&spec.name, &strings, window, cx))
            }
            kind => {
                let target = if is_style {
                    Target::Style(spec.name.clone())
                } else {
                    Target::Root(spec.name.clone(), kind.clone())
                };
                RowWidget::Scalar(self.build_scalar(kind, &value_str, target, window, cx))
            }
        };
        Row {
            key: spec.name.clone(),
            label: spec.name.clone(),
            is_default,
            widget,
            prefill,
        }
    }
}

fn row_prop_name(target: &Target) -> &str {
    match target {
        Target::Style(n) => n,
        Target::Root(n, _) => n,
        Target::Nested(_, n, _) => n,
    }
}

pub(super) fn render_scalar(
    widget: &ScalarWidget,
    cx: &Context<InspectorPanel>,
) -> gpui_kit::AnyElement {
    match widget {
        ScalarWidget::Switch { checked, target } => {
            let id = SharedString::from(format!("switch-{}", row_prop_name(target)));
            let target = target.clone();
            Switch::new(id)
                .checked(*checked)
                .on_change(cx.listener(move |this, checked: &bool, window, cx| {
                    this.commit_text(&target, if *checked { "true" } else { "false" }, window, cx);
                }))
                .into_any_element()
        }
        ScalarWidget::Select(state) => Select::new(state).w_full().into_any_element(),
        ScalarWidget::Color(c) => render_color_widget(c),
        ScalarWidget::SliderNum { slider, text, .. } => h_flex()
            .gap_2()
            .items_center()
            .w_full()
            .child(div().flex_1().child(Slider::new(slider)))
            .child(div().flex_none().w(px(52.)).child(Input::new(text).small()))
            .into_any_element(),
        ScalarWidget::Number(state) => Input::new(state).small().into_any_element(),
        ScalarWidget::Multiline { state, monospace } => {
            let area = Textarea::new(state);
            if *monospace {
                area.font_family("monospace").into_any_element()
            } else {
                area.into_any_element()
            }
        }
        ScalarWidget::Text(state) => Input::new(state).small().into_any_element(),
        ScalarWidget::Json(state) => Textarea::new(state).into_any_element(),
    }
}

fn render_color_widget(c: &ColorWidget) -> gpui_kit::AnyElement {
    h_flex()
        .gap_2()
        .items_center()
        .w_full()
        .child(ColorPicker::new(&c.picker))
        .child(div().flex_1().child(Input::new(&c.hex).small()))
        .into_any_element()
}

fn remove_button(
    id: SharedString,
    field: String,
    index: usize,
    cx: &Context<InspectorPanel>,
) -> impl IntoElement {
    Button::new(id)
        .icon(gpui_component::IconName::Delete)
        .ghost()
        .xsmall()
        .on_click(cx.listener(move |this, _, window, cx| {
            this.list_remove(&field, index, window, cx);
        }))
}

pub(super) fn render_row_widget(
    field: &str,
    widget: &RowWidget,
    prefill: Option<&'static [&'static str]>,
    cx: &Context<InspectorPanel>,
) -> gpui_kit::AnyElement {
    match widget {
        RowWidget::Scalar(scalar) => render_scalar(scalar, cx),
        RowWidget::ColorList(colors) => {
            let field_owned = field.to_string();
            let add_field = field_owned.clone();
            v_flex()
                .gap_1p5()
                .w_full()
                .children(colors.iter().enumerate().map(|(i, c)| {
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(render_color_widget(c))
                        .child(remove_button(
                            format!("colorlist-remove-{field_owned}-{i}").into(),
                            field_owned.clone(),
                            i,
                            cx,
                        ))
                }))
                .child(
                    Button::new(SharedString::from(format!("colorlist-add-{add_field}")))
                        .label("+ Add color")
                        .outline()
                        .xsmall()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.color_list_add(&add_field, prefill, window, cx);
                        })),
                )
                .into_any_element()
        }
        RowWidget::NumberList(entries) | RowWidget::StringList(entries) => {
            let is_number = matches!(widget, RowWidget::NumberList(_));
            let field_owned = field.to_string();
            let add_field = field_owned.clone();
            v_flex()
                .gap_1p5()
                .w_full()
                .children(entries.iter().enumerate().map(|(i, e)| {
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(div().flex_1().child(Input::new(e).small()))
                        .child(remove_button(
                            format!("list-remove-{field_owned}-{i}").into(),
                            field_owned.clone(),
                            i,
                            cx,
                        ))
                }))
                .child(
                    Button::new(SharedString::from(format!("list-add-{add_field}")))
                        .label("+ Add")
                        .outline()
                        .xsmall()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let value = if is_number {
                                serde_json::Value::from(0)
                            } else {
                                serde_json::Value::String(String::new())
                            };
                            this.list_add(&add_field, value, window, cx);
                        })),
                )
                .into_any_element()
        }
        RowWidget::Object(sub_rows) => v_flex()
            .gap_1p5()
            .w_full()
            .pl_2()
            .border_l_1()
            .border_color(cx.theme().border)
            .children(sub_rows.iter().map(|(name, _, widget)| {
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_none()
                            .w(px(76.))
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(name.clone()),
                    )
                    .child(div().flex_1().child(render_scalar(widget, cx)))
            }))
            .into_any_element(),
        RowWidget::Fill {
            mode,
            colors,
            angle,
        } => {
            let field_owned = field.to_string();
            let mut column = v_flex().gap_2().w_full().child(
                h_flex()
                    .gap_1()
                    .child(fill_seg_button(&field_owned, FillMode::Single, *mode, cx))
                    .child(fill_seg_button(&field_owned, FillMode::Linear, *mode, cx))
                    .child(fill_seg_button(&field_owned, FillMode::Radial, *mode, cx)),
            );
            if *mode == FillMode::Single {
                if let Some(c) = colors.first() {
                    column = column.child(render_color_widget(c));
                }
            } else {
                column = column.child(
                    v_flex()
                        .gap_1p5()
                        .children(colors.iter().map(render_color_widget)),
                );
                if *mode == FillMode::Linear {
                    column = column.child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Angle"),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .w(px(52.))
                                    .child(Input::new(angle).small()),
                            ),
                    );
                }
            }
            column.into_any_element()
        }
    }
}

fn fill_seg_button(
    field: &str,
    mode: FillMode,
    active_mode: FillMode,
    cx: &Context<InspectorPanel>,
) -> impl IntoElement {
    let label = match mode {
        FillMode::Single => "Single",
        FillMode::Linear => "Linear",
        FillMode::Radial => "Radial",
    };
    let field = field.to_string();
    let active = mode == active_mode;
    Button::new(SharedString::from(format!("fill-{field}-{label}")))
        .label(label)
        .when(active, |b| b.primary())
        .when(!active, |b| b.ghost())
        .xsmall()
        .on_click(cx.listener(move |this, _, window, cx| {
            this.fill_set_mode(&field, mode, window, cx);
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_picker_open_at_a_time() {
        assert_eq!(apply_open_change(None, 7, true), Some(7));
        assert_eq!(apply_open_change(Some(7), 9, true), Some(9));
        assert_eq!(apply_open_change(Some(7), 7, false), None);
        assert_eq!(apply_open_change(Some(9), 7, false), Some(9));
    }

    #[test]
    fn hex_round_trips_through_parse_and_format() {
        let c = parse_hex("#3B82F6").unwrap();
        assert_eq!(hex_string(c), "#3b82f6");
        assert_eq!(parse_hex("ffffff"), parse_hex("#fff"));
        assert!(parse_hex("not-a-color").is_none());
    }

    #[test]
    fn picker_ids_are_distinct() {
        let a = next_picker_id();
        let b = next_picker_id();
        assert_ne!(a, b);
    }
}
