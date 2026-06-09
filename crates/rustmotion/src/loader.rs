use crate::error::{Result, RustmotionError};
use crate::schema::{ResolvedScenario, Scenario};
use crate::{include, variables};
use std::path::PathBuf;

pub fn load_scenario(input: &PathBuf) -> Result<ResolvedScenario> {
    let json_str = std::fs::read_to_string(input)
        .map_err(|e| RustmotionError::FileRead { path: input.display().to_string(), source: e })?;
    let mut json_value: serde_json::Value =
        serde_json::from_str(&json_str).map_err(RustmotionError::from)?;

    // Apply variable defaults for standalone rendering
    variables::apply_defaults(&mut json_value)?;

    let scenario: Scenario =
        serde_json::from_value(json_value).map_err(RustmotionError::from)?;
    include::resolve_includes(scenario, &include::IncludeSource::File(input.clone()))
}

pub fn load_scenario_from_source(
    input: Option<&PathBuf>,
    json: Option<&str>,
) -> Result<ResolvedScenario> {
    match (input, json) {
        (Some(_), Some(_)) => {
            Err(RustmotionError::ConflictingInput.into())
        }
        (Some(path), None) => load_scenario(path),
        (None, Some(json_str)) => {
            let mut json_value: serde_json::Value =
                serde_json::from_str(json_str).map_err(RustmotionError::from)?;
            variables::apply_defaults(&mut json_value)?;
            let scenario: Scenario =
                serde_json::from_value(json_value).map_err(RustmotionError::from)?;
            include::resolve_includes(scenario, &include::IncludeSource::Inline)
        }
        (None, None) => {
            Err(RustmotionError::MissingInput.into())
        }
    }
}

/// Load a scenario authored in the HTML/CSS dialect: transpile to the scenario
/// JSON value, deserialize into `Scenario`, then resolve includes — reusing the
/// exact same pipeline as the JSON loader.
pub fn load_scenario_from_html(input: &PathBuf) -> Result<ResolvedScenario> {
    let html = std::fs::read_to_string(input)
        .map_err(|e| RustmotionError::FileRead { path: input.display().to_string(), source: e })?;
    let value = rustmotion_html::html_to_scenario_value(&html)
        .map_err(|e| RustmotionError::HtmlParse(e.to_string()))?;
    let scenario: Scenario = serde_json::from_value(value).map_err(RustmotionError::from)?;
    include::resolve_includes(scenario, &include::IncludeSource::File(input.clone()))
}

/// Dispatch by file extension: `.html`/`.htm` use the HTML transpiler, everything
/// else uses the JSON loader. Single entry point for all CLI commands.
pub fn load_input(input: &PathBuf) -> Result<ResolvedScenario> {
    match input.extension().and_then(|e| e.to_str()) {
        Some("html") | Some("htm") => load_scenario_from_html(input),
        _ => load_scenario(input),
    }
}

/// Transpile an HTML-dialect string into the scenario JSON value, for callers
/// that need the raw value (e.g. the validation pipeline reads it by pointer).
pub fn html_to_scenario_json(html: &str) -> Result<serde_json::Value> {
    rustmotion_html::html_to_scenario_value(html)
        .map_err(|e| RustmotionError::HtmlParse(e.to_string()))
}

/// True if the path uses the HTML dialect (`.html`/`.htm`).
pub fn is_html_path(path: &std::path::Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("html") | Some("htm")
    )
}

#[cfg(test)]
mod html_tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn loads_html_scenario_into_resolved() {
        let html = r##"<rustmotion width="1920" height="1080" fps="30" background="#0f172a">
            <scene duration="4"><h1 style="font-size:96; color:#ffffff">Hi</h1></scene>
        </rustmotion>"##;
        let dir = std::env::temp_dir();
        let path = dir.join("rm_html_loader_test.html");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(html.as_bytes()).unwrap();

        let resolved = load_input(&path).expect("html loads");
        assert_eq!(resolved.video.width, 1920);
        assert_eq!(resolved.views.len(), 1);
        assert_eq!(resolved.views[0].scenes.len(), 1);
        assert_eq!(resolved.views[0].scenes[0].duration, 4.0);
        let _ = std::fs::remove_file(&path);
    }
}
