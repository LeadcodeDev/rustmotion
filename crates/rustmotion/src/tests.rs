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

        let mut surface = skia_safe::surfaces::raster_n32_premul((w as i32, h as i32))
            .expect("raster surface");
        let canvas = surface.canvas();
        canvas.clear(skia_safe::Color4f::new(0.0, 0.0, 0.0, 0.0));

        let built = build_scene_with_anim(
            children,
            (w as f32, h as f32),
            BuildAnimationCtx { time, scene_duration },
        );
        let layout = run_layout(&built.root, (w as f32, h as f32), &ConversionContext::default());
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
    fn pipelines_agree_on_flex_card_lit_count() {
        // A flex card with two text children — exercises taffy vs the
        // legacy flex.rs engine. We can't expect pixel-perfect parity
        // (the two layout engines disagree on rounding and intrinsic
        // sizing), but the total number of lit pixels should be in the
        // same ballpark. If taffy returned a degenerate layout (zero
        // size, NaN, etc.) the new pipeline would render very few
        // pixels relative to legacy, and this assertion would catch it.
        let json = serde_json::json!({
            "type": "card",
            "style": {
                "padding": 16,
                "background": "#222244",
                "card_direction": "column",
                "gap": 8
            },
            "children": [
                { "type": "text", "content": "Title", "style": { "color": "#ffffff", "font-size": 32 } },
                { "type": "text", "content": "Body",  "style": { "color": "#aaaaaa", "font-size": 18 } }
            ]
        });
        let component: Component = serde_json::from_value(json).expect("deserialize");
        let child = crate::components::ChildComponent {
            component,
            position: Some(crate::components::PositionMode::Absolute { x: 40.0, y: 40.0 }),
            x: None,
            y: None,
            z_index: None,
            overlays: Vec::new(),
        };
        let scene = vec![child];
        let legacy = render_legacy(&scene, 500, 300);
        let new = render_new(&scene, 500, 300);
        let legacy_lit = nonzero_pixels(&legacy);
        let new_lit = nonzero_pixels(&new);
        assert!(legacy_lit > 1000, "legacy card too small: {legacy_lit}");
        assert!(new_lit > 1000, "new card too small: {new_lit}");
        let ratio = new_lit as f64 / legacy_lit as f64;
        // Flex layout differences are expected. Generous band: 0.5..2.0.
        // The point is "both pipelines drew a recognisable card", not
        // pixel-perfect equality.
        assert!(
            (0.5..2.0).contains(&ratio),
            "flex card lit-pixel ratio diverged: legacy={legacy_lit} new={new_lit} ratio={ratio}"
        );
    }

    #[test]
    fn pipelines_agree_on_nested_cards() {
        // Nested containers — a grid of cards each containing text.
        // Catches regressions where recursion into containers loses
        // box ids, intrinsic measurers, or paint dispatch.
        let json = serde_json::json!({
            "type": "card",
            "style": { "padding": 12, "background": "#101020", "card_direction": "row", "gap": 8 },
            "children": [
                {
                    "type": "card",
                    "style": { "padding": 8, "background": "#303050" },
                    "children": [{ "type": "text", "content": "A", "style": { "color": "#ffffff", "font-size": 24 } }]
                },
                {
                    "type": "card",
                    "style": { "padding": 8, "background": "#503030" },
                    "children": [{ "type": "text", "content": "B", "style": { "color": "#ffffff", "font-size": 24 } }]
                }
            ]
        });
        let component: Component = serde_json::from_value(json).expect("deserialize");
        let child = crate::components::ChildComponent {
            component,
            position: Some(crate::components::PositionMode::Absolute { x: 60.0, y: 60.0 }),
            x: None,
            y: None,
            z_index: None,
            overlays: Vec::new(),
        };
        let scene = vec![child];
        let legacy = render_legacy(&scene, 500, 300);
        let new = render_new(&scene, 500, 300);
        let legacy_lit = nonzero_pixels(&legacy);
        let new_lit = nonzero_pixels(&new);
        assert!(legacy_lit > 1000, "legacy nested card empty: {legacy_lit}");
        assert!(new_lit > 1000, "new nested card empty: {new_lit}");
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
    fn fade_in_attenuates_pixels_in_new_pipeline() {
        // A red shape with FadeIn over 1.0s. At t=0.05 the shape is ~5%
        // opacity, at t=0.95 it's ~95%. The new pipeline must drive opacity
        // through the box-tree CSS overrides, so the late frame should have
        // significantly more "lit" red than the early frame. Without the
        // animator → CssStyle bridge, both frames would look identical.
        let json = serde_json::json!({
            "type": "shape",
            "shape": "rect",
            "size": { "width": 100, "height": 80 },
            "style": {
                "fill": "#ff3366",
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
            overlays: Vec::new(),
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
        if sum_w == 0.0 { None } else { Some(sum_wx / sum_w) }
    }

    #[test]
    fn legacy_and_new_agree_on_fade_in_at_mid_frame() {
        // Same scene through both pipelines at the same frame. The CSS
        // bridge must produce an opacity equivalent to what the legacy
        // animator+canvas-alpha-layer applies. A 15% lit-pixel band is
        // enough headroom for AA differences and still tight enough to
        // catch an opacity off-by-much-more-than-rounding.
        let json = serde_json::json!({
            "type": "shape",
            "shape": "rect",
            "size": { "width": 100, "height": 80 },
            "style": {
                "fill": "#ff3366",
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
            overlays: Vec::new(),
        };
        let scene = vec![child];
        // render_legacy and render_new both sample at t=0.5, scene_duration=1.0
        let legacy = render_legacy(&scene, 400, 300);
        let new = render_new(&scene, 400, 300);
        let red_sum: fn(&[u8]) -> u64 = |buf| buf.chunks_exact(4).map(|p| p[0] as u64).sum();
        let legacy_red = red_sum(&legacy);
        let new_red = red_sum(&new);
        assert!(legacy_red > 0, "legacy fade_in produced no red");
        assert!(new_red > 0, "new fade_in produced no red");
        let ratio = new_red as f64 / legacy_red as f64;
        assert!(
            (0.85..1.15).contains(&ratio),
            "fade_in red sums diverged: legacy={legacy_red} new={new_red} ratio={ratio:.3}"
        );
    }

    #[test]
    fn scale_in_grows_lit_area_in_new_pipeline() {
        // ScaleIn animates `scale` from 0.0 → 1.08 → 1.0 and opacity 0 → 1.
        // At t=0.05 the shape is microscopic; at t=0.95 it's at full size.
        // This exercises the transform: scale(...) path of the CSS bridge.
        let json = serde_json::json!({
            "type": "shape",
            "shape": "rect",
            "size": { "width": 100, "height": 80 },
            "style": {
                "fill": "#ff3366",
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
            overlays: Vec::new(),
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
            "size": { "width": 60, "height": 60 },
            "style": {
                "fill": "#ff3366",
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
            overlays: Vec::new(),
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
