use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::schema::Spacing;

/// Configuration for visual style (opacity, padding, margin).
/// Embedded via `#[serde(flatten)]` in components that support styling.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StyleConfig {
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default)]
    pub padding: Option<Spacing>,
    #[serde(default)]
    pub margin: Option<Spacing>,
}

impl Default for StyleConfig {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            padding: None,
            margin: None,
        }
    }
}

fn default_opacity() -> f32 {
    1.0
}

/// Object-safe trait for components that support styling (opacity, padding, margin).
/// Used by the render pipeline via `as_styled() -> &dyn Styled`.
pub trait Styled {
    fn style_config(&self) -> &StyleConfig;

    fn opacity(&self) -> f32 {
        self.style_config().opacity
    }

    fn padding(&self) -> (f32, f32, f32, f32) {
        self.style_config()
            .padding
            .as_ref()
            .map(|p| p.resolve())
            .unwrap_or((0.0, 0.0, 0.0, 0.0))
    }

    fn margin(&self) -> (f32, f32, f32, f32) {
        self.style_config()
            .margin
            .as_ref()
            .map(|m| m.resolve())
            .unwrap_or((0.0, 0.0, 0.0, 0.0))
    }
}

/// Mutable access to style config — needed by builder traits.
pub trait StyledMut: Styled {
    fn style_config_mut(&mut self) -> &mut StyleConfig;
}

/// Tailwind/GPUI-style builder API for styling.
/// Not object-safe (requires `Sized`). Used at construction time.
///
/// ```ignore
/// let text = Text::new("hello")
///     .p_4()
///     .mx(8.0)
///     .opacity(0.8);
/// ```
pub trait StyledExt: StyledMut + Sized {
    // --- Opacity ---

    fn opacity(mut self, value: f32) -> Self {
        self.style_config_mut().opacity = value;
        self
    }

    // --- Padding: arbitrary values ---

    /// Set uniform padding on all sides.
    fn p(mut self, value: f32) -> Self {
        self.style_config_mut().padding = Some(Spacing::Uniform(value));
        self
    }

    /// Set horizontal padding (left + right).
    fn px(mut self, value: f32) -> Self {
        let cfg = self.style_config_mut();
        let (top, _, bottom, _) = cfg.padding.as_ref().map(|s| s.resolve()).unwrap_or_default();
        cfg.padding = Some(Spacing::Sides { top, right: value, bottom, left: value });
        self
    }

    /// Set vertical padding (top + bottom).
    fn py(mut self, value: f32) -> Self {
        let cfg = self.style_config_mut();
        let (_, right, _, left) = cfg.padding.as_ref().map(|s| s.resolve()).unwrap_or_default();
        cfg.padding = Some(Spacing::Sides { top: value, right, bottom: value, left });
        self
    }

    fn pt(mut self, value: f32) -> Self {
        let cfg = self.style_config_mut();
        let (_, right, bottom, left) = cfg.padding.as_ref().map(|s| s.resolve()).unwrap_or_default();
        cfg.padding = Some(Spacing::Sides { top: value, right, bottom, left });
        self
    }

    fn pr(mut self, value: f32) -> Self {
        let cfg = self.style_config_mut();
        let (top, _, bottom, left) = cfg.padding.as_ref().map(|s| s.resolve()).unwrap_or_default();
        cfg.padding = Some(Spacing::Sides { top, right: value, bottom, left });
        self
    }

    fn pb(mut self, value: f32) -> Self {
        let cfg = self.style_config_mut();
        let (top, right, _, left) = cfg.padding.as_ref().map(|s| s.resolve()).unwrap_or_default();
        cfg.padding = Some(Spacing::Sides { top, right, bottom: value, left });
        self
    }

    fn pl(mut self, value: f32) -> Self {
        let cfg = self.style_config_mut();
        let (top, right, bottom, _) = cfg.padding.as_ref().map(|s| s.resolve()).unwrap_or_default();
        cfg.padding = Some(Spacing::Sides { top, right, bottom, left: value });
        self
    }

    // --- Padding: Tailwind scale ---
    // 0=0, 1=4, 2=8, 3=12, 4=16, 5=20, 6=24, 7=28, 8=32, 10=40, 12=48, 16=64, 20=80, 24=96

    fn p_0(self) -> Self { self.p(0.0) }
    fn p_1(self) -> Self { self.p(4.0) }
    fn p_2(self) -> Self { self.p(8.0) }
    fn p_3(self) -> Self { self.p(12.0) }
    fn p_4(self) -> Self { self.p(16.0) }
    fn p_5(self) -> Self { self.p(20.0) }
    fn p_6(self) -> Self { self.p(24.0) }
    fn p_7(self) -> Self { self.p(28.0) }
    fn p_8(self) -> Self { self.p(32.0) }
    fn p_10(self) -> Self { self.p(40.0) }
    fn p_12(self) -> Self { self.p(48.0) }
    fn p_16(self) -> Self { self.p(64.0) }
    fn p_20(self) -> Self { self.p(80.0) }
    fn p_24(self) -> Self { self.p(96.0) }

