use rayon::prelude::*;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::engine::prefetch_icons;
use crate::error::{Result, RustmotionError};
use crate::schema::ResolvedScenario as Scenario;

use super::tasks::{build_frame_tasks, render_frame_task};
use super::EncodeProgress;

fn partial_sibling_dir(output_dir: &str) -> std::path::PathBuf {
    let path = std::path::Path::new(output_dir);
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    path.with_file_name(format!("{name}.partial"))
}

pub fn encode_png_sequence(
    scenario: &Scenario,
    output_dir: &str,
    quiet: bool,
    transparent: bool,
    on_progress: Option<&mut dyn FnMut(EncodeProgress)>,
) -> Result<()> {
    let partial_dir = partial_sibling_dir(output_dir);
    let _ = std::fs::remove_dir_all(&partial_dir);
    match encode_png_sequence_to_dir(scenario, &partial_dir, quiet, transparent, on_progress) {
        Ok(()) => {
            let _ = std::fs::remove_dir_all(output_dir);
            std::fs::rename(&partial_dir, output_dir)?;
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&partial_dir);
            Err(e)
        }
    }
}

fn encode_png_sequence_to_dir(
    scenario: &Scenario,
    output_dir: &std::path::Path,
    _quiet: bool,
    _transparent: bool,
    mut on_progress: Option<&mut dyn FnMut(EncodeProgress)>,
) -> Result<()> {
    let config = &scenario.video;
    let width = config.width;
    let height = config.height;

    for view in &scenario.views {
        prefetch_icons(&view.scenes)?;
    }

    let tasks = build_frame_tasks(scenario);
    let total_frames = tasks.len() as u32;

    if total_frames == 0 {
        return Err(RustmotionError::NoFrames);
    }

    std::fs::create_dir_all(output_dir)?;

    let batch_size = (rayon::current_num_threads() * 2).max(4);
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
            let path = output_dir.join(format!("frame_{:05}.png", frame_num));
            let img = image::RgbaImage::from_raw(width, height, rgba)
                .ok_or(RustmotionError::PixelImage)?;
            img.save(&path)?;
        }
    }

    Ok(())
}

fn partial_sibling_path(output_path: &str) -> std::path::PathBuf {
    let path = std::path::Path::new(output_path);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let name = match path.extension().and_then(|s| s.to_str()) {
        Some(ext) => format!("{stem}.partial.{ext}"),
        None => format!("{stem}.partial"),
    };
    path.with_file_name(name)
}

pub fn encode_gif(
    scenario: &Scenario,
    output_path: &str,
    quiet: bool,
    on_progress: Option<&mut dyn FnMut(EncodeProgress)>,
) -> Result<()> {
    let partial_path = partial_sibling_path(output_path);
    match encode_gif_to_path(scenario, &partial_path, quiet, on_progress) {
        Ok(()) => {
            std::fs::rename(&partial_path, output_path)?;
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&partial_path);
            Err(e)
        }
    }
}

fn encode_gif_to_path(
    scenario: &Scenario,
    partial_path: &std::path::Path,
    quiet: bool,
    mut on_progress: Option<&mut dyn FnMut(EncodeProgress)>,
) -> Result<()> {
    let config = &scenario.video;
    let width = config.width;
    let height = config.height;
    let fps = config.fps;

    for view in &scenario.views {
        prefetch_icons(&view.scenes)?;
    }

    let tasks = build_frame_tasks(scenario);
    let total_frames = tasks.len() as u32;

    if total_frames == 0 {
        return Err(RustmotionError::NoFrames);
    }

    let gif_w = width.min(65535) as u16;
    let gif_h = height.min(65535) as u16;

    let file = File::create(partial_path)?;
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

    if !quiet && fps > 50 {
        eprintln!(
            "rustmotion: GIF frame delay has a 1/100s resolution — {} fps cannot be represented \
             exactly; playback speed will be approximate.",
            fps
        );
    }

    let batch_size = (rayon::current_num_threads() * 2).max(4);
    let counter = AtomicU32::new(0);
    let mut frame_idx: u32 = 0;

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
            frame.delay = gif_frame_delay_cs(frame_idx, fps);
            frame_idx += 1;
            encoder
                .write_frame(&frame)
                .map_err(|e| RustmotionError::GifFrame {
                    reason: e.to_string(),
                })?;
        }
    }

    Ok(())
}

fn gif_frame_delay_cs(frame_index: u32, fps: u32) -> u16 {
    let cs_at = |i: u32| -> i64 { (100.0 * i as f64 / fps as f64).round() as i64 };
    let delay = cs_at(frame_index + 1) - cs_at(frame_index);
    delay.clamp(2, u16::MAX as i64) as u16
}

pub fn encode_raw_stdout(scenario: &Scenario, quiet: bool) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    encode_raw_frames(scenario, quiet, &mut stdout)
}

