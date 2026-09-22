//! The studio's owned state — the frozen contract of the gpui-kit rewrite.
//!
//! Under Dioxus the editor's state was ten `Signal<T>` scattered across the
//! component tree, each subscribing its own readers. gpui has no such thing:
//! state is owned by an `Entity<T>` and readers are notified explicitly. This
//! module is where that state lives, and it is written once — by the
//! orchestrator, before any workstream starts — precisely so six independent
//! rewrites cannot each invent their own shape for it.
//!
//! **No workstream modifies this file.** A workstream that needs a field it
//! does not find reports the exact line it wants; the orchestrator adds it.
//!
//! What is *not* here is as deliberate as what is: `StudioModel`, `Mutation`,
//! the history, baseline, sidecar and diff types all keep living in
//! [`crate::scenario`], untouched by the rewrite. They never knew about the UI
//! framework, which is the only reason this chantier is a view-layer rewrite
//! rather than a rewrite of the studio.

use std::sync::Arc;

use gpui_kit::RenderImage;

use crate::editor::diff_panel::DiffSide;
use crate::library::SharedLibrary;
use crate::scenario::{Shared, View};

/// Which element the inspector is editing.
///
/// Replaces the `(u32, String, String)` tuple the Dioxus signal carried. The
/// tuple was positional at ten call sites, and the two `String`s were
/// interchangeable at the type level — the compiler could not tell a pointer
/// passed as a kind from a correct one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    /// Node id from the engine's hit map. `u32::MAX` when the selection came
    /// from the diff panel, which addresses elements by pointer only.
    pub node_id: u32,
    /// Absolute JSON pointer into the scenario's `raw`.
    pub pointer: String,
    /// Component tag (`text`, `card`, …). Decides which inspector sections render.
    pub kind: String,
}

/// The user's theme choice, persisted across launches.
///
/// gpui-component's own `ThemeMode` has only `Light | Dark`; `System` is built
/// on top of it here, by observing the window appearance. Persisted next to the
/// recents, under `dirs::config_dir()/rustmotion/`, because the previous studio
/// reset to `System` at every launch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemePref {
    Light,
    Dark,
    #[default]
    System,
}

impl ThemePref {
    /// Cycle order of the topbar button: Dark → Light → System → Dark.
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

/// Application-level state: which screen is up, and the two shared handles
/// every screen needs.
pub struct StudioState {
    /// The live scenario model. Cloned under a brief lock and used *without*
    /// it — see [`crate::scenario::model`].
    pub shared: Shared,
    /// Workspace scan, recents, thumbnail cache, watcher channel.
    pub library: SharedLibrary,
    pub view: View,
    pub theme_pref: ThemePref,
}

/// Editor-screen state.
///
/// Everything here was a `Signal<T>` under Dioxus. The ordering below follows
/// the old declaration order in `editor/view.rs` so the two can be diffed.
pub struct EditorState {
    /// Playhead, in frames.
    pub current: u32,
    pub playing: bool,
    /// Preview audio mute. The mix itself is owned by [`crate::editor::audio`].
    pub muted: bool,
    /// Hot-reload revision. Bumped when the watcher swaps in a reloaded model,
    /// and by every optimistic edit, so the canvas refetches.
    pub rev: u64,
    pub selected: Option<Selection>,
    pub show_annotations: bool,
    /// The clickable-element overlay. Starts **enabled**, as it did before.
    pub show_hits: bool,
    pub diff_active: bool,
    /// A = baseline, B = current.
    pub diff_side: DiffSide,
    /// Preview render scale, percent. Mirrors the atomic in
    /// [`crate::editor::prefetch`], which non-UI threads read without a
    /// gpui context. Write the atomic *first*, then this field.
    pub preview_scale: u16,
    /// The frame currently on screen, in BGRA.
    ///
    /// Replacing this **must** hand the previous value to
    /// `cx.drop_image(old, Some(window))`. gpui's sprite atlas has no eviction
    /// and each 1920×1080 frame owns a ~8 MB Metal texture, so a swap that
    /// forgets to drop leaks roughly half a gigabyte per second at 60 fps.
    /// Drop in the update path, before paint — never while painting a frame
    /// that still references it.
    pub frame: Option<Arc<RenderImage>>,
}

impl EditorState {
    /// The hit map is a full main-thread paint pass (~150 ms measured), so it
    /// is computed at rest only — never per frame during playback, and never
    /// for the baseline side, which has no selectable elements.
    #[must_use]
    pub fn should_compute_hits(&self) -> bool {
        self.show_hits && !(self.diff_active && self.diff_side == DiffSide::A) && !self.playing
    }
}
