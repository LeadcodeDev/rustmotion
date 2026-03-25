//! Smoke tests: every component deserializes from minimal JSON and can be measured.

#[cfg(test)]
mod component_smoke {
    use crate::components::Component;
    use crate::layout::Constraints;
    use crate::traits::Widget;

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
        ("chart", r#"{"type":"chart","chart_type":"bar","data":[{"value":10}]}"#),
        ("countdown", r#"{"type":"countdown","seconds":60}"#),
        ("divider", r#"{"type":"divider"}"#),
        ("gauge", r#"{"type":"gauge","value":50}"#),
        ("gradient_text", r##"{"type":"gradient_text","content":"hello","colors":["#FF0000","#0000FF"]}"##),
        ("heatmap", r#"{"type":"heatmap","data":[[1,2],[3,4]]}"#),
        ("kbd", r#"{"type":"kbd","key":"Ctrl+C"}"#),
        ("list", r#"{"type":"list","items":[{"text":"one"},{"text":"two"}]}"#),
        ("marquee", r#"{"type":"marquee","content":"scrolling"}"#),
        ("particle", r#"{"type":"particle","particle_type":"confetti"}"#),
        ("progress", r#"{"type":"progress","value":0.5}"#),
        ("qr_code", r#"{"type":"qr_code","content":"https://example.com"}"#),
        ("rating", r#"{"type":"rating","value":3.5}"#),
        ("skeleton", r#"{"type":"skeleton"}"#),
        ("slider", r#"{"type":"slider","value":50}"#),
        ("sparkline", r#"{"type":"sparkline","data":[1,2,3,4,5]}"#),
        ("stat", r#"{"type":"stat","value":"42","label":"Users"}"#),
        ("stepper", r#"{"type":"stepper","steps":[{"label":"Step 1"},{"label":"Step 2"}]}"#),
        ("switch", r#"{"type":"switch"}"#),
        ("rich_text", r#"{"type":"rich_text","spans":[{"text":"hello"}]}"#),
        ("table", r#"{"type":"table","headers":["A","B"],"rows":[["1","2"]]}"#),
        ("tag_cloud", r#"{"type":"tag_cloud","tags":[{"text":"rust","weight":1.0}]}"#),
        ("terminal", r#"{"type":"terminal","lines":[{"text":"$ echo hello"}]}"#),
        ("timeline", r#"{"type":"timeline","steps":[{"label":"Start"}]}"#),
        ("treemap", r#"{"type":"treemap","data":[{"value":10,"label":"A"}]}"#),
        // Containers
        ("flex", r#"{"type":"flex","children":[{"type":"text","content":"hi"}]}"#),
        ("grid", r#"{"type":"grid","children":[{"type":"text","content":"hi"}]}"#),
        ("card", r#"{"type":"card","children":[{"type":"text","content":"hi"}]}"#),
        ("container", r#"{"type":"container","children":[{"type":"text","content":"hi"}]}"#),
        ("positioned", r#"{"type":"positioned","children":[{"type":"text","content":"hi","x":0,"y":0}]}"#),
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
