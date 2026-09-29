#[cfg(test)]
mod component_smoke {
    use crate::components::Component;

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
        ("emitter", r#"{"type":"emitter"}"#),
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
        ("success_check", r#"{"type":"success_check"}"#),
        ("number_wheel", r#"{"type":"number_wheel","value":"30222"}"#),
        (
            "pointer",
            r#"{"type":"pointer","path":[{"time":0.0,"x":0.0,"y":0.0},{"time":1.0,"x":200.0,"y":120.0}]}"#,
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
            "timeline",
            r#"{"type":"timeline","steps":[{"label":"Start"}]}"#,
        ),
        (
            "treemap",
            r#"{"type":"treemap","data":[{"value":10,"label":"A"}]}"#,
        ),
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
        ("pill_nav", r#"{"type":"pill_nav","items":["A","B"]}"#),
        ("tooltip", r#"{"type":"tooltip","text":"hi"}"#),
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

    const COMPONENT_ALIASES: &[(&str, &str)] = &[
        ("container", "div"),
        ("card", "div"),
        ("flex", "div"),
        ("grid", "div"),
        ("positioned", "div"),
        ("progress_bar", "progress"),
    ];

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

        assert!(
            serde_json::from_str::<ShapeType>(r#""star""#).is_err(),
            "a bare `star` must stay an error: it is what the old wording produced"
        );
    }

    #[test]
    fn all_components_serde_round_trip() {
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
        let bad = r#"{"type":"stepper","orientation":"diagonal","steps":[{"label":"A"}]}"#;
        assert!(serde_json::from_str::<Component>(bad).is_err());
    }

    #[test]
    fn corpus_covers_every_component_tag() {
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
            light: Default::default(),
            time: 0.5,
            scenario_time: 0.5,
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
                id: None,
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
        use crate::components::{ChildComponent, PositionMode};
        use rustmotion_components::box_builder::build_scene;
        use rustmotion_components::container::ContainerComponent;
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
            light: Default::default(),
            time: 0.0,
            scenario_time: 0.0,
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
                id: None,
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
                id: None,
                component: Component::Container(ContainerComponent {
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

    fn render_new(children: &[crate::components::ChildComponent], w: u32, h: u32) -> Vec<u8> {
        render_new_at(children, w, h, 0.5, 1.0)
    }

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
                scenario_time: time,
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
            light: Default::default(),
            time,
            scenario_time: time,
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

    fn nonzero_pixels(buf: &[u8]) -> usize {
        buf.as_chunks::<4>().0.iter().filter(|p| p[3] > 0).count()
    }

    #[test]
    fn new_pipeline_produces_pixels_for_text_in_card() {
        let json = serde_json::json!({
            "type": "card",
            "style": { "padding": 24, "background": "#1a1a2e" },
            "children": [
                { "type": "text", "content": "Hello", "style": { "color": "#ffffff", "font-size": 48 } }
            ]
        });
        let component: Component = serde_json::from_value(json).expect("deserialize");
        let child = crate::components::ChildComponent {
            id: None,
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
            id: None,
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
        let red_sum: fn(&[u8]) -> u64 =
            |buf| buf.as_chunks::<4>().0.iter().map(|p| p[0] as u64).sum();
        let early_red = red_sum(&early);
        let late_red = red_sum(&late);
        assert!(
            late_red > early_red * 3,
            "FadeIn did not amplify red over time (early={early_red}, late={late_red})"
        );
    }

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
            id: None,
            component,
            position: Some(crate::components::PositionMode::Absolute { x: 60.0, y: 40.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        }]
    }

    fn red_sum(buf: &[u8]) -> u64 {
        buf.as_chunks::<4>().0.iter().map(|p| p[0] as u64).sum()
    }

    #[test]
    fn start_at_hides_component_before_its_window() {
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
            id: None,
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
        let sharp = red_rect_scene(serde_json::json!({}));
        let blurred = red_rect_scene(serde_json::json!({
            "style": {
                "width": "100px",
                "height": "80px",
                "filter": [{ "fn": "blur", "radius": 12 }]
            }
        }));
        let count_mid = |buf: &[u8]| {
            buf.as_chunks::<4>()
                .0
                .iter()
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
        let inverted = red_rect_scene(serde_json::json!({
            "style": {
                "width": "100px",
                "height": "80px",
                "filter": [{ "fn": "invert", "value": 1.0 }]
            }
        }));
        let buf = render_new_at(&inverted, 400, 300, 0.5, 1.0);
        let flipped = buf
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[0] < 50 && p[1] > 150)
            .count();
        assert!(
            flipped > 3000,
            "invert(1) did not flip the rect's colors (flipped px = {flipped})"
        );
    }

    #[test]
    fn container_stagger_offsets_child_animations() {
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
            id: None,
            component,
            position: Some(crate::components::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        };
        let buf = render_new_at(&[child], 300, 200, 0.25, 2.0);
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
            id: None,
            component,
            position: Some(crate::components::PositionMode::Absolute { x: 40.0, y: 40.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        };
        let scene = vec![child];
        let blue_sum =
            |buf: &[u8]| -> u64 { buf.as_chunks::<4>().0.iter().map(|p| p[2] as u64).sum() };
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

    fn red_centroid_x(buf: &[u8], width: u32) -> Option<f64> {
        let mut sum_wx = 0.0_f64;
        let mut sum_w = 0.0_f64;
        for (i, p) in buf.as_chunks::<4>().0.iter().enumerate() {
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
            id: None,
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
        assert!(
            late_lit > early_lit * 10,
            "ScaleIn did not grow over time (early_lit={early_lit}, late_lit={late_lit})"
        );
        assert!(late_lit > 5000, "ScaleIn final frame too small: {late_lit}");
    }

    #[test]
    fn slide_in_left_translates_centroid_in_new_pipeline() {
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
            id: None,
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
        let dx = late_cx - early_cx;
        assert!(
            dx > 100.0,
            "SlideInLeft centroid did not move right (early_cx={early_cx:.1}, late_cx={late_cx:.1}, dx={dx:.1})"
        );
    }

    fn layout_for_unsized_component_in_flex_card(
        component_json: serde_json::Value,
    ) -> rustmotion_core::engine::layout_pass::BoxLayout {
        use crate::components::{ChildComponent, PositionMode};
        use rustmotion_components::box_builder::build_scene;
        use rustmotion_components::container::ContainerComponent;
        use rustmotion_core::css::style::{CssStyle, FlexDirection};
        use rustmotion_core::css::taffy_bridge::ConversionContext;
        use rustmotion_core::engine::layout_pass::run_layout;

        let component: Component = serde_json::from_value(component_json).expect("deserialize");
        let inner = ChildComponent {
            id: None,
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
            id: None,
            component: Component::Container(ContainerComponent {
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

        let component_node_id = {
            built
                .root
                .children
                .first()
                .and_then(|card_node| card_node.children.first())
                .map(|n| n.id)
                .expect("component node id must exist")
        };
        layout
            .get(component_node_id)
            .copied()
            .expect("component must have a layout entry")
    }

    #[test]
    fn table_intrinsic_height_reflects_row_count() {
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
            id: None,
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
                id: None,
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
                id: None,
                component,
                position: Some(crate::components::PositionMode::Absolute { x: 0.0, y: 0.0 }),
                x: None,
                y: None,
                z_index: None,
                bleed: false,
            }]
        };

        let at_3s = render_new_at(&make_scene(), 400, 300, 3.0, 6.0);
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
                id: None,
                component,
                position: Some(crate::components::PositionMode::Absolute { x: 0.0, y: 0.0 }),
                x: None,
                y: None,
                z_index: None,
                bleed: false,
            }]
        };

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

    #[test]
    fn nested_time_scale_and_offset_compose_and_a_frozen_global_time_freezes_the_whole_subtree() {
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
                id: None,
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

#[cfg(test)]
mod svg_draw_on_tests {
    use crate::components::{ChildComponent, Component, PositionMode};
    use rustmotion_components::box_builder::{build_scene_with_anim, BuildAnimationCtx};
    use rustmotion_components::legacy_dispatch::LegacyPaintDispatcher;
    use rustmotion_core::css::taffy_bridge::ConversionContext;
    use rustmotion_core::engine::layout_pass::run_layout;
    use rustmotion_core::engine::paint_pass::{paint_tree, PaintFrame};

    const TWO_STROKE_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
  <line x1="10" y1="30" x2="90" y2="30" stroke="white" stroke-width="4"/>
  <line x1="10" y1="70" x2="90" y2="70" stroke="white" stroke-width="4"/>
</svg>"#;

    const TWO_FILL_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
  <rect x="10" y="10" width="80" height="20" fill="white"/>
  <rect x="10" y="70" width="80" height="20" fill="white"/>
</svg>"#;

    fn render_svg_at(svg_data: &str, extra_fields: serde_json::Value, progress: f64) -> Vec<u8> {
        let mut json = serde_json::json!({
            "type": "svg",
            "data": svg_data,
            "style": { "width": "100px", "height": "100px" }
        });
        if let serde_json::Value::Object(map) = extra_fields {
            for (k, v) in map {
                json[k] = v;
            }
        }
        let component: Component = serde_json::from_value(json).expect("svg deserialize");
        let child = ChildComponent {
            id: None,
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
                scenario_time: progress,
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
            light: Default::default(),
            time: progress,
            scenario_time: progress,
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

    fn lit(buf: &[u8]) -> usize {
        buf.as_chunks::<4>().0.iter().filter(|p| p[3] > 0).count()
    }

    fn lit_band(buf: &[u8], y0: usize, y1: usize) -> usize {
        (y0..y1)
            .flat_map(|y| (0..100usize).map(move |x| (y * 100 + x) * 4))
            .filter(|&i| buf[i + 3] > 0)
            .count()
    }

    #[test]
    fn draw_progress_zero_paints_nothing() {
        let buf = render_svg_at(
            TWO_STROKE_SVG,
            serde_json::json!({
                "style": {
                    "width": "100px",
                    "height": "100px",
                    "animation": [{ "name": "draw_in", "duration": 1.0 }]
                }
            }),
            0.0,
        );
        assert_eq!(
            lit(&buf),
            0,
            "draw_progress=0 must produce zero lit pixels (got {})",
            lit(&buf)
        );
    }

    #[test]
    fn draw_progress_half_shows_first_path_only() {
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

    #[test]
    fn draw_progress_one_renders_complete_svg() {
        let buf = render_svg_at(
            TWO_STROKE_SVG,
            serde_json::json!({
                "style": {
                    "width": "100px",
                    "height": "100px",
                    "animation": [{ "name": "draw_in", "duration": 1.0 }]
                }
            }),
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

    #[test]
    fn draw_overlap_one_draws_all_paths_in_parallel() {
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

    #[test]
    fn fill_only_svg_draws_contour_in_draw_mode() {
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
            0.5,
        );
        assert!(
            lit(&buf) > 0,
            "fill-only SVG should produce lit pixels in draw mode at 0.5 (got {})",
            lit(&buf)
        );
    }

    #[test]
    fn static_svg_renders_without_draw_mode() {
        let filled_svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
  <rect x="10" y="10" width="80" height="80" fill="red"/>
</svg>"#;
        let buf = render_svg_at(filled_svg, serde_json::json!({}), 0.5);
        let red_pixels = buf
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] > 0 && p[0] > p[2] && p[0] > p[1])
            .count();
        assert!(
            red_pixels > 3000,
            "static render must produce many red-dominant pixels (got {red_pixels})"
        );
    }

    #[test]
    fn draw_true_shows_full_trace_without_animation() {
        let buf = render_svg_at(TWO_STROKE_SVG, serde_json::json!({ "draw": true }), 0.5);
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

    pub(super) fn make_sine_wav(
        total_samples: u32,
        sine_samples: u32,
        freq: f32,
        sample_rate: u32,
    ) -> Vec<u8> {
        let data_size = total_samples * 2;
        let mut wav = Vec::<u8>::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_size).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
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
    pub(super) fn nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    fn child_at_origin(json: serde_json::Value) -> crate::components::ChildComponent {
        let component: crate::components::Component =
            serde_json::from_value(json).expect("component json");
        crate::components::ChildComponent {
            id: None,
            component,
            position: Some(crate::components::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        }
    }

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
                scenario_time: time,
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
                light: Default::default(),
                time,
                scenario_time: time,
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
    fn the_analysis_follows_the_mixed_envelope_not_the_raw_file() {
        let sample_rate = 44100u32;
        let wav_path = std::env::temp_dir().join(format!("rustmotion_test_mix_{}.wav", nanos()));
        std::fs::write(
            &wav_path,
            make_sine_wav(sample_rate * 2, sample_rate * 2, 440.0, sample_rate),
        )
        .expect("write fixture");
        let wav_str = wav_path.to_str().unwrap().to_string();

        let json = serde_json::json!({
            "video": {"width": 32, "height": 32, "fps": 30},
            "audio": [{
                "src": wav_str,
                "volume_keyframes": [
                    {"time": 0.0, "volume": 1.0},
                    {"time": 1.0, "volume": 1.0},
                    {"time": 1.05, "volume": 0.0},
                    {"time": 2.0, "volume": 0.0}
                ]
            }],
            "scenes": [{"duration": 2.0, "children": []}]
        })
        .to_string();
        let scenario =
            crate::loader::load_scenario_from_source(None, Some(&json)).expect("load scenario");
        assert!(crate::encode::audio_analysis::analyze_scenario_audio(&scenario).is_empty());

        let analysis = audio_analysis_cache().get(&wav_str).unwrap().clone();
        std::fs::remove_file(&wav_path).ok();

        assert!(
            analysis.amplitude_at(0.5) > 0.5,
            "inside the audible half the envelope is open"
        );
        assert!(
            analysis.amplitude_at(1.5) < 0.05,
            "the keyframes take the track to silence — the visualisation must \
             follow it, not keep drawing the sine underneath"
        );
    }

    #[test]
    fn changing_the_envelope_re_analyses_the_same_file() {
        let sample_rate = 44100u32;
        let wav_path = std::env::temp_dir().join(format!("rustmotion_test_env_{}.wav", nanos()));
        std::fs::write(
            &wav_path,
            make_sine_wav(sample_rate, sample_rate, 440.0, sample_rate),
        )
        .expect("write fixture");
        let wav_str = wav_path.to_str().unwrap().to_string();

        let with_volume = |v: f32| {
            let json = serde_json::json!({
                "video": {"width": 32, "height": 32, "fps": 30},
                "audio": [{"src": wav_str, "volume": v}],
                "scenes": [{"duration": 1.0, "children": []}]
            })
            .to_string();
            let scenario =
                crate::loader::load_scenario_from_source(None, Some(&json)).expect("load");
            crate::encode::audio_analysis::analyze_scenario_audio(&scenario);
            audio_analysis_cache().get(&wav_str).unwrap().amplitude[10]
        };

        let loud = with_volume(1.0);
        let quiet = with_volume(0.0);
        std::fs::remove_file(&wav_path).ok();

        assert!(loud > 0.5, "the full-volume take is audible, got {loud}");
        assert_eq!(
            quiet, 0.0,
            "at volume 0 the analysis must be silent — a stale entry would \
             still report {loud}"
        );
    }

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

    #[test]
    fn a_track_start_offsets_the_analysis_lookup() {
        let sample_rate = 44100u32;
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

        assert_eq!(analysis.amplitude_smoothed(4.9, 3), 0.0);
        assert_eq!(analysis.band_at(4.9, 4), 0.0);
        assert_eq!(analysis.band_smoothed(4.9, 4, 3), 0.0);
    }

    #[test]
    fn a_waveform_reads_the_scenario_clock_not_the_scene_clock() {
        let sample_rate = 44100u32;
        let wav_path = std::env::temp_dir().join(format!("rustmotion_test_clock_{}.wav", nanos()));
        std::fs::write(
            &wav_path,
            make_sine_wav(sample_rate * 4, sample_rate * 3, 440.0, sample_rate),
        )
        .expect("write fixture");
        let wav_str = wav_path.to_str().unwrap().to_string();

        let json = serde_json::json!({
            "video": {"width": 200, "height": 80, "fps": 30, "background": "#000000"},
            "audio": [{"src": wav_str}],
            "scenes": [
                {"duration": 2.0, "children": []},
                {"duration": 2.0, "children": [
                    {"type": "waveform", "track": wav_str, "color": "#ffffff",
                     "draw_style": "filled", "window": 0.5,
                     "style": {"width": 200, "height": 80}}
                ]}
            ]
        })
        .to_string();
        let scenario =
            crate::loader::load_scenario_from_source(None, Some(&json)).expect("load scenario");
        crate::encode::audio_analysis::analyze_scenario_audio(&scenario);

        let tasks = crate::encode::build_frame_tasks(&scenario);
        let lit_at = |frame: usize| {
            crate::encode::video::render_frame_task(&scenario.video, &scenario, &tasks[frame])
                .expect("render")
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[0] > 40)
                .count()
        };

        let audible = lit_at(75);
        let silent = lit_at(105);
        std::fs::remove_file(&wav_path).ok();

        assert!(
            audible > 300,
            "sanity: with sound at that moment the waveform must be drawn, got {audible} lit pixels"
        );
        assert!(
            silent * 3 < audible,
            "the track is silent at 3.5 s of the scenario — a scene-local clock \
             would read 1.5 s, still inside the sine, and draw a waveform for \
             sound nobody hears. audible={audible} silent={silent}"
        );
    }

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

    #[test]
    fn analyze_scenario_audio_reruns_when_the_file_changes() {
        let sample_rate = 44100u32;
        let wav_path =
            std::env::temp_dir().join(format!("rustmotion_test_refresh_{}.wav", nanos()));
        let wav_str = wav_path.to_str().unwrap().to_string();

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

        let lo = 20.0f32.log2();
        let hi = 16000.0f32.log2();
        let expected_band = (((440.0f32.log2() - lo) / ((hi - lo) / 16.0)) as usize).min(15);
        let frame = &analysis.bands[5];
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
            fr[15] = 1.0;
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
        let pixels = paint_scene(child, 400, 200, 0.5, 30);
        let hot_bar = lit_in_columns(&pixels, 400, 378, 400);
        let cold_bar = lit_in_columns(&pixels, 400, 0, 23);
        assert!(
            hot_bar > cold_bar * 10,
            "hot band bar ({hot_bar} lit px) should tower over a cold bar ({cold_bar} lit px)"
        );

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
        let n = 60usize;
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
        let red_sum = |pixels: &[u8]| {
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| p[0] as u64)
                .sum::<u64>()
        };

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

#[cfg(test)]
mod motion_blur_trail {

    use crate::components::{ChildComponent, Component, PositionMode};
    use rustmotion_components::box_builder::{build_scene_with_anim, BuildAnimationCtx};
    use rustmotion_components::legacy_dispatch::LegacyPaintDispatcher;
    use rustmotion_core::css::taffy_bridge::ConversionContext;
    use rustmotion_core::engine::layout_pass::run_layout;
    use rustmotion_core::engine::paint_pass::{paint_tree, PaintFrame};

    const FPS: u32 = 30;

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
                scenario_time: time,
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
            light: Default::default(),
            time,
            scenario_time: time,
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

    fn max_red(pixels: &[u8]) -> u8 {
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| p[0])
            .max()
            .unwrap_or(0)
    }

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
            id: None,
            component,
            position: Some(PositionMode::Absolute { x: 200.0, y: 110.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        }]
    }

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
            id: None,
            component,
            position: Some(PositionMode::Absolute { x: 200.0, y: 110.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        }]
    }

    #[test]
    fn motion_blur_broadens_horizontal_span() {
        let w = 500u32;
        let h = 300u32;
        let without = make_motion_blur_scene(1, 1.0);
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

    #[test]
    fn motion_blur_samples_1_is_degenerate() {
        let w = 500u32;
        let h = 300u32;
        let no_effect = {
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
                id: None,
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
                id: None,
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
                id: None,
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

        assert!(
            span_blur <= span_no + 5,
            "static + motion_blur should not broaden: span_no={span_no}, span_blur={span_blur}"
        );

        let max_r = max_red(&buf_blur);
        assert!(
            max_r > 50,
            "static + motion_blur must still render visibly (max_r={max_r})"
        );
    }

    #[test]
    fn trail_produces_multiple_distinct_blobs() {
        let w = 600u32;
        let h = 300u32;

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
                id: None,
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

#[cfg(test)]
mod post_effects_pipeline {
    use crate::encode::video::{build_frame_tasks, render_frame_task, FrameTask};
    use crate::loader::load_scenario_from_source;

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
        let bg = "#ffffff";
        let with_vignette = format!(
            r#"{{"video":{{"width":100,"height":100,"background":"{bg}"}},"scenes":[{{"duration":1.0,"effects":[{{"type":"vignette","intensity":0.9,"radius":0.3}}],"children":[]}}]}}"#
        );
        let without_vignette = format!(
            r#"{{"video":{{"width":100,"height":100,"background":"{bg}"}},"scenes":[{{"duration":1.0,"children":[]}}]}}"#
        );

        let buf_v = render_first_frame(&with_vignette);
        let buf_plain = render_first_frame(&without_vignette);

        let corner_r_vignette = buf_v[0] as u16;
        let corner_r_plain = buf_plain[0] as u16;
        assert!(
            corner_r_vignette < corner_r_plain,
            "vignette corner must be darker: vignette={corner_r_vignette} plain={corner_r_plain}"
        );

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

    use crate::engine::render::render_frame_v2;
    use crate::schema::{Scene, VideoConfig};

    fn config(w: u32, h: u32) -> VideoConfig {
        serde_json::from_value(serde_json::json!({ "width": w, "height": h, "fps": 30 }))
            .expect("config")
    }

    pub(super) fn render_scene_json(
        scene_json: serde_json::Value,
        w: u32,
        h: u32,
        frame: u32,
    ) -> Vec<u8> {
        let scene: Scene = serde_json::from_value(scene_json).expect("scene json");
        let children = crate::engine::render::deserialize_children(&scene);
        let t = frame as f64 / 30.0;
        render_frame_v2(&config(w, h), &scene, frame, t, 120, &children).expect("render")
    }

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
        let scene = red_rect_scene_with_camera(serde_json::json!({
            "zoom": 2.0,
            "keyframes": [
                { "property": "origin.x", "values": [ { "time": 0.0, "value": 0.0 }, { "time": 2.0, "value": 200.0 } ] },
                { "property": "origin.y", "values": [ { "time": 0.0, "value": 0.0 }, { "time": 2.0, "value": 150.0 } ] }
            ]
        }));
        let buf_t0 = render_scene_json(scene.clone(), 400, 300, 0);
        let buf_t2 = render_scene_json(scene, 400, 300, 60);

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

        let (rx, _) = channel_centroid(&buf_cam, 400, 300, 0);
        assert!(
            rx > 330.0,
            "depth-1 plane must be zoomed toward bottom-right, centroid x = {rx}"
        );
    }
}

#[cfg(test)]
mod parallax_hitmap_tests {

    use crate::engine::render::render_scene_hits;
    use crate::schema::{Scene, VideoConfig};

    #[test]
    fn hit_rects_follow_their_plane_depth() {
        let config: VideoConfig =
            serde_json::from_value(serde_json::json!({ "width": 400, "height": 300, "fps": 30 }))
                .expect("config");
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
        for px in buf.as_chunks::<4>().0.iter() {
            sum += px[0] as u64 + px[1] as u64 + px[2] as u64;
            n += 3;
        }
        sum as f64 / n as f64
    }

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

        let pre = samples.first().unwrap().1;
        let post = samples.last().unwrap().1;
        let mid = samples[samples.len() / 2].1;
        assert!(
            (mid - pre).abs() > 1.0 && (mid - post).abs() > 1.0,
            "mid-pan luma {mid:.1} must differ meaningfully from both pre-pan {pre:.1} and \
             post-pan {post:.1} — background never faded if it matches either endpoint"
        );
    }

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
        assert_eq!(tasks.len(), 60);

        let render = |i: usize| render_frame_task(&scenario.video, &scenario, &tasks[i]).unwrap();
        let before_freeze = render(5);
        let after_freeze_a = render(45);
        let after_freeze_b = render(55);

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

        let (pre, post_a, post_b) = (5u32, 45u32, 55u32);

        let render_full =
            |f: u32| render_scene_frame_scaled(config, scene, f, f as f64 / 30.0, 60, 1.0).unwrap();
        let render_bg = |f: u32| render_scene_bg_scaled(config, scene, f, 1.0).unwrap();
        let render_fg =
            |f: u32| render_scene_fg_scaled(config, scene, f, f as f64 / 30.0, 60, 1.0).unwrap();

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

#[cfg(test)]
mod camera_shake_tests {

    use crate::engine::render::render_frame_v2;
    use crate::schema::{Scene, VideoConfig};
    use rustmotion_core::schema::shake::{SceneShake, ShakeImpact};
    use rustmotion_core::schema::time::{TimeCtx, TimePoint};

    fn config(w: u32, h: u32) -> VideoConfig {
        serde_json::from_value(serde_json::json!({ "width": w, "height": h, "fps": 30 }))
            .expect("config")
    }

    fn render_scene_json(scene_json: serde_json::Value, w: u32, h: u32, frame: u32) -> Vec<u8> {
        let scene: Scene = serde_json::from_value(scene_json).expect("scene json");
        let children = crate::engine::render::deserialize_children(&scene);
        let t = frame as f64 / 30.0;
        render_frame_v2(&config(w, h), &scene, frame, t, 120, &children).expect("render")
    }

    fn channel_centroid(buf: &[u8], w: u32, h: u32, channel: usize) -> (f32, f32) {
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

    fn red_rect_scene(extra: serde_json::Value) -> serde_json::Value {
        let mut base = serde_json::json!({
            "duration": 4.0,
            "children": [{
                "type": "shape",
                "shape": "rect",
                "fill": "#ff0000",
                "position": "absolute",
                "x": 150, "y": 100,
                "style": { "width": "100px", "height": "80px" }
            }]
        });
        if let (Some(base_obj), Some(extra_obj)) = (base.as_object_mut(), extra.as_object()) {
            for (k, v) in extra_obj {
                base_obj.insert(k.clone(), v.clone());
            }
        }
        base
    }

    #[test]
    fn a_scene_with_no_shake_field_renders_byte_identical_to_an_empty_impacts_shake() {
        let no_shake = render_scene_json(red_rect_scene(serde_json::json!({})), 400, 300, 5);
        let empty_shake = render_scene_json(
            red_rect_scene(serde_json::json!({ "shake": { "impacts": [] } })),
            400,
            300,
            5,
        );
        assert_eq!(
            no_shake, empty_shake,
            "absent shake and an empty-impacts shake must both be a no-op on the rendered frame"
        );
    }

    #[test]
    fn a_scene_with_camera_but_no_shake_is_unaffected_by_the_shake_wiring() {
        let panned = render_scene_json(
            red_rect_scene(serde_json::json!({ "camera": { "x": 30.0, "zoom": 1.0 } })),
            400,
            300,
            5,
        );
        let panned_again = render_scene_json(
            red_rect_scene(serde_json::json!({ "camera": { "x": 30.0, "zoom": 1.0 } })),
            400,
            300,
            5,
        );
        assert_eq!(
            panned, panned_again,
            "same camera-only scene must render identically twice"
        );
    }

    #[test]
    fn a_landed_shake_impact_visibly_perturbs_the_frame() {
        let no_shake = render_scene_json(red_rect_scene(serde_json::json!({})), 400, 300, 3);
        let with_shake = render_scene_json(
            red_rect_scene(serde_json::json!({
                "shake": {
                    "impacts": [ { "at": 0.0, "amplitude": 40.0 } ],
                    "decay": 4.0,
                    "frequency": 6.0
                }
            })),
            400,
            300,
            3,
        );
        assert_ne!(
            no_shake, with_shake,
            "a landed shake impact must move the rendered frame"
        );
    }

    #[test]
    fn shake_offset_moves_the_rendered_centroid_by_exactly_its_own_formula() {
        let shake = SceneShake {
            impacts: vec![ShakeImpact {
                at: TimePoint::Seconds(0.0),
                amplitude: 40.0,
            }],
            decay: 4.0,
            frequency: 6.0,
            rotation: 0.0,
        };
        let ctx = TimeCtx {
            bpm: None,
            beat_offset: 0.0,
            scene_start: 0.0,
        };
        let frame = 3u32;
        let t = frame as f64 / 30.0;
        let expected = rustmotion_core::engine::shake::shake_offset(&shake, &ctx, t)
            .expect("no bpm needed for a plain-seconds impact");

        let base = render_scene_json(red_rect_scene(serde_json::json!({})), 400, 300, frame);
        let shaken = render_scene_json(
            red_rect_scene(serde_json::json!({
                "shake": {
                    "impacts": [ { "at": 0.0, "amplitude": 40.0 } ],
                    "decay": 4.0,
                    "frequency": 6.0
                }
            })),
            400,
            300,
            frame,
        );

        let (bx, by) = channel_centroid(&base, 400, 300, 0);
        let (sx, sy) = channel_centroid(&shaken, 400, 300, 0);
        assert!(
            bx >= 0.0 && sx >= 0.0,
            "red rect must be visible in both frames"
        );

        let dx = sx - bx;
        let dy = sy - by;
        assert!(
            (dx - (-expected.x as f32)).abs() < 3.0,
            "centroid x shift {dx} must match -shake.x ({})",
            -expected.x
        );
        assert!(
            (dy - (-expected.y as f32)).abs() < 3.0,
            "centroid y shift {dy} must match -shake.y ({})",
            -expected.y
        );
    }

    #[test]
    fn shake_is_additive_over_an_existing_camera_pan_not_a_replacement() {
        let shake_cfg = serde_json::json!({
            "impacts": [ { "at": 0.0, "amplitude": 25.0 } ],
            "decay": 5.0,
            "frequency": 4.0
        });
        let frame = 2u32;

        let pan_only = render_scene_json(
            red_rect_scene(serde_json::json!({ "camera": { "x": 20.0, "zoom": 1.0 } })),
            400,
            300,
            frame,
        );
        let shake_only = render_scene_json(
            red_rect_scene(serde_json::json!({ "shake": shake_cfg })),
            400,
            300,
            frame,
        );
        let both = render_scene_json(
            red_rect_scene(serde_json::json!({
                "camera": { "x": 20.0, "zoom": 1.0 },
                "shake": shake_cfg
            })),
            400,
            300,
            frame,
        );

        assert_ne!(pan_only, both, "combined render must differ from pan alone");
        assert_ne!(
            shake_only, both,
            "combined render must differ from shake alone"
        );
        assert_ne!(
            pan_only, shake_only,
            "pan alone must differ from shake alone"
        );

        let (px, _) = channel_centroid(&pan_only, 400, 300, 0);
        let (bx0, _) = channel_centroid(
            &render_scene_json(red_rect_scene(serde_json::json!({})), 400, 300, frame),
            400,
            300,
            0,
        );
        let (cx, _) = channel_centroid(&both, 400, 300, 0);

        let pan_shift = px - bx0;
        let combined_shift = cx - bx0;
        assert!(
            (combined_shift - pan_shift).abs() > 1.0,
            "combined shift ({combined_shift}) must differ from the pan-only shift ({pan_shift}) \
             — shake must contribute on top of the pan, not disappear under it"
        );
    }
}

#[cfg(test)]
mod node_reference_resolution {

    use crate::engine::render::{deserialize_children, resolve_node_references, root_style};
    use crate::schema::{Scene, ViewType};
    use rustmotion_components::box_builder::{build_scene_from_refs, BuildAnimationCtx};
    use rustmotion_core::css::taffy_bridge::ConversionContext;
    use rustmotion_core::engine::deps::{DepGraph, FrameScope, NodeRef};
    use rustmotion_core::engine::layout_pass::run_layout;
    use rustmotion_core::engine::paint_pass::animated_transform;
    use rustmotion_core::expr::Expr;

    const VW: f32 = 400.0;
    const VH: f32 = 300.0;

    fn badge_and_line_scene(bx: f32, by: f32) -> Scene {
        let json = serde_json::json!({
            "duration": 4.0,
            "children": [
                {
                    "type": "shape",
                    "shape": "circle",
                    "id": "badge",
                    "position": "absolute", "x": 0, "y": 0,
                    "style": {
                        "width": "20px", "height": "20px",
                        "transform": [
                            { "fn": "translate", "x": format!("{bx}px"), "y": format!("{by}px") }
                        ]
                    }
                },
                {
                    "type": "line",
                    "id": "line",
                    "x1": 0.0, "y1": 0.0, "x2": 1.0, "y2": 1.0
                }
            ]
        });
        serde_json::from_value(json).expect("scene json")
    }

    fn refs_by_id() -> Vec<(String, Vec<NodeRef>)> {
        vec![
            (
                "line".to_string(),
                vec![NodeRef {
                    id: "badge".to_string(),
                    prop: "tx".to_string(),
                }],
            ),
            ("badge".to_string(), vec![]),
        ]
    }

    fn resolve_at(scene: &Scene) -> (f64, f32) {
        let children = deserialize_children(scene);
        let root_css = root_style(scene.layout.as_ref(), ViewType::Slide);
        let anim = Some(BuildAnimationCtx {
            time: 0.0,
            scenario_time: 0.0,
            scene_duration: scene.duration,
            fps: 30,
        });
        let built = build_scene_from_refs(children.iter(), (VW, VH), root_css, anim);
        let layout = run_layout(
            &built.root,
            (VW, VH),
            &ConversionContext::for_viewport(VW, VH),
        );

        let frame = resolve_node_references(&built, &layout, (VW, VH), &refs_by_id())
            .expect("dependency graph must build: no cycle, no unknown id");

        let scope = FrameScope(&frame);
        let resolved_x2 = Expr::parse(r#"= node("badge", "tx")"#)
            .unwrap()
            .eval(&scope)
            .expect("badge must already be resolved by the time line's expression evaluates");

        let badge_node_id = built
            .components
            .iter()
            .position(|c| c.and_then(|cc| cc.id.as_deref()) == Some("badge"))
            .expect("badge must be in the built scene") as u32;
        let badge_box = built.root.find(badge_node_id).expect("badge box node");
        let badge_layout = *layout.get(badge_box.id).expect("badge layout");
        let (badge_tx, ..) = animated_transform(&badge_box.css, &badge_layout, (VW, VH));

        (resolved_x2, badge_tx)
    }

    #[test]
    fn dep_graph_topological_order_puts_badge_before_line_despite_declaration_order() {
        let refs = refs_by_id();
        assert_eq!(
            refs[0].0, "line",
            "sanity: line is declared first in refs_by_id"
        );
        let graph = DepGraph::build(&refs, &std::collections::HashSet::new()).unwrap();
        assert_eq!(graph.order(), &["badge", "line"]);
    }

    #[test]
    fn resolved_reference_tracks_the_current_frames_badge_with_no_one_frame_lag() {
        let samples: [(f32, f32); 4] = [(150.0, 80.0), (-40.0, 220.0), (0.0, 0.0), (77.5, -12.5)];

        let mut resolved_values = Vec::new();
        for &(bx, by) in &samples {
            let scene = badge_and_line_scene(bx, by);
            let (resolved_x2, badge_tx_independent) = resolve_at(&scene);

            assert_eq!(
                resolved_x2, badge_tx_independent as f64,
                "line's node(\"badge\",\"tx\") must equal badge's own tx recomputed independently \
                 from the same tree, badge placed at x={bx}"
            );
            assert_eq!(
                resolved_x2, bx as f64,
                "badge's resolved tx must match the transform declared for this sample"
            );
            resolved_values.push(resolved_x2);
        }

        for pair in resolved_values.windows(2) {
            assert_ne!(
                pair[0], pair[1],
                "resolved value did not change between differently-placed badges — \
                 looks like a stale or cached ResolvedFrame"
            );
        }
    }

    #[test]
    fn unknown_or_duplicate_ids_are_reported_not_silently_dropped() {
        let bad_refs = vec![(
            "line".to_string(),
            vec![NodeRef {
                id: "does_not_exist".to_string(),
                prop: "tx".to_string(),
            }],
        )];
        let scene = badge_and_line_scene(0.0, 0.0);
        let children = deserialize_children(&scene);
        let root_css = root_style(scene.layout.as_ref(), ViewType::Slide);
        let built = build_scene_from_refs(children.iter(), (VW, VH), root_css, None);
        let layout = run_layout(
            &built.root,
            (VW, VH),
            &ConversionContext::for_viewport(VW, VH),
        );

        let err = resolve_node_references(&built, &layout, (VW, VH), &bad_refs).unwrap_err();
        assert!(matches!(
            err,
            rustmotion_core::engine::deps::DepsError::UnknownId { .. }
        ));
    }
}

#[cfg(test)]
mod self_painting_background_animation_tests {
    use crate::encode::video::{build_frame_tasks, render_frame_task, FrameTask};
    use crate::loader::load_scenario_from_source;

    const W: usize = 400;
    const H: usize = 200;

    fn scenario(component: &str) -> String {
        format!(
            r##"{{
              "version": "1.0",
              "video": {{ "width": {W}, "height": {H}, "fps": 30, "background": "#000000" }},
              "scenes": [{{ "duration": 2.0, "children": [{component}] }}]
            }}"##
        )
    }

    fn centre_rgb(json: &str, time: f64) -> (u8, u8, u8) {
        let scenario = load_scenario_from_source(None, Some(json)).expect("load");
        let fps = scenario.video.fps as f64;
        let wanted = (time * fps).round() as u32;
        let tasks = build_frame_tasks(&scenario);
        let task = tasks
            .iter()
            .find(|t| match t {
                FrameTask::Normal { global_frame, .. } => *global_frame == wanted,
                _ => false,
            })
            .expect("a normal frame at that instant");
        let frame = render_frame_task(&scenario.video, &scenario, task).expect("render");
        let i = ((H / 2) * W + W / 2) * 4;
        (frame[i], frame[i + 1], frame[i + 2])
    }

    const BADGE: &str = r##"{
        "type": "badge", "text": "AB",
        "style": { "position": "absolute", "left": 100, "top": 70, "font-size": 40,
                   "background": "#FF0000", "color": "#FFFFFF", "border-radius": 999,
                   "transition": { "duration": 1.0, "easing": "linear" } },
        "timeline": [{ "at": 0.5, "style": { "background": "#0000FF" } }] }"##;

    const STAT: &str = r##"{
        "type": "stat", "value": "42",
        "style": { "position": "absolute", "left": 100, "top": 50, "width": 200, "height": 100,
                   "background": "#FF0000", "color": "#FFFFFF",
                   "transition": { "duration": 1.0, "easing": "linear" } },
        "timeline": [{ "at": 0.5, "style": { "background": "#0000FF" } }] }"##;

    #[test]
    fn a_badge_follows_an_animated_background_instead_of_repainting_its_own() {
        let json = scenario(BADGE);
        assert_eq!(
            centre_rgb(&json, 0.2),
            (255, 0, 0),
            "before the step the badge is the colour it declares"
        );
        let (r, g, b) = centre_rgb(&json, 1.0);
        assert!(
            r > 80 && r < 180 && g < 20 && b > 80 && b < 180,
            "halfway through the transition the badge must be between its two colours, got \
             ({r}, {g}, {b}) — repainting its own static background gives (255, 0, 0)"
        );
        assert_eq!(
            centre_rgb(&json, 1.8),
            (0, 0, 255),
            "past the transition the badge is the colour the timeline asked for"
        );
    }

    #[test]
    fn a_stat_follows_it_too_although_the_cascade_never_clones_it() {
        let json = scenario(STAT);
        assert_eq!(centre_rgb(&json, 0.2), (255, 0, 0));
        let (r, g, b) = centre_rgb(&json, 1.0);
        assert!(
            r > 80 && r < 180 && g < 20 && b > 80 && b < 180,
            "stat is classified non-typographic, so with_cascaded_style returns None for it — \
             the resolved background has to reach it by its own path, got ({r}, {g}, {b})"
        );
        assert_eq!(centre_rgb(&json, 1.8), (0, 0, 255));
    }

    #[test]
    fn a_component_with_no_background_animation_is_untouched() {
        let json = scenario(
            r##"{ "type": "badge", "text": "AB",
                  "style": { "position": "absolute", "left": 100, "top": 70, "font-size": 40,
                             "background": "#FF0000", "color": "#FFFFFF", "border-radius": 999 } }"##,
        );
        assert_eq!(centre_rgb(&json, 0.2), (255, 0, 0));
        assert_eq!(
            centre_rgb(&json, 1.8),
            (255, 0, 0),
            "with nothing animating the background, the clone must not be taken at all"
        );
    }
}

#[cfg(test)]
mod still_runs_the_audio_analysis_tests {
    use super::audio_tests::{make_sine_wav, nanos};
    use rustmotion_core::engine::renderer::audio_analysis_cache;

    fn scenario_json(wav: &str) -> String {
        serde_json::json!({
            "video": { "width": 64, "height": 64, "fps": 30 },
            "audio": [{ "src": wav }],
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "audio_spectrum", "bars": 8, "mode": "bars", "color": "#ffffff",
                    "style": { "position": "absolute", "left": 0, "top": 0,
                               "width": 64, "height": 64 }
                }]
            }]
        })
        .to_string()
    }

    #[test]
    fn the_shared_preload_analyses_the_track_the_way_an_encode_does() {
        let sample_rate = 44100u32;
        let wav = make_sine_wav(sample_rate, sample_rate / 2, 440.0, sample_rate);
        let wav_path = std::env::temp_dir().join(format!("rm_still_audio_{}.wav", nanos()));
        std::fs::write(&wav_path, &wav).expect("write wav fixture");
        let src = wav_path.to_str().unwrap().to_string();

        let scenario = crate::loader::load_scenario_from_source(None, Some(&scenario_json(&src)))
            .expect("load scenario");

        audio_analysis_cache().remove(&src);
        assert!(
            audio_analysis_cache().get(&src).is_none(),
            "test setup: the track must not already be analysed"
        );

        crate::engine::preload::preload_scenario_assets(&scenario).expect("preload");

        let analysed = audio_analysis_cache().get(&src).is_some();
        std::fs::remove_file(&wav_path).ok();

        assert!(
            analysed,
            "still and sheet went through this preamble without analysing the track, so every \
             audio-reactive component in an exported frame sat at its min — the flat bars \
             reported in issue #349"
        );
    }
}

#[cfg(test)]
mod icon_render_tests {
    use crate::encode::video::{build_frame_tasks, render_frame_task, FrameTask};
    use crate::loader::load_scenario_from_source;
    use rustmotion_core::engine::renderer::{icon_cache_dir, icon_source_cache_file};

    const W: usize = 160;
    const H: usize = 160;

    const A_BAR: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="1em" height="1em" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="4"><path d="M2 12h20"/></svg>"#;

    struct SeededIcon {
        name: String,
        file: std::path::PathBuf,
    }

    impl SeededIcon {
        fn new(tag: &str) -> Self {
            let name = format!("rmtest{tag}:bar");
            let dir = icon_cache_dir();
            std::fs::create_dir_all(&dir).expect("cache dir");
            let file = icon_source_cache_file(&dir, &name);
            std::fs::write(&file, A_BAR).expect("seed the icon so the test needs no network");
            Self { name, file }
        }
    }

    impl Drop for SeededIcon {
        fn drop(&mut self) {
            std::fs::remove_file(&self.file).ok();
        }
    }

    fn render_first_frame(icon: &str) -> Vec<u8> {
        let json = format!(
            r##"{{
              "version": "1.0",
              "video": {{ "width": {W}, "height": {H}, "fps": 30, "background": "#FFFFFF" }},
              "scenes": [{{ "duration": 1.0, "children": [
                {{ "type": "icon", "icon": "{icon}",
                   "style": {{ "position": "absolute", "left": 38, "top": 38,
                              "width": 84, "height": 84, "color": "#2563EB" }} }}
              ]}}]
            }}"##
        );
        let scenario = load_scenario_from_source(None, Some(&json)).expect("load");
        crate::engine::preload::preload_scenario_assets(&scenario).expect("preload");
        let tasks = build_frame_tasks(&scenario);
        let task = tasks
            .iter()
            .find(|t| matches!(t, FrameTask::Normal { .. }))
            .expect("a normal frame");
        render_frame_task(&scenario.video, &scenario, task).expect("render")
    }

    fn blue_pixels(frame: &[u8]) -> usize {
        (0..W * H)
            .filter(|i| {
                let p = i * 4;
                frame[p] < 120 && frame[p + 2] > 150
            })
            .count()
    }

    #[test]
    fn an_icon_actually_paints_its_glyph_into_the_frame() {
        let seeded = SeededIcon::new("paint");
        let frame = render_first_frame(&seeded.name);
        let painted = blue_pixels(&frame);
        assert!(
            painted > 100,
            "the icon painted {painted} coloured pixels. Rewriting its width left a stray quote, \
             so the SVG did not parse, so nothing was drawn — and the only sign was a warning on \
             stderr with a zero exit code"
        );
    }

    #[test]
    fn the_icon_takes_the_colour_the_scenario_asks_for() {
        let seeded = SeededIcon::new("colour");
        let frame = render_first_frame(&seeded.name);
        let mut found = None;
        for i in 0..W * H {
            let p = i * 4;
            if frame[p] < 120 && frame[p + 2] > 150 && frame[p + 3] > 250 {
                found = Some((frame[p], frame[p + 1], frame[p + 2]));
                break;
            }
        }
        let (r, g, b) = found.expect("an opaque coloured pixel");
        assert!(
            r < 80 && g < 140 && b > 200,
            "currentColor must become the declared #2563EB, got ({r}, {g}, {b})"
        );
    }
}
