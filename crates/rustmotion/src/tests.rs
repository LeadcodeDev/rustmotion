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
        // Media & diagram components (minimal required fields)
        ("image", r#"{"type":"image","src":"a.png"}"#),
        ("video", r#"{"type":"video","src":"a.mp4"}"#),
        ("gif", r#"{"type":"gif","src":"a.gif"}"#),
        (
            "caption",
            r#"{"type":"caption","words":[{"text":"hi","start":0.0,"end":1.0}]}"#,
        ),
        (
            "connector",
            r#"{"type":"connector","from":{"x":0,"y":0},"to":{"x":10,"y":10}}"#,
        ),
        ("avatar", r#"{"type":"avatar","src":"a.png"}"#),
        (
            "avatar_group",
            r#"{"type":"avatar_group","avatars":[{"src":"a.png"}]}"#,
        ),
        ("arrow", r#"{"type":"arrow","x2":10,"y2":10}"#),
        ("comparison", r#"{"type":"comparison"}"#),
        (
            "dot_map",
            r#"{"type":"dot_map","points":[{"lat":48.8,"lng":2.3}]}"#,
        ),
        ("line", r#"{"type":"line","x2":10,"y2":10}"#),
        ("lottie", r#"{"type":"lottie"}"#),
        (
            "mockup",
            r#"{"type":"mockup","device":"browser","src":"a.png"}"#,
        ),
        ("notification", r#"{"type":"notification","title":"Hi"}"#),
        ("pill_nav", r#"{"type":"pill_nav","items":["A","B"]}"#),
        ("tooltip", r#"{"type":"tooltip","text":"hi"}"#),
        // Audio reactive components
        ("audio_spectrum", r#"{"type":"audio_spectrum"}"#),
        ("waveform", r#"{"type":"waveform"}"#),
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

    /// Input-only `type` aliases and the canonical tag they must serialize to.
    const COMPONENT_ALIASES: &[(&str, &str)] =
        &[("container", "div"), ("progress_bar", "progress")];

    #[test]
    fn all_components_serde_round_trip() {
        // deserialize → serialize → deserialize → serialize: the canonical
        // form must itself deserialize and re-serialize identically. Locks
        // tags, aliases and field names against silent schema drift (the
        // `container` alias was once lost by a bare rename to `div`).
        let mut failures = Vec::new();
        for (name, json) in COMPONENT_JSONS {
            let parsed: Component = match serde_json::from_str(json) {
                Ok(c) => c,
                Err(e) => {
                    failures.push(format!("{name}: does not deserialize: {e}"));
                    continue;
                }
            };
            let canonical = match serde_json::to_value(&parsed) {
                Ok(v) => v,
                Err(e) => {
                    failures.push(format!("{name}: does not serialize: {e}"));
                    continue;
                }
            };
            match serde_json::from_value::<Component>(canonical.clone()) {
                Ok(reparsed) => {
                    let again = serde_json::to_value(&reparsed).unwrap();
                    if canonical != again {
                        failures.push(format!("{name}: unstable serialization"));
                    }
                }
                Err(e) => {
                    failures.push(format!("{name}: canonical form does not deserialize: {e}"));
                }
            }
        }
        if !failures.is_empty() {
            panic!(
                "{} component(s) fail serde round-trip:\n{}",
                failures.len(),
                failures.join("\n")
            );
        }
    }

    #[test]
    fn component_aliases_map_to_canonical_tags() {
        for (alias, canonical) in COMPONENT_ALIASES {
            let (_, json) = COMPONENT_JSONS
                .iter()
                .find(|(n, _)| n == canonical || n == alias)
                .unwrap_or_else(|| panic!("no corpus entry for {canonical}"));
            let mut value: serde_json::Value = serde_json::from_str(json).unwrap();
            value["type"] = serde_json::Value::String(alias.to_string());
            let parsed: Component = serde_json::from_value(value)
                .unwrap_or_else(|e| panic!("alias {alias} does not deserialize: {e}"));
            let tag = serde_json::to_value(&parsed).unwrap()["type"]
                .as_str()
                .unwrap()
                .to_string();
            assert_eq!(
                &tag, canonical,
                "alias {alias} must serialize to canonical tag {canonical}"
            );
        }
    }

    #[test]
    fn corpus_covers_every_component_tag() {
        // Every `type` tag advertised by the generated JSON schema must have
        // a corpus entry, so the deserialize/round-trip tests above cannot
        // silently lose coverage when a component is added.
        let schema = serde_json::to_value(schemars::schema_for!(Component)).unwrap();
        let one_of = schema["oneOf"]
            .as_array()
            .expect("Component schema should be a oneOf over tagged variants");
        let schema_tags: Vec<String> = one_of
            .iter()
            .filter_map(|v| v["properties"]["type"]["enum"][0].as_str())
            .map(str::to_string)
            .collect();
        assert!(
            !schema_tags.is_empty(),
            "no tags extracted from schema — schemars layout changed?"
        );

        let covered: std::collections::HashSet<String> = COMPONENT_JSONS
            .iter()
            .map(|(name, json)| {
                let parsed: Component =
                    serde_json::from_str(json).unwrap_or_else(|e| panic!("{name}: {e}"));
                serde_json::to_value(&parsed).unwrap()["type"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();

        let missing: Vec<&String> = schema_tags
            .iter()
            .filter(|t| !covered.contains(*t))
            .collect();
        assert!(
            missing.is_empty(),
            "component tags with no minimal-JSON corpus entry: {missing:?}"
        );
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
                let dispatcher = LegacyPaintDispatcher::for_scene(&built);
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
                let dispatcher = LegacyPaintDispatcher::for_scene(&built);
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
        let dispatcher = LegacyPaintDispatcher::for_scene(&built);
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

    /// Build a one-child scene with a red 100x80 rect carrying `extra` fields
    /// merged into the component JSON, absolutely positioned at (60, 40).
    fn red_rect_scene(extra: serde_json::Value) -> Vec<crate::components::ChildComponent> {
        let mut json = serde_json::json!({
            "type": "shape",
            "shape": "rect",
            "fill": "#ff3366",
            "style": { "width": "100px", "height": "80px" }
        });
        for (k, v) in extra.as_object().unwrap() {
            json[k] = v.clone();
        }
        let component: Component = serde_json::from_value(json).expect("deserialize");
        vec![crate::components::ChildComponent {
            component,
            position: Some(crate::components::PositionMode::Absolute { x: 60.0, y: 40.0 }),
            x: None,
            y: None,
            z_index: None,
        }]
    }

    fn red_sum(buf: &[u8]) -> u64 {
        buf.chunks_exact(4).map(|p| p[0] as u64).sum()
    }

    #[test]
    fn start_at_hides_component_before_its_window() {
        // start_at gates *visibility* in the paint pass: nothing painted
        // before the window opens, normal paint after. It was silently
        // ignored by the box pipeline before this test existed.
        let scene = red_rect_scene(serde_json::json!({ "start_at": 2.0 }));
        let before = render_new_at(&scene, 400, 300, 1.0, 4.0);
        let after = render_new_at(&scene, 400, 300, 3.0, 4.0);
        assert_eq!(
            red_sum(&before),
            0,
            "component painted before its start_at window"
        );
        assert!(red_sum(&after) > 0, "component absent after start_at");
    }

    #[test]
    fn end_at_hides_component_after_its_window() {
        let scene = red_rect_scene(serde_json::json!({ "end_at": 2.0 }));
        let before = render_new_at(&scene, 400, 300, 1.0, 4.0);
        let after = render_new_at(&scene, 400, 300, 3.0, 4.0);
        assert!(red_sum(&before) > 0, "component absent before end_at");
        assert_eq!(
            red_sum(&after),
            0,
            "component painted after its end_at window"
        );
    }

    #[test]
    fn css_text_shadow_paints_through_the_bridge() {
        // `text-shadow` set inside `style` (the CSS dialect form) must paint
        // like the component-level field: blue text with a red css shadow
        // must produce red pixels. Before the bridge, css text_shadow was
        // parsed and silently dropped.
        let json = serde_json::json!({
            "type": "text",
            "content": "SHADOW",
            "style": {
                "font-size": "60px",
                "color": "#0000ff",
                "text-shadow": [{ "offset-x": 8, "offset-y": 8, "color": "#ff0000" }]
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
        let buf = render_new_at(&[child], 400, 300, 0.5, 1.0);
        assert!(
            red_sum(&buf) > 500,
            "css text-shadow painted no red pixels (red_sum={})",
            red_sum(&buf)
        );
    }

    #[test]
    fn css_filter_blur_softens_edges() {
        // A hard-edged red rect has almost no intermediate red values
        // (antialiasing only); `filter: blur(12px)` must smear the edges into
        // thousands of intermediate pixels. Before the fix, css `filter` was
        // written by the animator (blur_in/blur_out) and static styles but
        // consumed nowhere.
        let sharp = red_rect_scene(serde_json::json!({}));
        let blurred = red_rect_scene(serde_json::json!({
            "style": {
                "width": "100px",
                "height": "80px",
                "filter": [{ "fn": "blur", "radius": 12 }]
            }
        }));
        let count_mid = |buf: &[u8]| {
            buf.chunks_exact(4)
                .filter(|p| p[0] > 20 && p[0] < 220)
                .count()
        };
        let sharp_mid = count_mid(&render_new_at(&sharp, 400, 300, 0.5, 1.0));
        let blur_mid = count_mid(&render_new_at(&blurred, 400, 300, 0.5, 1.0));
        assert!(
            blur_mid > sharp_mid * 5 && blur_mid > 1000,
            "filter: blur did not soften edges (sharp={sharp_mid}, blurred={blur_mid})"
        );
    }

    #[test]
    fn css_filter_invert_flips_colors() {
        // Pure red #ff3366 inverted → #00cc99: red collapses, green rises.
        // Locks the color-matrix translation convention (0..255 space) —
        // a wrong convention would leave red pixels untouched or black.
        let inverted = red_rect_scene(serde_json::json!({
            "style": {
                "width": "100px",
                "height": "80px",
                "filter": [{ "fn": "invert", "value": 1.0 }]
            }
        }));
        let buf = render_new_at(&inverted, 400, 300, 0.5, 1.0);
        let flipped = buf
            .chunks_exact(4)
            .filter(|p| p[0] < 50 && p[1] > 150)
            .count();
        assert!(
            flipped > 3000,
            "invert(1) did not flip the rect's colors (flipped px = {flipped})"
        );
    }

    #[test]
    fn container_stagger_offsets_child_animations() {
        // flex `stagger: 0.2` with three children fading in over 0.2s each.
        // At t=0.25: child 0 finished, child 1 early in its (ease-out) fade,
        // child 2 not started (delay 0.4). The stagger field existed in the
        // schema but was wired to nothing.
        let json = serde_json::json!({
            "type": "flex",
            "stagger": 0.2,
            "style": { "flex-direction": "column", "gap": "10px", "width": "300px" },
            "children": [
                { "type": "shape", "shape": "rect", "fill": "#ff3366",
                  "style": { "width": "100px", "height": "40px",
                             "animation": [{ "name": "fade_in", "duration": 0.2 }] } },
                { "type": "shape", "shape": "rect", "fill": "#ff3366",
                  "style": { "width": "100px", "height": "40px",
                             "animation": [{ "name": "fade_in", "duration": 0.2 }] } },
                { "type": "shape", "shape": "rect", "fill": "#ff3366",
                  "style": { "width": "100px", "height": "40px",
                             "animation": [{ "name": "fade_in", "duration": 0.2 }] } }
            ]
        });
        let component: Component = serde_json::from_value(json).expect("deserialize");
        let child = crate::components::ChildComponent {
            component,
            position: Some(crate::components::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
        };
        let buf = render_new_at(&[child], 300, 200, 0.25, 2.0);
        // Children flow in a column with a 10px gap: bands 0..40, 50..90, 100..140.
        let band_red = |y0: usize, y1: usize| -> u64 {
            (y0..y1)
                .flat_map(|y| (0..300).map(move |x| (y * 300 + x) * 4))
                .map(|i| buf[i] as u64)
                .sum()
        };
        let (b0, b1, b2) = (band_red(0, 40), band_red(50, 90), band_red(100, 140));
        assert!(b0 > 0, "first child must be visible at t=0.25");
        assert!(
            b1 > 0 && b1 * 4 < b0 * 3,
            "second child must be partially faded (b0={b0}, b1={b1})"
        );
        assert_eq!(
            b2, 0,
            "third child must not have started (stagger delay 0.4 > t=0.25)"
        );
    }

    #[test]
    fn style_state_snaps_without_transition() {
        // A timeline step carrying a `style` state hard-cuts at `at` when no
        // transition is configured: full red before, 20% right after.
        let scene = red_rect_scene(serde_json::json!({
            "timeline": [{ "at": 1.0, "style": { "opacity": 0.2 } }]
        }));
        let before = red_sum(&render_new_at(&scene, 400, 300, 0.5, 4.0));
        let after = red_sum(&render_new_at(&scene, 400, 300, 1.5, 4.0));
        assert!(before > 0, "rect must be visible before the state");
        let ratio = after as f64 / before as f64;
        assert!(
            (ratio - 0.2).abs() < 0.08,
            "state must snap to opacity 0.2 (ratio={ratio:.3})"
        );
    }

    #[test]
    fn style_state_smooths_with_transition() {
        // Same state change with `transition: {duration: 1, easing: linear}`:
        // untouched at t=0.5, halfway (~0.6) at t=1.5, settled (0.2) at t=2.5.
        let scene = red_rect_scene(serde_json::json!({
            "timeline": [{ "at": 1.0, "style": { "opacity": 0.2 } }],
            "style": {
                "width": "100px",
                "height": "80px",
                "transition": { "duration": 1.0, "easing": "linear" }
            }
        }));
        let full = red_sum(&render_new_at(&scene, 400, 300, 0.5, 4.0)) as f64;
        let mid = red_sum(&render_new_at(&scene, 400, 300, 1.5, 4.0)) as f64;
        let settled = red_sum(&render_new_at(&scene, 400, 300, 2.5, 4.0)) as f64;
        assert!(full > 0.0);
        let mid_ratio = mid / full;
        let settled_ratio = settled / full;
        assert!(
            (mid_ratio - 0.6).abs() < 0.1,
            "mid-transition must sit near opacity 0.6 (ratio={mid_ratio:.3})"
        );
        assert!(
            (settled_ratio - 0.2).abs() < 0.08,
            "transition must settle at opacity 0.2 (ratio={settled_ratio:.3})"
        );
    }

    #[test]
    fn color_state_transitions_smoothly_on_text() {
        // Text color red → blue at t=1 with a 1s transition: pure red before,
        // both channels mid-way, pure blue after.
        let json = serde_json::json!({
            "type": "text",
            "content": "COLOR",
            "timeline": [{ "at": 1.0, "style": { "color": "#0000ff" } }],
            "style": {
                "font-size": "72px",
                "color": "#ff0000",
                "transition": { "duration": 1.0, "easing": "linear" }
            }
        });
        let component: Component = serde_json::from_value(json).expect("deserialize");
        let child = crate::components::ChildComponent {
            component,
            position: Some(crate::components::PositionMode::Absolute { x: 40.0, y: 40.0 }),
            x: None,
            y: None,
            z_index: None,
        };
        let scene = vec![child];
        let blue_sum = |buf: &[u8]| -> u64 { buf.chunks_exact(4).map(|p| p[2] as u64).sum() };
        let before = render_new_at(&scene, 400, 300, 0.5, 4.0);
        let mid = render_new_at(&scene, 400, 300, 1.5, 4.0);
        let after = render_new_at(&scene, 400, 300, 2.5, 4.0);
        assert!(red_sum(&before) > 500 && blue_sum(&before) < red_sum(&before) / 10);
        assert!(
            red_sum(&mid) > 500 && blue_sum(&mid) > 500,
            "mid-transition must blend red and blue (r={}, b={})",
            red_sum(&mid),
            blue_sum(&mid)
        );
        assert!(blue_sum(&after) > 500 && red_sum(&after) < blue_sum(&after) / 10);
    }

    #[test]
    fn keyframes_effect_honors_its_delay() {
        // KeyframesConfig.delay was silently ignored by the resolver: an
        // opacity ramp 0→1 over 1s with delay 2 must keep the rect invisible
        // at t=1 and fully visible at t=3.5.
        let scene = red_rect_scene(serde_json::json!({
            "style": {
                "width": "100px",
                "height": "80px",
                "animation": [{
                    "name": "keyframes",
                    "delay": 2.0,
                    "keyframes": [{
                        "property": "opacity",
                        "keyframes": [
                            { "time": 0.0, "value": 0.0 },
                            { "time": 1.0, "value": 1.0 }
                        ]
                    }]
                }]
            }
        }));
        let early = red_sum(&render_new_at(&scene, 400, 300, 1.0, 4.0));
        let late = red_sum(&render_new_at(&scene, 400, 300, 3.5, 4.0));
        assert_eq!(early, 0, "keyframes must not start before their delay");
        assert!(late > 0, "keyframes must complete after delay + ramp");
    }

    #[test]
    fn timeline_steps_trigger_delayed_animations() {
        // A timeline step resolves as its animations with `delay += at`:
        // fade_in(1s) at t=2 must be ~5% opaque at t=2.05 and ~95% at t=2.95.
        let scene = red_rect_scene(serde_json::json!({
            "timeline": [{ "at": 2.0, "animation": [{ "name": "fade_in", "duration": 1.0 }] }]
        }));
        let early = render_new_at(&scene, 400, 300, 2.05, 4.0);
        let late = render_new_at(&scene, 400, 300, 2.95, 4.0);
        let (early_red, late_red) = (red_sum(&early), red_sum(&late));
        assert!(
            late_red > early_red * 3,
            "timeline fade_in did not amplify red over its window (early={early_red}, late={late_red})"
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

    // ──────────────────────────────────────────────────────────────────────────
    // Intrinsic-measure tests for Terminal, Table, Codeblock
    //
    // Each test wraps the component in a flex card with NO explicit width/height,
    // runs the full layout pass, and asserts that the component's BoxLayout has
    // sensible non-zero dimensions derived from its content.
    // ──────────────────────────────────────────────────────────────────────────

    /// Helper: build a scene containing one flex card wrapping one child (the
    /// component under test, no explicit size), run layout at 1920×1080, and
    /// return the BoxLayout of the *first child of the card* (the component).
    fn layout_for_unsized_component_in_flex_card(
        component_json: serde_json::Value,
    ) -> rustmotion_core::engine::layout_pass::BoxLayout {
        use crate::components::{ChildComponent, PositionMode};
        use rustmotion_components::box_builder::build_scene;
        use rustmotion_components::card::Card;
        use rustmotion_core::css::style::{CssStyle, FlexDirection};
        use rustmotion_core::css::taffy_bridge::ConversionContext;
        use rustmotion_core::engine::layout_pass::run_layout;

        let component: Component = serde_json::from_value(component_json).expect("deserialize");
        let inner = ChildComponent {
            component,
            position: None,
            x: None,
            y: None,
            z_index: None,
        };

        let card_style = CssStyle {
            flex_direction: Some(FlexDirection::Column),
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
            position: Some(PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
        };
        let scene = vec![card_child];
        let built = build_scene(&scene, (1920.0, 1080.0));
        let layout = run_layout(&built.root, (1920.0, 1080.0), &ConversionContext::default());

        // The scene root is node 0 (the implicit scene root), the card is node 1,
        // and the component is node 2 (first child of the card).
        // Walk the tree to find node 2.
        let component_node_id = {
            // root → card_child (id=1) → card's inner child (the component, id=2)
            built
                .root
                .children
                .first() // card_child box node
                .and_then(|card_node| card_node.children.first()) // component box node
                .map(|n| n.id)
                .expect("component node id must exist")
        };
        layout
            .get(component_node_id)
            .copied()
            .expect("component must have a layout entry")
    }

    #[test]
    fn terminal_intrinsic_height_reflects_line_count() {
        // Terminal with 3 lines, default font size (14px), line-height ratio
        // 22/14 ≈ 1.57 → line_height = ceil(14 * 22/14) = 22.
        // Expected minimum height: chrome (36) + padding_top (16) + 3 * 22 + padding_bottom (16) = 134.
        // We assert ≥ 3 * line_height_min = 3 * 22 = 66 (conservative: chrome may be excluded
        // in some configs, let the intrinsic beat 0).
        let json = serde_json::json!({
            "type": "terminal",
            "lines": [
                { "text": "echo hello", "line_type": "command" },
                { "text": "hello", "line_type": "output" },
                { "text": "echo world", "line_type": "command" }
            ]
        });
        let layout = layout_for_unsized_component_in_flex_card(json);
        assert!(
            layout.height > 0.0,
            "terminal height should be > 0, got {}",
            layout.height
        );
        // With chrome (36) + 2*padding (32) + 3 lines * line_height (22) ≥ 134
        let min_expected = 3.0 * 22.0; // conservative: at least 3 line-heights
        assert!(
            layout.height >= min_expected,
            "terminal height {} should be ≥ {} (3 × line_height)",
            layout.height,
            min_expected
        );
        assert!(
            layout.width > 0.0,
            "terminal width should be > 0, got {}",
            layout.width
        );
    }

    #[test]
    fn table_intrinsic_height_reflects_row_count() {
        // Table with 1 header row + 2 data rows. Default font_size = 14, row_height = 14 * 2.5 = 35.
        // Total height = 3 rows * 35 = 105.
        let json = serde_json::json!({
            "type": "table",
            "headers": ["Name", "Value"],
            "rows": [
                ["Alice", "42"],
                ["Bob", "99"]
            ]
        });
        let layout = layout_for_unsized_component_in_flex_card(json);
        assert!(
            layout.height > 0.0,
            "table height should be > 0, got {}",
            layout.height
        );
        // 3 rows (1 header + 2 data) × row_height (35) = 105
        let expected_min = 3.0 * 35.0;
        assert!(
            layout.height >= expected_min,
            "table height {} should be ≥ {} (3 × row_height)",
            layout.height,
            expected_min
        );
        assert!(
            layout.width > 0.0,
            "table width should be > 0, got {}",
            layout.width
        );
    }

    #[test]
    fn codeblock_intrinsic_height_reflects_line_count() {
        // Codeblock with 3 lines of code, default font_size = 14.
        // Default padding = 16px each side. line_height = style.line_height_for(14) ≈ 14 * 1.5 = 21.
        // Expected: 3 lines * line_height + pad_top + pad_bottom ≥ 3 * 14 = 42.
        let json = serde_json::json!({
            "type": "codeblock",
            "code": "fn main() {\n    println!(\"hello\");\n}"
        });
        let layout = layout_for_unsized_component_in_flex_card(json);
        assert!(
            layout.height > 0.0,
            "codeblock height should be > 0, got {}",
            layout.height
        );
        let min_expected = 3.0 * 14.0; // very conservative: at least 3 × font_size
        assert!(
            layout.height >= min_expected,
            "codeblock height {} should be ≥ {} (3 × font_size)",
            layout.height,
            min_expected
        );
        assert!(
            layout.width > 0.0,
            "codeblock width should be > 0, got {}",
            layout.width
        );
    }
}

#[cfg(test)]
mod audio_tests {
    use std::sync::Arc;

    use rustmotion_core::engine::renderer::audio_analysis::{audio_analysis_cache, AudioAnalysis};

    /// Build a minimal PCM WAV file in memory: mono i16, 44100 Hz.
    /// The first `sine_samples` samples are a 440 Hz sine wave;
    /// the rest are silence up to `total_samples`.
    fn make_sine_wav(
        total_samples: u32,
        sine_samples: u32,
        freq: f32,
        sample_rate: u32,
    ) -> Vec<u8> {
        let data_size = total_samples * 2; // i16 mono
        let mut wav = Vec::<u8>::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_size).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // mono
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
        wav.extend_from_slice(&2u16.to_le_bytes()); // block align
        wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_size.to_le_bytes());
        for i in 0..total_samples {
            let s = if i < sine_samples {
                let t = i as f32 / sample_rate as f32;
                (2.0 * std::f32::consts::PI * freq * t).sin()
            } else {
                0.0
            };
            let pcm = (s * 32767.0) as i16;
            wav.extend_from_slice(&pcm.to_le_bytes());
        }
        wav
    }
    fn nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    /// Wrap a component JSON into a `ChildComponent` absolutely positioned at
    /// the canvas origin.
    fn child_at_origin(json: serde_json::Value) -> crate::components::ChildComponent {
        let component: crate::components::Component =
            serde_json::from_value(json).expect("component json");
        crate::components::ChildComponent {
            component,
            position: Some(crate::components::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
        }
    }

    /// Paint a single child through the full box_tree → layout → paint
    /// pipeline at `time` and return the RGBA8888 pixels.
    fn paint_scene(
        child: crate::components::ChildComponent,
        w: i32,
        h: i32,
        time: f64,
        fps: u32,
    ) -> Vec<u8> {
        use rustmotion_components::box_builder::{build_scene_with_anim, BuildAnimationCtx};
        use rustmotion_components::legacy_dispatch::LegacyPaintDispatcher;
        use rustmotion_core::css::taffy_bridge::ConversionContext;
        use rustmotion_core::engine::layout_pass::run_layout;
        use rustmotion_core::engine::paint_pass::{paint_tree, PaintFrame};

        let mut surface = skia_safe::surfaces::raster_n32_premul((w, h)).expect("raster surface");
        let canvas = surface.canvas();
        canvas.clear(skia_safe::Color4f::new(0.0, 0.0, 0.0, 0.0));
        let scene = vec![child];
        let built = build_scene_with_anim(
            &scene,
            (w as f32, h as f32),
            BuildAnimationCtx {
                time,
                scene_duration: 10.0,
            },
        );
        let layout = run_layout(
            &built.root,
            (w as f32, h as f32),
            &ConversionContext::default(),
        );
        let dispatcher = LegacyPaintDispatcher::for_scene(&built);
        paint_tree(
            canvas,
            &built.root,
            &layout,
            &PaintFrame {
                time,
                frame_index: (time * fps as f64) as u32,
                fps,
                video_width: w as u32,
                video_height: h as u32,
                scene_duration: 10.0,
            },
            &dispatcher,
        );
        let row_bytes = (w * 4) as usize;
        let mut pixels = vec![0u8; row_bytes * h as usize];
        let info = skia_safe::ImageInfo::new(
            (w, h),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        surface.read_pixels(&info, &mut pixels, row_bytes, (0, 0));
        pixels
    }

    /// Count non-transparent pixels in the column range `[x0, x1)`.
    fn lit_in_columns(pixels: &[u8], width: usize, x0: usize, x1: usize) -> usize {
        let height = pixels.len() / (width * 4);
        let mut count = 0;
        for y in 0..height {
            for x in x0..x1 {
                if pixels[(y * width + x) * 4 + 3] > 0 {
                    count += 1;
                }
            }
        }
        count
    }

    #[test]
    fn analyze_scenario_audio_computes_amplitude_and_440hz_band() {
        let sample_rate = 44100u32;
        // 1.0 s total: 0.5 s of 440 Hz sine, then 0.5 s of silence.
        let total_samples = sample_rate;
        let sine_samples = sample_rate / 2;
        let wav = make_sine_wav(total_samples, sine_samples, 440.0, sample_rate);

        let wav_path =
            std::env::temp_dir().join(format!("rustmotion_test_analysis_{}.wav", nanos()));
        std::fs::write(&wav_path, &wav).expect("write wav fixture");
        let wav_str = wav_path.to_str().unwrap().to_string();

        let json = serde_json::json!({
            "video": {"width": 32, "height": 32, "fps": 30},
            "audio": [{"src": wav_str}],
            "scenes": [{"duration": 1.0, "children": []}]
        })
        .to_string();
        let scenario =
            crate::loader::load_scenario_from_source(None, Some(&json)).expect("load scenario");
        crate::encode::audio_analysis::analyze_scenario_audio(&scenario);

        let analysis = audio_analysis_cache()
            .get(&wav_str)
            .expect("analysis must be cached under the track src")
            .clone();
        std::fs::remove_file(&wav_path).ok();

        assert_eq!(analysis.frame_rate, 30);
        assert!(
            analysis.amplitude.len() >= 29,
            "1 s at 30 fps should give ~30 frames, got {}",
            analysis.amplitude.len()
        );

        // Amplitude: ~1.0 during the sine (frames 0..14), ~0 during silence.
        let sine_max = analysis.amplitude[..14]
            .iter()
            .cloned()
            .fold(0.0f32, f32::max);
        let silence_max = analysis.amplitude[16..]
            .iter()
            .cloned()
            .fold(0.0f32, f32::max);
        assert!(
            sine_max > 0.9,
            "normalized amplitude during the sine should be ~1.0, got {sine_max}"
        );
        assert!(
            silence_max < 0.05,
            "amplitude during silence should be ~0, got {silence_max}"
        );

        // Band energy concentrated in the log band containing 440 Hz.
        let lo = 20.0f32.log2();
        let hi = 16000.0f32.log2();
        let expected_band = (((440.0f32.log2() - lo) / ((hi - lo) / 16.0)) as usize).min(15);
        let frame = &analysis.bands[5]; // mid-sine frame
        let (argmax, max_v) =
            frame.iter().enumerate().fold(
                (0usize, 0.0f32),
                |acc, (i, &v)| if v > acc.1 { (i, v) } else { acc },
            );
        assert_eq!(
            argmax, expected_band,
            "band {expected_band} should carry the 440 Hz energy (argmax was {argmax}: {frame:?})"
        );
        assert!(max_v > 0.5, "440 Hz band should be hot, got {max_v}");
        let second = frame
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != expected_band)
            .map(|(_, &v)| v)
            .fold(0.0f32, f32::max);
        assert!(
            frame[expected_band] > second * 3.0,
            "440 Hz band ({}) should dominate the runner-up ({second})",
            frame[expected_band]
        );
    }

    #[test]
    fn audio_spectrum_hot_band_renders_taller_bar() {
        let key = format!("test-spectrum-hot-{}", nanos());
        let mut bands = vec![[0.0f32; 16]; 30];
        for fr in &mut bands {
            fr[15] = 1.0; // hottest band = highest frequencies → right-most bar
        }
        audio_analysis_cache().insert(
            key.clone(),
            Arc::new(AudioAnalysis {
                frame_rate: 30,
                amplitude: vec![1.0; 30],
                bands,
            }),
        );

        let child = child_at_origin(serde_json::json!({
            "type": "audio_spectrum",
            "track": key,
            "bars": 16
        }));
        // Default intrinsic size 400x120; bar_w = (400 - 15*2)/16 ≈ 23.1,
        // bar 15 spans x ≈ 377..400.
        let pixels = paint_scene(child, 400, 200, 0.5, 30);
        let hot_bar = lit_in_columns(&pixels, 400, 378, 400);
        let cold_bar = lit_in_columns(&pixels, 400, 0, 23);
        assert!(
            hot_bar > cold_bar * 10,
            "hot band bar ({hot_bar} lit px) should tower over a cold bar ({cold_bar} lit px)"
        );

        // Missing track → graceful degradation: min_height bars only, glued
        // to the bottom edge of the 120px box.
        let missing = child_at_origin(serde_json::json!({
            "type": "audio_spectrum",
            "track": format!("test-spectrum-missing-{}", nanos()),
            "bars": 16
        }));
        let pixels = paint_scene(missing, 400, 200, 0.5, 30);
        let mut lit_total = 0usize;
        let mut lit_above_baseline = 0usize;
        for y in 0..200usize {
            for x in 0..400usize {
                if pixels[(y * 400 + x) * 4 + 3] > 0 {
                    lit_total += 1;
                    if y < 115 {
                        lit_above_baseline += 1;
                    }
                }
            }
        }
        assert!(lit_total > 0, "min_height bars should still render");
        assert_eq!(
            lit_above_baseline, 0,
            "empty cache must render nothing above the min-height baseline"
        );
    }

    #[test]
    fn waveform_ramp_renders_increasing_pixels_along_x() {
        let key = format!("test-waveform-ramp-{}", nanos());
        let n = 60usize; // 2 s at 30 fps
        let amplitude: Vec<f32> = (0..n).map(|i| i as f32 / (n - 1) as f32).collect();
        audio_analysis_cache().insert(
            key.clone(),
            Arc::new(AudioAnalysis {
                frame_rate: 30,
                amplitude,
                bands: vec![[0.0f32; 16]; 60],
            }),
        );

        let child = child_at_origin(serde_json::json!({
            "type": "waveform",
            "track": key,
            "draw_style": "filled",
            "window": 2.0
        }));
        // At t=1.0 the 2 s window covers the whole ramp: amplitude (and the
        // filled area under the curve) grows from left to right.
        let pixels = paint_scene(child, 400, 200, 1.0, 30);
        let left = lit_in_columns(&pixels, 400, 0, 133);
        let right = lit_in_columns(&pixels, 400, 267, 400);
        assert!(
            left > 0,
            "left third should have some lit pixels (outline at minimum)"
        );
        assert!(
            right > left * 2,
            "ramping amplitude: right third ({right} lit px) should clearly exceed left third ({left} lit px)"
        );
    }

    #[test]
    fn audio_reactive_opacity_binding_differs_between_loud_and_quiet() {
        let key = format!("test-ar-binding-{}", nanos());
        // Loud at t=0 (frame 0), silent afterwards.
        let mut amplitude = vec![0.0f32; 90];
        amplitude[0] = 1.0;
        audio_analysis_cache().insert(
            key.clone(),
            Arc::new(AudioAnalysis {
                frame_rate: 30,
                amplitude,
                bands: vec![[0.0f32; 16]; 90],
            }),
        );

        let make_child = || {
            child_at_origin(serde_json::json!({
                "type": "shape",
                "shape": "rect",
                "fill": "#ff0000",
                "style": {
                    "width": "100px",
                    "height": "100px",
                    "audio-reactive": {
                        "track": key,
                        "source": "amplitude",
                        "property": "opacity",
                        "min": 0.0,
                        "max": 1.0
                    }
                }
            }))
        };
        let red_sum = |pixels: &[u8]| pixels.chunks_exact(4).map(|p| p[0] as u64).sum::<u64>();

        let loud = paint_scene(make_child(), 200, 200, 0.0, 30);
        let quiet = paint_scene(make_child(), 200, 200, 0.5, 30);
        let (loud_red, quiet_red) = (red_sum(&loud), red_sum(&quiet));
        assert!(
            loud_red > 100_000,
            "loud frame should render the red rect (red_sum={loud_red})"
        );
        assert!(
            loud_red > quiet_red.saturating_mul(10).max(1),
            "red_sum must differ sharply between loud ({loud_red}) and quiet ({quiet_red}) frames"
        );
    }
}
