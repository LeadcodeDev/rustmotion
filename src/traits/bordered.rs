use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Border {
    pub color: String,
    #[serde(default = "default_border_width")]
    pub width: f32,
}

fn default_border_width() -> f32 {
    1.0
}

/// Trait for components that support a border.
pub trait Bordered {
    fn border(&self) -> Option<&Border>;
}

/// Mutable access to border — needed by builder traits.
pub trait BorderedMut: Bordered {
    fn set_border(&mut self, border: Option<Border>);
}

/// Builder API for border.
pub trait BorderedExt: BorderedMut + Sized {
    fn border_color(mut self, color: impl Into<String>, width: f32) -> Self {
        self.set_border(Some(Border { color: color.into(), width }));
        self
    }

    fn border_none(mut self) -> Self {
        self.set_border(None);
        self
    }
}

impl<T: BorderedMut + Sized> BorderedExt for T {}
