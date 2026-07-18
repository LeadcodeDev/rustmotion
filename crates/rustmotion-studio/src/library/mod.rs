//! The library home page: workspace scanning, recents, thumbnail cache, and the
//! card-grid UI that fronts the editor.

mod data;
mod view;

pub use data::{render_thumbnail, LibraryState, ScenarioEntry, SharedLibrary, WatchMsg};
pub use view::Library;
