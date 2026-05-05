use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Rect};

use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::paint_from_hex;
use rustmotion_core::layout::{layout_flex, Constraints, LayoutNode};
use rustmotion_core::schema::{LayerStyle, SizeDimension};
use rustmotion_core::traits::{
    Border, Bordered, BorderedMut, Container, FlexConfig, FlexContainer,
    FlexContainerMut, PaintCtx, Painter, RenderContext, Rounded, RoundedMut, Shadow, Shadowed,
    ShadowedMut, TimingConfig, Widget,
};

use crate::ChildComponent;

/// Flex size — each dimension can be fixed or auto.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FlexSize {
    pub width: SizeDimension,
    pub height: SizeDimension,
}

/// Flex container — children are positioned via flexbox layout.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Flex {
    #[serde(default)]
    pub children: Vec<ChildComponent>,
    #[serde(default)]
    pub size: Option<FlexSize>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

rustmotion_core::impl_traits!(Flex {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl Container for Flex {}

impl FlexContainer for Flex {
    fn flex_config(&self) -> &FlexConfig {
        // We need to construct a FlexConfig from LayerStyle on the fly.
        // Since FlexContainer returns a reference, we use a thread-local for the config.
        // This is a workaround - the layout code will be updated to read from LayerStyle directly.
        unreachable!("Use style directly for flex config")
    }
}

impl FlexContainerMut for Flex {
    fn flex_config_mut(&mut self) -> &mut FlexConfig {
        unreachable!("Use style directly for flex config")
    }
}

impl Bordered for Flex {
    fn border(&self) -> Option<&Border> {
        // Border in LayerStyle uses CardBorder, but trait uses Border.
        // They have the same shape. We use unsafe transmute or just return None and handle in render.
        None // Handled directly in render via self.style.border
    }
}

impl BorderedMut for Flex {
    fn set_border(&mut self, _border: Option<Border>) {}
}

impl Rounded for Flex {
    fn corner_radius(&self) -> f32 {
        self.style.border_radius_or(12.0)
    }
}

impl RoundedMut for Flex {
    fn set_corner_radius(&mut self, radius: f32) {
        self.style.border_radius = Some(radius);
    }
}

impl Shadowed for Flex {
    fn shadow(&self) -> Option<&Shadow> {
        // CardShadow and Shadow have the same shape
        None // Handled directly in render via self.style.box_shadow
    }
}

impl ShadowedMut for Flex {
    fn set_shadow(&mut self, _shadow: Option<Shadow>) {}
}

impl rustmotion_core::traits::Backgrounded for Flex {
    fn background(&self) -> Option<&str> {
        self.style.background.as_deref()
    }
}

impl rustmotion_core::traits::BackgroundedMut for Flex {
    fn set_background(&mut self, bg: Option<String>) {
        self.style.background = bg;
    }
}

impl rustmotion_core::traits::Clipped for Flex {
    fn clip(&self) -> bool {
        true
    }
}

impl Widget for Flex {
    fn render(&self, canvas: &Canvas, layout: &LayoutNode, ctx: &RenderContext, _props: &rustmotion_core::engine::animator::AnimatedProperties, pipeline: &dyn rustmotion_core::traits::RenderPipeline) -> Result<()> {
        let corner_radius = self.style.border_radius_or(12.0);
        let rect = Rect::from_xywh(0.0, 0.0, layout.width, layout.height);
        let rrect = skia_safe::RRect::new_rect_xy(rect, corner_radius, corner_radius);

        // 1. Shadow
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

        // 2. Background
        if let Some(ref bg) = self.style.background {
            let bg_paint = paint_from_hex(bg);
            canvas.draw_rrect(rrect, &bg_paint);
        }

        // 3. Clip to rounded rect for children
        canvas.save();
        canvas.clip_rrect(rrect, skia_safe::ClipOp::Intersect, true);

        // 4. Render children with animation support (with optional stagger)
        pipeline.render_children(canvas, &self.children as &dyn std::any::Any, layout, ctx, self.style.stagger)?;

        canvas.restore();

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
        let c = resolve_size_constraints(&self.size, constraints);
        layout_flex(&self.children, &self.style, &c)
    }
}

impl Painter for Flex {
    fn paint_content(&self, _canvas: &Canvas, _layout: &BoxLayout, _ctx: &PaintCtx) {}
}

/// Resolve FlexSize dimensions into constraints.
///
/// - `Fixed(v)` → tight (min=max=v)
/// - `Auto` → loose (min=0, max=parent.max) — fits content
/// - `None` / `Percent` → inherits parent constraints unchanged
pub(crate) fn resolve_size_constraints(size: &Option<FlexSize>, constraints: &Constraints) -> Constraints {
    let (w_min, w_max) = match size.as_ref().map(|s| &s.width) {
        Some(SizeDimension::Fixed(v)) => (*v, *v),
        Some(SizeDimension::Auto) => (0.0, constraints.max_width),
        _ => (constraints.min_width, constraints.max_width),
    };
    let (h_min, h_max) = match size.as_ref().map(|s| &s.height) {
        Some(SizeDimension::Fixed(v)) => (*v, *v),
        Some(SizeDimension::Auto) => (0.0, constraints.max_height),
        _ => (constraints.min_height, constraints.max_height),
    };
    Constraints {
        min_width: w_min,
        max_width: w_max,
        min_height: h_min,
        max_height: h_max,
    }
}
