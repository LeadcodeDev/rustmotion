use dioxus::desktop::{use_asset_handler, wry::http::Response, AssetRequest, RequestAsyncResponder};
use dioxus::prelude::*;

use super::frames::render_png;
use super::model::Shared;

/// Root component. Reads the shared studio model from context, registers an
/// asset handler that renders frames to PNG on demand, and shows the current
/// frame in an `<img>`.
#[component]
pub fn StudioApp() -> Element {
    let shared = use_context::<Shared>();

    // Asset handler: GET /frame/{idx} -> PNG of that frame.
    let handler_shared = shared.clone();
    use_asset_handler("frame", move |request: AssetRequest, responder: RequestAsyncResponder| {
        let uri = request.uri().to_string();
        let idx: u32 = uri
            .rsplit('/')
            .next()
            .and_then(|s| s.split('?').next())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let png = {
            let m = handler_shared.lock().unwrap();
            render_png(&m.scenario, &m.tasks, idx, 1.0).0
        };
        responder.respond(
            Response::builder()
                .header("Content-Type", "image/png")
                .body(png)
                .unwrap(),
        );
    });

    let (w, h, frames, err) = {
        let m = shared.lock().unwrap();
        (
            m.scenario.video.width,
            m.scenario.video.height,
            m.total_frames,
            m.error.clone(),
        )
    };

    rsx! {
        div { style: "margin:0; background:#0c0d10; min-height:100vh; color:#cfd3dc; font:13px -apple-system,sans-serif;",
            if let Some(e) = err {
                div { style: "padding:24px; color:#ff6b6b;", "Error: {e}" }
            } else {
                div { style: "display:flex; flex-direction:column; align-items:center; gap:8px; padding:16px;",
                    img {
                        src: "/frame/0",
                        style: "display:block; max-width:100%; max-height:80vh; height:auto; box-shadow:0 8px 40px rgba(0,0,0,0.5);",
                    }
                    div { style: "color:#7a7f8a;", "{w}×{h} · {frames} frames" }
                }
            }
        }
    }
}
