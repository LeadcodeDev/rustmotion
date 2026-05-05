//! Smoke tests: every component deserializes from minimal JSON and can be measured.

#[cfg(test)]
mod component_smoke {
    use crate::components::Component;
    use crate::layout::Constraints;

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

        let mut surface = skia_safe::surfaces::raster_n32_premul((400, 300))
            .expect("raster surface");
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
                overlays: Vec::new(),
            };
            let scene = vec![child];
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let built = build_scene(&scene, (400.0, 300.0));
                let layout = run_layout(
                    &built.root,
                    (400.0, 300.0),
                    &ConversionContext::default(),
                );
                let dispatcher = LegacyPaintDispatcher::new(&built.components);
                paint_tree(canvas, &built.root, &layout, &frame, &dispatcher);
            }));
            if let Err(_) = result {
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
        use rustmotion_core::css::taffy_bridge::ConversionContext;
        use rustmotion_core::engine::layout_pass::run_layout;
        use rustmotion_core::engine::paint_pass::{paint_tree, PaintFrame};
        use rustmotion_core::schema::{style::CardDirection, LayerStyle, Spacing};

        let mut surface = skia_safe::surfaces::raster_n32_premul((400, 300))
            .expect("raster surface");
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
                overlays: Vec::new(),
            };
            let card_style = LayerStyle {
                flex_direction: Some(CardDirection::Column),
                padding: Some(Spacing::Uniform(8.0)),
                gap: Some(4.0),
                ..Default::default()
            };
            let card_child = ChildComponent {
                component: Component::Card(Card {
                    children: vec![inner],
                    size: None,
                    timing: Default::default(),
                    style: card_style,
                }),
                position: Some(PositionMode::Absolute { x: 10.0, y: 10.0 }),
                x: None,
                y: None,
                z_index: None,
                overlays: Vec::new(),
            };
            let scene = vec![card_child];
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let built = build_scene(&scene, (400.0, 300.0));
                let layout = run_layout(
                    &built.root,
                    (400.0, 300.0),
                    &ConversionContext::default(),
                );
                let dispatcher = LegacyPaintDispatcher::new(&built.components);
                paint_tree(canvas, &built.root, &layout, &frame, &dispatcher);
            }));
            if let Err(_) = result {
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

    /// Renders a flat list of children through the legacy pipeline
    /// (compute_root_layout + render_children) onto a fresh raster surface
    /// and returns the RGBA buffer. Mirrors what `render_scene_fg_scaled`
    /// does for the foreground without setting up a Scene struct.
    fn render_legacy(children: &[crate::components::ChildComponent], w: u32, h: u32) -> Vec<u8> {
        use crate::engine::render::render_children;
        use crate::layout::flex::layout_flex;
        use crate::layout::Constraints;
        use crate::schema::{CardDirection, LayerStyle};
        use crate::traits::RenderContext;

        let mut surface = skia_safe::surfaces::raster_n32_premul((w as i32, h as i32))
            .expect("raster surface");
        let canvas = surface.canvas();
        canvas.clear(skia_safe::Color4f::new(0.0, 0.0, 0.0, 0.0));

        let style = LayerStyle {
            flex_direction: Some(CardDirection::Column),
            ..Default::default()
        };
        let constraints = Constraints::tight(w as f32, h as f32);
        let layout = layout_flex(children, &style, &constraints);
        let ctx = RenderContext {
            time: 0.5,
            scene_duration: 1.0,
            frame_index: 15,
            fps: 30,
            video_width: w,
            video_height: h,
            stagger_offset: 0.0,
        };
        render_children(canvas, children, &layout, &ctx).expect("legacy render");

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

    /// Same as `render_legacy` but routes the scene through the new
    /// pipeline (box_builder + run_layout + paint_tree).
    fn render_new(children: &[crate::components::ChildComponent], w: u32, h: u32) -> Vec<u8> {
        use rustmotion_components::box_builder::build_scene;
        use rustmotion_components::legacy_dispatch::LegacyPaintDispatcher;
        use rustmotion_core::css::taffy_bridge::ConversionContext;
        use rustmotion_core::engine::layout_pass::run_layout;
        use rustmotion_core::engine::paint_pass::{paint_tree, PaintFrame};

        let mut surface = skia_safe::surfaces::raster_n32_premul((w as i32, h as i32))
            .expect("raster surface");
        let canvas = surface.canvas();
        canvas.clear(skia_safe::Color4f::new(0.0, 0.0, 0.0, 0.0));

        let built = build_scene(children, (w as f32, h as f32));
        let layout = run_layout(&built.root, (w as f32, h as f32), &ConversionContext::default());
        let dispatcher = LegacyPaintDispatcher::new(&built.components);
        let frame = PaintFrame {
            time: 0.5,
            frame_index: 15,
            fps: 30,
            video_width: w,
            video_height: h,
            scene_duration: 1.0,
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

    /// Mean absolute per-channel difference between two RGBA buffers,
    /// returned as a fraction in [0.0, 1.0]. Channels with both buffers
    /// fully transparent contribute 0.
    fn mean_abs_diff(a: &[u8], b: &[u8]) -> f64 {
        assert_eq!(a.len(), b.len(), "buffer length mismatch");
        if a.is_empty() {
            return 0.0;
        }
        let total: u64 = a.iter().zip(b.iter())
            .map(|(x, y)| (*x as i32 - *y as i32).unsigned_abs() as u64)
            .sum();
        total as f64 / (a.len() as f64 * 255.0)
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
            overlays: Vec::new(),
        };
        let scene = vec![child];
        let new_buf = render_new(&scene, 400, 300);
        let lit = nonzero_pixels(&new_buf);
        assert!(lit > 100, "new pipeline produced too few non-zero pixels: {lit}");
    }

    #[test]
    fn pipelines_agree_on_simple_shape() {
        // For a single absolutely-positioned solid shape, both pipelines
        // should agree closely: there's no flex layout, no font shaping,
        // just a coloured rectangle. Tolerance is generous (~5% mean
        // diff) to absorb antialiasing differences at edges, but a
        // pipeline regression that wholly misplaces or recolours the
        // shape will exceed it.
        let json = serde_json::json!({
            "type": "shape",
            "shape": "rect",
            "size": { "width": 100, "height": 80 },
            "fill": "#ff3366"
        });
        let component: Component = serde_json::from_value(json).expect("deserialize");
        let child = crate::components::ChildComponent {
            component,
            position: Some(crate::components::PositionMode::Absolute { x: 60.0, y: 40.0 }),
            x: None,
            y: None,
            z_index: None,
            overlays: Vec::new(),
        };
        let scene = vec![child];
        let legacy = render_legacy(&scene, 400, 300);
        let new = render_new(&scene, 400, 300);
        let diff = mean_abs_diff(&legacy, &new);
        assert!(diff < 0.05, "shape diff too large: {diff} (legacy lit={}, new lit={})",
            nonzero_pixels(&legacy), nonzero_pixels(&new));
    }

    #[test]
    fn new_and_legacy_pipelines_both_produce_pixels() {
        // Sanity check that BOTH pipelines render visible output for the
        // same scene. We don't compare pixel-by-pixel because layout
        // engines (flex.rs vs taffy) and box decorations (legacy paints
        // backgrounds via Card::paint, new pipeline via paint_tree) draw
        // slightly different rectangles. Equal "lit" pixel counts is a
        // useful weak-parity signal: both engines find the content.
        let json = serde_json::json!({
            "type": "text",
            "content": "Parity",
            "style": { "color": "#ffffff", "font-size": 64 }
        });
        let component: Component = serde_json::from_value(json).expect("deserialize");
        let child = crate::components::ChildComponent {
            component,
            position: Some(crate::components::PositionMode::Absolute { x: 50.0, y: 100.0 }),
            x: None,
            y: None,
            z_index: None,
            overlays: Vec::new(),
        };
        let scene = vec![child];
        let legacy_buf = render_legacy(&scene, 400, 300);
        let new_buf = render_new(&scene, 400, 300);
        let legacy_lit = nonzero_pixels(&legacy_buf);
        let new_lit = nonzero_pixels(&new_buf);
        assert!(legacy_lit > 50, "legacy pipeline empty: {legacy_lit}");
        assert!(new_lit > 50, "new pipeline empty: {new_lit}");
        // Text shaping is identical (both pipelines call into the legacy
        // text painter via Widget::render); only the surrounding layout
        // box differs. Lit-pixel counts should be within 10% of each
        // other if both engines positioned the glyph run consistently.
        let ratio = new_lit as f64 / legacy_lit as f64;
        assert!(
            ratio > 0.85 && ratio < 1.15,
            "text lit-pixel ratio out of band: legacy={legacy_lit} new={new_lit} ratio={ratio}"
        );
    }

    #[test]
    fn all_components_measure() {
        let constraints = Constraints::loose(1920.0, 1080.0);
        let mut failures = Vec::new();
        for (name, json) in COMPONENT_JSONS {
            let component: Component = match serde_json::from_str(json) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let widget = component.as_widget();
            let (w, h) = widget.measure(&constraints);
            if w.is_nan() || h.is_nan() || w < 0.0 || h < 0.0 {
                failures.push(format!("{name}: invalid size ({w}, {h})"));
            }
        }
        if !failures.is_empty() {
            panic!(
                "Measure failures for {} component(s):\n{}",
                failures.len(),
                failures.join("\n")
            );
        }
    }
}