fn encode_raw_frames(scenario: &Scenario, quiet: bool, writer: &mut dyn Write) -> Result<()> {
    let config = &scenario.video;

    for view in &scenario.views {
        prefetch_icons(&view.scenes)?;
    }

    let tasks = build_frame_tasks(scenario);
    let total_frames = tasks.len() as u32;

    if total_frames == 0 {
        return Err(RustmotionError::NoFrames);
    }

    for (idx, task) in tasks.iter().enumerate() {
        let rgba = render_frame_task(config, scenario, task)?;
        writer.write_all(&rgba)?;

        if !quiet {
            eprint!("\rFrame {}", idx);
        }
    }

    if !quiet {
        eprintln!("\nDone: {} frames streamed to stdout", total_frames);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::load_scenario_from_source;
    use std::path::{Path, PathBuf};

    #[test]
    fn partial_sibling_path_keeps_the_extension_last() {
        assert_eq!(
            partial_sibling_path("/tmp/out.gif"),
            PathBuf::from("/tmp/out.partial.gif")
        );
        assert_eq!(
            partial_sibling_path("/tmp/out"),
            PathBuf::from("/tmp/out.partial")
        );
    }

    #[test]
    fn partial_sibling_dir_stays_next_to_the_final_directory() {
        let got = partial_sibling_dir("/a/b/frames");
        assert_eq!(got, PathBuf::from("/a/b/frames.partial"));
        assert_eq!(got.parent(), Some(Path::new("/a/b")));
    }

    #[test]
    fn encode_gif_leaves_no_partial_file_behind_on_an_empty_scenario() {
        let json = r#"{"video": {"width": 8, "height": 8, "fps": 10}, "scenes": []}"#;
        let scenario = load_scenario_from_source(None, Some(json)).expect("load");

        let out = std::env::temp_dir().join(format!(
            "rm_gif_no_debris_test_{}_{}.gif",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let partial = partial_sibling_path(out.to_str().unwrap());
        let _ = std::fs::remove_file(&out);
        let _ = std::fs::remove_file(&partial);

        let result = encode_gif(&scenario, out.to_str().unwrap(), true, None);
        assert!(result.is_err(), "an empty scenario has no frames to encode");
        assert!(!out.exists(), "no debris at the final output path");
        assert!(!partial.exists(), "no debris at the scratch path either");
    }

    #[test]
    fn encode_png_sequence_leaves_no_partial_dir_behind_on_an_empty_scenario() {
        let json = r#"{"video": {"width": 8, "height": 8, "fps": 10}, "scenes": []}"#;
        let scenario = load_scenario_from_source(None, Some(json)).expect("load");

        let out_dir = std::env::temp_dir().join(format!(
            "rm_png_seq_no_debris_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let partial_dir = partial_sibling_dir(out_dir.to_str().unwrap());
        let _ = std::fs::remove_dir_all(&out_dir);
        let _ = std::fs::remove_dir_all(&partial_dir);

        let result = encode_png_sequence(&scenario, out_dir.to_str().unwrap(), true, false, None);
        assert!(result.is_err(), "an empty scenario has no frames to encode");
        assert!(!out_dir.exists(), "no debris at the final output directory");
        assert!(
            !partial_dir.exists(),
            "no debris at the scratch directory either"
        );
    }

    fn expected_color(i: usize) -> (u8, u8, u8) {
        (
            ((i * 47) % 256) as u8,
            ((i * 91) % 256) as u8,
            ((i * 131) % 256) as u8,
        )
    }

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

    fn transition_scenario_json(width: u32, height: u32, fps: u32) -> String {
        format!(
            r##"{{"video": {{"width": {width}, "height": {height}, "fps": {fps}}},
            "scenes": [
                {{"duration": 2.0, "children": [
                    {{"type": "shape", "shape": "rect", "fill": "#ff0000",
                      "position": "absolute", "x": 0, "y": 0,
                      "style": {{"width": {width}, "height": {height}}}}}
                ]}},
                {{"duration": 2.0, "transition": {{"type": "fade", "duration": 1.0}}, "children": [
                    {{"type": "shape", "shape": "rect", "fill": "#0000ff",
                      "position": "absolute", "x": 0, "y": 0,
                      "style": {{"width": {width}, "height": {height}}}}}
                ]}}
            ]}}"##
        )
    }

    #[test]
    fn raw_frame_count_matches_build_frame_tasks_across_a_transition() {
        let fps = 30u32;
        let json = transition_scenario_json(8, 8, fps);
        let scenario = load_scenario_from_source(None, Some(&json)).expect("load");

        let mut buf: Vec<u8> = Vec::new();
        encode_raw_frames(&scenario, true, &mut buf).expect("raw encode");

        let bytes_per_frame = 8usize * 8 * 4;
        assert_eq!(
            buf.len() % bytes_per_frame,
            0,
            "raw output must be a whole number of frames"
        );
        let raw_frame_count = buf.len() / bytes_per_frame;

        let expected_frame_count = build_frame_tasks(&scenario).len();
        assert_eq!(
            raw_frame_count, expected_frame_count,
            "raw format must produce the same frame count as build_frame_tasks (matches \
             png-seq/mp4), not a naive per-scene sum that ignores transition overlap"
        );
        assert_eq!(
            expected_frame_count,
            (3.0 * fps as f64).round() as usize,
            "a 2s+2s scenario with a 1s fade must occupy 3.0s of output frames"
        );
    }

    #[test]
    fn gif_delay_sums_to_the_exact_target_duration_for_low_fps() {
        for fps in [20u32, 24, 25, 30, 40, 50] {
            let duration_s = 2.0_f64;
            let frame_count = (duration_s * fps as f64).round() as u32;
            let sum_cs: i64 = (0..frame_count)
                .map(|i| gif_frame_delay_cs(i, fps) as i64)
                .sum();
            let expected_cs = (duration_s * 100.0).round() as i64;
            assert_eq!(
                sum_cs, expected_cs,
                "fps={fps}: summed delays must equal the target duration exactly"
            );
        }
    }

    #[test]
    fn gif_delay_never_drops_below_the_two_centisecond_floor() {
        for fps in [60u32, 120, 240, 1000] {
            for i in 0..20 {
                let delay = gif_frame_delay_cs(i, fps);
                assert!(
                    delay >= 2,
                    "fps={fps} frame={i}: delay {delay}cs is below the floor"
                );
            }
        }
    }

    #[test]
    fn gif_total_playback_duration_matches_the_scenario_duration() {
        let fps = 30u32;
        let json = format!(
            r#"{{"video": {{"width": 8, "height": 8, "fps": {fps}}},
                 "scenes": [{{"duration": 2.0, "children": []}}]}}"#
        );
        let scenario = load_scenario_from_source(None, Some(&json)).expect("load");

        let out = std::env::temp_dir().join(format!(
            "rm_gif_duration_test_{}_{}.gif",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&out);
        encode_gif(&scenario, out.to_str().unwrap(), true, None).expect("encode gif");

        let file = std::fs::File::open(&out).expect("open gif");
        let mut decoder = gif::DecodeOptions::new()
            .read_info(file)
            .expect("read gif info");
        let mut total_cs: u32 = 0;
        let mut frame_count = 0u32;
        while let Some(frame) = decoder.read_next_frame().expect("read frame") {
            total_cs += frame.delay as u32;
            frame_count += 1;
        }

        assert_eq!(
            frame_count, 60,
            "expected 60 frames at 30fps for a 2.0s scene"
        );
        let total_secs = total_cs as f64 / 100.0;
        assert!(
            (total_secs - 2.0).abs() < 0.02,
            "gif total playback duration must match the scenario's 2.000s, got {total_secs:.3}s"
        );

        let _ = std::fs::remove_file(&out);
    }
}

fn containers_for(codec: &str) -> Option<&'static [&'static str]> {
    match codec {
        "h264" => Some(&["mp4", "mov", "mkv"]),
        "h265" | "hevc" => Some(&["mp4", "mov", "mkv"]),
        "vp9" => Some(&["webm", "mkv"]),
        "prores" => Some(&["mov", "mkv"]),
        _ => None,
    }
}

pub fn check_codec_container(codec: &str, container: &str) -> Result<()> {
    if matches!(container, "gif" | "png-seq" | "raw") {
        return Ok(());
    }
    let Some(allowed) = containers_for(codec) else {
        return Ok(());
    };
    if allowed.contains(&container) {
        return Ok(());
    }
    let fix = format!(
        "use -o <file>.{}{}",
        allowed[0],
        if codec == "prores" {
            ", or drop --codec for H.264"
        } else {
            ""
        }
    );
    Err(RustmotionError::CodecContainerMismatch {
        codec: codec.to_string(),
        container: container.to_string(),
        fix,
    })
}

#[cfg(test)]
mod codec_container_tests {
    use super::*;

    #[test]
    fn prores_into_mp4_is_refused_with_the_fix_named() {
        let err = check_codec_container("prores", "mp4").expect_err("prores/mp4 must be refused");
        let msg = err.to_string();
        assert!(msg.contains("prores"), "{msg}");
        assert!(
            msg.contains(".mov"),
            "the message must name what works: {msg}"
        );
    }

    #[test]
    fn documented_pairs_are_accepted() {
        for (codec, container) in [
            ("h264", "mp4"),
            ("h264", "mov"),
            ("h265", "mp4"),
            ("prores", "mov"),
            ("vp9", "webm"),
        ] {
            check_codec_container(codec, container)
                .unwrap_or_else(|e| panic!("{codec}/{container} must be accepted: {e}"));
        }
    }

    #[test]
    fn vp9_into_mp4_is_refused() {
        assert!(check_codec_container("vp9", "mp4").is_err());
    }

    #[test]
    fn unknown_codecs_pass_through() {
        check_codec_container("av1", "mp4").expect("unknown codec must not be second-guessed");
    }

    #[test]
    fn png_is_not_a_container_any_codec_claims() {
        assert!(
            check_codec_container("h264", "png").is_err(),
            "a still is not an h264 container — the caller must not ask"
        );
    }

    #[test]
    fn own_path_containers_are_left_alone() {
        for container in ["gif", "png-seq", "raw"] {
            check_codec_container("prores", container).expect("own-path container");
        }
    }
}
