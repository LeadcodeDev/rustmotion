mod controls;
mod sections;
mod write;

use std::collections::HashSet;

use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{h_flex, v_flex, ActiveTheme, Sizable as _, StyledExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, AppContext as _, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    Render, SharedString, StatefulInteractiveElement, Styled, Subscription, Task, Window,
};

use crate::app::state::EditorState;
use crate::editor::annotations::AnnotationCaptureBox;
use crate::editor::properties::{
    component_props, css_family, css_row_value, css_section_props, effective_element,
    visible_sections, CssSection, PropKind,
};
use crate::scenario::{read_field, read_style_object, scene_duration_for_pointer, Shared};

use controls::{Row, ScalarWidget};
use sections::{family, Ctrl, Family};
use write::Target;

#[allow(dead_code)]
struct SelectionData {
    pointer: String,
    kind: String,
    style: serde_json::Value,
    element: serde_json::Value,
    effective: serde_json::Value,
    content: Option<String>,
}

enum CuratedRow {
    Stateful {
        field: sections::Field,
        is_default: bool,
        widget: ScalarWidget,
    },
    Stateless {
        field: sections::Field,
        is_default: bool,
    },
}

pub struct InspectorPanel {
    shared: Shared,
    editor: Entity<EditorState>,
    selection: Option<crate::app::state::Selection>,
    controls_pointer: Option<String>,
    controls_generation: Option<u64>,
    force_rebuild: bool,
    open_picker: Option<u64>,
    color_pickers: Vec<(u64, Entity<gpui_component::color_picker::ColorPickerState>)>,
    subscriptions: Vec<Subscription>,
    pending_write: Option<Task<()>>,
    data: Option<SelectionData>,
    content_editor: Option<Entity<InputState>>,
    root_rows: Vec<Row>,
    timing: Option<(Entity<InputState>, Entity<InputState>)>,
    curated: Vec<(&'static str, Vec<CuratedRow>)>,
    css_sections: Vec<(CssSection, Vec<Row>)>,
    open_sections: HashSet<String>,
    annotation_box: Entity<AnnotationCaptureBox>,
}

impl InspectorPanel {
    pub fn new(
        shared: Shared,
        editor: Entity<EditorState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let annotation_box =
            cx.new(|cx| AnnotationCaptureBox::new(shared.clone(), editor.clone(), window, cx));
        Self {
            shared,
            editor,
            selection: None,
            controls_pointer: None,
            controls_generation: None,
            force_rebuild: false,
            open_picker: None,
            color_pickers: Vec::new(),
            subscriptions: Vec::new(),
            pending_write: None,
            data: None,
            content_editor: None,
            root_rows: Vec::new(),
            timing: None,
            curated: Vec::new(),
            css_sections: Vec::new(),
            open_sections: HashSet::new(),
            annotation_box,
        }
    }

    fn ensure_controls_for_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.editor.read(cx).selected.clone();
        let pointer = selected.as_ref().map(|s| s.pointer.clone());
        let generation = {
            let m = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            m.generation
        };
        if !self.force_rebuild
            && pointer == self.controls_pointer
            && Some(generation) == self.controls_generation
        {
            return;
        }
        self.force_rebuild = false;
        self.controls_pointer = pointer.clone();
        self.controls_generation = Some(generation);
        self.selection = selected.clone();
        self.subscriptions.clear();
        self.color_pickers.clear();
        self.open_picker = None;
        self.content_editor = None;
        self.root_rows.clear();
        self.timing = None;
        self.curated.clear();
        self.css_sections.clear();
        self.data = None;

        let Some(selection) = selected else {
            return;
        };
        let pointer = selection.pointer.clone();
        let kind = selection.kind.clone();

        let raw = {
            let m = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            m.raw.clone()
        };
        let style = read_style_object(&raw, &pointer);
        let content = read_field(&raw, &pointer, "content");
        let element = raw
            .pointer(&pointer)
            .cloned()
            .map(|mut v| {
                if let Some(o) = v.as_object_mut() {
                    o.remove("children");
                }
                v
            })
            .unwrap_or(serde_json::Value::Null);
        let effective = effective_element(&element);

