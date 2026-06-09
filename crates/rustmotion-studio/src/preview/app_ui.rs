use std::time::Duration;

use dioxus::desktop::{use_asset_handler, wry::http::Response, AssetRequest, RequestAsyncResponder};
use dioxus::prelude::*;

use super::frames::render_png;
use super::model::Shared;

/// Root component. Reads the shared studio model from context, registers an
/// asset handler that renders frames to PNG on demand, and provides playback,
/// scrubbing, and hot-reload of the current frame.
#[component]
pub fn StudioApp() -> Element {
    let shared = use_context::<Shared>();

    let mut current = use_signal(|| 0u32);
    let mut playing = use_signal(|| false);
    let rev = use_signal(|| 0u64);
    let mut selected = use_signal(|| false);

    // Asset handler: GET /frame/{idx} -> PNG of that frame.
    let handler_shared = shared.clone();
    use_asset_handler(
        "frame",
        move |request: AssetRequest, responder: RequestAsyncResponder| {
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
        },
    );

    // Playback clock: while playing, advance the frame at the scenario fps.
    let clock_shared = shared.clone();
    use_future(move || {
        let clock_shared = clock_shared.clone();
        let mut current = current;
        let playing = playing;
        async move {
            loop {
                let fps = clock_shared.lock().unwrap().scenario.video.fps.max(1);
                tokio::time::sleep(Duration::from_secs_f64(1.0 / fps as f64)).await;
                if playing() {
                    let total = clock_shared.lock().unwrap().total_frames.max(1);
                    let next = (current() + 1) % total;
                    current.set(next);
                }
            }
        }
    });

    // Hot-reload poller: when the watcher bumps the model generation, bump rev
    // so the <img> refetches the (now changed) current frame.
    let poll_shared = shared.clone();
    use_future(move || {
        let poll_shared = poll_shared.clone();
        let mut rev = rev;
        async move {
            let mut last_gen = 0u64;
            loop {
                tokio::time::sleep(Duration::from_millis(250)).await;
                let g = poll_shared.lock().unwrap().generation;
                if g != last_gen {
                    last_gen = g;
                    rev.set(rev() + 1);
                }
            }
        }
    });

    let (total, err) = {
        let m = shared.lock().unwrap();
        (m.total_frames, m.error.clone())
    };
    let max = total.saturating_sub(1);
    let cur = current().min(max);
    let r = rev();
    let is_playing = playing();

    // Hardcoded hotspot in percent of the frame — proves the overlay coordinate
    // model. The real studio will fill these from the engine hit-map.
    let hotspot_border = if selected() {
        "2px solid #4c8dff; background:rgba(76,141,255,0.15)"
    } else {
        "1px dashed rgba(255,255,255,0.45)"
    };
    let hotspot_style = format!(
        "position:absolute; left:20%; top:30%; width:40%; height:14%; box-sizing:border-box; cursor:pointer; border:{hotspot_border};"
    );

    if let Some(e) = err {
        return rsx! {
            div { style: "padding:24px; color:#ff6b6b; background:#0c0d10; min-height:100vh; font:13px sans-serif;",
                "Error: {e}"
            }
        };
    }

    rsx! {
        div { style: "margin:0; background:#0c0d10; min-height:100vh; color:#cfd3dc; font:13px -apple-system,sans-serif; display:flex; flex-direction:column;",
            div { style: "flex:1; display:flex; align-items:center; justify-content:center; padding:16px; overflow:hidden;",
                div { style: "position:relative; display:inline-block; box-shadow:0 8px 40px rgba(0,0,0,0.5); line-height:0;",
                    img {
                        src: "/frame/{cur}?v={r}",
                        style: "display:block; max-width:100%; max-height:78vh; height:auto;",
                    }
                    div { style: "position:absolute; inset:0;",
                        div {
                            style: "{hotspot_style}",
                            onclick: move |_| selected.set(!selected()),
                        }
                    }
                }
            }
            div { style: "display:flex; align-items:center; gap:12px; padding:12px 20px; border-top:1px solid #1c1f27; background:#10131a;",
                button {
                    style: "min-width:64px; padding:6px 10px; cursor:pointer;",
                    onclick: move |_| playing.set(!playing()),
                    if is_playing { "Pause" } else { "Play" }
                }
                button {
                    style: "padding:6px 10px; cursor:pointer;",
                    onclick: move |_| current.set(cur.saturating_sub(1)),
                    "‹"
                }
                button {
                    style: "padding:6px 10px; cursor:pointer;",
                    onclick: move |_| current.set((cur + 1).min(max)),
                    "›"
                }
                input {
                    r#type: "range",
                    min: "0",
                    max: "{max}",
                    value: "{cur}",
                    style: "flex:1;",
                    oninput: move |e| {
                        if let Ok(v) = e.value().parse::<u32>() {
                            current.set(v);
                        }
                    },
                }
                div { style: "min-width:120px; text-align:right; color:#7a7f8a;",
                    "{cur} / {max}"
                }
            }
        }
    }
}
