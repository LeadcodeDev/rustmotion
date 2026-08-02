use rayon::prelude::*;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::engine::prefetch_icons;
use crate::error::{Result, RustmotionError};
use crate::schema::ResolvedScenario as Scenario;

use super::tasks::{build_frame_tasks, render_frame_task};
use super::EncodeProgress;

/// Encode frames as a PNG sequence (one PNG file per frame)
pub fn encode_png_sequence(
    scenario: &Scenario,
    output_dir: &str,
    _quiet: bool,
    _transparent: bool,
    mut on_progress: Option<&mut dyn FnMut(EncodeProgress)>,
) -> Result<()> {
    let config = &scenario.video;
    let width = config.width;
    let height = config.height;

    for view in &scenario.views {
        prefetch_icons(&view.scenes);
    }

    let tasks = build_frame_tasks(scenario);
    let total_frames = tasks.len() as u32;

    if total_frames == 0 {
        return Err(RustmotionError::NoFrames);
    }

    std::fs::create_dir_all(output_dir)?;

    let batch_size = (rayon::current_num_threads() * 2).max(4);
    // Progress-only counter: rayon's scheduling order does not match task
    // position, so this must never be used to derive the output frame index
    // (that caused a frame permutation bug, see issue #129). The frame index
    // is instead derived from each task's position within `tasks`, via the
    // running `batch_base` offset combined with the in-batch enumerate index.
    let progress_counter = AtomicU32::new(0);
    let mut batch_base: u32 = 0;

    for batch in tasks.chunks(batch_size) {
        let results: Vec<Result<(u32, Vec<u8>)>> = batch
            .par_iter()
            .enumerate()
            .map(|(local_idx, task)| {
                let frame_num = batch_base + local_idx as u32;
                let rgba = render_frame_task(config, scenario, task)?;
                progress_counter.fetch_add(1, Ordering::Relaxed);
                Ok((frame_num, rgba))
            })
            .collect();

        batch_base += batch.len() as u32;

        if let Some(ref mut cb) = on_progress {
            cb(EncodeProgress::Rendering(
                progress_counter.load(Ordering::Relaxed),
                total_frames,
            ));
        }

        for result in results {
            let (frame_num, rgba) = result?;
            let path = format!("{}/frame_{:05}.png", output_dir, frame_num);
            let img = image::RgbaImage::from_raw(width, height, rgba)
                .ok_or(RustmotionError::PixelImage)?;
            img.save(&path)?;
        }
    }

    Ok(())
}

/// Encode frames as an animated GIF
pub fn encode_gif(
    scenario: &Scenario,
    output_path: &str,
    _quiet: bool,
    mut on_progress: Option<&mut dyn FnMut(EncodeProgress)>,
) -> Result<()> {
    let config = &scenario.video;
    let width = config.width;
    let height = config.height;
    let fps = config.fps;

    for view in &scenario.views {
        prefetch_icons(&view.scenes);
    }

    let tasks = build_frame_tasks(scenario);
    let total_frames = tasks.len() as u32;

    if total_frames == 0 {
        return Err(RustmotionError::NoFrames);
    }

    let gif_w = width.min(65535) as u16;
    let gif_h = height.min(65535) as u16;

    let file = File::create(output_path)?;
    let mut encoder = gif::Encoder::new(BufWriter::new(file), gif_w, gif_h, &[]).map_err(|e| {
        RustmotionError::GifEncoder {
            reason: e.to_string(),
        }
    })?;

    encoder
        .set_repeat(gif::Repeat::Infinite)
        .map_err(|e| RustmotionError::GifRepeat {
            reason: e.to_string(),
        })?;

    let delay = (100.0 / fps as f64).round() as u16;

    let batch_size = (rayon::current_num_threads() * 2).max(4);
    let counter = AtomicU32::new(0);

    for batch in tasks.chunks(batch_size) {
        let results: Vec<Result<Vec<u8>>> = batch
            .par_iter()
            .map(|task| {
                let rgba = render_frame_task(config, scenario, task)?;
                counter.fetch_add(1, Ordering::Relaxed);
                Ok(rgba)
            })
            .collect();

        if let Some(ref mut cb) = on_progress {
            cb(EncodeProgress::Rendering(
                counter.load(Ordering::Relaxed),
                total_frames,
            ));
        }

        for result in results {
            let rgba = result?;
            let mut frame = gif::Frame::from_rgba_speed(gif_w, gif_h, &mut rgba.clone(), 10);
            frame.delay = delay;
            encoder
                .write_frame(&frame)
                .map_err(|e| RustmotionError::GifFrame {
                    reason: e.to_string(),
                })?;
        }
    }

    Ok(())
}

