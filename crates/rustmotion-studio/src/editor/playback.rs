use std::time::Duration;

use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::slider::{Slider, SliderState, SliderValue};
use gpui_component::{h_flex, ActiveTheme, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, App, AppContext as _, Context, Entity, InteractiveElement, IntoElement, Modifiers,
    ParentElement, RenderOnce, SharedString, Styled, Window,
};

use crate::app::state::EditorState;
use crate::scenario::Shared;

use super::diff_panel::DiffSide;
use super::prefetch::{set_preview_scale_pct, PREVIEW_SCALE_CHOICES};
use super::surface::request_next_frame_if_playing;

gpui_kit::actions!(
    editor_playback,
    [
        TogglePlay,
        StepBackward,
        StepBackwardBig,
        StepForward,
        StepForwardBig,
        SeekToStart,
        SeekToEnd,
    ]
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackAction {
    TogglePlay,
    Step(i64),
    SeekStart,
    SeekEnd,
}

pub fn playback_action(key: &str, mods: Modifiers) -> Option<PlaybackAction> {
    if mods.control || mods.platform || mods.alt {
        return None;
    }
    let step = if mods.shift { 10 } else { 1 };
    match key {
        "space" if !mods.shift => Some(PlaybackAction::TogglePlay),
        "left" => Some(PlaybackAction::Step(-step)),
        "right" => Some(PlaybackAction::Step(step)),
        "home" => Some(PlaybackAction::SeekStart),
        "end" => Some(PlaybackAction::SeekEnd),
        _ => None,
    }
}

pub fn apply_playback_action(
    action: PlaybackAction,
    shared: &Shared,
    editor: &Entity<EditorState>,
    window: &mut Window,
    cx: &mut App,
) {
    let total_frames = || {
        shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .total_frames
    };
    match action {
        PlaybackAction::TogglePlay => {
            editor.update(cx, |state, cx| {
                state.playing = !state.playing;
                cx.notify();
            });
        }
        PlaybackAction::Step(delta) => {
            let max = total_frames().saturating_sub(1);
            editor.update(cx, |state, cx| {
                state.playing = false;
                let next = (state.current as i64 + delta).clamp(0, max as i64) as u32;
                state.current = next;
                cx.notify();
            });
        }
        PlaybackAction::SeekStart => {
            editor.update(cx, |state, cx| {
                state.current = 0;
                cx.notify();
            });
        }
        PlaybackAction::SeekEnd => {
            let max = total_frames().saturating_sub(1);
            editor.update(cx, |state, cx| {
                state.current = max;
                cx.notify();
            });
        }
    }
    let playing = editor.read(cx).playing;
    request_next_frame_if_playing(playing, window);
}

pub fn spawn_playback_clock<V: 'static>(
    shared: Shared,
    editor: Entity<EditorState>,
    cx: &mut Context<V>,
) {
    cx.spawn(async move |_this, cx| {
        let mut audio_armed = false;
        loop {
            let fps = {
                let m = shared.lock().unwrap_or_else(|e| e.into_inner());
                m.scenario.video.fps.max(1)
            };
            cx.background_executor()
                .timer(Duration::from_secs_f64(1.0 / fps as f64))
                .await;
            let playing = editor.read_with(cx, |state, _| state.playing);
            if !playing {
                if audio_armed {
                    super::audio::stop();
                    audio_armed = false;
                }
                continue;
            }
            let total = {
                let m = shared.lock().unwrap_or_else(|e| e.into_inner());
                m.total_frames.max(1)
            };
            let current = editor.read_with(cx, |state, _| state.current);
            if super::audio::has_audio() && !audio_armed {
                super::audio::play_from_frame(current, fps);
                audio_armed = true;
            }
            let next = match super::audio::position_frame(fps) {
                Some(f) if f < total => f,
                _ => (current + 1) % total,
            };
            if next < current {
                audio_armed = false;
            }
            editor.update(cx, |state, cx| {
                state.current = next;
                cx.notify();
            });
        }
    })
    .detach();
}

pub fn spawn_hot_reload<V: 'static>(
    shared: Shared,
    editor: Entity<EditorState>,
    cx: &mut Context<V>,
) {
    cx.spawn(async move |_this, cx| {
        let mut last_gen: Option<u64> = None;
        let mut last_audio_fp: Option<u64> = None;
        loop {
            let (g, scenario, total) = {
                let m = shared.lock().unwrap_or_else(|e| e.into_inner());
                (m.generation, m.scenario.clone(), m.total_frames)
            };
            if last_gen != Some(g) {
                if last_gen.is_some() {
                    editor.update(cx, |state, cx| {
                        state.rev = state.rev.wrapping_add(1);
                        cx.notify();
                    });
                }
                last_gen = Some(g);
                let fps = scenario.video.fps.max(1);
                let total_duration = total as f64 / fps as f64;
                let fp = super::audio::audio_fingerprint(&scenario.audio, total_duration);
                if last_audio_fp != Some(fp) {
                    last_audio_fp = Some(fp);
                    super::audio::prepare(scenario, total_duration);
                }
            }
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
        }
    })
    .detach();
}

