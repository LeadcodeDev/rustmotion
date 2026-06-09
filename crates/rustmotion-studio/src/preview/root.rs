use std::sync::Arc;

use dioxus::desktop::{use_asset_handler, wry::http::Response, AssetRequest, RequestAsyncResponder};
use dioxus::prelude::*;

use super::app_ui::StudioApp;
use super::home::Library;
use super::library::{render_thumbnail, SharedLibrary, View};
use super::model::Shared;

/// Theme variables required by the `components/` scaffold (dioxus-primitives
/// wrappers). A dark palette matching the editor.
const THEME_CSS: &str = r##"
:root {
  --primary-color: #cfd3dc;
  --primary-color-1: #ffffff;
  --primary-color-2: #e6e9ef;
  --primary-color-4: #9aa0ad;
  --primary-color-5: #7a7f8a;
  --primary-color-6: #4b5160;
  --primary-color-7: #343a47;
  --secondary-color-1: #10131a;
  --secondary-color-2: #4c8dff;
  --secondary-color-4: #1c1f27;
  --secondary-color-5: #13161d;
  --primary-error-color: #ff6b6b;
  --contrast-error-color: #ffffff;
  --focused-border-color: #4c8dff;
  --dx-sidebar-background: #13161d;
  --dx-sidebar-foreground: #cfd3dc;
  --dx-sidebar-border: #1c1f27;
  --dx-sidebar-accent: #1c2230;
  --dx-sidebar-accent-foreground: #ffffff;
  --dx-sidebar-ring: #4c8dff;
  --dx-sidebar-width: 280px;
  --dx-sidebar-width-icon: 56px;
  --dx-sidebar-width-mobile: 280px;
}
html, body { margin: 0; background: #0c0d10; color: #cfd3dc; font: 13px -apple-system, sans-serif; }
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
        {match view() {
            View::Library => rsx! { Library { view } },
            View::Editor => rsx! { StudioApp { view } },
        }}
    }
}