        self.data = Some(SelectionData {
            pointer: pointer.clone(),
            kind: kind.clone(),
            style: style.clone(),
            element: element.clone(),
            effective: effective.clone(),
            content: content.clone(),
        });

        let fam = family(&kind);
        let is_text_family = fam == Family::Text;

        if is_text_family {
            self.content_editor =
                Some(self.build_content_editor(content.clone().unwrap_or_default(), window, cx));
        }

        if let Some(props) = component_props(&kind) {
            let skip_content = is_text_family;
            for spec in props.iter() {
                if spec.name == "content" && skip_content {
                    continue;
                }
                if spec.name == "start_at" || spec.name == "end_at" {
                    continue;
                }
                let display_value = sections::prop_str(&effective, &spec.name);
                let is_default = element.get(&spec.name).is_none()
                    && effective
                        .get(&spec.name)
                        .map(|v| !v.is_null())
                        .unwrap_or(false);
                let row = self.build_property_row(
                    spec,
                    &effective,
                    display_value,
                    is_default,
                    false,
                    &kind,
                    window,
                    cx,
                );
                self.root_rows.push(row);
            }
            let has_timing = props
                .iter()
                .any(|p| p.name == "start_at" || p.name == "end_at");
            if has_timing {
                let max = scene_duration_for_pointer(&raw, &pointer);
                let start = sections::prop_str(&effective, "start_at");
                let end = sections::prop_str(&effective, "end_at");
                let start_state = self.build_timing_input(start, "start_at", max, window, cx);
                let end_state = self.build_timing_input(end, "end_at", max, window, cx);
                self.timing = Some((start_state, end_state));
            }
        }

        let curated_names = sections::curated_names(fam);
        for section in sections::sections(fam) {
            let mut rows = Vec::new();
            for field in section.fields {
                let (value, is_default) =
                    css_row_value(sections::prop_str(&style, field.name), &kind, field.name);
                let row = self.build_curated_row(field, &value, is_default, window, cx);
                rows.push(row);
            }
            self.curated.push((section.title, rows));
        }

        let css_fam = css_family(&kind);
        for section in visible_sections(css_fam) {
            let props: Vec<_> = css_section_props(section)
                .into_iter()
                .filter(|p| !curated_names.contains(p.name.as_str()))
                .cloned()
                .collect();
            if props.is_empty() {
                continue;
            }
            let mut rows = Vec::new();
            for spec in &props {
                let (value, is_default) =
                    css_row_value(sections::prop_str(&style, &spec.name), &kind, &spec.name);
                let row = self
                    .build_property_row(spec, &style, value, is_default, true, &kind, window, cx);
                rows.push(row);
            }
            self.css_sections.push((section, rows));
            self.open_sections.remove(section.label());
        }
    }