pub fn frame_to_fraction(frame: u32, total_frames: u32) -> f32 {
    let max = total_frames.saturating_sub(1);
    if max == 0 {
        return 0.0;
    }
    (frame.min(max) as f32 / max as f32).clamp(0.0, 1.0)
}

pub fn fraction_to_frame(fraction: f32, total_frames: u32) -> u32 {
    let max = total_frames.saturating_sub(1);
    (fraction.clamp(0.0, 1.0) * max as f32).round() as u32
}

pub fn new_scrubber(cx: &mut App) -> Entity<SliderState> {
    cx.new(|_| SliderState::new().min(0.).max(1.).step(0.0001))
}

pub struct TransportBar {
    shared: Shared,
    editor: Entity<EditorState>,
    scrubber: Entity<SliderState>,
}

impl TransportBar {
    pub fn new(shared: Shared, editor: Entity<EditorState>, scrubber: Entity<SliderState>) -> Self {
        Self {
            shared,
            editor,
            scrubber,
        }
    }
}

impl RenderOnce for TransportBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let TransportBar {
            shared,
            editor,
            scrubber,
        } = self;
        let state = editor.read(cx);
        let total = shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .total_frames;
        let max = total.saturating_sub(1);
        let cur = state.current.min(max);
        let is_playing = state.playing;
        let is_muted = state.muted;
        let diff_active = state.diff_active;
        let diff_side = state.diff_side;
        let preview_scale = state.preview_scale;

        scrubber.update(cx, |slider, cx| {
            slider.set_value(
                SliderValue::Single(frame_to_fraction(cur, total)),
                window,
                cx,
            );
        });

        let play_editor = editor.clone();
        let mute_editor = editor.clone();
        let side_a_editor = editor.clone();
        let side_b_editor = editor.clone();
        let step_back_editor = editor.clone();
        let step_fwd_editor = editor.clone();

        h_flex()
            .id("transport-bar")
            .items_center()
            .gap_3()
            .px_5()
            .py_3()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("transport-play")
                    .icon(if is_playing {
                        IconName::Pause
                    } else {
                        IconName::Play
                    })
                    .tooltip(if is_playing {
                        "Pause (Space)"
                    } else {
                        "Play (Space)"
                    })
                    .ghost()
                    .small()
                    .on_click(move |_, _, cx| {
                        play_editor.update(cx, |state, cx| {
                            state.playing = !state.playing;
                            cx.notify();
                        });
                    }),
            )
            .child(
                Button::new("transport-mute")
                    .label(if is_muted { "Unmute" } else { "Mute" })
                    .tooltip(if is_muted { "Unmute" } else { "Mute" })
                    .ghost()
                    .small()
                    .on_click(move |_, _, cx| {
                        mute_editor.update(cx, |state, cx| {
                            state.muted = !state.muted;
                            super::audio::set_muted(state.muted);
                            cx.notify();
                        });
                    }),
            )
            .when(diff_active, |el| {
                el.child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new("transport-side-a")
                                .label("A")
                                .tooltip("Baseline")
                                .when(diff_side == DiffSide::A, |b| b.primary())
                                .when(diff_side != DiffSide::A, |b| b.ghost())
                                .small()
                                .on_click(move |_, _, cx| {
                                    side_a_editor.update(cx, |state, cx| {
                                        state.diff_side = DiffSide::A;
                                        cx.notify();
                                    });
                                }),
                        )
                        .child(
                            Button::new("transport-side-b")
                                .label("B")
                                .tooltip("Current")
                                .when(diff_side == DiffSide::B, |b| b.primary())
                                .when(diff_side != DiffSide::B, |b| b.ghost())
                                .small()
                                .on_click(move |_, _, cx| {
                                    side_b_editor.update(cx, |state, cx| {
                                        state.diff_side = DiffSide::B;
                                        cx.notify();
                                    });
                                }),
                        ),
                )
            })
            .child(
                Button::new("transport-step-back")
                    .icon(IconName::ChevronLeft)
                    .ghost()
                    .small()
                    .on_click(move |_, _, cx| {
                        step_back_editor.update(cx, |state, cx| {
                            state.current = state.current.saturating_sub(1);
                            cx.notify();
                        });
                    }),
            )
            .child(
                Button::new("transport-step-forward")
                    .icon(IconName::ChevronRight)
                    .ghost()
                    .small()
                    .on_click(move |_, _, cx| {
                        step_fwd_editor.update(cx, |state, cx| {
                            state.current = (state.current + 1).min(max);
                            cx.notify();
                        });
                    }),
            )
            .child(div().flex_1().child(Slider::new(&scrubber)))
            .child(preview_scale_selector(editor.clone(), preview_scale))
            .child(
                div()
                    .min_w(px(96.))
                    .text_right()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{cur} / {max}")),
            )
    }
}