    fn px_0(self) -> Self { self.px(0.0) }
    fn px_1(self) -> Self { self.px(4.0) }
    fn px_2(self) -> Self { self.px(8.0) }
    fn px_3(self) -> Self { self.px(12.0) }
    fn px_4(self) -> Self { self.px(16.0) }
    fn px_5(self) -> Self { self.px(20.0) }
    fn px_6(self) -> Self { self.px(24.0) }
    fn px_8(self) -> Self { self.px(32.0) }

    fn py_0(self) -> Self { self.py(0.0) }
    fn py_1(self) -> Self { self.py(4.0) }
    fn py_2(self) -> Self { self.py(8.0) }
    fn py_3(self) -> Self { self.py(12.0) }
    fn py_4(self) -> Self { self.py(16.0) }
    fn py_5(self) -> Self { self.py(20.0) }
    fn py_6(self) -> Self { self.py(24.0) }
    fn py_8(self) -> Self { self.py(32.0) }

    // --- Margin: arbitrary values ---

    fn m(mut self, value: f32) -> Self {
        self.style_config_mut().margin = Some(Spacing::Uniform(value));
        self
    }

    fn mx(mut self, value: f32) -> Self {
        let cfg = self.style_config_mut();
        let (top, _, bottom, _) = cfg.margin.as_ref().map(|s| s.resolve()).unwrap_or_default();
        cfg.margin = Some(Spacing::Sides { top, right: value, bottom, left: value });
        self
    }

    fn my(mut self, value: f32) -> Self {
        let cfg = self.style_config_mut();
        let (_, right, _, left) = cfg.margin.as_ref().map(|s| s.resolve()).unwrap_or_default();
        cfg.margin = Some(Spacing::Sides { top: value, right, bottom: value, left });
        self
    }

    fn mt(mut self, value: f32) -> Self {
        let cfg = self.style_config_mut();
        let (_, right, bottom, left) = cfg.margin.as_ref().map(|s| s.resolve()).unwrap_or_default();
        cfg.margin = Some(Spacing::Sides { top: value, right, bottom, left });
        self
    }

    fn mr(mut self, value: f32) -> Self {
        let cfg = self.style_config_mut();
        let (top, _, bottom, left) = cfg.margin.as_ref().map(|s| s.resolve()).unwrap_or_default();
        cfg.margin = Some(Spacing::Sides { top, right: value, bottom, left });
        self
    }

    fn mb(mut self, value: f32) -> Self {
        let cfg = self.style_config_mut();
        let (top, right, _, left) = cfg.margin.as_ref().map(|s| s.resolve()).unwrap_or_default();
        cfg.margin = Some(Spacing::Sides { top, right, bottom: value, left });
        self
    }

    fn ml(mut self, value: f32) -> Self {
        let cfg = self.style_config_mut();
        let (top, right, bottom, _) = cfg.margin.as_ref().map(|s| s.resolve()).unwrap_or_default();
        cfg.margin = Some(Spacing::Sides { top, right, bottom, left: value });
        self
    }

    // --- Margin: Tailwind scale ---

    fn m_0(self) -> Self { self.m(0.0) }
    fn m_1(self) -> Self { self.m(4.0) }
    fn m_2(self) -> Self { self.m(8.0) }
    fn m_3(self) -> Self { self.m(12.0) }
    fn m_4(self) -> Self { self.m(16.0) }
    fn m_5(self) -> Self { self.m(20.0) }
    fn m_6(self) -> Self { self.m(24.0) }
    fn m_8(self) -> Self { self.m(32.0) }

    fn mx_0(self) -> Self { self.mx(0.0) }
    fn mx_1(self) -> Self { self.mx(4.0) }
    fn mx_2(self) -> Self { self.mx(8.0) }
    fn mx_4(self) -> Self { self.mx(16.0) }
    fn mx_8(self) -> Self { self.mx(32.0) }

    fn my_0(self) -> Self { self.my(0.0) }
    fn my_1(self) -> Self { self.my(4.0) }
    fn my_2(self) -> Self { self.my(8.0) }
    fn my_4(self) -> Self { self.my(16.0) }
    fn my_8(self) -> Self { self.my(32.0) }
}

/// Blanket impl: any type implementing `StyledMut` automatically gets all builder methods.
impl<T: StyledMut + Sized> StyledExt for T {}
