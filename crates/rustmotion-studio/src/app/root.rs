use std::sync::Arc;

use dioxus::desktop::{
    use_asset_handler, wry::http::Response, AssetRequest, RequestAsyncResponder,
};
use dioxus::prelude::*;

use crate::editor::StudioApp;
use crate::library::{render_thumbnail, Library, SharedLibrary};
use crate::scenario::{Shared, Theme, View};

/// dx-component CSS, injected globally. `#[css_module]` only injects its compiled
/// stylesheet through the `dx` asset pipeline, which our plain `cargo`-built
/// `rustmotion studio` binary doesn't run — so we bundle the (unscoped) CSS at
/// compile time and inject it as a `<style>` block, the same way the theme is
/// applied. The matching component wrappers use plain class names.
const SELECT_CSS: &str = include_str!("../components/select/style.css");
const COLOR_PICKER_CSS: &str = include_str!("../components/color_picker/style.css");
const SWITCH_CSS: &str = include_str!("../components/switch/style.css");
const SLIDER_CSS: &str = include_str!("../components/slider/style.css");
const INPUT_CSS: &str = include_str!("../components/input/style.css");
const BUTTON_CSS: &str = include_str!("../components/button/style.css");

/// Semantic palette tokens (`--rm-*`) for each theme, plus the `--dx-*` vars the
/// `components/` scaffold reads. `System` defaults to the light palette and
/// swaps to dark under `prefers-color-scheme: dark`. Every studio inline style
/// references these variables so a theme switch repaints the whole app.
const THEME_CSS: &str = r##"
.rm-dark, .rm-system {
  --rm-bg:#0c0d10; --rm-surface:#13161d; --rm-surface-2:#0f1218; --rm-surface-3:#1c2230;
  --rm-border:#1c1f27; --rm-border-2:#2a2f3a;
  --rm-text:#cfd3dc; --rm-text-strong:#e6e9ef; --rm-text-muted:#7a7f8a;
  --rm-accent:#4c8dff; --rm-on-accent:#ffffff; --rm-error:#ff6b6b;
  --rm-overlay-hover:rgba(255,255,255,0.06); --rm-overlay-border:rgba(255,255,255,0.45);
}
.rm-light {
  --rm-bg:#f4f2ee; --rm-surface:#ffffff; --rm-surface-2:#faf8f5; --rm-surface-3:#efece6;
  --rm-border:#e7e4df; --rm-border-2:#d8d3cc;
  --rm-text:#3f3a35; --rm-text-strong:#1c1917; --rm-text-muted:#857d73;
  --rm-accent:#2563eb; --rm-on-accent:#ffffff; --rm-error:#d92d20;
  --rm-overlay-hover:rgba(0,0,0,0.05); --rm-overlay-border:rgba(37,99,235,0.55);
}
@media (prefers-color-scheme: light) {
  .rm-system {
    --rm-bg:#f4f2ee; --rm-surface:#ffffff; --rm-surface-2:#faf8f5; --rm-surface-3:#efece6;
    --rm-border:#e7e4df; --rm-border-2:#d8d3cc;
    --rm-text:#3f3a35; --rm-text-strong:#1c1917; --rm-text-muted:#857d73;
    --rm-accent:#2563eb; --rm-on-accent:#ffffff; --rm-error:#d92d20;
    --rm-overlay-hover:rgba(0,0,0,0.05); --rm-overlay-border:rgba(37,99,235,0.55);
  }
}
.rm-dark, .rm-light, .rm-system {
  --primary-color: var(--rm-text);
  --primary-color-1: var(--rm-text-strong);
  --primary-color-2: var(--rm-text-strong);
  --primary-color-4: var(--rm-text-muted);
  --primary-color-5: var(--rm-text-muted);
  --primary-color-6: var(--rm-border-2);
  --primary-color-7: var(--rm-border);
  --secondary-color-1: var(--rm-surface-2);
  --secondary-color-2: var(--rm-accent);
  --secondary-color-4: var(--rm-border);
  --secondary-color-5: var(--rm-surface);
  --primary-error-color: var(--rm-error);
  --contrast-error-color: var(--rm-on-accent);
  --focused-border-color: var(--rm-accent);
  --dx-sidebar-background: var(--rm-surface);
  --dx-sidebar-foreground: var(--rm-text);
  --dx-sidebar-border: var(--rm-border);
  --dx-sidebar-accent: var(--rm-surface-3);
  --dx-sidebar-accent-foreground: var(--rm-text-strong);
  --dx-sidebar-ring: var(--rm-accent);
  --dx-sidebar-width: 280px;
  --dx-sidebar-width-icon: 56px;
  --dx-sidebar-width-mobile: 280px;
}
html, body { margin: 0; font: 13px -apple-system, sans-serif; }
* { box-sizing: border-box; }
"##;

