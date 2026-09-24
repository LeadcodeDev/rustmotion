use gpui_component::Root;
use gpui_kit::{
    px, size, App, AppContext as _, AsyncApp, Bounds, Pixels, TitlebarOptions, WindowBounds,
    WindowOptions,
};

use crate::app::root::StudioRoot;
use crate::app::state::{StudioState, ThemePref};
use crate::library::SharedLibrary;
use crate::scenario::{Shared, View};
use crate::theme;

pub fn open(
    shared: Shared,
    library: SharedLibrary,
    view: View,
    theme_pref: ThemePref,
    cx: &mut AsyncApp,
) {
    let bounds = cx.update(initial_bounds);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("Rustmotion Studio".into()),
            ..Default::default()
        }),
        window_min_size: Some(size(px(720.), px(480.))),
        ..Default::default()
    };

    cx.open_window(options, move |window, cx| {
        let state = cx.new(|_| StudioState {
            shared,
            library,
            view,
            theme_pref,
        });

        theme::apply(theme_pref, Some(window), cx);
        theme::watch_system_appearance(state.clone(), window);

        let root_view = cx.new(|cx| StudioRoot::new(state, window, cx));
        cx.new(|cx| Root::new(root_view, window, cx))
    })
    .expect("failed to open window");
}

fn initial_bounds(cx: &mut App) -> Bounds<Pixels> {
    let target = cx
        .primary_display()
        .map(|display| display.bounds().size)
        .map(|full| size(full.width * 0.75, full.height * 0.75))
        .unwrap_or_else(|| size(px(1280.), px(800.)));
    Bounds::centered(None, target, cx)
}
