use crate::cli::OutputFormat;
use rustmotion::encode;
use rustmotion::engine;
use rustmotion::engine::renderer::subpixel_font;
use rustmotion::error::{Result, RustmotionError};
use rustmotion::schema::ResolvedScenario;
use skia_safe::{
    images, surfaces, AlphaType, Canvas, Color4f, ColorType, Data, Font, FontStyle, ImageInfo,
    Paint, PaintStyle, RRect, Rect, TextBlob,
};
use std::path::{Path, PathBuf};

fn temp_sibling_path(output: &Path) -> PathBuf {
    let ext = output.extension().and_then(|e| e.to_str());
    let stem = output
        .file_stem()
        .and_then(|e| e.to_str())
        .unwrap_or("sheet");
    let name = match ext {
        Some(ext) => format!(".{stem}.rustmotion-tmp.{ext}"),
        None => format!(".{stem}.rustmotion-tmp"),
    };
    output.with_file_name(name)
}

fn parse_at_list(spec: &str) -> Result<Vec<f64>> {
    let mut times = Vec::new();
    for (i, raw) in spec.split(',').enumerate() {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(RustmotionError::Generic(format!(
                "sheet: --at token {} is empty — expected a comma-separated list of seconds, \
                 e.g. \"3.2,4.8,19.9\"",
                i + 1
            )));
        }
        let t: f64 = trimmed.parse().map_err(|_| {
            RustmotionError::Generic(format!(
                "sheet: --at token {} ('{}') is not a valid number of seconds",
                i + 1,
                trimmed
            ))
        })?;
        times.push(t);
    }
    Ok(times)
}

fn generate_every(step: f64, last_valid_time: f64) -> Result<Vec<f64>> {
    if !step.is_finite() || step <= 0.0 {
        return Err(RustmotionError::Generic(format!(
            "sheet: --every must be a positive number of seconds, got {step}"
        )));
    }
    const MAX_CELLS: usize = 10_000;
    let mut times = Vec::new();
    let mut i: u64 = 0;
    loop {
        let t = i as f64 * step;
        if t > last_valid_time + 1e-9 {
            break;
        }
        times.push(t);
        i += 1;
        if times.len() > MAX_CELLS {
            return Err(RustmotionError::Generic(format!(
                "sheet: --every {step}s would generate more than {MAX_CELLS} cells for a \
                 {last_valid_time:.2}s scenario — use a larger step"
            )));
        }
    }
    if times.is_empty() {
        times.push(0.0);
    }
    Ok(times)
}

struct GridLayout {
    columns: usize,
    rows: usize,
    cell_width: u32,
    cell_height: u32,
    gap: u32,
    margin: u32,
    total_width: u32,
    total_height: u32,
}

fn compute_layout(
    count: usize,
    columns: usize,
    cell_width: u32,
    video_width: u32,
    video_height: u32,
) -> GridLayout {
    let rows = count.div_ceil(columns);
    let cell_height = ((cell_width as f64 * video_height as f64) / video_width as f64)
        .round()
        .max(1.0) as u32;
    let gap: u32 = 10;
    let margin: u32 = 16;
    let total_width = margin * 2 + columns as u32 * cell_width + gap * (columns as u32 - 1);
    let total_height = margin * 2 + rows as u32 * cell_height + gap * (rows as u32 - 1);
    GridLayout {
        columns,
        rows,
        cell_width,
        cell_height,
        gap,
        margin,
        total_width,
        total_height,
    }
}

fn label_font(cell_width: u32) -> Result<Font> {
    let typeface = engine::typeface_with_fallback("", FontStyle::bold())?;
    let size = (cell_width as f32 * 0.06).clamp(14.0, 26.0);
    Ok(subpixel_font(typeface, size))
}

