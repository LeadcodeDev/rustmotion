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
