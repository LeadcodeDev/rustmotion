//! CSS-like styling system for the browser-mode renderer.
//!
//! Inspired by Remotion (React inline styles) and CSS box model.
//! Provides:
//! - `CssStyle`: a struct mirroring the CSS properties supported by the engine
//! - `units`: parsing/resolution of CSS lengths (px, %, em, rem, vw, vh, fr, auto)
//! - `cascade`: parent → child inheritance pass
//! - `taffy_bridge`: conversion `CssStyle` → `taffy::Style` for layout
//!
//! Layout is computed by [`taffy`]; paint is done by Skia in `engine::paint_pass`.

pub mod animation;
pub mod style;
pub mod units;
pub mod cascade;
pub mod taffy_bridge;
pub mod legacy;

pub use animation::apply_animated_props;
pub use style::CssStyle;
pub use units::{Length, LengthContext, LengthPercentage};
pub use legacy::layer_to_css;
