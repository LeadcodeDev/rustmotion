//! Scenario data layer: the live studio model, JSON-pointer style edits, and
//! the top-level view enum shared across features.

mod edit;
mod model;

pub use edit::{
    append_annotation, list_annotations, read_field, read_style_object, remove_annotation,
    set_field, set_style,
};
pub use model::{empty_scenario, Shared, StudioModel};

/// Which top-level view is shown (library home vs. the editor).
#[derive(Clone, Copy, PartialEq)]
pub enum View {
    Library,
    Editor,
}

/// The active UI theme. `System` follows the OS `prefers-color-scheme`.
#[derive(Clone, Copy, PartialEq)]
pub enum Theme {
    Dark,
    Light,
    System,
}

impl Theme {
    /// CSS class applied to the themed root; the stylesheet keys palette
    /// variables off it.
    pub fn class(self) -> &'static str {
        match self {
            Theme::Dark => "rm-dark",
            Theme::Light => "rm-light",
            Theme::System => "rm-system",
        }
    }

    /// Cycle Dark → Light → System → Dark.
    pub fn next(self) -> Theme {
        match self {
            Theme::Dark => Theme::Light,
            Theme::Light => Theme::System,
            Theme::System => Theme::Dark,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Theme::Dark => "Dark",
            Theme::Light => "Light",
            Theme::System => "System",
        }
    }
}