/// Stream raw RGBA pixel data to stdout for piping to external tools
pub fn encode_raw_stdout(scenario: &Scenario, quiet: bool) -> Result<()> {
    let config = &scenario.video;
    let fps = config.fps;

    let mut stdout = std::io::stdout().lock();

    let mut frame_offset = 0u32;
    for view in &scenario.views {
        for scene in &view.scenes {
            let scene_frames = (scene.duration * fps as f64).round() as u32;

            for local_frame in 0..scene_frames {
                let rgba = crate::engine::render::render_scene_frame(
                    config,
                    scene,
                    local_frame,
                    scene_frames,
                )?;
                stdout.write_all(&rgba)?;

                if !quiet {
                    let global_frame = frame_offset + local_frame;
                    eprint!("\rFrame {}", global_frame);
                }
            }
            frame_offset += scene_frames;
        }
    }

    if !quiet {
        eprintln!("\nDone: {} frames streamed to stdout", frame_offset);
    }

    Ok(())
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::load_scenario_from_source;
    use std::path::{Path, PathBuf};

    /// Deterministic color for a given frame index: distinct enough across
    /// nearby indices that a swapped/misplaced frame is detected by a plain
    /// pixel comparison.
    fn expected_color(i: usize) -> (u8, u8, u8) {
        (
            ((i * 47) % 256) as u8,
            ((i * 91) % 256) as u8,
            ((i * 131) % 256) as u8,
        )
    }

    /// Scenario made of `n` one-frame scenes, each a solid-color full-canvas
    /// rect uniquely colored by its scene index (see `expected_color`).
    fn build_scenario(n: usize) -> Scenario {
        let mut scenes = Vec::with_capacity(n);
        for i in 0..n {
            let (r, g, b) = expected_color(i);
            scenes.push(format!(
                r##"{{"duration": 0.1, "children": [
                    {{"type": "shape", "shape": "rect", "fill": "#{:02x}{:02x}{:02x}",
                      "position": "absolute", "x": 0, "y": 0,
                      "style": {{"width": 8, "height": 8}}}}
                ]}}"##,
                r, g, b
            ));
        }
        let json = format!(
            r#"{{"video": {{"width": 8, "height": 8, "fps": 10}}, "scenes": [{}]}}"#,
            scenes.join(",")
        );
        load_scenario_from_source(None, Some(&json)).expect("load test scenario")
    }

    fn read_dir_sorted(dir: &Path) -> Vec<PathBuf> {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
            .expect("read output dir")
            .map(|e| e.expect("dir entry").path())
            .collect();
        entries.sort();
        entries
    }

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rustmotion_png_seq_test_{}_{}_{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// Regression test for issue #129: the frame index must be derived from
    /// the task's position in `tasks`, not from completion order under
    /// rayon's work-stealing scheduler. Every frame is rendered with a color
    /// unique to its index; the saved `frame_NNNNN.png` must contain exactly
    /// the color expected for index N.
    #[test]
    fn png_sequence_frames_are_not_permuted() {
        let n = 200;
        let scenario = build_scenario(n);

        let out_dir = scratch_dir("order");
        encode_png_sequence(&scenario, out_dir.to_str().unwrap(), true, false, None)
            .expect("encode png sequence");

        let files = read_dir_sorted(&out_dir);
        assert_eq!(files.len(), n, "expected one PNG per frame");

        for (i, path) in files.iter().enumerate() {
            let expected_name = format!("frame_{:05}.png", i);
            assert_eq!(
                path.file_name().unwrap().to_str().unwrap(),
                expected_name,
                "frame files must be contiguously numbered"
            );

            let img = image::open(path)
                .unwrap_or_else(|e| panic!("decode {}: {e}", path.display()))
                .to_rgba8();
            let pixel = img.get_pixel(0, 0);
            let (er, eg, eb) = expected_color(i);
            assert_eq!(
                (pixel[0], pixel[1], pixel[2]),
                (er, eg, eb),
                "frame {} has the wrong content: frame index does not match task position",
                i
            );
        }

        let _ = std::fs::remove_dir_all(&out_dir);
    }

    /// Two multithreaded runs of the same scenario must be byte-identical,
    /// and must match a run pinned to a single thread. Before the fix, the
    /// shared-atomic frame index made this fail under rayon's work-stealing
    /// scheduler (see issue #129).
    #[test]
    fn png_sequence_is_deterministic_across_thread_counts() {
        let n = 200;
        let scenario = build_scenario(n);

        let dir_mt1 = scratch_dir("mt1");
        let dir_mt2 = scratch_dir("mt2");
        let dir_st = scratch_dir("st1");

        encode_png_sequence(&scenario, dir_mt1.to_str().unwrap(), true, false, None)
            .expect("mt run 1");
        encode_png_sequence(&scenario, dir_mt2.to_str().unwrap(), true, false, None)
            .expect("mt run 2");

        let single_threaded_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .expect("build single-threaded pool");
        single_threaded_pool.install(|| {
            encode_png_sequence(&scenario, dir_st.to_str().unwrap(), true, false, None)
                .expect("st run")
        });

        let files_mt1 = read_dir_sorted(&dir_mt1);
        let files_mt2 = read_dir_sorted(&dir_mt2);
        let files_st = read_dir_sorted(&dir_st);
        assert_eq!(files_mt1.len(), n);
        assert_eq!(files_mt2.len(), n);
        assert_eq!(files_st.len(), n);

        for i in 0..n {
            let a = std::fs::read(&files_mt1[i]).unwrap();
            let b = std::fs::read(&files_mt2[i]).unwrap();
            let c = std::fs::read(&files_st[i]).unwrap();
            assert_eq!(a, b, "frame {} differs between two multithreaded runs", i);
            assert_eq!(
                a, c,
                "frame {} differs between multithreaded and single-threaded runs",
                i
            );
        }

        let _ = std::fs::remove_dir_all(&dir_mt1);
        let _ = std::fs::remove_dir_all(&dir_mt2);
        let _ = std::fs::remove_dir_all(&dir_st);
    }
}
