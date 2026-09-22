use gpui_component::Root;
use gpui_kit::*;

pub fn render(window: &mut Window, cx: &mut App) -> impl IntoElement {
    div()
        .children(Root::render_sheet_layer(window, cx))
        .children(Root::render_dialog_layer(window, cx))
        .children(Root::render_notification_layer(window, cx))
}
