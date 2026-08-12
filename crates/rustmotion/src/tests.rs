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
        (
            "cursor",
            r#"{"type":"cursor","cursor_style":"pointer","path_easing":"linear"}"#,
        ),
        ("codeblock", r#"{"type":"codeblock","code":"fn main() {}"}"#),
        ("badge", r#"{"type":"badge","text":"New"}"#),
        ("callout", r#"{"type":"callout","text":"Hello"}"#),
        (
            "chart",
            r#"{"type":"chart","chart_type":"bar","data":[{"value":10}]}"#,
        ),
        (
            "chart",
            r#"{"type":"chart","chart_type":"funnel","direction":"horizontal","data":[{"value":10}]}"#,
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
            r#"{"type":"stepper","orientation":"vertical","steps":[{"label":"Step 1"},{"label":"Step 2"}]}"#,
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

    /// The exact `shape` spellings SKILL.md documents must parse.
    ///
    /// `ShapeType` is externally tagged, and the skill used to describe the
    /// parameterised variants as `star` "(with `points`)", which reads as a
    /// sibling field and fails with `invalid type: unit variant, expected
    /// struct variant`. The generator writes what the skill says, so a wrong
    /// line there is not a documentation nit — it is a stream of invalid
    /// scenarios. Pin the spellings.
    #[test]
    fn skill_documented_shape_spellings_parse() {
        use rustmotion_core::schema::ShapeType;

        for spelling in [
            r#""rect""#,
            r#""circle""#,
            r#""rounded_rect""#,
            r#""ellipse""#,
            r#""triangle""#,
            r#"{ "star": { "points": 6 } }"#,
            r#"{ "polygon": { "sides": 6 } }"#,
            r#"{ "path": { "data": "M0 0 L10 10" } }"#,
        ] {
            serde_json::from_str::<ShapeType>(spelling).unwrap_or_else(|e| {
                panic!("SKILL.md documents `{spelling}`, which does not parse: {e}")
            });
        }

        // …and the shape the skill used to imply must still be rejected, so a
        // future edit cannot quietly reintroduce it.
        assert!(
            serde_json::from_str::<ShapeType>(r#""star""#).is_err(),
            "a bare `star` must stay an error: it is what the old wording produced"
        );
    }

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
    fn unknown_enum_value_fails_the_typed_parse() {
        // Converted stringly-typed fields (stepper.orientation,
        // chart.direction, cursor.cursor_style/path_easing) now reject
        // unknown values at the typed parse — a blocking validate error
        // instead of a silent fallback (philosophy of #33).
        let bad = r#"{"type":"stepper","orientation":"diagonal","steps":[{"label":"A"}]}"#;
        assert!(serde_json::from_str::<Component>(bad).is_err());
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
            camera: None,
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
                bleed: false,
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
            camera: None,
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
                bleed: false,
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
                    time_scale: None,
                    time_offset: None,
                }),
                position: Some(PositionMode::Absolute { x: 10.0, y: 10.0 }),
                x: None,
                y: None,
                z_index: None,
                bleed: false,
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
                fps: 30,
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
            camera: None,
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
            bleed: false,
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
            bleed: false,
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
            bleed: false,
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
            bleed: false,
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
            bleed: false,
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
            bleed: false,
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
            bleed: false,
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
            bleed: false,
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
            bleed: false,
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
                time_scale: None,
                time_offset: None,
            }),
            position: Some(PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
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

    // ─── Time Remapping Tests ────────────────────────────────────────────────────

    /// Build a scene with one flex container (time_scale, time_offset) wrapping
    /// a red rect with fade_in, absolutely positioned at (0, 0).
    fn flex_fade_in_scene(
        time_scale: Option<f64>,
        time_offset: Option<f64>,
        fade_duration: f64,
    ) -> Vec<crate::components::ChildComponent> {
        let json = serde_json::json!({
            "type": "flex",
            "time_scale": time_scale,
            "time_offset": time_offset,
            "style": { "width": "400px", "height": "300px" },
            "children": [{
                "type": "shape",
                "shape": "rect",
                "fill": "#ff0000",
                "style": {
                    "width": "200px",
                    "height": "200px",
                    "animation": [{ "name": "fade_in", "duration": fade_duration }]
                }
            }]
        });
        let component: Component = serde_json::from_value(json).expect("deserialize flex");
        vec![crate::components::ChildComponent {
            component,
            position: Some(crate::components::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        }]
    }

    #[test]
    fn time_scale_slows_fade_in_animation() {
        // A flex container with time_scale 0.5 wraps a red shape with fade_in duration=1s.
        // At t_global=1s the child should be at t_local=0.5s → half-faded.
        // At t_global=2s the child should be at t_local=1.0s → fully visible.
        // Without remap, at t_global=1s the fade would be complete.
        // With scale 0.5, it should still be in-progress at t=1s.
        let at_1s = render_new_at(
            &flex_fade_in_scene(Some(0.5), None, 1.0),
            400,
            300,
            1.0,
            4.0,
        );
        let at_2s = render_new_at(
            &flex_fade_in_scene(Some(0.5), None, 1.0),
            400,
            300,
            2.0,
            4.0,
        );

        let red_at_1 = red_sum(&at_1s);
        let red_at_2 = red_sum(&at_2s);
        assert!(
            red_at_2 > red_at_1 + 1000,
            "time_scale=0.5: at t=1s (half-faded) red_sum={} should be less than at t=2s (fully visible) red_sum={}",
            red_at_1, red_at_2
        );
        assert!(
            red_at_2 > 5000,
            "at t=2s (t_local=1.0s, fade complete) shape should be fully visible, red_sum={}",
            red_at_2
        );
    }

    #[test]
    fn time_offset_delays_animation_start() {
        // A flex container with time_offset=1.0 wraps a shape with fade_in duration=0.5s.
        // t_local = (t_global - 1.0) * 1.0
        // At t_global=0.9s: t_local=-0.1s → nothing visible yet.
        // At t_global=1.6s: t_local=0.6s → fade complete (duration=0.5).
        let before = render_new_at(
            &flex_fade_in_scene(None, Some(1.0), 0.5),
            400,
            300,
            0.9,
            4.0,
        );
        let after = render_new_at(
            &flex_fade_in_scene(None, Some(1.0), 0.5),
            400,
            300,
            1.6,
            4.0,
        );

        let red_before = red_sum(&before);
        let red_after = red_sum(&after);

        assert!(
            red_before < 1000,
            "time_offset=1.0: at t=0.9s animation hasn't started, expected near-zero red, got {}",
            red_before
        );
        assert!(
            red_after > 5000,
            "time_offset=1.0: at t=1.6s fade should be complete, expected high red, got {}",
            red_after
        );
    }

    #[test]
    fn start_at_window_respected_under_time_scale() {
        // Child with start_at=1.0 inside flex with time_scale=0.5.
        // start_at is in local time. In global: t_global = start_at / scale = 1.0/0.5 = 2.0s.
        // At t_global=1.5s: child should still be invisible (local time=0.75 < start_at=1.0).
        // At t_global=2.5s: child should be visible (local time=1.25 > start_at=1.0).
        let json = serde_json::json!({
            "type": "flex",
            "time_scale": 0.5,
            "style": { "width": "400px", "height": "300px" },
            "children": [{
                "type": "shape",
                "shape": "rect",
                "fill": "#ff0000",
                "start_at": 1.0,
                "style": { "width": "200px", "height": "200px" }
            }]
        });
        let make_scene = || {
            let component: Component = serde_json::from_value(json.clone()).expect("deserialize");
            vec![crate::components::ChildComponent {
                component,
                position: Some(crate::components::PositionMode::Absolute { x: 0.0, y: 0.0 }),
                x: None,
                y: None,
                z_index: None,
                bleed: false,
            }]
        };

        let invisible = render_new_at(&make_scene(), 400, 300, 1.5, 6.0);
        let visible = render_new_at(&make_scene(), 400, 300, 2.5, 6.0);

        assert!(
            red_sum(&invisible) < 500,
            "start_at=1.0 with scale=0.5: at t_global=1.5 (t_local=0.75) shape should be invisible, red_sum={}",
            red_sum(&invisible)
        );
        assert!(
            red_sum(&visible) > 5000,
            "start_at=1.0 with scale=0.5: at t_global=2.5 (t_local=1.25 > start_at=1.0) shape should be visible, red_sum={}",
            red_sum(&visible)
        );
    }

    #[test]
    fn cascaded_time_scale_compounds() {
        // Outer flex: time_scale=0.5. Inner flex: time_scale=0.5.
        // Effective scale on the shape = 0.5 * 0.5 = 0.25.
        // Shape has fade_in duration=1s.
        // t_local = t_global * 0.25 (both offsets are 0).
        // At t_global=3.0s: t_local=0.75s → still fading (not at 1s yet).
        // At t_global=4.5s: t_local=1.125s → fade complete.
        let json = serde_json::json!({
            "type": "flex",
            "time_scale": 0.5,
            "style": { "width": "400px", "height": "300px" },
            "children": [{
                "type": "flex",
                "time_scale": 0.5,
                "children": [{
                    "type": "shape",
                    "shape": "rect",
                    "fill": "#ff0000",
                    "style": {
                        "width": "100px",
                        "height": "100px",
                        "animation": [{ "name": "fade_in", "duration": 1.0 }]
                    }
                }]
            }]
        });
        let make_scene = || {
            let component: Component = serde_json::from_value(json.clone()).expect("deserialize");
            vec![crate::components::ChildComponent {
                component,
                position: Some(crate::components::PositionMode::Absolute { x: 0.0, y: 0.0 }),
                x: None,
                y: None,
                z_index: None,
                bleed: false,
            }]
        };

        // At t_global=3s: t_local = 3 * 0.25 = 0.75s → fade still incomplete
        let at_3s = render_new_at(&make_scene(), 400, 300, 3.0, 6.0);
        // At t_global=4.5s: t_local = 4.5 * 0.25 = 1.125s → fade complete
        let at_4_5s = render_new_at(&make_scene(), 400, 300, 4.5, 6.0);

        let red_3 = red_sum(&at_3s);
        let red_4_5 = red_sum(&at_4_5s);

        assert!(
            red_4_5 > red_3 + 500,
            "cascaded scale 0.5×0.5=0.25: at t=3s red={} should be less than t=4.5s red={}",
            red_3,
            red_4_5
        );
        assert!(
            red_4_5 > 2000,
            "cascaded scale: at t=4.5s (t_local=1.125s) fade should be complete, red_sum={}",
            red_4_5
        );
    }

    #[test]
    fn time_scale_affects_internal_paint_ctx_time() {
        // A line with draw_in duration=1s inside a flex with time_scale=0.5.
        // At t_global=1s (t_local=0.5s): the line should be about half-drawn.
        // Without remapping, the line would be fully drawn at t=1s.
        // We compare draw extent vs a reference without remap.
        let make_scene = |time_scale: Option<f64>| {
            let json = serde_json::json!({
                "type": "flex",
                "time_scale": time_scale,
                "children": [{
                    "type": "line",
                    "x1": 0.0,
                    "y1": 150.0,
                    "x2": 400.0,
                    "y2": 150.0,
                    "width": 8.0,
                    "color": "#ff0000",
                    "style": {
                        "animation": [{ "name": "draw_in", "duration": 1.0 }]
                    }
                }]
            });
            let component: Component = serde_json::from_value(json).expect("deserialize flex+line");
            vec![crate::components::ChildComponent {
                component,
                position: Some(crate::components::PositionMode::Absolute { x: 0.0, y: 0.0 }),
                x: None,
                y: None,
                z_index: None,
                bleed: false,
            }]
        };

        // At t_global=1s: without remap the line is fully drawn; with scale=0.5 it should be ~half.
        let without_remap = render_new_at(&make_scene(None), 400, 300, 1.0, 2.0);
        let with_remap = render_new_at(&make_scene(Some(0.5)), 400, 300, 1.0, 2.0);

        let red_no_remap = red_sum(&without_remap);
        let red_remapped = red_sum(&with_remap);

        assert!(
            red_no_remap > red_remapped + 500,
            "line draw_in with scale=0.5: at t=1s remapped line (red={}) should have less extent \
             than non-remapped (red={})",
            red_remapped,
            red_no_remap
        );
        assert!(
            red_remapped > 200,
            "line draw_in with scale=0.5: at t=1s (t_local=0.5s) line should be partially drawn, red_sum={}",
            red_remapped
        );
    }

    // ─── time-container chantier: composition + freeze (issue #164) ──────────

    #[test]
    fn nested_time_scale_and_offset_compose_and_a_frozen_global_time_freezes_the_whole_subtree() {
        // Direct answer to "a card with time_scale: 2 containing a flex with
        // time_offset: -1: what does the grandchild see?" — per the
        // documented composition rule (`t_local = (t_parent - offset) *
        // scale`, applied per level,
        // .claude/skills/rustmotion/rules/time-remapping.md):
        //   card:  t_card = (T - 0) * 2        = 2T
        //   flex:  t_flex = (t_card - (-1)) * 1 = 2T + 1
        // A shape with a 4s fade_in nested two levels deep should be 25%
        // faded at T=0 (t_local=1) and 50% faded at T=0.5 (t_local=2).
        let json = serde_json::json!({
            "type": "card",
            "time_scale": 2.0,
            "style": { "width": "400px", "height": "300px" },
            "children": [{
                "type": "flex",
                "time_offset": -1.0,
                "children": [{
                    "type": "shape",
                    "shape": "rect",
                    "fill": "#ff0000",
                    "style": {
                        "width": "200px",
                        "height": "200px",
                        "animation": [{ "name": "fade_in", "duration": 4.0 }]
                    }
                }]
            }]
        });
        let make_scene = || {
            let component: Component = serde_json::from_value(json.clone()).expect("deserialize");
            vec![crate::components::ChildComponent {
                component,
                position: Some(crate::components::PositionMode::Absolute { x: 0.0, y: 0.0 }),
                x: None,
                y: None,
                z_index: None,
                bleed: false,
            }]
        };

        let at_t0 = render_new_at(&make_scene(), 400, 300, 0.0, 6.0);
        let at_t_half = render_new_at(&make_scene(), 400, 300, 0.5, 6.0);
        let red_t0 = red_sum(&at_t0);
        let red_t_half = red_sum(&at_t_half);
        assert!(
            red_t_half > red_t0 + 500,
            "card time_scale=2 > flex time_offset=-1: grandchild local time is 2*T+1; T=0.5 \
             (local=2, 50% faded, red={}) must be more opaque than T=0 (local=1, 25% faded, red={})",
            red_t_half,
            red_t0
        );

        // Simulate what a Scene-level `freeze_at: 0.5` does *upstream* of
        // this box tree: `scene.rs` clamps the GLOBAL time to `freeze_at`
        // once, before it ever reaches `build_scene_with_anim` — a nested
        // time_scale/time_offset subtree never sees a global time past the
        // freeze point in the first place. Clamping *before* the affine
        // composition is equivalent to clamping *after* it whenever every
        // ancestor's `time_scale` is positive (which the validator already
        // enforces — see `validate_schema.rs`'s `time_scale must be > 0`).
        // So: two different raw global times that both get clamped to the
        // same freeze point must render identically, however deep the
        // nesting — this is *why* freeze_at needs no changes to
        // `box_builder.rs`'s time_remap composition at all, only a single,
        // upstream clamp of the scene's own global time (see `scene.rs`'s
        // `SceneTime`).
        let freeze_at = 0.5;
        let frozen_a = render_new_at(&make_scene(), 400, 300, 1.5_f64.min(freeze_at), 6.0);
        let frozen_b = render_new_at(&make_scene(), 400, 300, 2.5_f64.min(freeze_at), 6.0);
        assert_eq!(
            frozen_a, frozen_b,
            "two different global times both clamped to the same freeze_at before reaching a \
             nested time_scale/time_offset subtree must render pixel-identical"
        );
        assert_eq!(
            frozen_a, at_t_half,
            "clamping T to freeze_at=0.5 must render exactly like rendering at T=0.5 directly \
             (frozen == the frame at the freeze point, not some other value)"
        );
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// SVG draw-on (draw_progress) pixel tests
// ──────────────────────────────────────────────────────────────────────────────
//
// The SVG under test contains two horizontal strokes spatially separated:
//   - path 1: a line from (10,30) to (90,30)   → top half of the 100×100 box
//   - path 2: a line from (10,70) to (90,70)   → bottom half of the 100×100 box
//
// Both paths have an explicit stroke (white, width 4) so the draw-on mode
// picks up the SVG stroke color rather than the fill-fallback.
//
// The component is rendered in a 100×100 pixel canvas.
#[cfg(test)]
mod svg_draw_on_tests {
    use crate::components::{ChildComponent, Component, PositionMode};
    use rustmotion_components::box_builder::{build_scene_with_anim, BuildAnimationCtx};
    use rustmotion_components::legacy_dispatch::LegacyPaintDispatcher;
    use rustmotion_core::css::taffy_bridge::ConversionContext;
    use rustmotion_core::engine::layout_pass::run_layout;
    use rustmotion_core::engine::paint_pass::{paint_tree, PaintFrame};

    // SVG with two horizontal stroked paths, vertically separated.
    // Path 1 at y=30 (top region), Path 2 at y=70 (bottom region).
    const TWO_STROKE_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
  <line x1="10" y1="30" x2="90" y2="30" stroke="white" stroke-width="4"/>
  <line x1="10" y1="70" x2="90" y2="70" stroke="white" stroke-width="4"/>
</svg>"#;

    // SVG with two filled rects (no stroke) — draw mode should trace their contour.
    const TWO_FILL_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
  <rect x="10" y="10" width="80" height="20" fill="white"/>
  <rect x="10" y="70" width="80" height="20" fill="white"/>
</svg>"#;

    /// Render an SVG component with the given JSON fields at the specified
    /// draw_progress (via animation) and return the raw RGBA8888 pixels.
    fn render_svg_at(svg_data: &str, extra_fields: serde_json::Value, progress: f64) -> Vec<u8> {
        let mut json = serde_json::json!({
            "type": "svg",
            "data": svg_data,
            "style": { "width": "100px", "height": "100px" }
        });
        // Merge extra_fields
        if let serde_json::Value::Object(map) = extra_fields {
            for (k, v) in map {
                json[k] = v;
            }
        }
        let component: Component = serde_json::from_value(json).expect("svg deserialize");
        let child = ChildComponent {
            component,
            position: Some(PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        };
        let scene = vec![child];

        let w = 100u32;
        let h = 100u32;
        let scene_duration = 1.0f64;

        let mut surface =
            skia_safe::surfaces::raster_n32_premul((w as i32, h as i32)).expect("surface");
        let canvas = surface.canvas();
        canvas.clear(skia_safe::Color4f::new(0.0, 0.0, 0.0, 0.0));

        let built = build_scene_with_anim(
            &scene,
            (w as f32, h as f32),
            BuildAnimationCtx {
                time: progress,
                scene_duration,
                fps: 30,
            },
        );
        let layout = run_layout(
            &built.root,
            (w as f32, h as f32),
            &ConversionContext::default(),
        );
        let dispatcher = LegacyPaintDispatcher::for_scene(&built);
        let frame = PaintFrame {
            time: progress,
            frame_index: (progress * 30.0) as u32,
            fps: 30,
            video_width: w,
            video_height: h,
            scene_duration,
            camera: None,
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

    /// Count non-transparent pixels in RGBA buffer.
    fn lit(buf: &[u8]) -> usize {
        buf.chunks_exact(4).filter(|p| p[3] > 0).count()
    }

    /// Count non-transparent pixels in horizontal band [y0, y1) of a 100-wide canvas.
    fn lit_band(buf: &[u8], y0: usize, y1: usize) -> usize {
        (y0..y1)
            .flat_map(|y| (0..100usize).map(move |x| (y * 100 + x) * 4))
            .filter(|&i| buf[i + 3] > 0)
            .count()
    }

    // ── T1: draw_progress=0 → zero pixels ────────────────────────────────────
    #[test]
    fn draw_progress_zero_paints_nothing() {
        // No animation configured but draw_progress is forced to 0 via anim at t=0.
        // We use a draw_in animation so progress maps to draw_progress.
        let buf = render_svg_at(
            TWO_STROKE_SVG,
            serde_json::json!({
                "style": {
                    "width": "100px",
                    "height": "100px",
                    "animation": [{ "name": "draw_in", "duration": 1.0 }]
                }
            }),
            // t=0: draw_progress animates from 0.0 at delay=0.
            0.0,
        );
        assert_eq!(
            lit(&buf),
            0,
            "draw_progress=0 must produce zero lit pixels (got {})",
            lit(&buf)
        );
    }

    // ── T2: draw_progress=0.5 → only first path visible (overlap=0) ──────────
    #[test]
    fn draw_progress_half_shows_first_path_only() {
        // Two equal-length strokes → at progress=0.5 the first is fully drawn,
        // the second has not started (sequential, overlap=0).
        let buf = render_svg_at(
            TWO_STROKE_SVG,
            serde_json::json!({
                "draw_overlap": 0.0,
                "style": {
                    "width": "100px",
                    "height": "100px",
                    "animation": [{ "name": "draw_in", "duration": 1.0 }]
                }
            }),
            0.5,
        );
        // Path 1 is near y=30, path 2 near y=70.
        let top = lit_band(&buf, 25, 35);
        let bot = lit_band(&buf, 65, 75);
        assert!(
            top > 0,
            "first path (y≈30) must be visible at draw_progress≈0.5 (top={top})"
        );
        assert_eq!(
            bot, 0,
            "second path (y≈70) must be absent at draw_progress≈0.5 (bot={bot})"
        );
    }

    // ── T3: draw_progress=1.0 → resvg full render (filled pixels) ───────────
    #[test]
    fn draw_progress_one_renders_complete_svg() {
        // At progress=1.0 we fall through to resvg so fills are visible.
        // TWO_STROKE_SVG only has strokes — just assert both bands are lit.
        let buf = render_svg_at(
            TWO_STROKE_SVG,
            serde_json::json!({
                "style": {
                    "width": "100px",
                    "height": "100px",
                    "animation": [{ "name": "draw_in", "duration": 1.0 }]
                }
            }),
            // At t slightly before 1.0 draw_progress may still be <1.0.
            // Use t=1.0 (end of 1s animation) to get progress=1.0.
            1.0,
        );
        let top = lit_band(&buf, 25, 35);
        let bot = lit_band(&buf, 65, 75);
        assert!(
            top > 0,
            "top band must be lit at draw_progress=1 (top={top})"
        );
        assert!(
            bot > 0,
            "bot band must be lit at draw_progress=1 (bot={bot})"
        );
    }

    // ── T4: draw_overlap=1.0 → both paths drawn in parallel at 0.5 ──────────
    #[test]
    fn draw_overlap_one_draws_all_paths_in_parallel() {
        // With overlap=1.0 every path's window spans the full [0,1] range.
        // At progress=0.5, both paths are 50% drawn.
        let buf = render_svg_at(
            TWO_STROKE_SVG,
            serde_json::json!({
                "draw_overlap": 1.0,
                "style": {
                    "width": "100px",
                    "height": "100px",
                    "animation": [{ "name": "draw_in", "duration": 1.0 }]
                }
            }),
            0.5,
        );
        let top = lit_band(&buf, 25, 35);
        let bot = lit_band(&buf, 65, 75);
        assert!(
            top > 0,
            "with overlap=1 both paths must be partially drawn at 0.5; top={top}"
        );
        assert!(
            bot > 0,
            "with overlap=1 both paths must be partially drawn at 0.5; bot={bot}"
        );
    }

    // ── T5: fill-only SVG → contour visible in draw mode ─────────────────────
    #[test]
    fn fill_only_svg_draws_contour_in_draw_mode() {
        // TWO_FILL_SVG has no stroke; draw mode should trace the contour using
        // the fill color and draw_stroke_width.
        let buf = render_svg_at(
            TWO_FILL_SVG,
            serde_json::json!({
                "draw_stroke_width": 3.0,
                "draw_overlap": 0.0,
                "style": {
                    "width": "100px",
                    "height": "100px",
                    "animation": [{ "name": "draw_in", "duration": 1.0 }]
                }
            }),
            // At t=0.5, at least some pixels should be visible (first rect partially drawn).
            0.5,
        );
        assert!(
            lit(&buf) > 0,
            "fill-only SVG should produce lit pixels in draw mode at 0.5 (got {})",
            lit(&buf)
        );
    }

    // ── T6: static render (no draw, no animation) → resvg bitmap unchanged ───
    #[test]
    fn static_svg_renders_without_draw_mode() {
        // A simple filled-rect SVG with no draw fields. The resvg path must
        // produce filled pixels in the fill color.
        let filled_svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
  <rect x="10" y="10" width="80" height="80" fill="red"/>
</svg>"#;
        let buf = render_svg_at(
            filled_svg,
            serde_json::json!({}), // no extra fields → static resvg path
            0.5,
        );
        // resvg fills the rect red. Count red-dominant pixels.
        let red_pixels = buf
            .chunks_exact(4)
            .filter(|p| p[3] > 0 && p[0] > p[2] && p[0] > p[1])
            .count();
        assert!(
            red_pixels > 3000,
            "static render must produce many red-dominant pixels (got {red_pixels})"
        );
    }

    // ── T7: draw: true → static draw trace visible without animation ──────────
    #[test]
    fn draw_true_shows_full_trace_without_animation() {
        // draw: true forces draw-on mode with progress=1.0 when no animation is active.
        // That should fall through to the resvg render (showing the complete SVG).
        let buf = render_svg_at(
            TWO_STROKE_SVG,
            serde_json::json!({ "draw": true }),
            0.5, // time doesn't matter, no animation
        );
        let top = lit_band(&buf, 25, 35);
        let bot = lit_band(&buf, 65, 75);
        assert!(top > 0, "draw:true must show top path (top={top})");
        assert!(bot > 0, "draw:true must show bot path (bot={bot})");
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
            bleed: false,
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
                fps,
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
                camera: None,
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

    /// The failure the whole rewrite exists for: a scenario naming an asset
    /// beside itself must load identically whatever directory the process
    /// runs from. Authoring from the scenario's folder worked; the studio,
    /// which runs from the repository root, resolved nothing.
    #[test]
    fn a_relative_asset_resolves_against_the_scenario_not_the_cwd() {
        let dir = std::env::temp_dir().join(format!("rustmotion_cwd_{}", nanos()));
        std::fs::create_dir_all(dir.join("assets")).expect("scratch");
        std::fs::write(
            dir.join("assets/t.wav"),
            make_sine_wav(4410, 4410, 440.0, 44100),
        )
        .expect("fixture");

        let scenario_path = dir.join("scene.json");
        std::fs::write(
            &scenario_path,
            serde_json::json!({
                "video": {"width": 32, "height": 32, "fps": 30},
                "audio": [{"src": "assets/t.wav"}],
                "scenes": [{"duration": 0.1, "children": []}]
            })
            .to_string(),
        )
        .expect("write scenario");

        let loaded = crate::loader::load_scenario_with_vars(&scenario_path, None)
            .expect("scenario must load");
        let src = &loaded.audio[0].src;

        assert!(
            std::path::Path::new(src).is_absolute(),
            "the asset path must not stay relative to the process: {src}"
        );
        assert!(
            std::path::Path::new(src).is_file(),
            "and it must point at the file beside the scenario: {src}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A track placed at `start` must be *read* from `start` too.
    ///
    /// The mux places the file at `track.start` on the scenario timeline and
    /// copies it from its own sample 0; the analysis indexes the file from its
    /// own frame 0 while every painter asks in scenario time. The two only
    /// lined up when `start == 0` — a soundtrack gated to one scene played its
    /// opening while the waveform drew the file's content at that timestamp.
    #[test]
    fn a_track_start_offsets_the_analysis_lookup() {
        let sample_rate = 44100u32;
        // 1 s of sine then 1 s of silence, placed at t = 5 and cut at t = 6.5.
        let wav_path = std::env::temp_dir().join(format!("rustmotion_test_offset_{}.wav", nanos()));
        std::fs::write(
            &wav_path,
            make_sine_wav(sample_rate * 2, sample_rate, 440.0, sample_rate),
        )
        .expect("write fixture");
        let wav_str = wav_path.to_str().unwrap().to_string();

        let json = serde_json::json!({
            "video": {"width": 32, "height": 32, "fps": 30},
            "audio": [{"src": wav_str, "start": 5.0, "end": 6.5}],
            "scenes": [{"duration": 8.0, "children": []}]
        })
        .to_string();
        let scenario =
            crate::loader::load_scenario_from_source(None, Some(&json)).expect("load scenario");
        assert!(crate::encode::audio_analysis::analyze_scenario_audio(&scenario).is_empty());

        let analysis = audio_analysis_cache().get(&wav_str).unwrap().clone();
        std::fs::remove_file(&wav_path).ok();

        assert_eq!(
            analysis.amplitude_at(4.9),
            0.0,
            "before `start` the track is not playing"
        );
        assert!(
            analysis.amplitude_at(5.2) > 0.5,
            "0.2 s after `start` is 0.2 s into the file — inside the sine"
        );
        assert!(
            analysis.amplitude_at(6.2) < 0.1,
            "1.2 s after `start` is 1.2 s into the file — inside the silence"
        );
        assert_eq!(
            analysis.amplitude_at(6.6),
            0.0,
            "past `end` the track is cut, so the visualisation must go flat \
             rather than keep drawing an envelope nobody hears"
        );

        // The smoothed accessors take the same path, or a bound component
        // would disagree with the waveform next to it.
        assert_eq!(analysis.amplitude_smoothed(4.9, 3), 0.0);
        assert_eq!(analysis.band_at(4.9, 4), 0.0);
        assert_eq!(analysis.band_smoothed(4.9, 4, 3), 0.0);
    }

    /// A track that cannot be decoded must be *reported*, not swallowed:
    /// silence here leaves `waveform`/`audio_spectrum` on their flat fallback
    /// with nothing anywhere saying why.
    #[test]
    fn analyze_scenario_audio_reports_an_undecodable_track() {
        let missing = std::env::temp_dir()
            .join(format!("rustmotion_test_absent_{}.wav", nanos()))
            .to_str()
            .unwrap()
            .to_string();

        let json = serde_json::json!({
            "video": {"width": 32, "height": 32, "fps": 30},
            "audio": [{"src": missing}],
            "scenes": [{"duration": 1.0, "children": []}]
        })
        .to_string();
        let scenario =
            crate::loader::load_scenario_from_source(None, Some(&json)).expect("load scenario");

        let failures = crate::encode::audio_analysis::analyze_scenario_audio(&scenario);
        assert_eq!(failures.len(), 1, "the missing track must be reported");
        assert_eq!(failures[0].src, missing);
        assert!(
            !failures[0].reason.is_empty(),
            "a failure must carry a reason, got {failures:?}"
        );
        assert!(
            audio_analysis_cache().get(&missing).is_none(),
            "a failed decode must not leave an entry behind"
        );
    }

    /// The cache is keyed by path, so a track re-exported under the same name
    /// used to keep serving the first envelope for the life of the process —
    /// exactly what a studio session does when the mix is updated.
    #[test]
    fn analyze_scenario_audio_reruns_when_the_file_changes() {
        let sample_rate = 44100u32;
        let wav_path =
            std::env::temp_dir().join(format!("rustmotion_test_refresh_{}.wav", nanos()));
        let wav_str = wav_path.to_str().unwrap().to_string();

        // First: a full second of sine — amplitude high throughout.
        std::fs::write(
            &wav_path,
            make_sine_wav(sample_rate, sample_rate, 440.0, sample_rate),
        )
        .expect("write first fixture");

        let json = serde_json::json!({
            "video": {"width": 32, "height": 32, "fps": 30},
            "audio": [{"src": wav_str}],
            "scenes": [{"duration": 1.0, "children": []}]
        })
        .to_string();
        let scenario =
            crate::loader::load_scenario_from_source(None, Some(&json)).expect("load scenario");
        assert!(crate::encode::audio_analysis::analyze_scenario_audio(&scenario).is_empty());
        let late_before = audio_analysis_cache().get(&wav_str).unwrap().amplitude[25];

        // Rewrite the same path with sine only in the first half. mtime has
        // 1 ns resolution on the platforms we target, but the length differs
        // too, so the fingerprint changes either way.
        std::fs::write(
            &wav_path,
            make_sine_wav(sample_rate * 2, sample_rate / 2, 440.0, sample_rate),
        )
        .expect("write second fixture");

        assert!(crate::encode::audio_analysis::analyze_scenario_audio(&scenario).is_empty());
        let late_after = audio_analysis_cache().get(&wav_str).unwrap().amplitude[25];
        std::fs::remove_file(&wav_path).ok();

        assert!(
            late_before > 0.5,
            "frame 25 of the first take is inside the sine, got {late_before}"
        );
        assert!(
            late_after < 0.1,
            "frame 25 of the second take is silence — a stale analysis would \
             still report {late_before}, got {late_after}"
        );
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
                start: 0.0,
                end: None,
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
                start: 0.0,
                end: None,
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
                start: 0.0,
                end: None,
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

// ─── Motion blur / Trail effect tests ─────────────────────────────────────────

#[cfg(test)]
mod motion_blur_trail {
    //! TDD tests for motion_blur and trail animation effects.
    //!
    //! These tests drive the pipeline through `render_new_at` and inspect raw
    //! pixel buffers to verify the ghost-based temporal sampling.

    use crate::components::{ChildComponent, Component, PositionMode};
    use rustmotion_components::box_builder::{build_scene_with_anim, BuildAnimationCtx};
    use rustmotion_components::legacy_dispatch::LegacyPaintDispatcher;
    use rustmotion_core::css::taffy_bridge::ConversionContext;
    use rustmotion_core::engine::layout_pass::run_layout;
    use rustmotion_core::engine::paint_pass::{paint_tree, PaintFrame};

    const FPS: u32 = 30;

    /// Render the scene at a specific time through the new pipeline.
    /// Returns RGBA8888 (premul) pixels.
    fn render_at(
        children: &[ChildComponent],
        w: u32,
        h: u32,
        time: f64,
        scene_duration: f64,
    ) -> Vec<u8> {
        let mut surface =
            skia_safe::surfaces::raster_n32_premul((w as i32, h as i32)).expect("surface");
        let canvas = surface.canvas();
        canvas.clear(skia_safe::Color4f::new(0.0, 0.0, 0.0, 0.0));
        let built = build_scene_with_anim(
            children,
            (w as f32, h as f32),
            BuildAnimationCtx {
                time,
                scene_duration,
                fps: FPS,
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
            frame_index: (time * FPS as f64) as u32,
            fps: FPS,
            video_width: w,
            video_height: h,
            scene_duration,
            camera: None,
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

    /// Count unique x-columns (0..w) that have at least one lit pixel (R > threshold).
    fn lit_column_span(pixels: &[u8], w: usize, h: usize, r_threshold: u8) -> usize {
        let mut hit = vec![false; w];
        for y in 0..h {
            for x in 0..w {
                let r = pixels[(y * w + x) * 4];
                if r > r_threshold {
                    hit[x] = true;
                }
            }
        }
        hit.iter().filter(|&&b| b).count()
    }

    /// Find the maximum red channel value across all pixels.
    fn max_red(pixels: &[u8]) -> u8 {
        pixels.chunks_exact(4).map(|p| p[0]).max().unwrap_or(0)
    }

    /// Make a red Shape with slide_in_left + motion_blur, positioned at the center.
    fn make_motion_blur_scene(samples: u32, intensity: f32) -> Vec<ChildComponent> {
        let json = serde_json::json!({
            "type": "shape",
            "shape": "rect",
            "fill": "#ff0000",
            "style": {
                "width": "80px",
                "height": "80px",
                "animation": [
                    { "name": "slide_in_left", "duration": 1.0 },
                    { "name": "motion_blur", "intensity": intensity, "samples": samples }
                ]
            }
        });
        let component: Component = serde_json::from_value(json).expect("motion_blur json");
        vec![ChildComponent {
            component,
            position: Some(PositionMode::Absolute { x: 200.0, y: 110.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        }]
    }

    /// Make a red Shape with slide_in_left + trail effect.
    fn make_trail_scene(copies: u32, spacing: f64, falloff: f32) -> Vec<ChildComponent> {
        let json = serde_json::json!({
            "type": "shape",
            "shape": "rect",
            "fill": "#ff0000",
            "style": {
                "width": "80px",
                "height": "80px",
                "animation": [
                    { "name": "slide_in_left", "duration": 1.0 },
                    { "name": "trail", "copies": copies, "spacing": spacing, "falloff": falloff }
                ]
            }
        });
        let component: Component = serde_json::from_value(json).expect("trail json");
        vec![ChildComponent {
            component,
            position: Some(PositionMode::Absolute { x: 200.0, y: 110.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        }]
    }

    // ──────────────────────────────────────────────────────────────────────────
    // TDD RED TESTS — these will fail until the implementation is in place.
    // ──────────────────────────────────────────────────────────────────────────

    /// Motion blur broadens the x-span of lit pixels compared to no-blur.
    #[test]
    fn motion_blur_broadens_horizontal_span() {
        let w = 500u32;
        let h = 300u32;
        // t=0.5 → shape mid-animation → significant motion → ghosts should
        // land to the left of the principal node.
        let without = make_motion_blur_scene(1, 1.0); // samples=1 → no-op (degenerate)
        let with_blur = make_motion_blur_scene(6, 1.0);

        let buf_without = render_at(&without, w, h, 0.5, 1.0);
        let buf_with = render_at(&with_blur, w, h, 0.5, 1.0);

        let span_without = lit_column_span(&buf_without, w as usize, h as usize, 30);
        let span_with = lit_column_span(&buf_with, w as usize, h as usize, 30);

        assert!(
            span_with > span_without,
            "motion_blur (samples=6) should broaden horizontal span: without={span_without}, with={span_with}"
        );
    }

    /// samples=1 is the degenerate case — behaves identically to no blur.
    /// We relax this: samples=1 produces one ghost but ghost and principal
    /// overlap at t_0 - 0 = t_0. Result should be visually equivalent
    /// (total span ≤ span_without + 5 pixels of rounding noise).
    #[test]
    fn motion_blur_samples_1_is_degenerate() {
        let w = 500u32;
        let h = 300u32;
        let no_effect = {
            // A plain shape with no motion_blur at all
            let json = serde_json::json!({
                "type": "shape",
                "shape": "rect",
                "fill": "#ff0000",
                "style": {
                    "width": "80px",
                    "height": "80px",
                    "animation": [{ "name": "slide_in_left", "duration": 1.0 }]
                }
            });
            let component: Component = serde_json::from_value(json).unwrap();
            vec![ChildComponent {
                component,
                position: Some(PositionMode::Absolute { x: 200.0, y: 110.0 }),
                x: None,
                y: None,
                z_index: None,
                bleed: false,
            }]
        };
        let with_1 = make_motion_blur_scene(1, 1.0);

        let buf_no = render_at(&no_effect, w, h, 0.5, 1.0);
        let buf_1 = render_at(&with_1, w, h, 0.5, 1.0);

        let span_no = lit_column_span(&buf_no, w as usize, h as usize, 30);
        let span_1 = lit_column_span(&buf_1, w as usize, h as usize, 30);

        assert!(
            span_1 <= span_no + 5,
            "motion_blur samples=1 should match no-blur (span_no={span_no}, span_1={span_1})"
        );
    }

    /// Static component (no transform animation) + motion_blur → ghosts overlap
    /// exactly. The render should look the same as without blur except possibly
    /// for a reduced opacity of the principal (opacity accumulation).
    /// We verify: no horizontal broadening and the image is not black.
    #[test]
    fn motion_blur_static_component_no_broadening() {
        let w = 500u32;
        let h = 300u32;

        let no_blur = {
            let json = serde_json::json!({
                "type": "shape",
                "shape": "rect",
                "fill": "#ff0000",
                "style": { "width": "80px", "height": "80px" }
            });
            let component: Component = serde_json::from_value(json).unwrap();
            vec![ChildComponent {
                component,
                position: Some(PositionMode::Absolute { x: 200.0, y: 110.0 }),
                x: None,
                y: None,
                z_index: None,
                bleed: false,
            }]
        };
        let with_blur = {
            let json = serde_json::json!({
                "type": "shape",
                "shape": "rect",
                "fill": "#ff0000",
                "style": {
                    "width": "80px",
                    "height": "80px",
                    "animation": [
                        { "name": "motion_blur", "intensity": 1.0, "samples": 6 }
                    ]
                }
            });
            let component: Component = serde_json::from_value(json).unwrap();
            vec![ChildComponent {
                component,
                position: Some(PositionMode::Absolute { x: 200.0, y: 110.0 }),
                x: None,
                y: None,
                z_index: None,
                bleed: false,
            }]
        };

        let buf_no = render_at(&no_blur, w, h, 0.5, 1.0);
        let buf_blur = render_at(&with_blur, w, h, 0.5, 1.0);

        let span_no = lit_column_span(&buf_no, w as usize, h as usize, 30);
        let span_blur = lit_column_span(&buf_blur, w as usize, h as usize, 30);

        // No broadening (ghosts superimpose).
        assert!(
            span_blur <= span_no + 5,
            "static + motion_blur should not broaden: span_no={span_no}, span_blur={span_blur}"
        );

        // Image must not be black (ghosts accumulate enough opacity).
        let max_r = max_red(&buf_blur);
        assert!(
            max_r > 50,
            "static + motion_blur must still render visibly (max_r={max_r})"
        );
    }

    /// Trail with copies=3 should produce a wider horizontal span than without,
    /// and the leading edge (rightmost in slide_in_left animation) should be
    /// brighter than the trailing edge.
    #[test]
    fn trail_produces_multiple_distinct_blobs() {
        let w = 600u32;
        let h = 300u32;

        // At t=0.5 the shape is at mid-slide. Trail ghosts are at
        // t-spacing, t-2*spacing, t-3*spacing → further left.
        let no_trail = {
            let json = serde_json::json!({
                "type": "shape",
                "shape": "rect",
                "fill": "#ff0000",
                "style": {
                    "width": "60px",
                    "height": "60px",
                    "animation": [{ "name": "slide_in_left", "duration": 1.0 }]
                }
            });
            let component: Component = serde_json::from_value(json).unwrap();
            vec![ChildComponent {
                component,
                position: Some(PositionMode::Absolute { x: 300.0, y: 120.0 }),
                x: None,
                y: None,
                z_index: None,
                bleed: false,
            }]
        };
        let with_trail = make_trail_scene(3, 0.1, 0.6);

        let buf_no = render_at(&no_trail, w, h, 0.5, 1.0);
        let buf_trail = render_at(&with_trail, w, h, 0.5, 1.0);

        let span_no = lit_column_span(&buf_no, w as usize, h as usize, 20);
        let span_trail = lit_column_span(&buf_trail, w as usize, h as usize, 20);

        assert!(
            span_trail > span_no,
            "trail (copies=3) should broaden horizontal span: span_no={span_no}, span_trail={span_trail}"
        );
    }
}

// ─── Post-effects pipeline tests ──────────────────────────────────────────────

#[cfg(test)]
mod post_effects_pipeline {
    use crate::encode::video::{build_frame_tasks, render_frame_task, FrameTask};
    use crate::loader::load_scenario_from_source;

    /// Render frame 0 of the first scene in a scenario JSON string.
    fn render_first_frame(json: &str) -> Vec<u8> {
        let scenario = load_scenario_from_source(None, Some(json)).expect("load");
        let tasks = build_frame_tasks(&scenario);
        let task = tasks
            .iter()
            .find(|t| matches!(t, FrameTask::Normal { .. }))
            .expect("normal task");
        render_frame_task(&scenario.video, &scenario, task).expect("render")
    }

    #[test]
    fn vignette_makes_corners_darker_than_center() {
        // A scene with strong vignette on a light background — corners must be darker.
        // Using JSON string concatenation to avoid raw string + hex conflicts.
        let bg = "#ffffff";
        let with_vignette = format!(
            r#"{{"video":{{"width":100,"height":100,"background":"{bg}"}},"scenes":[{{"duration":1.0,"effects":[{{"type":"vignette","intensity":0.9,"radius":0.3}}],"children":[]}}]}}"#
        );
        let without_vignette = format!(
            r#"{{"video":{{"width":100,"height":100,"background":"{bg}"}},"scenes":[{{"duration":1.0,"children":[]}}]}}"#
        );

        let buf_v = render_first_frame(&with_vignette);
        let buf_plain = render_first_frame(&without_vignette);

        // Top-left corner pixel (0, 0) — RGBA at byte 0
        let corner_r_vignette = buf_v[0] as u16;
        let corner_r_plain = buf_plain[0] as u16;
        assert!(
            corner_r_vignette < corner_r_plain,
            "vignette corner must be darker: vignette={corner_r_vignette} plain={corner_r_plain}"
        );

        // Centre pixel (50, 50) in a 100×100 image
        let center_base = (50 * 100 + 50) * 4;
        let center_r_vignette = buf_v[center_base] as u16;
        let center_r_plain = buf_plain[center_base] as u16;
        assert!(
            center_r_vignette >= center_r_plain.saturating_sub(5),
            "vignette center should be approximately unchanged: vignette={center_r_vignette} plain={center_r_plain}"
        );
    }

    #[test]
    fn scene_effects_field_defaults_to_empty() {
        // A scene without an effects key must parse without error and render cleanly.
        let json =
            r#"{"video":{"width":32,"height":32},"scenes":[{"duration":1.0,"children":[]}]}"#;
        let scenario = load_scenario_from_source(None, Some(json)).expect("load");
        let scene = &scenario.views[0].scenes[0];
        assert!(
            scene.effects.is_empty(),
            "effects must default to empty Vec"
        );
    }

    #[test]
    fn grain_effect_changes_buffer_vs_no_effect() {
        // Grain at high intensity should change pixels on a plain background.
        let bg = "#808080";
        let with_grain = format!(
            r#"{{"video":{{"width":32,"height":32,"background":"{bg}"}},"scenes":[{{"duration":1.0,"effects":[{{"type":"grain","intensity":0.5,"seed":42,"animated":false}}],"children":[]}}]}}"#
        );
        let without = format!(
            r#"{{"video":{{"width":32,"height":32,"background":"{bg}"}},"scenes":[{{"duration":1.0,"children":[]}}]}}"#
        );
        let a = render_first_frame(&with_grain);
        let b = render_first_frame(&without);
        assert_ne!(a, b, "grain effect must change the buffer");
    }

    #[test]
    fn post_effect_schema_deserializes_all_variants() {
        use rustmotion_core::schema::scenario::PostEffect;
        let cases = [
            r#"{"type":"grain","intensity":0.2,"seed":10,"animated":true}"#,
            r#"{"type":"vignette","intensity":0.6,"radius":0.8}"#,
            r#"{"type":"pixelate","size":8}"#,
            r#"{"type":"progressive_blur","direction":"bottom","start":0.5,"max_radius":12.0}"#,
            r#"{"type":"progressive_blur","direction":"top","start":0.3,"max_radius":8.0}"#,
        ];
        for case in &cases {
            serde_json::from_str::<PostEffect>(case)
                .unwrap_or_else(|e| panic!("failed: {e}\nJSON: {case}"));
        }
    }

    #[test]
    fn post_effect_unknown_type_fails() {
        // Unknown type tags must produce a deserialization error.
        use rustmotion_core::schema::scenario::PostEffect;
        let bad = r#"{"type":"unknown_effect"}"#;
        let result = serde_json::from_str::<PostEffect>(bad);
        assert!(
            result.is_err(),
            "unknown effect type must fail to deserialize"
        );
    }
}

#[cfg(test)]
mod camera_focal_tests {
    //! Issue #89: camera focal point (`camera.origin`) — the zoom/rotation
    //! pivot becomes configurable and keyframable instead of the hard-coded
    //! frame centre.

    use crate::engine::render::render_frame_v2;
    use crate::schema::{Scene, VideoConfig};

    fn config(w: u32, h: u32) -> VideoConfig {
        serde_json::from_value(serde_json::json!({ "width": w, "height": h, "fps": 30 }))
            .expect("config")
    }

    /// Render one frame of a scene described as JSON; returns RGBA bytes.
    pub(super) fn render_scene_json(
        scene_json: serde_json::Value,
        w: u32,
        h: u32,
        frame: u32,
    ) -> Vec<u8> {
        let scene: Scene = serde_json::from_value(scene_json).expect("scene json");
        let children = crate::engine::render::deserialize_children(&scene);
        render_frame_v2(&config(w, h), &scene, frame, 120, &children).expect("render")
    }

    /// Centroid (x, y) of pixels dominated by the given channel (0=r, 2=b).
    pub(super) fn channel_centroid(buf: &[u8], w: u32, h: u32, channel: usize) -> (f32, f32) {
        let (mut sx, mut sy, mut n) = (0.0f64, 0.0f64, 0.0f64);
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                let v = buf[i + channel];
                let others: u16 = (0..3)
                    .filter(|c| *c != channel)
                    .map(|c| buf[i + c] as u16)
                    .sum();
                if v > 180 && others < 160 {
                    sx += x as f64;
                    sy += y as f64;
                    n += 1.0;
                }
            }
        }
        if n == 0.0 {
            (-1.0, -1.0)
        } else {
            ((sx / n) as f32, (sy / n) as f32)
        }
    }

    fn red_rect_scene_with_camera(camera: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "duration": 4.0,
            "camera": camera,
            "children": [{
                "type": "shape",
                "shape": "rect",
                "fill": "#ff0000",
                "position": "absolute",
                "x": 60, "y": 40,
                "style": { "width": "100px", "height": "80px" }
            }]
        })
    }

    #[test]
    fn zoom_origin_top_left_differs_predictably_from_center() {
        // Rect at (60..160, 40..120) in a 400x300 frame, zoom 2x.
        // Origin (0,0): every point p maps to 2p → rect at (120..320, 80..240),
        // centroid ~(220, 160).
        // Center origin (200,150): p → 2p - (200,150) → rect at
        // (-80..120, -70..90), visible part (0..120, 0..90), centroid ~(60, 45).
        let buf_center = render_scene_json(
            red_rect_scene_with_camera(serde_json::json!({ "zoom": 2.0 })),
            400,
            300,
            0,
        );
        let buf_tl = render_scene_json(
            red_rect_scene_with_camera(
                serde_json::json!({ "zoom": 2.0, "origin": { "x": 0, "y": 0 } }),
            ),
            400,
            300,
            0,
        );

        let (cx_c, cy_c) = channel_centroid(&buf_center, 400, 300, 0);
        let (cx_tl, cy_tl) = channel_centroid(&buf_tl, 400, 300, 0);

        assert!(
            (cx_tl - 219.5).abs() < 4.0 && (cy_tl - 159.5).abs() < 4.0,
            "top-left origin zoom: expected centroid ~(220,160), got ({cx_tl},{cy_tl})"
        );
        assert!(
            (cx_c - 59.5).abs() < 4.0 && (cy_c - 44.5).abs() < 4.0,
            "center origin zoom: expected centroid ~(60,45), got ({cx_c},{cy_c})"
        );
    }

    #[test]
    fn origin_at_center_is_byte_identical_to_absent() {
        let buf_absent = render_scene_json(
            red_rect_scene_with_camera(serde_json::json!({ "zoom": 2.0, "rotation": 17.0 })),
            400,
            300,
            0,
        );
        let buf_center = render_scene_json(
            red_rect_scene_with_camera(serde_json::json!({
                "zoom": 2.0, "rotation": 17.0, "origin": { "x": 200, "y": 150 }
            })),
            400,
            300,
            0,
        );
        assert_eq!(
            buf_absent, buf_center,
            "origin at frame centre must be byte-identical to absent origin"
        );
    }

    #[test]
    fn keyframed_origin_moves_visible_content_at_fixed_zoom() {
        // Zoom fixed at 2; origin animates (0,0) → frame centre (200,150)
        // over 2s. The pivot change alone must move the rendered content
        // between frames while keeping the rect visible in both.
        let scene = red_rect_scene_with_camera(serde_json::json!({
            "zoom": 2.0,
            "keyframes": [
                { "property": "origin.x", "values": [ { "time": 0.0, "value": 0.0 }, { "time": 2.0, "value": 200.0 } ] },
                { "property": "origin.y", "values": [ { "time": 0.0, "value": 0.0 }, { "time": 2.0, "value": 150.0 } ] }
            ]
        }));
        let buf_t0 = render_scene_json(scene.clone(), 400, 300, 0);
        let buf_t2 = render_scene_json(scene, 400, 300, 60); // 60 / 30fps = 2s

        let (x0, y0) = channel_centroid(&buf_t0, 400, 300, 0);
        let (x2, y2) = channel_centroid(&buf_t2, 400, 300, 0);
        assert!(x0 >= 0.0 && x2 >= 0.0, "red must be visible in both frames");
        let dist = ((x2 - x0).powi(2) + (y2 - y0).powi(2)).sqrt();
        assert!(
            dist > 50.0,
            "keyframed origin must move content: t0=({x0},{y0}) t2=({x2},{y2}) dist={dist}"
        );
    }
}

#[cfg(test)]
mod parallax_tests {
    //! Issue #90: multi-plane parallax — `style.depth` scales the scene
    //! camera per top-level plane (0 = locked, 1 = normal, >1 = amplified).

    use super::camera_focal_tests::{channel_centroid, render_scene_json};

    fn rect(color: &str, x: f32, y: f32, depth: Option<f64>) -> serde_json::Value {
        let mut style = serde_json::json!({ "width": "80px", "height": "60px" });
        if let Some(d) = depth {
            style["depth"] = serde_json::json!(d);
        }
        serde_json::json!({
            "type": "shape", "shape": "rect", "fill": color,
            "position": "absolute", "x": x, "y": y,
            "style": style
        })
    }

    fn scene(camera: serde_json::Value, children: Vec<serde_json::Value>) -> serde_json::Value {
        serde_json::json!({ "duration": 4.0, "camera": camera, "children": children })
    }

    #[test]
    fn depth_zero_locks_plane_while_depth_one_pans() {
        // Camera pans x 0 → 100 over 2s. Blue rect depth 0 must not move;
        // red rect (depth 1, activated by the blue's explicit depth) moves
        // left by 100.
        let cam = serde_json::json!({
            "keyframes": [
                { "property": "x", "values": [ { "time": 0.0, "value": 0.0 }, { "time": 2.0, "value": 100.0 } ] }
            ]
        });
        let children = vec![
            rect("#0000ff", 40.0, 40.0, Some(0.0)),
            rect("#ff0000", 240.0, 150.0, Some(1.0)),
        ];
        let s = scene(cam, children);
        let t0 = render_scene_json(s.clone(), 400, 300, 0);
        let t2 = render_scene_json(s, 400, 300, 60);

        let (bx0, by0) = channel_centroid(&t0, 400, 300, 2);
        let (bx2, by2) = channel_centroid(&t2, 400, 300, 2);
        let (rx0, _) = channel_centroid(&t0, 400, 300, 0);
        let (rx2, _) = channel_centroid(&t2, 400, 300, 0);

        assert!(
            (bx0 - bx2).abs() < 0.5 && (by0 - by2).abs() < 0.5,
            "depth-0 plane must not move: ({bx0},{by0}) vs ({bx2},{by2})"
        );
        assert!(
            (rx0 - rx2 - 100.0).abs() < 2.0,
            "depth-1 plane must pan by -100: {rx0} -> {rx2}"
        );
    }

    #[test]
    fn depth_two_moves_twice_as_much() {
        // Camera pans x 0 → 50: depth-1 red moves 50, depth-2 green moves 100.
        let cam = serde_json::json!({
            "keyframes": [
                { "property": "x", "values": [ { "time": 0.0, "value": 0.0 }, { "time": 2.0, "value": 50.0 } ] }
            ]
        });
        let children = vec![
            rect("#ff0000", 200.0, 60.0, Some(1.0)),
            rect("#00ff00", 200.0, 180.0, Some(2.0)),
        ];
        let s = scene(cam, children);
        let t0 = render_scene_json(s.clone(), 400, 300, 0);
        let t2 = render_scene_json(s, 400, 300, 60);

        let (rx0, _) = channel_centroid(&t0, 400, 300, 0);
        let (rx2, _) = channel_centroid(&t2, 400, 300, 0);
        let (gx0, _) = channel_centroid(&t0, 400, 300, 1);
        let (gx2, _) = channel_centroid(&t2, 400, 300, 1);

        let red_shift = rx0 - rx2;
        let green_shift = gx0 - gx2;
        assert!(
            (red_shift - 50.0).abs() < 2.0,
            "depth 1 must shift by 50, got {red_shift}"
        );
        assert!(
            (green_shift - 100.0).abs() < 2.0,
            "depth 2 must shift by 100 (2x), got {green_shift}"
        );
    }

    #[test]
    fn depth_one_everywhere_is_byte_identical_to_no_depth() {
        // Explicit depth 1.0 on every child (per-plane camera path) must
        // produce the same bytes as no depth at all (global camera path).
        let cam = serde_json::json!({ "x": 30.0, "y": 10.0, "zoom": 1.5, "rotation": 8.0 });
        let plain = scene(
            cam.clone(),
            vec![
                rect("#ff0000", 100.0, 60.0, None),
                rect("#0000ff", 220.0, 150.0, None),
            ],
        );
        let with_depth = scene(
            cam,
            vec![
                rect("#ff0000", 100.0, 60.0, Some(1.0)),
                rect("#0000ff", 220.0, 150.0, Some(1.0)),
            ],
        );
        let a = render_scene_json(plain, 400, 300, 0);
        let b = render_scene_json(with_depth, 400, 300, 0);
        assert_eq!(
            a, b,
            "depth 1.0 everywhere must be byte-identical to the global camera path"
        );
    }

    #[test]
    fn zoom_does_not_scale_locked_plane() {
        // Camera zoom 2: the depth-0 blue rect keeps its exact size/position
        // (identical pixels to a no-camera render), the depth-1 red grows.
        let with_cam = scene(
            serde_json::json!({ "zoom": 2.0 }),
            vec![
                rect("#0000ff", 20.0, 20.0, Some(0.0)),
                rect("#ff0000", 250.0, 160.0, Some(1.0)),
            ],
        );
        let no_cam = serde_json::json!({
            "duration": 4.0,
            "children": [ rect("#0000ff", 20.0, 20.0, Some(0.0)) ]
        });
        let buf_cam = render_scene_json(with_cam, 400, 300, 0);
        let buf_ref = render_scene_json(no_cam, 400, 300, 0);

        // Blue region (locked plane) identical to the camera-less reference.
        let blue = |buf: &[u8]| -> Vec<u8> {
            let mut out = Vec::new();
            for y in 10..100u32 {
                for x in 10..120u32 {
                    let i = ((y * 400 + x) * 4) as usize;
                    out.extend_from_slice(&buf[i..i + 4]);
                }
            }
            out
        };
        assert_eq!(
            blue(&buf_cam),
            blue(&buf_ref),
            "depth-0 plane must be unaffected by camera zoom"
        );

        // The red rect (depth 1) must be zoomed: with center-origin zoom 2 a
        // rect at (250..330, 160..220) maps to (300..460, 170..290) clipped —
        // its centroid moves right and its visible area differs from 80x60.
        let (rx, _) = channel_centroid(&buf_cam, 400, 300, 0);
        assert!(
            rx > 330.0,
            "depth-1 plane must be zoomed toward bottom-right, centroid x = {rx}"
        );
    }
}

#[cfg(test)]
mod parallax_hitmap_tests {
    //! Studio hit-map under per-plane parallax: rects must follow their
    //! plane's (depth-scaled) camera transform via the canvas matrix.

    use crate::engine::render::render_scene_hits;
    use crate::schema::{Scene, VideoConfig};

    #[test]
    fn hit_rects_follow_their_plane_depth() {
        let config: VideoConfig =
            serde_json::from_value(serde_json::json!({ "width": 400, "height": 300, "fps": 30 }))
                .expect("config");
        // Static camera pan x=100; blue locked (depth 0), red normal (depth 1).
        let scene: Scene = serde_json::from_value(serde_json::json!({
            "duration": 4.0,
            "camera": { "x": 100.0 },
            "children": [
                { "type": "shape", "shape": "rect", "fill": "#0000ff",
                  "position": "absolute", "x": 40, "y": 40,
                  "style": { "width": "80px", "height": "60px", "depth": 0.0 } },
                { "type": "shape", "shape": "rect", "fill": "#ff0000",
                  "position": "absolute", "x": 240, "y": 150,
                  "style": { "width": "80px", "height": "60px", "depth": 1.0 } }
            ]
        }))
        .expect("scene");

        let hits = render_scene_hits(&config, &scene, 0);
        assert_eq!(hits.len(), 2, "expected two component hits");

        // Paint order matches child order: [0] = blue (depth 0), [1] = red.
        let blue = &hits[0].rect;
        let red = &hits[1].rect;
        assert!(
            (blue.x - 40.0).abs() < 0.5,
            "depth-0 hit rect must ignore the camera pan, x = {}",
            blue.x
        );
        assert!(
            (red.x - 140.0).abs() < 0.5,
            "depth-1 hit rect must follow the pan (240 - 100), x = {}",
            red.x
        );
    }
}

// ─── World-view regressions (audit lot "transitions", constats 2/7/8) ────────

#[cfg(test)]
mod world_view_regressions {
    use crate::encode::video::{build_frame_tasks, render_frame_task, FrameTask};
    use crate::engine::render::{
        render_scene_bg_scaled, render_scene_fg_scaled, render_scene_frame_scaled,
        render_scene_hits,
    };
    use crate::loader::load_scenario_from_source;
    use crate::schema::ResolvedScenario;

    fn scenario(json: &str) -> ResolvedScenario {
        load_scenario_from_source(None, Some(json)).expect("load")
    }

    fn avg_luma(buf: &[u8]) -> f64 {
        let mut sum = 0u64;
        let mut n = 0u64;
        for px in buf.chunks_exact(4) {
            sum += px[0] as u64 + px[1] as u64 + px[2] as u64;
            n += 3;
        }
        sum as f64 / n as f64
    }

    // Constat 2: the world-view background crossfade must genuinely fade the
    // outgoing scene's background during the pan, and must not jump when the
    // pan ends and the renderer switches from the two-scene crossfade branch
    // to the single-active-scene branch.
    #[test]
    fn outgoing_world_background_fades_gradually_instead_of_holding_then_jumping() {
        let json = r##"{
            "video": { "width": 320, "height": 180, "fps": 30, "background": "#000000" },
            "composition": [
                { "type": "world", "camera_pan_duration": 0.8, "camera_easing": "linear",
                  "scenes": [
                    { "duration": 2.0, "children": [],
                      "background": { "preset": "halo", "zones": [
                        { "color": "#FFFFFF80", "x": 0.5, "y": 0.5, "radius": 3.0 }
                      ] } },
                    { "duration": 2.0, "children": [],
                      "background": { "preset": "halo", "zones": [
                        { "color": "#00000000", "x": 0.5, "y": 0.5, "radius": 3.0 }
                      ] } }
                  ] }
            ]
        }"##;
        let scenario = scenario(json);
        let tasks = build_frame_tasks(&scenario);
        let fps = scenario.video.fps;

        // Pan window: boundary at t=2.0, half=0.4s -> [1.6, 2.4]. Sample a
        // margin either side too, at frame granularity (the actual render
        // grain), from t=1.5 to t=2.5.
        let render_at = |t: f64| {
            let f = (t * fps as f64).round() as usize;
            let task = tasks
                .iter()
                .find(|task| matches!(task, FrameTask::WorldFrame { frame_in_view, .. } if *frame_in_view as usize == f))
                .unwrap_or_else(|| panic!("no WorldFrame task for frame {f} (t={t})"));
            render_frame_task(&scenario.video, &scenario, task).unwrap()
        };

        let mut samples = Vec::new();
        let start_f = (1.5 * fps as f64).round() as i32;
        let end_f = (2.5 * fps as f64).round() as i32;
        for f in start_f..=end_f {
            let t = f as f64 / fps as f64;
            samples.push((f, avg_luma(&render_at(t))));
        }

        // No single-frame jump: the old bug held scene A's halo at full
        // alpha for the entire pan, then a hard cut when the "single active
        // scene" branch took over at the pan's end.
        let mut max_jump = 0.0_f64;
        let mut worst = (0, 0);
        for w in samples.windows(2) {
            let jump = (w[1].1 - w[0].1).abs();
            if jump > max_jump {
                max_jump = jump;
                worst = (w[0].0, w[1].0);
            }
        }
        assert!(
            max_jump < 15.0,
            "avg-luma jump of {max_jump:.1} between frames {worst:?} — background must fade \
             gradually, not hold then cut. Samples: {samples:?}"
        );

        // Genuine fade, not a flat hold: the value partway through the pan
        // must sit strictly between the pre-pan and post-pan levels, not
        // equal either endpoint (the "never fades" half of the bug).
        let pre = samples.first().unwrap().1;
        let post = samples.last().unwrap().1;
        let mid = samples[samples.len() / 2].1;
        assert!(
            (mid - pre).abs() > 1.0 && (mid - post).abs() > 1.0,
            "mid-pan luma {mid:.1} must differ meaningfully from both pre-pan {pre:.1} and \
             post-pan {post:.1} — background never faded if it matches either endpoint"
        );
    }

    // Constat 7: `freeze_at` on a world-view scene must stop its animated
    // content exactly like it does in a slide view, not keep advancing.
    #[test]
    fn freeze_at_stops_animation_inside_a_world_view() {
        let json = r##"{
            "video": { "width": 200, "height": 200, "fps": 30, "background": "#000000" },
            "composition": [
                { "type": "world", "scenes": [
                    { "duration": 2.0, "freeze_at": 0.5, "children": [
                        { "type": "counter", "from": 0, "to": 200,
                          "style": { "font-size": 48, "color": "#ffffff" } }
                    ] }
                ] }
            ]
        }"##;
        let scenario = scenario(json);
        let tasks = build_frame_tasks(&scenario);
        // 2.0s @ 30fps = 60 WorldFrame tasks; freeze_at=0.5s = frame 15.
        assert_eq!(tasks.len(), 60);

        let render = |i: usize| render_frame_task(&scenario.video, &scenario, &tasks[i]).unwrap();
        let before_freeze = render(5); // t ~= 0.167s, counter still climbing
        let after_freeze_a = render(45); // t = 1.5s, well past freeze_at
        let after_freeze_b = render(55); // t ~= 1.833s, also well past freeze_at

        assert_ne!(
            before_freeze, after_freeze_a,
            "counter must have visibly changed before the freeze point"
        );
        assert_eq!(
            after_freeze_a, after_freeze_b,
            "frames 45 and 55 are both past freeze_at=0.5s and must be pixel-identical \
             (the counter must have stopped, not kept incrementing)"
        );
    }

    // Issue #164: `freeze_at` was applied by hand in five different render
    // paths inside `crates/rustmotion/src/engine/render/scene.rs`, and
    // nothing ever asserted the five agree — PR #152 only tested the world
    // path it was repairing (the test above). This is that missing test,
    // written before the `SceneTime` consolidation refactor: a scene whose
    // animated content spans everything a "frozen" frame must actually hold
    // still — the component tree (a counter), the per-scene camera (an x
    // keyframe), and the animated background (`gradient_shift`) — so a path
    // that forgot the clamp anywhere would show it.
    #[test]
    fn freeze_at_produces_the_same_frame_on_every_render_path() {
        let slide_json = r##"{
            "video": { "width": 200, "height": 200, "fps": 30, "background": "#000000" },
            "scenes": [
                { "duration": 2.0, "freeze_at": 0.5,
                  "background": { "preset": "gradient_shift",
                                   "colors": ["#101020", "#4422aa"], "speed": 60 },
                  "camera": { "keyframes": [
                      { "property": "x", "values": [
                          { "time": 0.0, "value": 0.0 }, { "time": 2.0, "value": 80.0 }
                      ] }
                  ] },
                  "children": [
                      { "type": "counter", "from": 0, "to": 200,
                        "style": { "font-size": 48, "color": "#ffffff" } }
                  ] }
            ]
        }"##;
        let slide = scenario(slide_json);
        let config = &slide.video;
        let scenes = slide.all_scenes_vec();
        let scene = scenes[0];

        // Frame 5 (t~0.167s) is before freeze_at=0.5s: content still moving.
        // Frames 45 (t=1.5s) and 55 (t~1.833s) are both well past it.
        let (pre, post_a, post_b) = (5u32, 45u32, 55u32);

        let render_full = |f: u32| render_scene_frame_scaled(config, scene, f, 60, 1.0).unwrap();
        let render_bg = |f: u32| render_scene_bg_scaled(config, scene, f, 1.0).unwrap();
        let render_fg = |f: u32| render_scene_fg_scaled(config, scene, f, 60, 1.0).unwrap();

        let pixel_paths: [(&str, &dyn Fn(u32) -> Vec<u8>); 3] = [
            ("render_scene_frame_scaled", &render_full),
            ("render_scene_bg_scaled", &render_bg),
            ("render_scene_fg_scaled", &render_fg),
        ];
        for (name, render) in pixel_paths {
            let before = render(pre);
            let after_a = render(post_a);
            let after_b = render(post_b);
            assert_ne!(
                before, after_a,
                "{name}: frame {pre} (pre-freeze) must differ from frame {post_a} (post-freeze)"
            );
            assert_eq!(
                after_a, after_b,
                "{name}: frames {post_a} and {post_b} are both past freeze_at=0.5s and must be \
                 pixel-identical"
            );
        }

        // render_scene_hits: a different output shape (bounding boxes, not
        // pixels) but the same claim — the camera pan (and therefore every
        // hit rect) must stop moving past the freeze point.
        let hit_rects = |f: u32| -> Vec<_> {
            render_scene_hits(config, scene, f)
                .into_iter()
                .map(|h| h.rect)
                .collect::<Vec<_>>()
        };
        let hits_pre = hit_rects(pre);
        let hits_post_a = hit_rects(post_a);
        let hits_post_b = hit_rects(post_b);
        assert_ne!(
            hits_pre, hits_post_a,
            "render_scene_hits: hit rects at frame {pre} (pre-freeze, camera still panning) \
             must differ from frame {post_a}"
        );
        assert_eq!(
            hits_post_a, hits_post_b,
            "render_scene_hits: hit rects at frames {post_a} and {post_b} (both past \
             freeze_at) must be identical — the camera pan must have stopped"
        );

        // render_world_frame_scaled: same scene, wrapped in a world view so
        // the world-specific freeze copy (the one PR #152 added — `.min()`
        // instead of the other four's `if`) is exercised too.
        let world_json = r##"{
            "video": { "width": 200, "height": 200, "fps": 30, "background": "#000000" },
            "composition": [
                { "type": "world", "scenes": [
                    { "duration": 2.0, "freeze_at": 0.5,
                      "background": { "preset": "gradient_shift",
                                       "colors": ["#101020", "#4422aa"], "speed": 60 },
                      "camera": { "keyframes": [
                          { "property": "x", "values": [
                              { "time": 0.0, "value": 0.0 }, { "time": 2.0, "value": 80.0 }
                          ] }
                      ] },
                      "children": [
                          { "type": "counter", "from": 0, "to": 200,
                            "style": { "font-size": 48, "color": "#ffffff" } }
                      ] }
                ] }
            ]
        }"##;
        let world = scenario(world_json);
        let tasks = build_frame_tasks(&world);
        let world_render = |frame_in_view: u32| -> Vec<u8> {
            let task = tasks
                .iter()
                .find(
                    |t| matches!(t, FrameTask::WorldFrame { frame_in_view: f, .. } if *f == frame_in_view),
                )
                .unwrap_or_else(|| panic!("no WorldFrame task for frame {frame_in_view}"));
            render_frame_task(&world.video, &world, task).unwrap()
        };
        let w_before = world_render(pre);
        let w_after_a = world_render(post_a);
        let w_after_b = world_render(post_b);
        assert_ne!(
            w_before, w_after_a,
            "render_world_frame_scaled: frame {pre} (pre-freeze) must differ from frame {post_a}"
        );
        assert_eq!(
            w_after_a, w_after_b,
            "render_world_frame_scaled: frames {post_a} and {post_b} (both past freeze_at) \
             must be pixel-identical"
        );
    }

    // Constat 8a: `scene.effects` (post-effects) must apply on WorldFrame
    // tasks, not just Normal/SlideTransition ones.
    #[test]
    fn post_effects_apply_on_world_frames() {
        let json = r##"{
            "video": { "width": 100, "height": 100, "background": "#ffffff" },
            "composition": [
                { "type": "world", "scenes": [
                    { "duration": 1.0, "children": [],
                      "effects": [ { "type": "vignette", "intensity": 0.9, "radius": 0.3 } ] }
                ] }
            ]
        }"##;
        let scenario = scenario(json);
        let tasks = build_frame_tasks(&scenario);
        let task = tasks
            .iter()
            .find(|t| matches!(t, FrameTask::WorldFrame { .. }))
            .expect("world frame task");
        let buf = render_frame_task(&scenario.video, &scenario, task).unwrap();

        let corner_r = buf[0] as u16;
        let center_base = (50 * 100 + 50) * 4;
        let center_r = buf[center_base] as u16;
        assert!(
            corner_r < center_r,
            "vignette must darken the corner of a WorldFrame: corner={corner_r} center={center_r}"
        );
    }

    // Constat 8b: a ViewTransition composite must carry the incoming view's
    // effects too, by symmetry with SlideTransition (so an effect present on
    // both sides' Normal frames doesn't disappear for the transition and pop
    // back).
    #[test]
    fn post_effects_apply_on_view_transition_frames() {
        let json = r##"{
            "video": { "width": 100, "height": 100, "fps": 10, "background": "#ffffff" },
            "composition": [
                { "type": "slide", "scenes": [
                    { "duration": 0.5, "children": [],
                      "effects": [ { "type": "vignette", "intensity": 0.9, "radius": 0.3 } ] }
                ] },
                { "type": "slide", "transition": { "type": "fade", "duration": 0.3 },
                  "scenes": [
                    { "duration": 0.5, "children": [],
                      "effects": [ { "type": "vignette", "intensity": 0.9, "radius": 0.3 } ] }
                ] }
            ]
        }"##;
        let scenario = scenario(json);
        let tasks = build_frame_tasks(&scenario);
        let task = tasks
            .iter()
            .find(|t| matches!(t, FrameTask::ViewTransition { .. }))
            .expect("view transition task");
        let buf = render_frame_task(&scenario.video, &scenario, task).unwrap();

        let corner_r = buf[0] as u16;
        let center_base = (50 * 100 + 50) * 4;
        let center_r = buf[center_base] as u16;
        assert!(
            corner_r < center_r,
            "vignette must darken the corner of a ViewTransition frame: corner={corner_r} center={center_r}"
        );
    }
}