fn preview_scale_selector(editor: Entity<EditorState>, current_pct: u16) -> impl IntoElement {
    h_flex()
        .id("preview-scale")
        .gap_1()
        .children(PREVIEW_SCALE_CHOICES.iter().map(|&pct| {
            let active = pct == current_pct;
            let editor = editor.clone();
            Button::new(SharedString::from(format!("preview-scale-{pct}")))
                .label(format!("{pct}%"))
                .tooltip("Preview quality (export is always 100%)")
                .when(active, |b| b.primary())
                .when(!active, |b| b.ghost())
                .xsmall()
                .on_click(move |_, _, cx| {
                    set_preview_scale_pct(pct);
                    editor.update(cx, |state, cx| {
                        state.preview_scale = pct;
                        cx.notify();
                    });
                })
        }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn scrubber_fraction_spans_the_whole_timeline() {
        assert_eq!(frame_to_fraction(0, 4340), 0.0);
        assert_eq!(frame_to_fraction(4339, 4340), 1.0);
        let midpoint = frame_to_fraction(2169, 4340);
        assert!((midpoint - 0.5).abs() < 0.001, "got {midpoint}");
    }

    #[test]
    fn scrubber_fraction_round_trips_to_the_same_frame() {
        for frame in [0u32, 1, 23, 2169, 4339] {
            let back = fraction_to_frame(frame_to_fraction(frame, 4340), 4340);
            assert_eq!(back, frame, "frame {frame} did not survive the round trip");
        }
    }

    #[test]
    fn scrubber_fraction_is_zero_when_the_scenario_has_one_frame() {
        assert_eq!(frame_to_fraction(0, 1), 0.0);
        assert_eq!(frame_to_fraction(5, 1), 0.0);
        assert_eq!(fraction_to_frame(1.0, 1), 0);
    }

    #[test]
    fn scrubber_fraction_clamps_out_of_range_input() {
        assert_eq!(fraction_to_frame(-0.5, 100), 0);
        assert_eq!(fraction_to_frame(1.5, 100), 99);
        assert_eq!(frame_to_fraction(999, 100), 1.0);
    }

    use super::*;

    fn mods(shift: bool, control: bool, alt: bool, platform: bool) -> Modifiers {
        Modifiers {
            shift,
            control,
            alt,
            platform,
            function: false,
        }
    }

    #[test]
    fn space_toggles_play() {
        assert_eq!(
            playback_action("space", Modifiers::none()),
            Some(PlaybackAction::TogglePlay)
        );
    }

    #[test]
    fn arrows_step_one_or_ten() {
        assert_eq!(
            playback_action("left", Modifiers::none()),
            Some(PlaybackAction::Step(-1))
        );
        assert_eq!(
            playback_action("right", Modifiers::none()),
            Some(PlaybackAction::Step(1))
        );
        assert_eq!(
            playback_action("right", mods(true, false, false, false)),
            Some(PlaybackAction::Step(10))
        );
        assert_eq!(
            playback_action("left", mods(true, false, false, false)),
            Some(PlaybackAction::Step(-10))
        );
    }

    #[test]
    fn home_end_seek() {
        assert_eq!(
            playback_action("home", Modifiers::none()),
            Some(PlaybackAction::SeekStart)
        );
        assert_eq!(
            playback_action("end", Modifiers::none()),
            Some(PlaybackAction::SeekEnd)
        );
    }

    #[test]
    fn unhandled_or_modified_keys_are_none() {
        assert_eq!(playback_action("z", Modifiers::none()), None);
        assert_eq!(
            playback_action("space", mods(false, false, false, true)),
            None
        );
        assert_eq!(
            playback_action("space", mods(true, false, false, false)),
            None
        );
        assert_eq!(
            playback_action("right", mods(false, true, false, false)),
            None
        );
        assert_eq!(playback_action("enter", Modifiers::none()), None);
    }
}
