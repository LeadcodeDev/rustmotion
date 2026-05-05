use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Rect};

use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::paint_from_hex;
use rustmotion_core::layout::{layout_flex, layout_grid_with_config, Constraints, LayoutNode};
use rustmotion_core::schema::{CardDisplay, LayerStyle};
use rustmotion_core::traits::{
    Border, Bordered, BorderedMut, Container, FlexConfig, FlexContainer,
    FlexContainerMut, GridConfig, PaintCtx, Painter, RenderContext, Rounded, RoundedMut, Shadow,
    Shadowed, ShadowedMut, TimingConfig, Widget,
};

use crate::flex::FlexSize;
use crate::ChildComponent;

/// Card container — backward-compatible with v1 `"type": "card"`.
/// Supports both flex and grid display modes via the `display` style field.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Card {
    #[serde(default)]
    pub children: Vec<ChildComponent>,
    #[serde(default)]
    pub size: Option<FlexSize>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

rustmotion_core::impl_traits!(Card {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl Container for Card {}

impl FlexContainer for Card {
    fn flex_config(&self) -> &FlexConfig {
        unreachable!("Use style directly for flex config")
    }
}

impl FlexContainerMut for Card {
    fn flex_config_mut(&mut self) -> &mut FlexConfig {
        unreachable!("Use style directly for flex config")
    }
}

impl Card {
    fn grid_config_owned(&self) -> GridConfig {
        GridConfig {
            grid_template_columns: self.style.grid_template_columns.clone(),
            grid_template_rows: self.style.grid_template_rows.clone(),
            gap: self.style.gap_or(0.0),
        }
    }
}

impl Bordered for Card {
    fn border(&self) -> Option<&Border> {
        None // Handled directly in render via self.style.border
    }
}

impl BorderedMut for Card {
    fn set_border(&mut self, _border: Option<Border>) {}
}

impl Rounded for Card {
    fn corner_radius(&self) -> f32 {
        self.style.border_radius_or(12.0)
    }
}

impl RoundedMut for Card {
    fn set_corner_radius(&mut self, radius: f32) {
        self.style.border_radius = Some(radius);
    }
}

impl Shadowed for Card {
    fn shadow(&self) -> Option<&Shadow> {
        None // Handled directly in render via self.style.box_shadow
    }
}

impl ShadowedMut for Card {
    fn set_shadow(&mut self, _shadow: Option<Shadow>) {}
}

impl rustmotion_core::traits::Backgrounded for Card {
    fn background(&self) -> Option<&str> {
        self.style.background.as_deref()
    }
}

impl rustmotion_core::traits::BackgroundedMut for Card {
    fn set_background(&mut self, bg: Option<String>) {
        self.style.background = bg;
    }
}

impl rustmotion_core::traits::Clipped for Card {
    fn clip(&self) -> bool {
        true
    }
}

impl Widget for Card {
    fn render(&self, canvas: &Canvas, layout: &LayoutNode, ctx: &RenderContext, props: &rustmotion_core::engine::animator::AnimatedProperties, pipeline: &dyn rustmotion_core::traits::RenderPipeline) -> Result<()> {
        let corner_radius = self.style.border_radius_or(12.0);
        let rect = Rect::from_xywh(0.0, 0.0, layout.width, layout.height);
        let rrect = skia_safe::RRect::new_rect_xy(rect, corner_radius, corner_radius);

        // 1. Shadow (skip if 3D active — render_v2 draws adaptive ground shadow instead)
        let has_3d = props.rotate_x.abs() > 0.01 || props.rotate_y.abs() > 0.01 || props.perspective >= 0.0;
        if !has_3d {
        if let Some(ref shadow) = self.style.box_shadow {
            let shadow_rect = Rect::from_xywh(
                shadow.offset_x, shadow.offset_y,
                layout.width, layout.height,
            );
            let shadow_rrect = skia_safe::RRect::new_rect_xy(
                shadow_rect, corner_radius, corner_radius,
            );
            let mut shadow_paint = paint_from_hex(&shadow.color);
            if shadow.blur > 0.0 {
                shadow_paint.set_mask_filter(skia_safe::MaskFilter::blur(
                    skia_safe::BlurStyle::Normal,
                    shadow.blur / 2.0,
                    false,
                ));
            }
            canvas.draw_rrect(shadow_rrect, &shadow_paint);
        }
        }

        // 2. Background
        if let Some(ref bg) = self.style.background {
            let bg_paint = paint_from_hex(bg);
            canvas.draw_rrect(rrect, &bg_paint);
        }

        // 3. Clip to rounded rect for children (only when card has a visible background)
        let should_clip = self.style.background.is_some();
        if should_clip {
            canvas.save();
            canvas.clip_rrect(rrect, skia_safe::ClipOp::Intersect, true);
        }

        // 4. Render children with animation support (with optional stagger)
        pipeline.render_children(canvas, &self.children as &dyn std::any::Any, layout, ctx, self.style.stagger)?;

        if should_clip {
            canvas.restore();
        }

        // 5. Border
        if let Some(ref border) = self.style.border {
            let mut border_paint = paint_from_hex(&border.color);
            border_paint.set_style(PaintStyle::Stroke);
            border_paint.set_stroke_width(border.width);
            canvas.draw_rrect(rrect, &border_paint);
        }

        // 5b. Gradient border
        if let Some(ref gb) = self.style.gradient_border {
            crate::draw_gradient_border(canvas, &rrect, gb);
        }

        Ok(())
    }

    fn measure(&self, constraints: &Constraints) -> (f32, f32) {
        let layout = self.layout(constraints);
        (layout.width, layout.height)
    }

    fn layout(&self, constraints: &Constraints) -> LayoutNode {
        let c = crate::flex::resolve_size_constraints(&self.size, constraints);
        match self.style.display_or(CardDisplay::Flex) {
            CardDisplay::Flex => layout_flex(&self.children, &self.style, &c),
            CardDisplay::Grid => layout_grid_for_card(self, &c),
        }
    }
}

fn layout_grid_for_card(card: &Card, constraints: &Constraints) -> LayoutNode {
    let grid_config = card.grid_config_owned();
    layout_grid_with_config(&card.children, &card.style, &grid_config, constraints)
}

impl Painter for Card {
    fn paint_content(&self, _canvas: &Canvas, _layout: &BoxLayout, _ctx: &PaintCtx) {}
}
