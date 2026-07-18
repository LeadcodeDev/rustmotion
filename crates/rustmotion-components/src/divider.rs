use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Path, Rect};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::paint_from_hex;
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum DividerDirection {
    #[default]
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum DividerLineStyle {
    #[default]
    Solid,
    Dashed,
    Dotted,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Divider {
    #[serde(default)]
    pub direction: DividerDirection,
    #[serde(default = "default_thickness")]
    pub thickness: f32,
    #[serde(default)]
    pub line_style: DividerLineStyle,
    #[serde(default)]
    pub length: Option<f32>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

fn default_thickness() -> f32 {
    2.0
}

rustmotion_core::impl_traits!(Divider {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Painter for Divider {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &rustmotion_core::engine::animator::AnimatedProperties,
        _ctx: &PaintCtx,
    ) {
        let color = self.style.color_str_or("#FFFFFF");
        let mut paint = paint_from_hex(color);
        paint.set_anti_alias(true);

        let is_horizontal = matches!(self.direction, DividerDirection::Horizontal);

        match self.line_style {
            DividerLineStyle::Solid => {
                paint.set_style(PaintStyle::Fill);
                if is_horizontal {
                    let rect = Rect::from_xywh(0.0, 0.0, layout.width, self.thickness);
                    canvas.draw_rect(rect, &paint);
                } else {
                    let rect = Rect::from_xywh(0.0, 0.0, self.thickness, layout.height);
                    canvas.draw_rect(rect, &paint);
                }
            }
            DividerLineStyle::Dashed | DividerLineStyle::Dotted => {
                paint.set_style(PaintStyle::Stroke);
                paint.set_stroke_width(self.thickness);

                let intervals = if matches!(self.line_style, DividerLineStyle::Dashed) {
                    [self.thickness * 4.0, self.thickness * 3.0]
                } else {
                    [self.thickness, self.thickness * 2.0]
                };

                if let Some(effect) = skia_safe::PathEffect::dash(&intervals, 0.0) {
                    paint.set_path_effect(effect);
                }

                let mut path = Path::new();
                if is_horizontal {
                    let y = self.thickness / 2.0;
                    path.move_to((0.0, y));
                    path.line_to((layout.width, y));
                } else {
                    let x = self.thickness / 2.0;
                    path.move_to((x, 0.0));
                    path.line_to((x, layout.height));
                }
                canvas.draw_path(&path, &paint);
            }
        }
    }
}
