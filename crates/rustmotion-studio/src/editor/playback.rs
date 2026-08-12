use std::time::Duration;

use dioxus::prelude::*;
use dioxus_icons::lucide::{Pause, Play, Volume2, VolumeX};

use crate::components::button::{Button, ButtonSize, ButtonVariant};
use crate::components::select::{Select, SelectOption};
use crate::scenario::Shared;

use super::prefetch::{set_preview_scale_pct, PREVIEW_SCALE_CHOICES};

/// What a playback keyboard shortcut does (see [`playback_action`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackAction {
    TogglePlay,
    /// Step the playhead by N frames (pausing first).
    Step(i64),
    SeekStart,
    SeekEnd,
}

/// Map a key press to a playback action: Space → toggle, arrows → ±1 frame
/// (±10 with Shift), Home/End → first/last frame. `None` for anything else or
/// when command modifiers are held (those belong to other shortcuts).
pub fn playback_action(key: &Key, mods: Modifiers) -> Option<PlaybackAction> {
    if mods.meta() || mods.ctrl() || mods.alt() {
        return None;
    }
    let step = if mods.shift() { 10 } else { 1 };
    match key {
        Key::Character(c) if c == " " && !mods.shift() => Some(PlaybackAction::TogglePlay),
        Key::ArrowLeft => Some(PlaybackAction::Step(-step)),
        Key::ArrowRight => Some(PlaybackAction::Step(step)),
        Key::Home => Some(PlaybackAction::SeekStart),
        Key::End => Some(PlaybackAction::SeekEnd),
        _ => None,
    }
}

/// Advance the playhead while `playing` is true, and keep the sound with it.
///
/// When the scenario has audio the playhead follows the *audio* clock rather
/// than its own timer: a timer ticking at 1/fps drifts against the sound card
/// over a long scenario, and by the end the picture no longer matches what you
/// hear. With no track (or no output device) it falls back to the timer.
pub fn use_playback_clock(shared: Shared, mut current: Signal<u32>, playing: Signal<bool>) {
    use_future(move || {
        let shared = shared.clone();
        async move {
            // Whether the sound for this playback run has been started. Reset on
            // pause and on loop, so a mix that finishes preparing mid-playback
            // still gets picked up.
            let mut audio_armed = false;
            loop {
                let fps = shared
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .scenario
                    .video
                    .fps
                    .max(1);
                tokio::time::sleep(Duration::from_secs_f64(1.0 / fps as f64)).await;
                if !playing() {
                    if audio_armed {
                        super::audio::stop();
                        audio_armed = false;
                    }
                    continue;
                }
                let total = shared
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .total_frames
                    .max(1);
                if super::audio::has_audio() && !audio_armed {
                    super::audio::play_from_frame(current(), fps);
                    audio_armed = true;
                }
                let from = current();
                let next = match super::audio::position_frame(fps) {
                    Some(f) if f < total => f,
                    _ => (from + 1) % total,
                };
                // Wrapping past the last frame restarts the track with the picture.
                if next < from {
                    audio_armed = false;
                }
                current.set(next);
            }
        }
    });
}