fn draw_timestamp_stamp(
    canvas: &Canvas,
    font: &Font,
    t: f64,
    cell_x: f32,
    cell_y: f32,
    cell_h: f32,
) {
    let label = format!("{t:.2}s");
    let (text_w, _) = font.measure_str(&label, None);
    let (_, metrics) = font.metrics();
    let ascent = -metrics.ascent;
    let descent = metrics.descent;
    let text_h = ascent + descent;

    let pad_x = 8.0f32;
    let pad_y = 5.0f32;
    let badge_w = text_w + pad_x * 2.0;
    let badge_h = text_h + pad_y * 2.0;
    let badge_x = cell_x + 6.0;
    let badge_y = cell_y + cell_h - badge_h - 6.0;

    let badge_rect = Rect::from_xywh(badge_x, badge_y, badge_w, badge_h);
    let rrect = RRect::new_rect_xy(badge_rect, 4.0, 4.0);
    let mut bg_paint = Paint::new(Color4f::new(0.0, 0.0, 0.0, 0.62), None);
    bg_paint.set_anti_alias(true);
    canvas.draw_rrect(rrect, &bg_paint);

    let mut text_paint = Paint::new(Color4f::new(1.0, 1.0, 1.0, 1.0), None);
    text_paint.set_anti_alias(true);
    let baseline_y = badge_y + pad_y + ascent;
    if let Some(blob) = TextBlob::new(&label, font) {
        canvas.draw_text_blob(&blob, (badge_x + pad_x, baseline_y), &text_paint);
    }
}

fn compose_grid(cells: &[(f64, image::RgbaImage)], layout: &GridLayout) -> Result<Vec<u8>> {
    let info = ImageInfo::new(
        (layout.total_width as i32, layout.total_height as i32),
        ColorType::RGBA8888,
        AlphaType::Unpremul,
        None,
    );
    let mut surface =
        surfaces::raster(&info, None, None).ok_or(RustmotionError::SurfaceCreation)?;
    let canvas = surface.canvas();
    canvas.clear(Color4f::new(0.08, 0.08, 0.08, 1.0));

    let font = label_font(layout.cell_width)?;
    let mut border_paint = Paint::default();
    border_paint.set_anti_alias(true);
    border_paint.set_style(PaintStyle::Stroke);
    border_paint.set_stroke_width(1.5);
    border_paint.set_color4f(Color4f::new(1.0, 1.0, 1.0, 0.15), None);

    for (idx, (t, img)) in cells.iter().enumerate() {
        let col = idx % layout.columns;
        let row = idx / layout.columns;
        let x = layout.margin as f32 + col as f32 * (layout.cell_width + layout.gap) as f32;
        let y = layout.margin as f32 + row as f32 * (layout.cell_height + layout.gap) as f32;

        let src_info = ImageInfo::new(
            (img.width() as i32, img.height() as i32),
            ColorType::RGBA8888,
            AlphaType::Unpremul,
            None,
        );
        let data = Data::new_copy(img.as_raw());
        let sk_img = images::raster_from_data(&src_info, data, img.width() as usize * 4)
            .ok_or(RustmotionError::PixelImage)?;

        let dst = Rect::from_xywh(x, y, layout.cell_width as f32, layout.cell_height as f32);
        canvas.draw_image_rect(&sk_img, None, dst, &Paint::default());
        canvas.draw_rect(dst, &border_paint);

        draw_timestamp_stamp(canvas, &font, *t, x, y, layout.cell_height as f32);
    }

    let row_bytes = layout.total_width as usize * 4;
    let mut pixels = vec![0u8; row_bytes * layout.total_height as usize];
    surface.read_pixels(&info, &mut pixels, row_bytes, (0, 0));
    Ok(pixels)
}

