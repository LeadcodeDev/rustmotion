use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::Canvas;

use crate::layout::{Constraints, LayoutNode};
use crate::schema::{
    CodeblockChrome, CodeblockHighlight, CodeblockReveal,
    CodeblockState, LayerStyle, Size,
};
use crate::traits::{AnimationConfig, RenderContext, TimingConfig, Widget};

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
    // Composed behaviors
    #[serde(flatten)]
    pub animation: AnimationConfig,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

crate::impl_traits!(Codeblock {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Widget for Codeblock {
    fn render(&self, canvas: &Canvas, _layout: &LayoutNode, ctx: &RenderContext, _props: &crate::engine::animator::AnimatedProperties) -> Result<()> {
        crate::engine::codeblock::render_codeblock_v2(canvas, self, ctx.time)
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
