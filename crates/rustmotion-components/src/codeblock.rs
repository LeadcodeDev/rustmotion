use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::Canvas;

use rustmotion_core::layout::{Constraints, LayoutNode};
use rustmotion_core::schema::{
    CodeblockChrome, CodeblockHighlight, CodeblockReveal,
    CodeblockState, LayerStyle, Size,
};
use rustmotion_core::traits::{RenderContext, TimingConfig, Widget};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Codeblock {
    pub code: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub size: Option<Size>,
    #[serde(default)]
    pub show_line_numbers: bool,
    #[serde(default)]
    pub chrome: Option<CodeblockChrome>,
    #[serde(default)]
    pub highlights: Vec<CodeblockHighlight>,
    #[serde(default)]
    pub reveal: Option<CodeblockReveal>,
    #[serde(default)]
    pub states: Vec<CodeblockState>,
    /// Enable diff mode: lines starting with `+` get green background, `-` get red background.
    #[serde(default)]
    pub diff: bool,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

rustmotion_core::impl_traits!(Codeblock {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl Widget for Codeblock {
    fn render(&self, _canvas: &Canvas, _layout: &LayoutNode, _ctx: &RenderContext, _props: &rustmotion_core::engine::animator::AnimatedProperties, _pipeline: &dyn rustmotion_core::traits::RenderPipeline) -> Result<()> {
        // Codeblock rendering is handled by the engine::codeblock module in the rustmotion crate.
        // The render pipeline in the main crate special-cases this component.
        Ok(())
    }

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        match &self.size {
            Some(s) => (s.width, s.height),
            None => (400.0, 300.0),
        }
    }
}

fn default_language() -> String { "plain".to_string() }
fn default_theme() -> String { "base16-ocean.dark".to_string() }
