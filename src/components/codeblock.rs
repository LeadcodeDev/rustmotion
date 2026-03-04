use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::Canvas;

use crate::layout::{Constraints, LayoutNode};
use crate::schema::{
    CodeblockChrome, CodeblockHighlight, CodeblockPadding, CodeblockReveal,
    CodeblockState, Size,
};
use crate::traits::{AnimationConfig, RenderContext, StyleConfig, TimingConfig, Widget};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Codeblock {
    pub code: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub size: Option<Size>,
    #[serde(default = "default_font_family")]
    pub font_family: String,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_font_weight")]
    pub font_weight: u16,
    #[serde(default = "default_line_height")]
    pub line_height: f32,
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub show_line_numbers: bool,
    #[serde(default)]
    pub chrome: Option<CodeblockChrome>,
    #[serde(default)]
    pub padding: Option<CodeblockPadding>,
    #[serde(default = "default_corner_radius")]
    pub corner_radius: f32,
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
    #[serde(flatten)]
    pub style: StyleConfig,
}

crate::impl_traits!(Codeblock {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Widget for Codeblock {
    fn render(&self, canvas: &Canvas, _layout: &LayoutNode, ctx: &RenderContext) -> Result<()> {
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
fn default_font_family() -> String { "JetBrains Mono".to_string() }
fn default_font_size() -> f32 { 16.0 }
fn default_font_weight() -> u16 { 400 }
fn default_line_height() -> f32 { 1.5 }
fn default_corner_radius() -> f32 { 12.0 }