/// Bump `rev` whenever the watcher swaps in a reloaded model, so the `<img>`
/// refetches the (now changed) current frame — and re-mix the preview audio for
/// whatever scenario is now loaded. The mix has to follow the model: opening
/// another file, or editing this one's tracks, otherwise keeps playing the
/// previous scenario's sound.
pub fn use_hot_reload(shared: Shared, mut rev: Signal<u64>) {
    use_future(move || {
        let shared = shared.clone();
        async move {
            let mut last_gen: Option<u64> = None;
            loop {
                let (g, scenario, total) = {
                    let m = shared.lock().unwrap_or_else(|e| e.into_inner());
                    (m.generation, m.scenario.clone(), m.total_frames)
                };
                if last_gen != Some(g) {
                    if last_gen.is_some() {
                        rev.set(rev() + 1);
                    }
                    last_gen = Some(g);
                    let fps = scenario.video.fps.max(1);
                    super::audio::prepare(scenario, total as f64 / fps as f64);
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
    });
}

/// The bottom transport bar: play/pause, step, scrub, and a frame counter.
/// In diff mode an A|B segmented control flips the canvas between the baseline
/// (A) and the current state (B) at the same frame — the classic motion-review
/// gesture.
#[component]
pub fn PlaybackBar(
    current: Signal<u32>,
    playing: Signal<bool>,
    total: u32,
    fps: u32,
    mut muted: Signal<bool>,
    diff_active: Signal<bool>,
    mut diff_side: Signal<super::diff_panel::DiffSide>,
    mut preview_scale: Signal<u16>,
) -> Element {
    use super::diff_panel::DiffSide;

    let max = total.saturating_sub(1);
    let cur = current().min(max);
    let is_playing = playing();
    let is_muted = muted();
    let side = diff_side();

    rsx! {
        div {
            style: "display:flex; align-items:center; gap:12px; padding:12px 20px; border-top:1px solid var(--rm-border); background:var(--rm-surface-2);",
            // Focused transport controls behave natively (arrows on the range
            // slider step the frame, Space re-activates the focused button —
            // both ARE playback actions); stopping propagation prevents the
            // root shortcut handler from double-applying them.
            onkeydown: move |evt: KeyboardEvent| evt.stop_propagation(),
            Button {
                variant: ButtonVariant::Secondary,
                size: ButtonSize::IconSm,
                title: if is_playing { "Pause (Space)" } else { "Play (Space)" },
                onclick: move |_| playing.set(!playing()),
                if is_playing {
                    Pause { size: 15 }
                } else {
                    Play { size: 15 }
                }
            }
            Button {
                variant: ButtonVariant::Secondary,
                size: ButtonSize::IconSm,
                title: if is_muted { "Unmute" } else { "Mute" },
                onclick: move |_| {
                    let next = !muted();
                    muted.set(next);
                    super::audio::set_muted(next);
                },
                if is_muted {
                    VolumeX { size: 15 }
                } else {
                    Volume2 { size: 15 }
                }
            }
            if diff_active() {
                div { class: "rm-seg", style: "width:auto; flex:none;",
                    Button {
                        variant: if side == DiffSide::A { ButtonVariant::Primary } else { ButtonVariant::Ghost },
                        size: ButtonSize::Sm,
                        title: "Baseline",
                        onclick: move |_| diff_side.set(DiffSide::A),
                        "A"
                    }
                    Button {
                        variant: if side == DiffSide::B { ButtonVariant::Primary } else { ButtonVariant::Ghost },
                        size: ButtonSize::Sm,
                        title: "Current",
                        onclick: move |_| diff_side.set(DiffSide::B),
                        "B"
                    }
                }
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
                        if playing() {
                            super::audio::play_from_frame(v, fps);
                        }
                    }
                },
            }
            // Preview quality: render scale of the preview frames only (the
            // export always renders at 100%). Lower = smoother playback on
            // heavy scenarios (glass, camera, parallax).
            div {
                title: "Preview quality (export is always 100%)",
                style: "width:96px; flex:none;",
                Select::<u16> {
                    default_value: Some(preview_scale()),
                    on_value_change: move |v: Option<u16>| {
                        if let Some(pct) = v {
                            // Atomic first: the render threads and the asset
                            // handler must see the new scale before the signal
                            // change triggers the <img> refetch.
                            set_preview_scale_pct(pct);
                            preview_scale.set(pct);
                        }
                    },
                    for (i, pct) in PREVIEW_SCALE_CHOICES.iter().enumerate() {
                        SelectOption::<u16> {
                            key: "{pct}",
                            index: i,
                            value: *pct,
                            text_value: "{pct}%",
                            "{pct}%"
                        }
                    }
                }
            }
            div { style: "min-width:120px; text-align:right; color:var(--rm-text-muted);",
                "{cur} / {max}"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(s: &str) -> Key {
        Key::Character(s.to_string())
    }

    #[test]
    fn space_toggles_play() {
        assert_eq!(
            playback_action(&ch(" "), Modifiers::empty()),
            Some(PlaybackAction::TogglePlay)
        );
    }

    #[test]
    fn arrows_step_one_or_ten() {
        assert_eq!(
            playback_action(&Key::ArrowLeft, Modifiers::empty()),
            Some(PlaybackAction::Step(-1))
        );
        assert_eq!(
            playback_action(&Key::ArrowRight, Modifiers::empty()),
            Some(PlaybackAction::Step(1))
        );
        assert_eq!(
            playback_action(&Key::ArrowRight, Modifiers::SHIFT),
            Some(PlaybackAction::Step(10))
        );
        assert_eq!(
            playback_action(&Key::ArrowLeft, Modifiers::SHIFT),
            Some(PlaybackAction::Step(-10))
        );
    }

    #[test]
    fn home_end_seek() {
        assert_eq!(
            playback_action(&Key::Home, Modifiers::empty()),
            Some(PlaybackAction::SeekStart)
        );
        assert_eq!(
            playback_action(&Key::End, Modifiers::empty()),
            Some(PlaybackAction::SeekEnd)
        );
    }

    #[test]
    fn unhandled_or_modified_keys_are_none() {
        assert_eq!(playback_action(&ch("z"), Modifiers::empty()), None);
        assert_eq!(playback_action(&ch(" "), Modifiers::META), None);
        assert_eq!(playback_action(&ch(" "), Modifiers::SHIFT), None);
        assert_eq!(playback_action(&Key::ArrowRight, Modifiers::CONTROL), None);
        assert_eq!(playback_action(&Key::Enter, Modifiers::empty()), None);
    }
}
