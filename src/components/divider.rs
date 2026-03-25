use crate::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Path, Rect};

use crate::engine::renderer::paint_from_hex;
use crate::layout::{Constraints, LayoutNode};
use crate::schema::LayerStyle;
use crate::traits::{RenderContext, TimingConfig, Widget};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DividerDirection {
    Horizontal,
    Vertical,
}

impl Default for DividerDirection {
    fn default() -> Self {
        Self::Horizontal
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DividerLineStyle {
    Solid,
    Dashed,
    Dotted,
}

impl Default for DividerLineStyle {
    fn default() -> Self {
        Self::Solid
    }
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
    pub style: LayerStyle,
}

fn default_thickness() -> f32 {
    2.0
}

crate::impl_traits!(Divider {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl Widget for Divider {
    fn render(
        &self,
        canvas: &Canvas,
        layout: &LayoutNode,
        _ctx: &RenderContext,
        _props: &crate::engine::animator::AnimatedProperties,
    ) -> Result<()> {
        let color = self.style.color.as_deref().unwrap_or("#FFFFFF");
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

        Ok(())
    }

    fn measure(&self, constraints: &Constraints) -> (f32, f32) {
        match self.direction {
            DividerDirection::Horizontal => {
                let w = self.length.unwrap_or(constraints.max_width);
                (w, self.thickness)
            }
            DividerDirection::Vertical => {
                let h = self.length.unwrap_or(constraints.max_height);
                (self.thickness, h)
            }
        }
    }
}
