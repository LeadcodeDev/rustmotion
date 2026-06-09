use std::sync::Arc;

use dioxus::desktop::{use_asset_handler, wry::http::Response, AssetRequest, RequestAsyncResponder};
use dioxus::prelude::*;

use crate::editor::StudioApp;
use crate::library::{render_thumbnail, Library, SharedLibrary};
use crate::scenario::{Shared, Theme, View};

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
  --rm-bg:#f5f6f8; --rm-surface:#ffffff; --rm-surface-2:#eef0f3; --rm-surface-3:#e3e8f0;
  --rm-border:#e2e5ea; --rm-border-2:#d0d4dc;
  --rm-text:#2b2f38; --rm-text-strong:#0c0d10; --rm-text-muted:#6b7280;
  --rm-accent:#2f6fe0; --rm-on-accent:#ffffff; --rm-error:#d92d20;
  --rm-overlay-hover:rgba(0,0,0,0.06); --rm-overlay-border:rgba(0,0,0,0.5);
}
@media (prefers-color-scheme: light) {
  .rm-system {
    --rm-bg:#f5f6f8; --rm-surface:#ffffff; --rm-surface-2:#eef0f3; --rm-surface-3:#e3e8f0;
    --rm-border:#e2e5ea; --rm-border-2:#d0d4dc;
    --rm-text:#2b2f38; --rm-text-strong:#0c0d10; --rm-text-muted:#6b7280;
    --rm-accent:#2f6fe0; --rm-on-accent:#ffffff; --rm-error:#d92d20;
    --rm-overlay-hover:rgba(0,0,0,0.06); --rm-overlay-border:rgba(0,0,0,0.5);
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
                let lib = thumb_lib.lock().unwrap();
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
                        thumb_lib.lock().unwrap().thumb_cache.insert(p, arc.clone());
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
        div { class: "{theme().class()}", style: "min-height:100vh; background:var(--rm-bg); color:var(--rm-text);",
            {match view() {
                View::Library => rsx! { Library { view } },
                View::Editor => rsx! { StudioApp { view } },
            }}
        }
    }
}
