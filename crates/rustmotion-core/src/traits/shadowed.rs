use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Shadow {
    pub color: String,
    #[serde(default)]
    pub offset_x: f32,
    #[serde(default)]
    pub offset_y: f32,
    #[serde(default)]
    pub blur: f32,
}

/// Trait for components that support a drop shadow.
pub trait Shadowed {
    fn shadow(&self) -> Option<&Shadow>;
}

/// Mutable access to shadow — needed by builder traits.
pub trait ShadowedMut: Shadowed {
    fn set_shadow(&mut self, shadow: Option<Shadow>);
}

/// Builder API for shadow.
pub trait ShadowedExt: ShadowedMut + Sized {
    fn shadow_sm(mut self) -> Self {
        self.set_shadow(Some(Shadow {
            color: "rgba(0,0,0,0.1)".into(),
            offset_x: 0.0, offset_y: 1.0, blur: 2.0,
        }));
        self
    }

    fn shadow_md(mut self) -> Self {
        self.set_shadow(Some(Shadow {
            color: "rgba(0,0,0,0.15)".into(),
            offset_x: 0.0, offset_y: 4.0, blur: 6.0,
        }));
        self
    }

    fn shadow_lg(mut self) -> Self {
        self.set_shadow(Some(Shadow {
            color: "rgba(0,0,0,0.2)".into(),
            offset_x: 0.0, offset_y: 8.0, blur: 16.0,
        }));
        self
    }

    fn shadow_xl(mut self) -> Self {
        self.set_shadow(Some(Shadow {
            color: "rgba(0,0,0,0.25)".into(),
            offset_x: 0.0, offset_y: 16.0, blur: 32.0,
        }));
        self
    }

    fn shadow_none(mut self) -> Self {
        self.set_shadow(None);
        self
    }

    fn shadow_custom(mut self, color: impl Into<String>, offset_x: f32, offset_y: f32, blur: f32) -> Self {
        self.set_shadow(Some(Shadow {
            color: color.into(),
            offset_x, offset_y, blur,
        }));
        self
    }
}

impl<T: ShadowedMut + Sized> ShadowedExt for T {}
