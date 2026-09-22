use std::sync::Arc;

use image::{Frame, ImageBuffer, Rgba};
use smallvec::smallvec;

use gpui_kit::{
    div, img, AnyElement, App, ImageSource, IntoElement, ObjectFit, ParentElement, RenderImage,
    Styled, StyledImage, Window,
};

use rustmotion::encode::video::FrameTask;
use rustmotion::schema::ResolvedScenario;

use crate::app::state::EditorState;
use crate::scenario::Shared;

use super::diff_panel::DiffSide;
use super::frames::render_frame_rgba_deep;
use super::prefetch::{ensure_prefetcher, frame_cache, publish_target, FrameKey, PrefetchTarget};

pub fn frame_from_rgba(width: u32, height: u32, mut rgba: Vec<u8>) -> Arc<RenderImage> {
    let (pixels, _) = rgba.as_chunks_mut::<4>();
    for pixel in pixels {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }
    let buffer: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_raw(width, height, rgba).expect("rgba byte length matches dimensions");
    Arc::new(RenderImage::new(smallvec![Frame::new(buffer)]))
}

#[allow(clippy::result_unit_err)]
pub fn render_current_frame(
    scenario: &ResolvedScenario,
    tasks: &[FrameTask],
    frame: u32,
    scale: f32,
) -> Result<Arc<RenderImage>, ()> {
    let (width, height, rgba) = render_frame_rgba_deep(scenario, tasks, frame, scale)?;
    if width == 0 || height == 0 {
        return Err(());
    }
    Ok(frame_from_rgba(width, height, rgba))
}

pub fn swap_frame(
    editor: &mut EditorState,
    image: Arc<RenderImage>,
    window: &mut Window,
    cx: &mut App,
) {
    if let Some(old) = editor.frame.replace(image) {
        cx.drop_image(old, Some(window));
    }
}

pub fn request_next_frame_if_playing(playing: bool, window: &Window) {
    if playing {
        window.request_animation_frame();
    }
}

pub fn frame_surface(frame: Option<Arc<RenderImage>>) -> AnyElement {
    match frame {
        Some(image) => img(ImageSource::Render(image))
            .size_full()
            .object_fit(ObjectFit::Contain)
            .into_any_element(),
        None => div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(gpui_component::spinner::Spinner::new())
            .into_any_element(),
    }
}

pub fn frame_from_jpeg(bytes: &[u8]) -> Option<Arc<RenderImage>> {
    let decoded = image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg).ok()?;
    let rgba = decoded.to_rgba8();
    let (width, height) = rgba.dimensions();
    Some(frame_from_rgba(width, height, rgba.into_raw()))
}

pub fn prefetched_frame(key: &FrameKey) -> Option<Arc<RenderImage>> {
    let bytes = frame_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(key)?;
    frame_from_jpeg(&bytes)
}

fn prefetch_target_for(shared: &Shared, editor: &EditorState) -> PrefetchTarget {
    let side = if editor.diff_active {
        editor.diff_side
    } else {
        DiffSide::B
    };
    let (scenario, tasks, generation, path) = {
        let model = shared.lock().unwrap_or_else(|e| e.into_inner());
        (
            Some(model.scenario.clone()),
            Some(model.tasks.clone()),
            model.generation,
            model.path.clone(),
        )
    };
    PrefetchTarget {
        current: editor.current,
        playing: editor.playing,
        generation,
        side,
        scenario,
        tasks,
        path,
    }
}

pub fn publish_prefetch_target(shared: &Shared, editor: &EditorState) {
    ensure_prefetcher();
    publish_target(prefetch_target_for(shared, editor));
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCENARIO: &str = r##"{ "video": { "width": 8, "height": 4, "background": "#101418" }, "scenes": [ { "duration": 1.0 } ] }"##;

    #[test]
    fn frame_from_rgba_swaps_red_and_blue_and_forces_opaque() {
        let rgba = vec![10u8, 20, 30, 0, 200, 210, 220, 128];
        let image = frame_from_rgba(2, 1, rgba);
        let bytes = image.as_bytes(0).expect("first frame present");
        assert_eq!(bytes, &[30, 20, 10, 255, 220, 210, 200, 255]);
    }

    #[test]
    fn frame_from_rgba_reports_the_given_dimensions() {
        let rgba = vec![0u8; (4 * 3 * 4) as usize];
        let image = frame_from_rgba(4, 3, rgba);
        let size = image.size(0);
        assert_eq!((size.width.0, size.height.0), (4, 3));
    }

    #[test]
    fn render_current_frame_matches_the_scenario_dimensions() {
        let scenario = rustmotion::loader::load_scenario_from_source(None, Some(SCENARIO)).unwrap();
        let tasks = rustmotion::encode::build_frame_tasks(&scenario);
        let image = render_current_frame(&scenario, &tasks, 0, 1.0).unwrap();
        let size = image.size(0);
        assert_eq!((size.width.0, size.height.0), (8, 4));
    }

    #[test]
    fn render_current_frame_rejects_an_empty_task_list() {
        let scenario = rustmotion::loader::load_scenario_from_source(None, Some(SCENARIO)).unwrap();
        assert!(render_current_frame(&scenario, &[], 0, 1.0).is_err());
    }

    fn editor_state(
        current: u32,
        playing: bool,
        diff_active: bool,
        diff_side: DiffSide,
    ) -> EditorState {
        EditorState {
            current,
            playing,
            muted: false,
            rev: 0,
            selected: None,
            show_annotations: false,
            show_hits: true,
            diff_active,
            diff_side,
            preview_scale: 100,
            frame: None,
        }
    }

    fn shared_model() -> Shared {
        use crate::scenario::StudioModel;
        use std::sync::{Arc as StdArc, Mutex};
        let scenario = rustmotion::loader::load_scenario_from_source(None, Some(SCENARIO)).unwrap();
        StdArc::new(Mutex::new(StudioModel::new(scenario, None, None)))
    }

    #[test]
    fn prefetch_target_carries_editor_state_and_model_snapshot() {
        let shared = shared_model();
        let target = prefetch_target_for(&shared, &editor_state(3, true, true, DiffSide::A));

        assert_eq!(target.current, 3);
        assert!(target.playing);
        assert_eq!(target.side, DiffSide::A);
        assert!(
            target.scenario.is_some() && target.tasks.is_some(),
            "workers render from these snapshots and must never take the model lock"
        );
    }

    #[test]
    fn prefetch_target_forces_side_b_when_diff_is_off() {
        let shared = shared_model();
        let target = prefetch_target_for(&shared, &editor_state(0, false, false, DiffSide::A));

        assert_eq!(
            target.side,
            DiffSide::B,
            "a stale diff_side must not survive leaving diff mode"
        );
    }
}