    fn build_content_editor(
        &mut self,
        content: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let state = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_value(content, window, cx);
            s
        });
        self.subscriptions.push(cx.subscribe_in(
            &state,
            window,
            move |this, input, event: &InputEvent, window, cx| {
                if let InputEvent::Change = event {
                    let text = input.read(cx).value().to_string();
                    this.write_content(&text, window, cx);
                }
            },
        ));
        state
    }

    fn build_timing_input(
        &mut self,
        value: String,
        field: &'static str,
        _max: Option<f64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        self.build_text(
            &value,
            Some("scene"),
            Target::Root(field.to_string(), PropKind::Float),
            window,
            cx,
        )
    }

    fn build_curated_row(
        &mut self,
        field: &sections::Field,
        value: &str,
        is_default: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> CuratedRow {
        match field.ctrl {
            Ctrl::Text => {
                let target = Target::Style(field.name.to_string(), PropKind::String);
                let widget = ScalarWidget::Text(self.build_text(value, None, target, window, cx));
                CuratedRow::Stateful {
                    field: *field,
                    is_default,
                    widget,
                }
            }
            Ctrl::Number => {
                let target = Target::Style(field.name.to_string(), PropKind::Integer);
                let widget = ScalarWidget::Text(self.build_text(value, None, target, window, cx));
                CuratedRow::Stateful {
                    field: *field,
                    is_default,
                    widget,
                }
            }
            Ctrl::UnitSlider {
                min,
                max,
                step,
                unit,
            } => {
                let target = Target::Style(field.name.to_string(), PropKind::String);
                let (slider, text) =
                    self.build_slider(value, min, max, step, unit, target, window, cx);
                CuratedRow::Stateful {
                    field: *field,
                    is_default,
                    widget: ScalarWidget::SliderNum {
                        slider,
                        text,
                        unit,
                        step,
                    },
                }
            }
            Ctrl::Slider { min, max, step } => {
                let target = Target::Style(field.name.to_string(), PropKind::Float);
                let (slider, text) =
                    self.build_slider(value, min, max, step, "", target, window, cx);
                CuratedRow::Stateful {
                    field: *field,
                    is_default,
                    widget: ScalarWidget::SliderNum {
                        slider,
                        text,
                        unit: "",
                        step,
                    },
                }
            }
            Ctrl::Select(options) => {
                let target = Target::Style(field.name.to_string(), PropKind::String);
                let opts: Vec<String> = options.iter().map(|s| s.to_string()).collect();
                let widget =
                    ScalarWidget::Select(self.build_select(opts, value, target, window, cx));
                CuratedRow::Stateful {
                    field: *field,
                    is_default,
                    widget,
                }
            }
            Ctrl::Weight => {
                let target = Target::Style("font-weight".to_string(), PropKind::Integer);
                let opts: Vec<String> = sections::WEIGHTS
                    .iter()
                    .map(|(v, _)| v.to_string())
                    .collect();
                let widget =
                    ScalarWidget::Select(self.build_select(opts, value, target, window, cx));
                CuratedRow::Stateful {
                    field: *field,
                    is_default,
                    widget,
                }
            }
            Ctrl::Color { .. } => {
                let target = Target::Style(field.name.to_string(), PropKind::Color);
                let widget = ScalarWidget::Color(self.build_color(value, target, window, cx));
                CuratedRow::Stateful {
                    field: *field,
                    is_default,
                    widget,
                }
            }
            Ctrl::Align | Ctrl::StyleToggles | Ctrl::Switch(_, _) => CuratedRow::Stateless {
                field: *field,
                is_default,
            },
        }
    }

    fn render_header(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let kind = self
            .data
            .as_ref()
            .map(|d| d.kind.clone())
            .unwrap_or_default();
        h_flex()
            .items_center()
            .justify_between()
            .p_3p5()
            .child(
                v_flex()
                    .child(
                        div()
                            .font_semibold()
                            .text_color(cx.theme().accent)
                            .child("Inspector"),
                    )
                    .when(!kind.is_empty(), |el| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(kind.clone()),
                        )
                    }),
            )
            .child(
                Button::new("inspector-close")
                    .icon(gpui_component::IconName::Close)
                    .ghost()
                    .xsmall()
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.editor.update(cx, |state, cx| {
                            state.selected = None;
                            cx.notify();
                        });
                        cx.notify();
                    })),
            )
    }

    fn render_content_editor(&self, cx: &Context<Self>) -> impl IntoElement {
        let content = self.content_editor.clone();
        v_flex()
            .gap_1p5()
            .p_3p5()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(section_header("Content", cx))
            .children(content.map(|state| Input::new(&state)))
    }

    fn render_root_properties(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let has_rows = !self.root_rows.is_empty();
        let has_timing = self.timing.is_some();
        v_flex()
            .w_full()
            .when(has_rows, |el| {
                el.child(collapsible_section(
                    "Properties",
                    true,
                    self.root_rows
                        .iter()
                        .map(|row| labeled_row(&row.label, row.is_default, render_row(row, cx)))
                        .collect::<Vec<_>>(),
                    cx,
                ))
            })
            .when(has_timing, |el| {
                let (start, end) = self.timing.clone().unwrap();
                el.child(collapsible_section(
                    "Timing",
                    true,
                    vec![
                        labeled_row(
                            "Visible from (s)",
                            false,
                            Input::new(&start).small().into_any_element(),
                        ),
                        labeled_row(
                            "Visible until (s)",
                            false,
                            Input::new(&end).small().into_any_element(),
                        ),
                    ],
                    cx,
                ))
            })
    }

    fn render_curated(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let sections: Vec<(&'static str, Vec<gpui_kit::AnyElement>)> = self
            .curated
            .iter()
            .map(|(title, rows)| {
                let elements = rows
                    .iter()
                    .map(|row| render_curated_row(row, &self.shared, self.selection.as_ref(), cx))
                    .collect();
                (*title, elements)
            })
            .collect();
        v_flex()
            .w_full()
            .children(sections.into_iter().map(|(title, rows)| {
                v_flex()
                    .w_full()
                    .gap_1p5()
                    .p_3p5()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(section_header(title, cx))
                    .children(rows)
            }))
    }

    fn render_generic_css(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let sections = std::mem::take(&mut self.css_sections);
        let open_sections = self.open_sections.clone();
        let built = sections
            .iter()
            .map(|(section, rows)| {
                let label = section.label();
                let open = open_sections.contains(label);
                let elements: Vec<_> = rows
                    .iter()
                    .map(|row| labeled_row(&row.label, row.is_default, render_row(row, cx)))
                    .collect();
                (label, open, elements)
            })
            .collect::<Vec<_>>();
        self.css_sections = sections;
        v_flex().w_full().children(
            built
                .into_iter()
                .map(|(label, open, rows)| collapsible_section(label, open, rows, cx)),
        )
    }
}