/// The inspector's segmented-control container (the Align / B-I groups). The
/// buttons inside are now `Button` components; this just gives the group its
/// inset background and rounded border.
const INSPECTOR_CSS: &str = r##"
.rm-seg { display:flex; gap:2px; width:100%; box-sizing:border-box; background:var(--rm-surface-2); border:1px solid var(--rm-border); border-radius:7px; padding:2px; }
"##;

/// Root: switches between the library home and the editor, and serves
/// per-scenario thumbnails (persists across view switches).
#[component]
pub fn StudioRoot() -> Element {
    let library = use_context::<SharedLibrary>();
    let _shared = use_context::<Shared>();

    let view = use_signal(|| {
        if library.lock().map(|l| l.start_in_editor).unwrap_or(false) {
            View::Editor
        } else {
            View::Library
        }
    });

    // Active theme, shared via context so the topbar's swap control can flip it.
    let theme = use_signal(|| Theme::System);
    use_context_provider(|| theme);

    // /thumb/{i} -> scenario at flat index i, frame-0 JPEG (cached; rendered
    // off-lock to avoid blocking other asset requests).
    let thumb_lib = library.clone();
    use_asset_handler(
        "thumb",
        move |request: AssetRequest, responder: RequestAsyncResponder| {
            let uri = request.uri().to_string();
            let i: usize = uri
                .rsplit('/')
                .next()
                .and_then(|s| s.split('?').next())
                .and_then(|s| s.parse().ok())
                .unwrap_or(usize::MAX);

            let (cached, path) = {
                let lib = thumb_lib.lock().unwrap_or_else(|e| e.into_inner());
                let path = lib.path_at(i);
                let cached = path.as_ref().and_then(|p| lib.thumb_cache.get(p).cloned());
                (cached, path)
            };

            let bytes: Vec<u8> = if let Some(c) = cached {
                (*c).clone()
            } else if let Some(p) = path {
                match render_thumbnail(&p) {
                    Some(jpeg) => {
                        let arc = Arc::new(jpeg);
                        thumb_lib
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .thumb_cache
                            .insert(p, arc.clone());
                        (*arc).clone()
                    }
                    None => Vec::new(),
                }
            } else {
                Vec::new()
            };

            responder.respond(
                Response::builder()
                    .header("Content-Type", "image/jpeg")
                    .header("Cache-Control", "no-store")
                    .body(bytes)
                    .unwrap(),
            );
        },
    );

    rsx! {
        style { "{THEME_CSS}" }
        style { "{SELECT_CSS}" }
        style { "{COLOR_PICKER_CSS}" }
        style { "{SWITCH_CSS}" }
        style { "{SLIDER_CSS}" }
        style { "{INPUT_CSS}" }
        style { "{BUTTON_CSS}" }
        style { "{INSPECTOR_CSS}" }
        div { class: "{theme().class()}", style: "min-height:100vh; background:var(--rm-bg); color:var(--rm-text);",
            {match view() {
                View::Library => rsx! { Library { view } },
                View::Editor => rsx! { StudioApp { view } },
            }}
        }
    }
}
