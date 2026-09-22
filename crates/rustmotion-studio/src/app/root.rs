use gpui_component::ActiveTheme;
use gpui_kit::base::v_flex;
use gpui_kit::*;

use crate::app::overlays;
use crate::app::state::StudioState;
use crate::library::Library;
use crate::scenario::View;

pub struct StudioRoot {
    state: Entity<StudioState>,
    library: Entity<Library>,
}

impl StudioRoot {
    pub fn new(state: Entity<StudioState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let library = cx.new(|cx| Library::new(state.clone(), window, cx));
        cx.observe(&state, |_, _, cx| cx.notify()).detach();
        Self { state, library }
    }
}

impl Render for StudioRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.state.read(cx).view;

        let screen = match view {
            View::Library => self.library.clone().into_any_element(),
            View::Editor => v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child("Editor — screen not yet ported to gpui-kit")
                .into_any_element(),
        };

        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(screen)
            .child(overlays::render(window, cx))
    }
}
