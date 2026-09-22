use std::path::PathBuf;

use crate::app::state::ThemePref;

fn theme_pref_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("rustmotion").join("theme.json"))
}

#[allow(dead_code)]
fn label_for(pref: ThemePref) -> &'static str {
    match pref {
        ThemePref::Light => "light",
        ThemePref::Dark => "dark",
        ThemePref::System => "system",
    }
}

fn parse_label(label: &str) -> ThemePref {
    match label {
        "light" => ThemePref::Light,
        "dark" => ThemePref::Dark,
        _ => ThemePref::default(),
    }
}

pub fn load_theme_pref() -> ThemePref {
    let path = match theme_pref_path() {
        Some(p) => p,
        None => return ThemePref::default(),
    };
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return ThemePref::default(),
    };
    let label: String = serde_json::from_str(&raw).unwrap_or_default();
    parse_label(&label)
}

#[allow(dead_code)]
pub fn save_theme_pref(pref: ThemePref) {
    if let Some(path) = theme_pref_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(s) = serde_json::to_string(label_for(pref)) {
            let _ = std::fs::write(&path, s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preference_roundtrips_through_its_label() {
        for pref in [ThemePref::Light, ThemePref::Dark, ThemePref::System] {
            assert_eq!(parse_label(label_for(pref)), pref);
        }
    }

    #[test]
    fn unknown_or_corrupt_label_falls_back_to_the_default() {
        assert_eq!(parse_label("nonsense"), ThemePref::default());
        assert_eq!(parse_label(""), ThemePref::default());
    }
}
