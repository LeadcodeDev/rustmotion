//! The scenario editor: playback, the clickable element overlay, the property
//! inspector, and the comments/annotations panel.

mod annotations;
mod export;
pub mod frames;
mod inspector;
mod playback;
mod topbar;
mod view;

pub use view::StudioApp;