fn section_header(title: &str, cx: &Context<InspectorPanel>) -> impl IntoElement {
    div()
        .text_xs()
        .font_semibold()
        .text_color(cx.theme().muted_foreground)
        .child(title.to_string().to_uppercase())
}

fn labeled_row(label: &str, is_default: bool, control: gpui_kit::AnyElement) -> impl IntoElement {
    h_flex()
        .items_center()
        .gap_2()
        .w_full()
        .child(
            div()
                .flex_none()
                .w(px(76.))
                .text_xs()
                .text_color(gpui_kit::hsla(0.0, 0.0, 0.6, 1.0))
                .child(format!(
                    "{label}{}",
                    if is_default { " \u{b7}" } else { "" }
                )),
        )
        .child(div().flex_1().child(control))
}

fn render_row(row: &Row, cx: &Context<InspectorPanel>) -> gpui_kit::AnyElement {
    controls::render_row_widget(&row.key, &row.widget, row.prefill, cx)
}

fn collapsible_section(
    title: &str,
    open: bool,
    rows: Vec<impl IntoElement>,
    cx: &mut Context<InspectorPanel>,
) -> impl IntoElement {
    let title_owned = title.to_string();
    let chevron = if open { "\u{25be}" } else { "\u{25b8}" };
    v_flex()
        .w_full()
        .gap_1p5()
        .p_3p5()
        .border_t_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .id(SharedString::from(format!("section-toggle-{title_owned}")))
                .cursor_pointer()
                .text_xs()
                .font_semibold()
                .text_color(cx.theme().muted_foreground)
                .child(format!("{chevron} {}", title_owned.to_uppercase()))
                .on_click(cx.listener(move |this, _, _window, cx| {
                    if !this.open_sections.remove(&title_owned) {
                        this.open_sections.insert(title_owned.clone());
                    }
                    cx.notify();
                })),
        )
        .when(open, |el| el.children(rows))
}

fn render_curated_row(
    row: &CuratedRow,
    shared: &Shared,
    selection: Option<&crate::app::state::Selection>,
    cx: &mut Context<InspectorPanel>,
) -> gpui_kit::AnyElement {
    match row {
        CuratedRow::Stateful {
            field,
            is_default,
            widget,
        } => labeled_row(
            field.label,
            *is_default,
            controls::render_scalar(widget, cx),
        )
        .into_any_element(),
        CuratedRow::Stateless { field, is_default } => {
            let style = selection
                .map(|s| {
                    let m = shared.lock().unwrap_or_else(|e| e.into_inner());
                    read_style_object(&m.raw, &s.pointer)
                })
                .unwrap_or(serde_json::Value::Null);
            let control = render_stateless_curated(field, &style, cx);
            labeled_row(field.label, *is_default, control).into_any_element()
        }
    }
}

