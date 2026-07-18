//! Paint dispatcher for the new `paint_tree` pipeline. Bridges from the
//! taffy-laid-out `BoxNode` tree back to component-typed `Painter` impls.
//!
//! Naming kept as "legacy" for now to avoid churn in callers; this is the
//! sole dispatcher in use since all 51 components implement `Painter`.
//!
//! Containers (Card / Flex / Grid / Container / Positioned) are intentionally
//! skipped: paint_pass already paints their box decorations and recurses into
//! children — so calling the container's own `paint_content` would do nothing
//! anyway, and we save a no-op call.

use rustmotion_core::engine::animator::{resolve_props_for_effects, AnimatedProperties};
use rustmotion_core::engine::box_tree::NodeId;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::paint_pass::{PaintDispatcher, PaintFrame};
use rustmotion_core::traits::PaintCtx;
use skia_safe::Canvas;

use crate::{ChildComponent, Component};

/// Maps NodeIds to `ChildComponent`s and dispatches paint to their
/// `Painter::paint_content` impls.
pub struct LegacyPaintDispatcher<'a> {
    /// `components[id as usize]` is the component for `id`. Slot 0 is the
    /// synthetic root and is always `None`.
    components: &'a [Option<&'a ChildComponent>],
    /// Per-node container-stagger delay (indexed like `components`); empty
    /// when the caller doesn't carry stagger information.
    stagger_delays: &'a [f64],
}

impl<'a> LegacyPaintDispatcher<'a> {
    pub fn new(components: &'a [Option<&'a ChildComponent>]) -> Self {
        Self {
            components,
            stagger_delays: &[],
        }
    }

    /// Build from a [`BuiltScene`], carrying its stagger delays so internal
    /// animations shift by the same amount as the CSS overrides.
    pub fn for_scene(built: &'a crate::box_builder::BuiltScene<'a>) -> Self {
        Self {
            components: &built.components,
            stagger_delays: &built.stagger_delays,
        }
    }

    fn lookup(&self, id: NodeId) -> Option<&'a ChildComponent> {
        let idx = id as usize;
        self.components.get(idx).copied().flatten()
    }
}

impl<'a> PaintDispatcher for LegacyPaintDispatcher<'a> {
    fn dispatch(
        &self,
        canvas: &Canvas,
        payload: &(dyn std::any::Any + Send + Sync),
        _css: &rustmotion_core::css::CssStyle,
        layout: &BoxLayout,
        frame: &PaintFrame,
    ) {
        let Some(node_id) = payload.downcast_ref::<NodeId>() else {
            return;
        };
        let Some(child) = self.lookup(*node_id) else {
            return;
        };

        // Containers paint nothing of their own here — children are handled
        // recursively by paint_tree, and decorations were already painted.
        if is_container(&child.component) {
            return;
        }

        // Resolve animations for this leaf. Outer transforms (translate /
        // scale / rotate / opacity / blur) are applied by `paint_pass` via
        // the CSS overrides injected at box-tree build time, so we don't
        // wrap the canvas here. `props` is still needed for internal-only
        // fields like `draw_progress`, `stroke_width`, `visible_chars*`,
        // and `char_animation`. Timeline steps and container-stagger delays
        // are folded in so those internal animations shift exactly like the
        // CSS overrides do.
        let stagger_delay = self
            .stagger_delays
            .get(*node_id as usize)
            .copied()
            .unwrap_or(0.0);
        let props = match crate::box_builder::effective_effects(&child.component, stagger_delay) {
            Some(effects) => resolve_props_for_effects(&effects, frame.time, frame.scene_duration),
            None => AnimatedProperties::default(),
        };
        if props.opacity <= 0.0 {
            return;
        }

        let Some(painter) = child.component.as_painter() else {
            return;
        };

        canvas.save();
        canvas.translate((layout.x, layout.y));

        let paint_ctx = PaintCtx {
            time: frame.time,
            scene_duration: frame.scene_duration,
            frame_index: frame.frame_index,
            fps: frame.fps,
            video_width: frame.video_width,
            video_height: frame.video_height,
            stagger_offset: stagger_delay,
        };
        let local = BoxLayout {
            x: 0.0,
            y: 0.0,
            width: layout.width,
            height: layout.height,
            ..Default::default()
        };
        painter.paint_content(canvas, &local, &props, &paint_ctx);

        canvas.restore();
    }
}

