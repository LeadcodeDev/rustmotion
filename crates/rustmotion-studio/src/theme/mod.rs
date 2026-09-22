pub mod persist;

use gpui_component::{Theme, ThemeMode};
use gpui_kit::{App, Entity, Window};

use crate::app::state::{StudioState, ThemePref};

pub fn apply(pref: ThemePref, window: Option<&mut Window>, cx: &mut App) {
    match pref {
        ThemePref::System => Theme::sync_system_appearance(window, cx),
        ThemePref::Light => Theme::change(ThemeMode::Light, window, cx),
        ThemePref::Dark => Theme::change(ThemeMode::Dark, window, cx),
    }
}

pub fn set(state: &Entity<StudioState>, pref: ThemePref, window: &mut Window, cx: &mut App) {
    state.update(cx, |state, cx| {
        state.theme_pref = pref;
        cx.notify();
    });
    persist::save_theme_pref(pref);
    apply(pref, Some(window), cx);
}

pub fn watch_system_appearance(state: Entity<StudioState>, window: &mut Window) {
    window
        .observe_window_appearance(move |window, cx| {
            if state.read(cx).theme_pref == ThemePref::System {
                Theme::sync_system_appearance(Some(window), cx);
            }
        })
        .detach();
}