fn render_stateless_curated(
    field: &sections::Field,
    style: &serde_json::Value,
    cx: &mut Context<InspectorPanel>,
) -> gpui_kit::AnyElement {
    match field.ctrl {
        Ctrl::Align => {
            let value = sections::prop_str(style, field.name);
            let options: [(&str, &str); 4] = [
                ("left", "Left"),
                ("center", "Center"),
                ("right", "Right"),
                ("justify", "Justify"),
            ];
            h_flex()
                .gap_1()
                .children(options.into_iter().map(|(v, label)| {
                    let active = value == v;
                    let value_owned = v.to_string();
                    Button::new(SharedString::from(format!("align-{v}")))
                        .label(label)
                        .when(active, |b| b.primary())
                        .when(!active, |b| b.ghost())
                        .xsmall()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.commit_text(
                                &Target::Style("text-align".to_string(), PropKind::String),
                                &value_owned,
                                window,
                                cx,
                            );
                        }))
                }))
                .into_any_element()
        }
        Ctrl::StyleToggles => {
            let weight = sections::prop_str(style, "font-weight");
            let is_bold =
                weight == "bold" || weight.parse::<i32>().map(|w| w >= 600).unwrap_or(false);
            let fstyle = sections::prop_str(style, "font-style");
            let is_italic = fstyle == "italic" || fstyle == "oblique";
            h_flex()
                .gap_1()
                .child(
                    Button::new("style-toggle-bold")
                        .label("B")
                        .when(is_bold, |b| b.primary())
                        .when(!is_bold, |b| b.ghost())
                        .xsmall()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let next = if is_bold { "400" } else { "700" };
                            this.commit_text(
                                &Target::Style("font-weight".to_string(), PropKind::Integer),
                                next,
                                window,
                                cx,
                            );
                        })),
                )
                .child(
                    Button::new("style-toggle-italic")
                        .label("I")
                        .when(is_italic, |b| b.primary())
                        .when(!is_italic, |b| b.ghost())
                        .xsmall()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let next = if is_italic { "normal" } else { "italic" };
                            this.commit_text(
                                &Target::Style("font-style".to_string(), PropKind::String),
                                next,
                                window,
                                cx,
                            );
                        })),
                )
                .into_any_element()
        }
        Ctrl::Switch(on, off) => {
            let value = sections::prop_str(style, field.name);
            let checked = value == on;
            let id = SharedString::from(format!("switch-{}", field.name));
            let target = Target::Style(field.name.to_string(), PropKind::String);
            let (on, off) = (on, off);
            gpui_component::switch::Switch::new(id)
                .checked(checked)
                .on_change(cx.listener(move |this, checked: &bool, window, cx| {
                    this.commit_text(&target, if *checked { on } else { off }, window, cx);
                }))
                .into_any_element()
        }
        _ => div().into_any_element(),
    }
}

impl Render for InspectorPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_controls_for_selection(window, cx);
        let has_selection = self.selection.is_some();
        let kind = self
            .data
            .as_ref()
            .map(|d| d.kind.clone())
            .unwrap_or_default();
        let content_first = has_selection && sections::content_before_properties(&kind);

        let header = self.render_header(cx).into_any_element();
        let properties = self.render_root_properties(cx).into_any_element();
        let curated = self.render_curated(cx).into_any_element();
        let generic = self.render_generic_css(cx).into_any_element();
        let annotation_box = self.annotation_box.clone();

        let mut body = v_flex()
            .id("inspector-panel")
            .flex_none()
            .w(px(300.))
            .h_full()
            .bg(cx.theme().sidebar)
            .border_l_1()
            .border_color(cx.theme().border)
            .overflow_y_scroll()
            .on_scroll_wheel(cx.listener(|this, _, _window, cx| {
                if this.open_picker.is_some() {
                    this.open_picker = None;
                    for (_, picker) in this.color_pickers.clone() {
                        picker.update(cx, |s, cx| s.set_open(false, cx));
                    }
                    cx.notify();
                }
            }))
            .child(header);

        if has_selection {
            if content_first {
                body = body.child(self.render_content_editor(cx).into_any_element());
            }
            body = body.child(properties).child(curated).child(generic);
            if !content_first {
                body = body.child(self.render_content_editor(cx).into_any_element());
            }
            body = body.child(annotation_box);
        }

        body
    }
}
