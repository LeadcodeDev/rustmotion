//! Smoke tests: every component deserializes from minimal JSON and can be measured.

#[cfg(test)]
mod component_smoke {
    use crate::components::Component;

    /// Minimal JSON for each component type.
    const COMPONENT_JSONS: &[(&str, &str)] = &[
        ("text", r#"{"type":"text","content":"hello"}"#),
        ("shape", r#"{"type":"shape","shape":"rect"}"#),
        ("icon", r#"{"type":"icon","icon":"check"}"#),
        ("svg", r#"{"type":"svg","content":"<svg></svg>"}"#),
        ("counter", r#"{"type":"counter","from":0,"to":100}"#),
        ("cursor", r#"{"type":"cursor"}"#),
        ("codeblock", r#"{"type":"codeblock","code":"fn main() {}"}"#),
        ("badge", r#"{"type":"badge","text":"New"}"#),
        ("callout", r#"{"type":"callout","text":"Hello"}"#),
        (
            "chart",
            r#"{"type":"chart","chart_type":"bar","data":[{"value":10}]}"#,
        ),
        ("countdown", r#"{"type":"countdown","seconds":60}"#),
        ("divider", r#"{"type":"divider"}"#),
        ("gauge", r#"{"type":"gauge","value":50}"#),
        (
            "gradient_text",
            r##"{"type":"gradient_text","content":"hello","colors":["#FF0000","#0000FF"]}"##,
        ),
        ("heatmap", r#"{"type":"heatmap","data":[[1,2],[3,4]]}"#),
        ("kbd", r#"{"type":"kbd","key":"Ctrl+C"}"#),
        (
            "list",
            r#"{"type":"list","items":[{"text":"one"},{"text":"two"}]}"#,
        ),
        ("marquee", r#"{"type":"marquee","content":"scrolling"}"#),
        (
            "particle",
            r#"{"type":"particle","particle_type":"confetti"}"#,
        ),
        ("progress", r#"{"type":"progress","value":0.5}"#),
        (
            "qr_code",
            r#"{"type":"qr_code","content":"https://example.com"}"#,
        ),
        ("rating", r#"{"type":"rating","value":3.5}"#),
        ("skeleton", r#"{"type":"skeleton"}"#),
        ("slider", r#"{"type":"slider","value":50}"#),
        ("sparkline", r#"{"type":"sparkline","data":[1,2,3,4,5]}"#),
        ("stat", r#"{"type":"stat","value":"42","label":"Users"}"#),
        (
            "stepper",
            r#"{"type":"stepper","steps":[{"label":"Step 1"},{"label":"Step 2"}]}"#,
        ),
        ("switch", r#"{"type":"switch"}"#),
        (
            "rich_text",
            r#"{"type":"rich_text","spans":[{"text":"hello"}]}"#,
        ),
        (
            "table",
            r#"{"type":"table","headers":["A","B"],"rows":[["1","2"]]}"#,
        ),
        (
            "tag_cloud",
            r#"{"type":"tag_cloud","tags":[{"text":"rust","weight":1.0}]}"#,
        ),
        (
            "terminal",
            r#"{"type":"terminal","lines":[{"text":"$ echo hello"}]}"#,
        ),
        (
            "timeline",
            r#"{"type":"timeline","steps":[{"label":"Start"}]}"#,
        ),
        (
            "treemap",
            r#"{"type":"treemap","data":[{"value":10,"label":"A"}]}"#,
        ),
        // Containers
        (
            "flex",
            r#"{"type":"flex","children":[{"type":"text","content":"hi"}]}"#,
        ),
        (
            "grid",
            r#"{"type":"grid","children":[{"type":"text","content":"hi"}]}"#,
        ),
        (
            "card",
            r#"{"type":"card","children":[{"type":"text","content":"hi"}]}"#,
        ),
        (
            "container",
            r#"{"type":"container","children":[{"type":"text","content":"hi"}]}"#,
        ),
        (
            "positioned",
            r#"{"type":"positioned","children":[{"type":"text","content":"hi","x":0,"y":0}]}"#,
        ),
    ];

    #[test]
    fn all_components_deserialize() {
        let mut failures = Vec::new();
        for (name, json) in COMPONENT_JSONS {
            match serde_json::from_str::<Component>(json) {
                Ok(_) => {}
                Err(e) => failures.push(format!("{name}: {e}")),
            }
        }
        if !failures.is_empty() {
            panic!(
                "Failed to deserialize {} component(s):\n{}",
                failures.len(),
                failures.join("\n")
            );
        }
    }

    #[test]
    fn all_components_paint_through_new_pipeline() {
        // Build a tiny scene per component (one absolutely-positioned child)
        // and run it through box_builder + run_layout + paint_tree with the
        // LegacyPaintDispatcher. The point is to catch panics in the bridge:
        // missing intrinsic measurers, misclassified containers, downcast
        // failures, etc. A non-zero pixel readback isn't required.
        use crate::components::{ChildComponent, PositionMode};
        use rustmotion_components::box_builder::build_scene;
        use rustmotion_components::legacy_dispatch::LegacyPaintDispatcher;
        use rustmotion_core::css::taffy_bridge::ConversionContext;
        use rustmotion_core::engine::layout_pass::run_layout;
        use rustmotion_core::engine::paint_pass::{paint_tree, PaintFrame};

        let mut surface =
            skia_safe::surfaces::raster_n32_premul((400, 300)).expect("raster surface");
        let canvas = surface.canvas();
        let frame = PaintFrame {
            time: 0.5,
            frame_index: 15,
            fps: 30,
            video_width: 400,
            video_height: 300,
            scene_duration: 1.0,
        };

        let mut failures = Vec::new();
        for (name, json) in COMPONENT_JSONS {
            let component: Component = match serde_json::from_str(json) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let child = ChildComponent {
                component,
                position: Some(PositionMode::Absolute { x: 10.0, y: 10.0 }),
                x: None,
                y: None,
                z_index: None,
            };
            let scene = vec![child];
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let built = build_scene(&scene, (400.0, 300.0));
                let layout = run_layout(&built.root, (400.0, 300.0), &ConversionContext::default());
                let dispatcher = LegacyPaintDispatcher::new(&built.components);
                paint_tree(canvas, &built.root, &layout, &frame, &dispatcher);
            }));
            if result.is_err() {
                failures.push(name);
            }
        }
        if !failures.is_empty() {
            panic!(
                "New pipeline panicked on {} component(s): {:?}",
                failures.len(),
                failures
            );
        }
    }

    #[test]
    fn all_components_paint_inside_flex_card() {
        // Same idea as `all_components_paint_through_new_pipeline` but each
        // component is wrapped in a flex card with no fixed size, forcing
        // taffy to use the component's intrinsic measurer (or its size field)
        // to determine the parent's dimensions. This catches regressions
        // where a component reports zero size and the card collapses.
        use crate::components::{ChildComponent, PositionMode};
        use rustmotion_components::box_builder::build_scene;
        use rustmotion_components::card::Card;
        use rustmotion_components::legacy_dispatch::LegacyPaintDispatcher;
        use rustmotion_core::css::style::{CssStyle, Edges, FlexDirection, Gap};
        use rustmotion_core::css::taffy_bridge::ConversionContext;
        use rustmotion_core::css::units::LengthPercentage;
        use rustmotion_core::engine::layout_pass::run_layout;
        use rustmotion_core::engine::paint_pass::{paint_tree, PaintFrame};

        let mut surface =
            skia_safe::surfaces::raster_n32_premul((400, 300)).expect("raster surface");
        let canvas = surface.canvas();
        let frame = PaintFrame {
            time: 0.0,
            frame_index: 0,
            fps: 30,
            video_width: 400,
            video_height: 300,
            scene_duration: 1.0,
        };

        let mut failures = Vec::new();
        for (name, json) in COMPONENT_JSONS {
            let component: Component = match serde_json::from_str(json) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let inner = ChildComponent {
                component,
                position: None,
                x: None,
                y: None,
                z_index: None,
            };
            let card_style = CssStyle {
                flex_direction: Some(FlexDirection::Column),
                padding: Some(Edges::Uniform(LengthPercentage::Px(8.0))),
                gap: Some(Gap::Uniform(LengthPercentage::Px(4.0))),
                ..Default::default()
            };
            let card_child = ChildComponent {
                component: Component::Card(Card {
                    children: vec![inner],
                    timing: Default::default(),
                    style: card_style,
                    timeline: Vec::new(),
                    stagger: None,
                }),
                position: Some(PositionMode::Absolute { x: 10.0, y: 10.0 }),
                x: None,
                y: None,
                z_index: None,
            };
            let scene = vec![card_child];
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let built = build_scene(&scene, (400.0, 300.0));
                let layout = run_layout(&built.root, (400.0, 300.0), &ConversionContext::default());
                let dispatcher = LegacyPaintDispatcher::new(&built.components);
                paint_tree(canvas, &built.root, &layout, &frame, &dispatcher);
            }));
            if result.is_err() {
                failures.push(name);
            }
        }
        if !failures.is_empty() {
            panic!(
                "New pipeline panicked for {} component(s) inside flex card: {:?}",
                failures.len(),
                failures
            );
        }
    }

    /// Routes the scene through the new pipeline
    /// (box_builder + run_layout + paint_tree).
    fn render_new(children: &[crate::components::ChildComponent], w: u32, h: u32) -> Vec<u8> {
        render_new_at(children, w, h, 0.5, 1.0)
    }

    /// Render the scene through the new pipeline at a specific time within a
    /// scene of `scene_duration` seconds. Resolves animations into CSS via
    /// `build_scene_with_anim` so the box tree carries transform/opacity/
    /// filter overrides from the animator.
    fn render_new_at(
        children: &[crate::components::ChildComponent],
        w: u32,
        h: u32,
        time: f64,
        scene_duration: f64,
    ) -> Vec<u8> {
        use rustmotion_components::box_builder::{build_scene_with_anim, BuildAnimationCtx};
        use rustmotion_components::legacy_dispatch::LegacyPaintDispatcher;
        use rustmotion_core::css::taffy_bridge::ConversionContext;
        use rustmotion_core::engine::layout_pass::run_layout;
        use rustmotion_core::engine::paint_pass::{paint_tree, PaintFrame};

        let mut surface =
            skia_safe::surfaces::raster_n32_premul((w as i32, h as i32)).expect("raster surface");
        let canvas = surface.canvas();
        canvas.clear(skia_safe::Color4f::new(0.0, 0.0, 0.0, 0.0));

        let built = build_scene_with_anim(
            children,
            (w as f32, h as f32),
            BuildAnimationCtx {
                time,
                scene_duration,
            },
        );
        let layout = run_layout(
            &built.root,
            (w as f32, h as f32),
            &ConversionContext::default(),
        );
        let dispatcher = LegacyPaintDispatcher::new(&built.components);
        let frame = PaintFrame {
            time,
            frame_index: (time * 30.0) as u32,
            fps: 30,
            video_width: w,
            video_height: h,
            scene_duration,
        };
        paint_tree(canvas, &built.root, &layout, &frame, &dispatcher);

        let row_bytes = w as usize * 4;
        let mut pixels = vec![0u8; row_bytes * h as usize];
        let info = skia_safe::ImageInfo::new(
            (w as i32, h as i32),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        surface.read_pixels(&info, &mut pixels, row_bytes, (0, 0));
        pixels
    }

    /// Counts non-transparent pixels in an RGBA buffer (alpha > 0).
    fn nonzero_pixels(buf: &[u8]) -> usize {
        buf.chunks_exact(4).filter(|p| p[3] > 0).count()
    }

    #[test]
    fn new_pipeline_produces_pixels_for_text_in_card() {
        // The new pipeline should render at least *something* when given a
        // non-trivial scene (a card containing centered text). This is a
        // coarse smoke test — it doesn't assert pixel-perfect parity with
        // legacy (taffy and the old flex.rs differ on edge cases), only
        // that the new pipeline isn't producing a fully-blank canvas.
        let json = serde_json::json!({
            "type": "card",
            "style": { "padding": 24, "background": "#1a1a2e" },
            "children": [
                { "type": "text", "content": "Hello", "style": { "color": "#ffffff", "font-size": 48 } }
            ]
        });
        let component: Component = serde_json::from_value(json).expect("deserialize");
        let child = crate::components::ChildComponent {
            component,
            position: None,
            x: None,
            y: None,
            z_index: None,
        };
        let scene = vec![child];
        let new_buf = render_new(&scene, 400, 300);
        let lit = nonzero_pixels(&new_buf);
        assert!(
            lit > 100,
            "new pipeline produced too few non-zero pixels: {lit}"
        );
    }

    #[test]
    fn fade_in_attenuates_pixels_in_new_pipeline() {
        // A red shape with FadeIn over 1.0s. At t=0.05 the shape is ~5%
        // opacity, at t=0.95 it's ~95%. The new pipeline must drive opacity
        // through the box-tree CSS overrides, so the late frame should have
        // significantly more "lit" red than the early frame. Without the
        // animator → CssStyle bridge, both frames would look identical.
        let json = serde_json::json!({
            "type": "shape",
            "shape": "rect",
            "fill": "#ff3366",
            "style": {
                "width": "100px",
                "height": "80px",
                "animation": [{ "name": "fade_in", "duration": 1.0 }]
            }
        });
        let component: Component = serde_json::from_value(json).expect("deserialize");
        let child = crate::components::ChildComponent {
            component,
            position: Some(crate::components::PositionMode::Absolute { x: 60.0, y: 40.0 }),
            x: None,
            y: None,
            z_index: None,
        };
        let scene = vec![child];
        let early = render_new_at(&scene, 400, 300, 0.05, 1.0);
        let late = render_new_at(&scene, 400, 300, 0.95, 1.0);
        // Total red intensity (sum of red channel) — alpha-attenuated pixels
        // contribute less to this sum even when premul keeps the count up.
        let red_sum: fn(&[u8]) -> u64 = |buf| buf.chunks_exact(4).map(|p| p[0] as u64).sum();
        let early_red = red_sum(&early);
        let late_red = red_sum(&late);
        assert!(
            late_red > early_red * 3,
            "FadeIn did not amplify red over time (early={early_red}, late={late_red})"
        );
    }

    /// X centroid of lit pixels, weighted by red intensity.
    /// Returns None when no pixels are lit.
    fn red_centroid_x(buf: &[u8], width: u32) -> Option<f64> {
        let mut sum_wx = 0.0_f64;
        let mut sum_w = 0.0_f64;
        for (i, p) in buf.chunks_exact(4).enumerate() {
            let x = (i as u32 % width) as f64;
            let w = p[0] as f64;
            sum_wx += w * x;
            sum_w += w;
        }
        if sum_w == 0.0 {
            None
        } else {
            Some(sum_wx / sum_w)
        }
    }

    #[test]
    fn scale_in_grows_lit_area_in_new_pipeline() {
        // ScaleIn animates `scale` from 0.0 → 1.08 → 1.0 and opacity 0 → 1.
        // At t=0.05 the shape is microscopic; at t=0.95 it's at full size.
        // This exercises the transform: scale(...) path of the CSS bridge.
        let json = serde_json::json!({
            "type": "shape",
            "shape": "rect",
            "fill": "#ff3366",
            "style": {
                "width": "100px",
                "height": "80px",
                "animation": [{ "name": "scale_in", "duration": 1.0 }]
            }
        });
        let component: Component = serde_json::from_value(json).expect("deserialize");
        let child = crate::components::ChildComponent {
            component,
            position: Some(crate::components::PositionMode::Absolute { x: 150.0, y: 110.0 }),
            x: None,
            y: None,
            z_index: None,
        };
        let scene = vec![child];
        let early = render_new_at(&scene, 400, 300, 0.05, 1.0);
        let late = render_new_at(&scene, 400, 300, 0.95, 1.0);
        let early_lit = nonzero_pixels(&early);
        let late_lit = nonzero_pixels(&late);
        // At t=0.95 the box is ~100x80 = 8000 px. At t=0.05, scale<0.1 and
        // opacity<0.2, so almost nothing is drawn. The bridge is doing its
        // job if the late frame fills out an order of magnitude more area.
        assert!(
            late_lit > early_lit * 10,
            "ScaleIn did not grow over time (early_lit={early_lit}, late_lit={late_lit})"
        );
        assert!(late_lit > 5000, "ScaleIn final frame too small: {late_lit}");
    }

    #[test]
    fn slide_in_left_translates_centroid_in_new_pipeline() {
        // SlideInLeft animates position.x from -200 → 0 (relative to the
        // shape's anchor). The new pipeline must turn that into
        // transform: translateX(...) so the rendered centroid moves
        // rightward with time.
        let json = serde_json::json!({
            "type": "shape",
            "shape": "rect",
            "fill": "#ff3366",
            "style": {
                "width": "60px",
                "height": "60px",
                "animation": [{ "name": "slide_in_left", "duration": 1.0 }]
            }
        });
        let component: Component = serde_json::from_value(json).expect("deserialize");
        let child = crate::components::ChildComponent {
            component,
            position: Some(crate::components::PositionMode::Absolute { x: 250.0, y: 120.0 }),
            x: None,
            y: None,
            z_index: None,
        };
        let scene = vec![child];
        let early = render_new_at(&scene, 500, 300, 0.05, 1.0);
        let late = render_new_at(&scene, 500, 300, 0.95, 1.0);
        let early_cx = red_centroid_x(&early, 500).expect("early frame has lit pixels");
        let late_cx = red_centroid_x(&late, 500).expect("late frame has lit pixels");
        // EaseOutCubic at t=0.05 leaves the shape close to the start (-200
        // offset → centroid near x≈80). At t=0.95 it's near origin
        // (centroid ≈ 280). A delta of at least 100 px guarantees the
        // translate is being honoured by paint_pass.
        let dx = late_cx - early_cx;
        assert!(
            dx > 100.0,
            "SlideInLeft centroid did not move right (early_cx={early_cx:.1}, late_cx={late_cx:.1}, dx={dx:.1})"
        );
    }
}
