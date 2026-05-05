mod chrome;
mod diff;
mod dimensions;
mod highlight;
mod render;
mod reveal;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::Canvas;

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::schema::{CodeblockChrome, CodeblockHighlight, CodeblockReveal, CodeblockState, TimelineStep};
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Codeblock {
    pub code: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_theme")]
    pub theme: String,
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
    /// When the rendered content overflows the box vertically, scroll up so the
    /// last revealed line stays visible. Default: `true`. Set to `false` to
    /// require that all content fits — the geometry validator will fail
    /// otherwise. Font size is never reduced.
    #[serde(default = "default_auto_scroll")]
    pub auto_scroll: bool,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

fn default_auto_scroll() -> bool { true }

rustmotion_core::impl_traits!(Codeblock {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Painter for Codeblock {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        render::render_codeblock(canvas, self, layout, props, ctx);
    }
}

fn default_language() -> String { "plain".to_string() }
fn default_theme() -> String { "base16-ocean.dark".to_string() }
