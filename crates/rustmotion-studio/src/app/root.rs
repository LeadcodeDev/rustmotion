use gpui_component::ActiveTheme;
use gpui_kit::base::v_flex;
use gpui_kit::*;

use crate::app::overlays;
use crate::app::state::StudioState;
use crate::scenario::View;

pub struct StudioRoot {
    state: Entity<StudioState>,
}

impl StudioRoot {
    pub fn new(state: Entity<StudioState>, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self { state }
    }
}

impl Render for StudioRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.state.read(cx).view;

        let label = match view {
            View::Library => "Library",
            View::Editor => "Editor",
        };

        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(format!("{label} — screen not yet ported to gpui-kit"))
            .child(overlays::render(window, cx))
    }
}
