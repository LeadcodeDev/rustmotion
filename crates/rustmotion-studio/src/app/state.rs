use std::sync::Arc;

use gpui_kit::RenderImage;

use crate::editor::diff_panel::DiffSide;
use crate::library::SharedLibrary;
use crate::scenario::{Shared, View};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub node_id: u32,
    pub pointer: String,
    pub kind: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemePref {
    Light,
    Dark,
    #[default]
    System,
}

impl ThemePref {
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::System,
            Self::System => Self::Dark,
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
            Self::System => "System",
        }
    }
}

pub struct StudioState {
    pub shared: Shared,
    pub library: SharedLibrary,
    pub view: View,
    pub theme_pref: ThemePref,
}

pub struct EditorState {
    pub current: u32,
    pub playing: bool,
    pub muted: bool,
    pub rev: u64,
    pub selected: Option<Selection>,
    pub show_annotations: bool,
    pub show_hits: bool,
    pub diff_active: bool,
    pub diff_side: DiffSide,
    pub preview_scale: u16,
    pub frame: Option<Arc<RenderImage>>,
}

impl EditorState {
    #[must_use]
    pub fn should_compute_hits(&self) -> bool {
        self.show_hits && !(self.diff_active && self.diff_side == DiffSide::A) && !self.playing
    }
}