#[allow(clippy::too_many_arguments)]
pub fn cmd_sheet(
    scenario: ResolvedScenario,
    output: &Path,
    at: Option<&str>,
    every: Option<f64>,
    columns: usize,
    cell_width: u32,
    output_format: Option<&OutputFormat>,
    quiet: bool,
) -> Result<()> {
    if columns == 0 {
        return Err(RustmotionError::Generic(
            "sheet: --columns must be at least 1".to_string(),
        ));
    }
    if cell_width == 0 {
        return Err(RustmotionError::Generic(
            "sheet: --cell-width must be at least 1".to_string(),
        ));
    }

    engine::preload::preload_scenario_assets(&scenario)?;

    let start_time = std::time::Instant::now();
    let config = &scenario.video;
    let fps = config.fps;

    let tasks = encode::build_frame_tasks(&scenario);
    let total_frames = tasks.len() as u32;
    if total_frames == 0 {
        return Err(RustmotionError::NoFrames);
    }
    let last_valid_time = (total_frames - 1) as f64 / fps as f64;

    let times = match (at, every) {
        (Some(spec), None) => parse_at_list(spec)?,
        (None, Some(step)) => generate_every(step, last_valid_time)?,
        (Some(_), Some(_)) | (None, None) => {
            return Err(RustmotionError::Generic(
                "sheet: exactly one of --at or --every is required".to_string(),
            ));
        }
    };
    if times.is_empty() {
        return Err(RustmotionError::Generic(
            "sheet: no instants requested".to_string(),
        ));
    }

    let mut frame_indices = Vec::with_capacity(times.len());
    for (i, &t) in times.iter().enumerate() {
        if !t.is_finite() {
            return Err(RustmotionError::Generic(format!(
                "sheet: instant {} of {} is not a finite number of seconds",
                i + 1,
                times.len()
            )));
        }
        if t < 0.0 || t > last_valid_time + 1e-9 {
            return Err(RustmotionError::Generic(format!(
                "sheet: instant {} of {} ({:.3}s) is beyond the scenario — it runs from 0.00s \
                 to {:.3}s ({total_frames} frame(s) at {fps} fps)",
                i + 1,
                times.len(),
                t,
                last_valid_time
            )));
        }
        let raw_index = (t * fps as f64).round() as i64;
        let frame_index = raw_index.clamp(0, total_frames as i64 - 1) as u32;
        frame_indices.push(frame_index);
    }

    let mut cells: Vec<(f64, image::RgbaImage)> = Vec::with_capacity(times.len());
    for (&t, &frame_index) in times.iter().zip(frame_indices.iter()) {
        let task = &tasks[frame_index as usize];
        let rgba = encode::render_frame_task_scaled(config, &scenario, task, 1.0)?;
        let img = image::RgbaImage::from_raw(config.width, config.height, rgba)
            .ok_or(RustmotionError::PixelImage)?;
        cells.push((t, img));
    }

    let layout = compute_layout(
        cells.len(),
        columns,
        cell_width,
        config.width,
        config.height,
    );
    let pixels = compose_grid(&cells, &layout)?;

    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let sheet_img = image::RgbaImage::from_raw(layout.total_width, layout.total_height, pixels)
        .ok_or(RustmotionError::PixelImage)?;

    let tmp_path = temp_sibling_path(output);
    match sheet_img.save(&tmp_path) {
        Ok(()) => {
            std::fs::rename(&tmp_path, output)?;
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(RustmotionError::from(e));
        }
    }

    if !quiet {
        eprintln!(
            "Contact sheet ({} cell(s), {}x{} grid, {}x{}px) saved to {}",
            cells.len(),
            layout.columns,
            layout.rows,
            layout.total_width,
            layout.total_height,
            output.display()
        );
    }

    let elapsed = start_time.elapsed();
    if let Some(OutputFormat::Json) = output_format {
        let result = serde_json::json!({
            "status": "success",
            "output": output.to_string_lossy(),
            "cells": cells.len(),
            "columns": layout.columns,
            "rows": layout.rows,
            "cell_width": layout.cell_width,
            "cell_height": layout.cell_height,
            "width": layout.total_width,
            "height": layout.total_height,
            "times": times,
            "duration_ms": elapsed.as_millis(),
        });
        println!("{}", serde_json::to_string(&result)?);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion::loader::load_scenario_from_source;

    fn scratch_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "rm_sheet_test_{}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            name
        ))
    }

    fn colored_scenario(width: u32, height: u32, fps: u32, duration: f64) -> ResolvedScenario {
        let json = format!(
            r##"{{"video": {{"width": {width}, "height": {height}, "fps": {fps}}},
                 "scenes": [{{"duration": {duration}, "children": [
                    {{"type": "shape", "shape": "rect", "fill": "#3366ff",
                      "position": "absolute", "x": 0, "y": 0,
                      "style": {{"width": {width}, "height": {height}}}}}
                 ]}}]}}"##
        );
        load_scenario_from_source(None, Some(&json)).expect("load")
    }

    #[test]
    fn at_produces_a_grid_with_exactly_that_many_cells() {
        let scenario = colored_scenario(64, 64, 10, 2.0);
        let out = scratch_path("at_grid.png");
        let _ = std::fs::remove_file(&out);

        cmd_sheet(scenario, &out, Some("0.0,0.5,1.0"), None, 2, 64, None, true)
            .expect("sheet must succeed");

        let img = image::open(&out).expect("must decode as a valid image");
        let expected_w = 16 * 2 + 64 * 2 + 10;
        let expected_h = 16 * 2 + 64 * 2 + 10;
        assert_eq!(img.width(), expected_w as u32);
        assert_eq!(img.height(), expected_h as u32);

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn every_samples_from_zero_to_the_scenario_end() {
        let scenario = colored_scenario(32, 32, 10, 1.0);
        let out = scratch_path("every_grid.png");
        let _ = std::fs::remove_file(&out);

        cmd_sheet(scenario, &out, None, Some(0.5), 2, 32, None, true).expect("sheet must succeed");

        let img = image::open(&out).expect("must decode as a valid image");
        let expected_w = 16 * 2 + 32 * 2 + 10;
        let expected_h = 16 * 2 + 32;
        assert_eq!(img.width(), expected_w as u32);
        assert_eq!(img.height(), expected_h as u32);

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn an_at_time_past_the_scenario_end_is_a_located_error_not_a_panic() {
        let scenario = colored_scenario(32, 32, 10, 1.0);
        let out = scratch_path("oob.png");
        let _ = std::fs::remove_file(&out);

        let err = cmd_sheet(scenario, &out, Some("0.1,5.0"), None, 4, 32, None, true)
            .expect_err("a time past the scenario's end must be a named error");

        let msg = err.to_string();
        assert!(
            msg.contains("instant 2 of 2"),
            "error must locate which instant of the list failed: {msg}"
        );
        assert!(
            msg.contains("5.000"),
            "error must name the offending value: {msg}"
        );
        assert!(
            !out.exists(),
            "no partial sheet must be written on an out-of-range instant"
        );
    }

    #[test]
    fn parse_at_list_rejects_a_non_numeric_token() {
        let err = parse_at_list("1.0,not-a-number,2.0").expect_err("must reject");
        assert!(err.to_string().contains("token 2"));
    }

    #[test]
    fn parse_at_list_preserves_order() {
        let times = parse_at_list("3.2,4.8,19.9").expect("must parse");
        assert_eq!(times, vec![3.2, 4.8, 19.9]);
    }

    #[test]
    fn generate_every_rejects_a_non_positive_step() {
        assert!(generate_every(0.0, 5.0).is_err());
        assert!(generate_every(-1.0, 5.0).is_err());
    }

    #[test]
    fn compute_layout_lays_out_left_to_right_top_to_bottom() {
        let layout = compute_layout(5, 2, 100, 100, 100);
        assert_eq!(layout.columns, 2);
        assert_eq!(layout.rows, 3);
    }
}
