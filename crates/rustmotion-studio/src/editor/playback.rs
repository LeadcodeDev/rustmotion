use std::time::Duration;

use dioxus::prelude::*;

use crate::components::button::{Button, ButtonSize, ButtonVariant};
use crate::scenario::Shared;

/// Advance the playhead at the scenario fps while `playing` is true. A custom
/// hook so the editor view stays focused on layout.
pub fn use_playback_clock(shared: Shared, mut current: Signal<u32>, playing: Signal<bool>) {
    use_future(move || {
        let shared = shared.clone();
        async move {
            loop {
                let fps = shared
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .scenario
                    .video
                    .fps
                    .max(1);
                tokio::time::sleep(Duration::from_secs_f64(1.0 / fps as f64)).await;
                if playing() {
                    let total = shared
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .total_frames
                        .max(1);
                    let next = (current() + 1) % total;
                    current.set(next);
                }
            }
        }
    });
}

/// Bump `rev` whenever the watcher swaps in a reloaded model, so the `<img>`
/// refetches the (now changed) current frame.
pub fn use_hot_reload(shared: Shared, mut rev: Signal<u64>) {
    use_future(move || {
        let shared = shared.clone();
        async move {
            let mut last_gen = 0u64;
            loop {
                tokio::time::sleep(Duration::from_millis(250)).await;
                let g = shared.lock().unwrap_or_else(|e| e.into_inner()).generation;
                if g != last_gen {
                    last_gen = g;
                    rev.set(rev() + 1);
                }
            }
        }
    });
}

/// The bottom transport bar: play/pause, step, scrub, and a frame counter.
#[component]
pub fn PlaybackBar(current: Signal<u32>, playing: Signal<bool>, total: u32) -> Element {
    let max = total.saturating_sub(1);
    let cur = current().min(max);
    let is_playing = playing();

    rsx! {
        div { style: "display:flex; align-items:center; gap:12px; padding:12px 20px; border-top:1px solid var(--rm-border); background:var(--rm-surface-2);",
            Button {
                variant: ButtonVariant::Secondary,
                size: ButtonSize::Sm,
                onclick: move |_| playing.set(!playing()),
                if is_playing { "Pause" } else { "Play" }
            }
            Button {
                variant: ButtonVariant::Ghost,
                size: ButtonSize::IconSm,
                onclick: move |_| current.set(cur.saturating_sub(1)),
                "‹"
            }
            Button {
                variant: ButtonVariant::Ghost,
                size: ButtonSize::IconSm,
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
            div { style: "min-width:120px; text-align:right; color:var(--rm-text-muted);",
                "{cur} / {max}"
            }
        }
    }
}
