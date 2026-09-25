pub mod animation;
pub mod background;
pub mod codeblock_types;
pub mod scenario;
pub mod style;
pub mod video;

pub use animation::*;
pub use background::*;
pub use codeblock_types::*;
pub use scenario::*;
pub use style::*;
pub use video::*;

pub fn generate_json_schema() -> serde_json::Value {
    let schema = schemars::schema_for!(scenario::Scenario);
    serde_json::to_value(schema).unwrap()
}

pub(crate) fn fold_separators_and_camel_case_boundaries(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    let mut prev_was_word_char = false;
    for c in s.chars() {
        if c == '-' || c == '_' || c == ' ' || c == '.' {
            if !out.ends_with('_') {
                out.push('_');
            }
            prev_was_word_char = false;
            continue;
        }
        if c.is_uppercase() && prev_was_word_char && !out.ends_with('_') {
            out.push('_');
        }
        out.extend(c.to_lowercase());
        prev_was_word_char = true;
    }
    out
}

#[cfg(test)]
mod fold_separators_and_camel_case_boundaries_tests {
    use super::*;

    #[test]
    fn camel_case_gets_a_word_boundary_inserted() {
        assert_eq!(
            fold_separators_and_camel_case_boundaries("translateX"),
            "translate_x"
        );
        assert_eq!(
            fold_separators_and_camel_case_boundaries("originX"),
            "origin_x"
        );
    }

    #[test]
    fn mixed_dot_and_underscore_conventions_fold_to_the_same_shape() {
        assert_eq!(
            fold_separators_and_camel_case_boundaries("position.x"),
            fold_separators_and_camel_case_boundaries("position_x")
        );
    }

    #[test]
    fn already_canonical_names_are_unchanged() {
        assert_eq!(
            fold_separators_and_camel_case_boundaries("translate_x"),
            "translate_x"
        );
        assert_eq!(
            fold_separators_and_camel_case_boundaries("opacity"),
            "opacity"
        );
    }
}