fn is_container(c: &Component) -> bool {
    matches!(
        c,
        Component::Card(_)
            | Component::Flex(_)
            | Component::Grid(_)
            | Component::Container(_)
            | Component::Positioned(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::box_builder::build_scene;
    use crate::shape::Shape;
    use crate::PositionMode;
    use rustmotion_core::css::style::{CssStyle, Size as CSize};
    use rustmotion_core::css::taffy_bridge::ConversionContext;
    use rustmotion_core::css::units::LengthPercentage as CLP;
    use rustmotion_core::engine::box_tree::BoxKind;
    use rustmotion_core::engine::layout_pass::run_layout;
    use rustmotion_core::schema::ShapeType;
    use std::sync::Arc;

    fn shape_child(w: f32, h: f32, x: f32, y: f32) -> ChildComponent {
        ChildComponent {
            component: Component::Shape(Shape {
                shape: ShapeType::Rect,
                text: None,
                timing: Default::default(),
                style: CssStyle {
                    width: Some(CSize::Length(CLP::Px(w))),
                    height: Some(CSize::Length(CLP::Px(h))),
                    ..Default::default()
                },
                timeline: Vec::new(),
                stagger: None,
                fill: Some(rustmotion_core::schema::Fill::Solid("#ff0000".into())),
                stroke: None,
            }),
            position: Some(PositionMode::Absolute { x, y }),
            x: None,
            y: None,
            z_index: None,
        }
    }

    #[test]
    fn dispatch_runs_paint_content_on_leaf() {
        let scene = vec![shape_child(50.0, 30.0, 10.0, 20.0)];
        let built = build_scene(&scene, (200.0, 200.0));
        let layout = run_layout(&built.root, (200.0, 200.0), &ConversionContext::default());

        // Sanity: the only component slot at idx 1 points to the shape.
        assert!(built.components[0].is_none());
        assert!(built.components[1].is_some());

        // Build a Skia raster surface and run a paint pass against the
        // dispatcher — this just exercises the dispatcher hook end-to-end.
        let mut surface =
            skia_safe::surfaces::raster_n32_premul((200, 200)).expect("raster surface");
        let canvas = surface.canvas();
        let dispatcher = LegacyPaintDispatcher::new(&built.components);
        let frame = PaintFrame {
            time: 0.0,
            frame_index: 0,
            fps: 30,
            video_width: 200,
            video_height: 200,
            scene_duration: 1.0,
        };
        rustmotion_core::engine::paint_pass::paint_tree(
            canvas,
            &built.root,
            &layout,
            &frame,
            &dispatcher,
        );

        // Read back the pixel at the centre of the shape (10+25, 20+15)=(35,35)
        // and assert it's red-ish — confirms paint_content painted.
        let snapshot = surface.image_snapshot();
        let mut buf = [0u8; 4];
        let info = skia_safe::ImageInfo::new(
            (1, 1),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let read_ok = snapshot.read_pixels(
            &info,
            &mut buf,
            4,
            skia_safe::IPoint::new(35, 35),
            skia_safe::image::CachingHint::Disallow,
        );
        assert!(read_ok, "pixel read should succeed");
        // Red channel should dominate (#ff0000).
        assert!(buf[0] > 200, "expected red, got rgba {:?}", buf);
        assert!(buf[1] < 50, "green should be low, got rgba {:?}", buf);
        assert!(buf[2] < 50, "blue should be low, got rgba {:?}", buf);
    }

    #[test]
    fn card_background_painted_with_red_shape_inside() {
        // Card 100×80 at (40,30), green background, contains a red 30×20 shape
        // absolutely positioned at (10,10) inside the card.
        use crate::card::Card;

        use rustmotion_core::css::style::{Background, Color};

        let red_shape = ChildComponent {
            component: Component::Shape(Shape {
                shape: ShapeType::Rect,
                text: None,
                timing: Default::default(),
                style: CssStyle {
                    width: Some(CSize::Length(CLP::Px(30.0))),
                    height: Some(CSize::Length(CLP::Px(20.0))),
                    ..Default::default()
                },
                timeline: Vec::new(),
                stagger: None,
                fill: Some(rustmotion_core::schema::Fill::Solid("#ff0000".into())),
                stroke: None,
            }),
            position: Some(PositionMode::Absolute { x: 10.0, y: 10.0 }),
            x: None,
            y: None,
            z_index: None,
        };

        let card = ChildComponent {
            component: Component::Card(Card {
                children: vec![red_shape],
                timing: Default::default(),
                style: CssStyle {
                    width: Some(CSize::Length(CLP::Px(100.0))),
                    height: Some(CSize::Length(CLP::Px(80.0))),
                    background: Some(Background::Color(Color::String("#00ff00".into()))),
                    ..Default::default()
                },
                timeline: Vec::new(),
                stagger: None,
            }),
            position: Some(PositionMode::Absolute { x: 40.0, y: 30.0 }),
            x: None,
            y: None,
            z_index: None,
        };

        let scene = vec![card];
        let built = build_scene(&scene, (200.0, 200.0));
        let layout = run_layout(&built.root, (200.0, 200.0), &ConversionContext::default());

        let mut surface =
            skia_safe::surfaces::raster_n32_premul((200, 200)).expect("raster surface");
        let canvas = surface.canvas();
        let dispatcher = LegacyPaintDispatcher::new(&built.components);
        let frame = PaintFrame {
            time: 0.0,
            frame_index: 0,
            fps: 30,
            video_width: 200,
            video_height: 200,
            scene_duration: 1.0,
        };
        rustmotion_core::engine::paint_pass::paint_tree(
            canvas,
            &built.root,
            &layout,
            &frame,
            &dispatcher,
        );

        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (1, 1),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let read = |x: i32, y: i32| -> [u8; 4] {
            let mut buf = [0u8; 4];
            assert!(snapshot.read_pixels(
                &info,
                &mut buf,
                4,
                skia_safe::IPoint::new(x, y),
                skia_safe::image::CachingHint::Disallow,
            ));
            buf
        };

        // Card background area: bottom-right corner of the card (well away
        // from the red shape at (10,10)+(30,20)). Card spans x∈[40,140],
        // y∈[30,110]. Pick (130, 100) — green.
        let bg = read(130, 100);
        assert!(bg[1] > 200, "expected green card bg, got {:?}", bg);
        assert!(bg[0] < 50, "red should be low at bg, got {:?}", bg);

        // Red shape area: shape spans (50,40)→(80,60). Pick centre (65, 50).
        let fg = read(65, 50);
        assert!(fg[0] > 200, "expected red shape, got {:?}", fg);
        assert!(fg[1] < 50, "green should be low at shape, got {:?}", fg);
    }

    #[test]
    fn fade_in_preset_drives_alpha_through_dispatcher() {
        // A red shape with a `FadeIn` preset over 0.5s, sampled at two points:
        //   t=0.05s — early in the curve, opacity should be near zero
        //   t=0.5s  — at the end of the curve, opacity should be ~1
        // The dispatcher must wire animator output into the canvas alpha,
        // otherwise both samples render fully opaque and the test fails.
        use crate::shape::Shape;
        use rustmotion_core::schema::{AnimationEffect, AnimationTiming, ShapeType};

        let make_scene = || {
            let shape = ChildComponent {
                component: Component::Shape(Shape {
                    shape: ShapeType::Rect,
                    text: None,
                    timing: Default::default(),
                    style: CssStyle {
                        width: Some(CSize::Length(CLP::Px(100.0))),
                        height: Some(CSize::Length(CLP::Px(100.0))),
                        animation: vec![AnimationEffect::FadeIn(AnimationTiming {
                            duration: 0.5,
                            delay: 0.0,
                            repeat: false,
                            overshoot: None,
                        })],
                        ..Default::default()
                    },
                    timeline: Vec::new(),
                    stagger: None,
                    fill: Some(rustmotion_core::schema::Fill::Solid("#ff0000".into())),
                    stroke: None,
                }),
                position: Some(PositionMode::Absolute { x: 0.0, y: 0.0 }),
                x: None,
                y: None,
                z_index: None,
            };
            vec![shape]
        };

        let sample_red_at = |time: f64| -> u8 {
            let scene = make_scene();
            // Build the box tree with an animation context so the FadeIn
            // preset is resolved into CSS overrides (transform/opacity) on
            // each box. paint_pass then applies those during painting.
            let built = crate::box_builder::build_scene_with_anim(
                &scene,
                (200.0, 200.0),
                crate::box_builder::BuildAnimationCtx {
                    time,
                    scene_duration: 1.0,
                },
            );
            let layout = run_layout(&built.root, (200.0, 200.0), &ConversionContext::default());
            let mut surface =
                skia_safe::surfaces::raster_n32_premul((200, 200)).expect("raster surface");
            let canvas = surface.canvas();
            canvas.clear(skia_safe::Color::BLACK);
            let dispatcher = LegacyPaintDispatcher::new(&built.components);
            let frame = PaintFrame {
                time,
                frame_index: 0,
                fps: 30,
                video_width: 200,
                video_height: 200,
                scene_duration: 1.0,
            };
            rustmotion_core::engine::paint_pass::paint_tree(
                canvas,
                &built.root,
                &layout,
                &frame,
                &dispatcher,
            );
            let snap = surface.image_snapshot();
            let info = skia_safe::ImageInfo::new(
                (1, 1),
                skia_safe::ColorType::RGBA8888,
                skia_safe::AlphaType::Premul,
                None,
            );
            let mut buf = [0u8; 4];
            assert!(snap.read_pixels(
                &info,
                &mut buf,
                4,
                skia_safe::IPoint::new(50, 50),
                skia_safe::image::CachingHint::Disallow,
            ));
            buf[0]
        };

        let early = sample_red_at(0.05);
        let late = sample_red_at(0.5);
        assert!(
            late > early + 50,
            "FadeIn should produce a clearly higher red at t=0.5 than at t=0.05 \
             (early={}, late={})",
            early,
            late,
        );
        assert!(
            late > 200,
            "at t=duration the shape should be ~fully opaque red, got {}",
            late
        );
        assert!(
            early < 150,
            "at t=0.05 the shape should be mostly transparent, got {}",
            early
        );
    }

    #[test]
    fn dispatch_skips_unknown_payloads() {
        let dispatcher = LegacyPaintDispatcher::new(&[]);
        let mut surface = skia_safe::surfaces::raster_n32_premul((10, 10)).unwrap();
        let canvas = surface.canvas();
        let css = rustmotion_core::css::CssStyle::default();
        let layout = BoxLayout {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
            ..Default::default()
        };
        let frame = PaintFrame {
            time: 0.0,
            frame_index: 0,
            fps: 30,
            video_width: 10,
            video_height: 10,
            scene_duration: 1.0,
        };
        // Wrong payload type — must not panic.
        let bogus: Arc<dyn std::any::Any + Send + Sync> = Arc::new(42i64);
        // Reach into the dispatcher trait method.
        // (BoxKind::Component is just a marker here; we drive dispatch directly.)
        dispatcher.dispatch(canvas, bogus.as_ref(), &css, &layout, &frame);
        let _ = BoxKind::Container; // touch import
    }
}
