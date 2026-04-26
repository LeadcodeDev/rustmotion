use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use skia_safe::{
    surfaces, AlphaType, Color4f, ColorType, Font, FontStyle, ImageInfo, Paint, Path, RRect, Rect,
    TextBlob,
};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{Window, WindowId};

use rustmotion::encode::{encode_video, encode_with_ffmpeg, EncodeProgress};
use rustmotion::engine;
use rustmotion::engine::renderer::font_mgr;

use super::render_worker::{RenderRequest, RenderResponse, ReloadResponse};
use super::ui::*;

use std::sync::mpsc;

pub(super) fn send_notification(title: &str, body: &str) {
    let title = title.to_string();
    let body = body.to_string();
    std::thread::spawn(move || {
        #[cfg(target_os = "macos")]
        let _ = notify_rust::set_application("com.apple.Finder");
        let _ = notify_rust::Notification::new()
            .summary(&title)
            .body(&body)
            .show();
    });
}

pub(super) struct PreviewApp {
    // Video metadata
    pub total_frames: u32,
    pub fps: u32,
    pub video_width: u32,
    pub video_height: u32,
    pub input_path: Option<PathBuf>,

    // Playback
    pub current_frame: u32,
    pub playing: bool,
    pub last_frame_time: Instant,
    pub frame_duration: Duration,

    // Frame cache
    pub frame_cache: HashMap<u32, Vec<u8>>,
    pub rendered_width: u32,
    pub rendered_height: u32,
    pub last_displayed_frame: Option<u32>,

    // Render thread
    pub render_tx: Sender<RenderRequest>,
    pub render_rx: Receiver<RenderResponse>,
    pub reload_rx: Receiver<ReloadResponse>,
    pub pending_frame: Option<u32>,

    // File watch
    pub file_change_rx: Option<Receiver<()>>,

    // Window
    pub window: Option<Arc<Window>>,
    pub surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    pub display_width: u32,
    pub display_height: u32,
    pub scale: f64,

    // Mouse
    pub modifiers: ModifiersState,
    pub timeline_dragging: bool,
    pub cursor_x: f64,
    pub cursor_y: f64,

    // Error overlay
    pub error_message: Option<String>,

    // Export
    pub export_state: ExportState,
    pub export_done_rx: Option<Receiver<std::result::Result<String, String>>>,
    pub export_progress: f32,
    pub export_progress_label: String,
    pub export_progress_rx: Option<Receiver<(f32, String)>>,
}

impl PreviewApp {
    pub fn request_frame(&mut self, frame: u32) {
        if frame >= self.total_frames {
            return;
        }
        if self.frame_cache.contains_key(&frame) {
            if self.pending_frame == Some(frame) {
                self.pending_frame = None;
            }
            if let Some(window) = &self.window {
                window.request_redraw();
            }
            return;
        }
        if self.pending_frame == Some(frame) {
            return;
        }
        self.pending_frame = Some(frame);
        let _ = self.render_tx.send(RenderRequest::Frame(frame));
    }

    pub fn go_to_frame(&mut self, frame: u32) {
        let frame = frame.min(self.total_frames.saturating_sub(1));
        self.current_frame = frame;
        self.request_frame(frame);
        self.update_title();
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn step_frame(&mut self, delta: i32) {
        self.playing = false;
        let new_frame = (self.current_frame as i64 + delta as i64)
            .clamp(0, self.total_frames.saturating_sub(1) as i64) as u32;
        self.go_to_frame(new_frame);
    }

    fn toggle_playback(&mut self) {
        self.playing = !self.playing;
        if self.playing {
            self.last_frame_time = Instant::now();
            if self.current_frame >= self.total_frames.saturating_sub(1) {
                self.current_frame = 0;
            }
            self.request_frame(self.current_frame);
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        self.update_title();
    }

    pub fn update_title(&self) {
        if let Some(window) = &self.window {
            let name = self
                .input_path
                .as_ref()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("preview");
            let time = self.current_frame as f64 / self.fps.max(1) as f64;
            let total_time = self.total_frames as f64 / self.fps.max(1) as f64;
            let icon = if self.playing { "\u{25B6}" } else { "\u{23F8}" };
            window.set_title(&format!(
                "rustmotion \u{2014} {} [{}/{} \u{00B7} {:.1}s/{:.1}s] {}",
                name, self.current_frame, self.total_frames, time, total_time, icon
            ));
        }
    }

    pub fn reload_scenario(&mut self) {
        if let Some(ref path) = self.input_path {
            match rustmotion::loader::load_scenario(path) {
                Ok(scenario) => {
                    self.error_message = None;
                    if !scenario.fonts.is_empty() {
                        engine::renderer::load_custom_fonts(&scenario.fonts);
                    }
                    for view in &scenario.views {
                        engine::prefetch_icons(&view.scenes);
                        engine::preextract_video_frames(&view.scenes, scenario.video.fps);
                    }
                    let _ = self.render_tx.send(RenderRequest::Reload(scenario));
                }
                Err(e) => {
                    eprintln!("Reload error: {}", e);
                    self.error_message = Some(format!("{}", e));
                }
            }
        }
    }

    fn start_export(&mut self) {
        if self.export_state == ExportState::Exporting {
            return;
        }
        let Some(ref input_path) = self.input_path else {
            return;
        };

        let default_name = input_path
            .with_extension("mp4")
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "output.mp4".to_string());

        let default_dir = input_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        let dialog = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .set_directory(&default_dir)
            .add_filter("MP4 Video", &["mp4"]);

        let Some(output_path) = dialog.save_file() else {
            return;
        };

        self.export_state = ExportState::Exporting;
        self.export_progress = 0.0;
        self.export_progress_label = String::new();
        let input = input_path.clone();
        let output_str = output_path.to_string_lossy().to_string();
        let (tx, rx) = mpsc::channel();
        let (progress_tx, progress_rx) = mpsc::channel();
        self.export_done_rx = Some(rx);
        self.export_progress_rx = Some(progress_rx);
        std::thread::spawn(move || {
            let result = (|| -> std::result::Result<String, String> {
                let scenario = rustmotion::loader::load_scenario(&input)
                    .map_err(|e| format!("Load error: {}", e))?;
                if !scenario.fonts.is_empty() {
                    rustmotion::engine::renderer::load_custom_fonts(&scenario.fonts);
                }
                for view in &scenario.views {
                    rustmotion::engine::prefetch_icons(&view.scenes);
                    rustmotion::engine::preextract_video_frames(&view.scenes, scenario.video.fps);
                }
                let progress_sender = progress_tx;
                let mut on_progress = move |p: EncodeProgress| {
                    let (frac, label) = match p {
                        EncodeProgress::Rendering(cur, total) => {
                            (cur as f32 / total as f32, format!("Rendering {}/{}", cur, total))
                        }
                        EncodeProgress::Encoding(cur, total) => {
                            (cur as f32 / total as f32, format!("Encoding {}/{}", cur, total))
                        }
                        EncodeProgress::Muxing => {
                            (1.0, "Muxing…".to_string())
                        }
                    };
                    let _ = progress_sender.send((frac, label));
                };
                let ffmpeg_available = std::process::Command::new("ffmpeg")
                    .arg("-version")
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);

                if ffmpeg_available {
                    let codec = match scenario.video.codec {
                        Some(rustmotion::schema::VideoCodec::H265) => "h265",
                        Some(rustmotion::schema::VideoCodec::Vp9) => "vp9",
                        Some(rustmotion::schema::VideoCodec::Prores) => "prores",
                        _ => "h264",
                    };
                    encode_with_ffmpeg(
                        &scenario,
                        &output_str,
                        true,
                        codec,
                        scenario.video.crf,
                        false,
                        Some(&mut on_progress),
                    )
                    .map_err(|e| format!("Encode error: {}", e))?;
                } else {
                    encode_video(&scenario, &output_str, true, Some(&mut on_progress))
                        .map_err(|e| format!("Encode error: {}", e))?;
                }
                Ok(output_str)
            })();
            let _ = tx.send(result);
        });
    }

    fn make_ui_font(&self, size: f32) -> Font {
        let fm = font_mgr();
        let typeface = fm
            .match_family_style("SF Pro", FontStyle::normal())
            .or_else(|| fm.match_family_style("Helvetica Neue", FontStyle::normal()))
            .or_else(|| fm.match_family_style("Helvetica", FontStyle::normal()))
            .or_else(|| fm.match_family_style("Arial", FontStyle::normal()))
            .unwrap_or_else(|| {
                fm.legacy_make_typeface(None, FontStyle::normal())
                    .expect("No fallback font")
            });
        Font::from_typeface(typeface, size)
    }

    pub fn blit(&mut self) {
        let current_frame = self.current_frame;
        let total_frames = self.total_frames;
        let fps = self.fps;
        let playing = self.playing;
        let rendered_width = self.rendered_width;
        let rendered_height = self.rendered_height;
        let export_state = self.export_state;

        let ui_font = self.make_ui_font(11.0);
        let error_message = self.error_message.clone();
        let export_progress = self.export_progress;
        let export_progress_label = self.export_progress_label.clone();
        let small_font = self.make_ui_font(10.0);
        let label_font = self.make_ui_font(11.0);
        let error_title_font = self.make_ui_font(16.0);
        let error_body_font = self.make_ui_font(13.0);

        let Some(surface) = &mut self.surface else {
            return;
        };
        let Some(window) = &self.window else {
            return;
        };

        let physical = window.inner_size();
        let width = physical.width;
        let height = physical.height;

        if width != self.display_width || height != self.display_height {
            self.display_width = width;
            self.display_height = height;
            if let (Some(w), Some(h)) = (NonZeroU32::new(width), NonZeroU32::new(height)) {
                let _ = surface.resize(w, h);
            }
        }

        let Ok(mut buffer) = surface.buffer_mut() else {
            return;
        };

        let buf_info = ImageInfo::new(
            (width as i32, height as i32),
            ColorType::BGRA8888,
            AlphaType::Premul,
            None,
        );
        let byte_buf = unsafe {
            std::slice::from_raw_parts_mut(
                buffer.as_mut_ptr() as *mut u8,
                (width * height) as usize * 4,
            )
        };
        let Some(mut skia_surface) =
            surfaces::wrap_pixels(&buf_info, byte_buf, width as usize * 4, None)
        else {
            let _ = buffer.present();
            return;
        };

        let canvas = skia_surface.canvas();
        canvas.clear(skia_safe::Color::BLACK);

        let w = width as f32;
        let h = height as f32;

        // ── Video frame ────────────────────────────────────────────
        // When the requested frame isn't cached yet, keep showing the last
        // frame we successfully displayed instead of snapping to whatever
        // happens to be the nearest cached frame. The "nearest" fallback
        // produced visible glitches during scrubbing/playback (a transient
        // unrelated frame would flash on screen until the requested one
        // finished rendering).
        let display_frame = if self.frame_cache.contains_key(&current_frame) {
            self.last_displayed_frame = Some(current_frame);
            Some(current_frame)
        } else {
            self.last_displayed_frame
                .filter(|f| self.frame_cache.contains_key(f))
        };

        if let Some(frame_id) = display_frame {
            if let Some(rgba) = self.frame_cache.get(&frame_id) {
                let rw = rendered_width;
                let rh = rendered_height;
                let src_info = ImageInfo::new(
                    (rw as i32, rh as i32),
                    ColorType::RGBA8888,
                    AlphaType::Premul,
                    None,
                );
                let src_data = skia_safe::Data::new_copy(rgba);
                if let Some(img) =
                    skia_safe::images::raster_from_data(&src_info, src_data, rw as usize * 4)
                {
                    let dst_rect = skia_safe::Rect::from_wh(w, h);
                    let mut paint = Paint::default();
                    paint.set_anti_alias(true);
                    canvas.draw_image_rect(&img, None, dst_rect, &paint);
                }
            }
        }

        // ── Floating control bar ───────────────────────────────────
        let layout = compute_control_bar_layout(w, h);

        let mut bg_paint = Paint::default();
        bg_paint.set_color4f(Color4f::new(0.1, 0.1, 0.1, 0.82), None);
        bg_paint.set_anti_alias(true);
        let bar_rrect = RRect::new_rect_xy(layout.bar_rect, CONTROLS_BAR_RADIUS, CONTROLS_BAR_RADIUS);
        canvas.draw_rrect(bar_rrect, &bg_paint);

        let mut border_paint = Paint::default();
        border_paint.set_color4f(Color4f::new(1.0, 1.0, 1.0, 0.1), None);
        border_paint.set_anti_alias(true);
        border_paint.set_style(skia_safe::PaintStyle::Stroke);
        border_paint.set_stroke_width(0.5);
        canvas.draw_rrect(bar_rrect, &border_paint);

        let mut icon_paint = Paint::default();
        icon_paint.set_color4f(Color4f::new(1.0, 1.0, 1.0, 0.9), None);
        icon_paint.set_anti_alias(true);

        draw_prev_icon(canvas, &layout.prev_btn, &icon_paint);
        if playing {
            draw_pause_icon(canvas, &layout.play_btn, &icon_paint);
        } else {
            draw_play_icon(canvas, &layout.play_btn, &icon_paint);
        }
        draw_next_icon(canvas, &layout.next_btn, &icon_paint);

        // Export button
        match export_state {
            ExportState::Idle => {
                draw_export_icon(canvas, &layout.export_btn, &icon_paint);
            }
            ExportState::Exporting => {
                let ecx = layout.export_btn.center_x();
                let ecy = layout.export_btn.center_y();
                let r = BUTTON_ICON_SIZE / 2.0;

                let mut track_paint = Paint::default();
                track_paint.set_color4f(Color4f::new(1.0, 1.0, 1.0, 0.2), None);
                track_paint.set_anti_alias(true);
                track_paint.set_style(skia_safe::PaintStyle::Stroke);
                track_paint.set_stroke_width(2.0);
                let oval = Rect::from_xywh(ecx - r, ecy - r, r * 2.0, r * 2.0);
                canvas.draw_arc(oval, 0.0, 360.0, false, &track_paint);

                let sweep = export_progress * 360.0;
                let mut progress_paint = Paint::default();
                progress_paint.set_color4f(Color4f::new(0.3, 0.7, 1.0, 0.9), None);
                progress_paint.set_anti_alias(true);
                progress_paint.set_style(skia_safe::PaintStyle::Stroke);
                progress_paint.set_stroke_width(2.0);
                progress_paint.set_stroke_cap(skia_safe::paint::Cap::Round);
                canvas.draw_arc(oval, -90.0, sweep, false, &progress_paint);

                let pct = (export_progress * 100.0).round() as u32;
                let pct_text = format!("{}%", pct);
                let mut text_paint = Paint::default();
                text_paint.set_color4f(Color4f::new(1.0, 1.0, 1.0, 0.9), None);
                text_paint.set_anti_alias(true);
                if let Some(blob) = TextBlob::new(&pct_text, &small_font) {
                    let bounds = blob.bounds();
                    canvas.draw_text_blob(
                        &blob,
                        (ecx - bounds.width() / 2.0, ecy + bounds.height() / 2.0 - 1.0),
                        &text_paint,
                    );
                }

                if !export_progress_label.is_empty() {
                    let mut label_paint = Paint::default();
                    label_paint.set_color4f(Color4f::new(1.0, 1.0, 1.0, 0.7), None);
                    label_paint.set_anti_alias(true);
                    if let Some(blob) = TextBlob::new(&export_progress_label, &label_font) {
                        let bounds = blob.bounds();
                        let lx = layout.bar_rect.center_x() - bounds.width() / 2.0;
                        let ly = layout.bar_rect.bottom() + 20.0;
                        canvas.draw_text_blob(&blob, (lx, ly), &label_paint);
                    }
                }
            }
            ExportState::Done(_) => {
                let ecx = layout.export_btn.center_x();
                let ecy = layout.export_btn.center_y();
                let s = BUTTON_ICON_SIZE / 2.0;
                let mut check = Path::new();
                check.move_to((ecx - s * 0.5, ecy));
                check.line_to((ecx - s * 0.1, ecy + s * 0.4));
                check.line_to((ecx + s * 0.5, ecy - s * 0.4));
                let mut check_paint = Paint::default();
                check_paint.set_color4f(Color4f::new(0.2, 0.9, 0.4, 0.9), None);
                check_paint.set_anti_alias(true);
                check_paint.set_style(skia_safe::PaintStyle::Stroke);
                check_paint.set_stroke_width(2.0);
                check_paint.set_stroke_cap(skia_safe::paint::Cap::Round);
                check_paint.set_stroke_join(skia_safe::paint::Join::Round);
                canvas.draw_path(&check, &check_paint);
            }
            ExportState::Error(_) => {
                let ecx = layout.export_btn.center_x();
                let ecy = layout.export_btn.center_y();
                let s = BUTTON_ICON_SIZE / 2.0 * 0.5;
                let mut x_path = Path::new();
                x_path.move_to((ecx - s, ecy - s));
                x_path.line_to((ecx + s, ecy + s));
                x_path.move_to((ecx + s, ecy - s));
                x_path.line_to((ecx - s, ecy + s));
                let mut err_paint = Paint::default();
                err_paint.set_color4f(Color4f::new(1.0, 0.3, 0.3, 0.9), None);
                err_paint.set_anti_alias(true);
                err_paint.set_style(skia_safe::PaintStyle::Stroke);
                err_paint.set_stroke_width(2.0);
                err_paint.set_stroke_cap(skia_safe::paint::Cap::Round);
                canvas.draw_path(&x_path, &err_paint);
            }
        }

        // Timeline row
        if total_frames > 0 {
            let progress = current_frame as f32 / total_frames.max(1) as f32;

            let mut track_paint = Paint::default();
            track_paint.set_color4f(Color4f::new(1.0, 1.0, 1.0, 0.15), None);
            track_paint.set_anti_alias(true);
            let track_rrect = RRect::new_rect_xy(
                layout.timeline_rect,
                TIMELINE_BAR_RADIUS,
                TIMELINE_BAR_RADIUS,
            );
            canvas.draw_rrect(track_rrect, &track_paint);

            let filled_w = layout.timeline_rect.width() * progress;
            let filled_rect = Rect::from_xywh(
                layout.timeline_rect.left,
                layout.timeline_rect.top,
                filled_w,
                TIMELINE_BAR_H,
            );
            let mut fill_paint = Paint::default();
            fill_paint.set_color4f(Color4f::new(1.0, 1.0, 1.0, 0.6), None);
            fill_paint.set_anti_alias(true);
            let fill_rrect =
                RRect::new_rect_xy(filled_rect, TIMELINE_BAR_RADIUS, TIMELINE_BAR_RADIUS);
            canvas.draw_rrect(fill_rrect, &fill_paint);

            let ph_x = layout.timeline_rect.left + filled_w;
            let ph_cy = layout.timeline_rect.center_y();
            let mut ph_paint = Paint::default();
            ph_paint.set_color(skia_safe::Color::WHITE);
            ph_paint.set_anti_alias(true);
            canvas.draw_circle((ph_x, ph_cy), 5.0, &ph_paint);

            let cur_time = current_frame as f64 / fps.max(1) as f64;
            let cur_min = (cur_time / 60.0) as u32;
            let cur_sec = cur_time % 60.0;
            let cur_text = format!("{:02}:{:02}", cur_min, cur_sec as u32);
            if let Some(blob) = TextBlob::new(&cur_text, &ui_font) {
                let mut tp = Paint::default();
                tp.set_color4f(Color4f::new(1.0, 1.0, 1.0, 0.5), None);
                tp.set_anti_alias(true);
                canvas.draw_text_blob(&blob, layout.time_left_pos, &tp);
            }

            let tot_time = total_frames as f64 / fps.max(1) as f64;
            let tot_min = (tot_time / 60.0) as u32;
            let tot_sec = tot_time % 60.0;
            let tot_text = format!("{:02}:{:02}", tot_min, tot_sec as u32);
            if let Some(blob) = TextBlob::new(&tot_text, &ui_font) {
                let mut tp = Paint::default();
                tp.set_color4f(Color4f::new(1.0, 1.0, 1.0, 0.5), None);
                tp.set_anti_alias(true);
                let text_w = blob.bounds().width();
                let x = layout.time_right_pos.0 - text_w;
                canvas.draw_text_blob(&blob, (x, layout.time_right_pos.1), &tp);
            }
        }

        // Error overlay
        if let Some(ref err_msg) = error_message {
            let mut backdrop = Paint::default();
            backdrop.set_color4f(Color4f::new(0.0, 0.0, 0.0, 0.75), None);
            canvas.draw_rect(Rect::from_wh(w, h), &backdrop);

            let box_w = (w * 0.8).min(600.0);
            let box_x = (w - box_w) / 2.0;
            let box_y = h * 0.3;
            let padding = 24.0;

            let title = "Schema Error";
            let mut title_paint = Paint::default();
            title_paint.set_color4f(Color4f::new(1.0, 0.35, 0.35, 1.0), None);
            title_paint.set_anti_alias(true);

            let mut bg_paint = Paint::default();
            bg_paint.set_color4f(Color4f::new(0.12, 0.12, 0.12, 0.95), None);

            let max_text_w = box_w - padding * 2.0;
            let mut lines: Vec<String> = Vec::new();
            for raw_line in err_msg.lines() {
                let mut current = String::new();
                for word in raw_line.split_whitespace() {
                    let test = if current.is_empty() {
                        word.to_string()
                    } else {
                        format!("{} {}", current, word)
                    };
                    if let Some(blob) = TextBlob::new(&test, &error_body_font) {
                        if blob.bounds().width() > max_text_w && !current.is_empty() {
                            lines.push(current);
                            current = word.to_string();
                        } else {
                            current = test;
                        }
                    } else {
                        current = test;
                    }
                }
                if !current.is_empty() {
                    lines.push(current);
                }
            }

            let line_h = 18.0;
            let title_h = 24.0;
            let box_h = padding + title_h + 12.0 + lines.len() as f32 * line_h + padding;

            let rrect = RRect::new_rect_xy(
                Rect::from_xywh(box_x, box_y, box_w, box_h),
                12.0, 12.0,
            );
            canvas.draw_rrect(rrect, &bg_paint);

            if let Some(blob) = TextBlob::new(title, &error_title_font) {
                canvas.draw_text_blob(&blob, (box_x + padding, box_y + padding + title_h * 0.7), &title_paint);
            }

            let mut body_paint = Paint::default();
            body_paint.set_color4f(Color4f::new(0.9, 0.9, 0.9, 0.9), None);
            body_paint.set_anti_alias(true);
            let text_y_start = box_y + padding + title_h + 12.0;
            for (i, line) in lines.iter().enumerate() {
                if let Some(blob) = TextBlob::new(line, &error_body_font) {
                    canvas.draw_text_blob(
                        &blob,
                        (box_x + padding, text_y_start + i as f32 * line_h + line_h * 0.7),
                        &body_paint,
                    );
                }
            }
        }

        drop(skia_surface);

        for p in buffer.iter_mut() {
            *p &= 0x00FFFFFF;
        }

        let _ = buffer.present();
    }
}

impl ApplicationHandler for PreviewApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let monitor = event_loop
            .primary_monitor()
            .or_else(|| event_loop.available_monitors().next());
        let (max_w, max_h) = monitor
            .map(|m| {
                let size = m.size();
                let scale_factor = m.scale_factor();
                (
                    size.width as f64 / scale_factor * 0.85,
                    size.height as f64 / scale_factor * 0.85,
                )
            })
            .unwrap_or((1920.0, 1080.0));

        self.scale = (max_w / self.video_width as f64)
            .min(max_h / self.video_height as f64)
            .min(1.0);

        let logical_w = ((self.video_width as f64 * self.scale) as u32).max(1);
        let logical_h = ((self.video_height as f64 * self.scale) as u32).max(1);

        let window_attrs = Window::default_attributes()
            .with_title("rustmotion \u{2014} preview")
            .with_inner_size(LogicalSize::new(logical_w, logical_h))
            .with_resizable(false);

        let window =
            Arc::new(event_loop.create_window(window_attrs).expect("Failed to create window"));

        let scale_factor = window.scale_factor();
        self.display_width = ((logical_w as f64 * scale_factor) as u32).max(1);
        self.display_height = ((logical_h as f64 * scale_factor) as u32).max(1);
        let context =
            softbuffer::Context::new(window.clone()).expect("Failed to create softbuffer context");
        let mut surface =
            softbuffer::Surface::new(&context, window.clone()).expect("Failed to create surface");

        surface
            .resize(
                NonZeroU32::new(self.display_width).unwrap(),
                NonZeroU32::new(self.display_height).unwrap(),
            )
            .expect("Failed to resize surface");

        self.window = Some(window);
        self.surface = Some(surface);

        let render_scale = (self.display_width as f32 / self.video_width as f32)
            .min(self.display_height as f32 / self.video_height as f32);
        let _ = self.render_tx.send(RenderRequest::SetScale(render_scale));

        self.update_title();
        self.request_frame(0);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                let _ = self.render_tx.send(RenderRequest::Shutdown);
                event_loop.exit();
            }

            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods.state();
            }

            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(key),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                let shift = self.modifiers.shift_key();
                match key {
                    KeyCode::Space => self.toggle_playback(),
                    KeyCode::ArrowRight if shift => self.step_frame(10),
                    KeyCode::ArrowLeft if shift => self.step_frame(-10),
                    KeyCode::ArrowRight => self.step_frame(1),
                    KeyCode::ArrowLeft => self.step_frame(-1),
                    KeyCode::Home => { self.playing = false; self.go_to_frame(0); }
                    KeyCode::End => { self.playing = false; self.go_to_frame(self.total_frames.saturating_sub(1)); }
                    KeyCode::Escape => {
                        let _ = self.render_tx.send(RenderRequest::Shutdown);
                        event_loop.exit();
                    }
                    _ => {}
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_x = position.x;
                self.cursor_y = position.y;
                if self.timeline_dragging {
                    let layout = compute_control_bar_layout(
                        self.display_width as f32,
                        self.display_height as f32,
                    );
                    let frame = timeline_x_to_frame(
                        position.x,
                        layout.timeline_rect.left as f64,
                        layout.timeline_rect.right as f64,
                        self.total_frames,
                    );
                    self.playing = false;
                    self.go_to_frame(frame);
                }
            }

            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                let layout = compute_control_bar_layout(
                    self.display_width as f32,
                    self.display_height as f32,
                );
                let pt = (self.cursor_x as f32, self.cursor_y as f32);

                match state {
                    ElementState::Pressed => {
                        if rect_contains(&layout.export_btn, pt.0, pt.1) {
                            self.start_export();
                        } else if rect_contains(&layout.prev_btn, pt.0, pt.1) {
                            self.step_frame(-1);
                        } else if rect_contains(&layout.play_btn, pt.0, pt.1) {
                            self.toggle_playback();
                        } else if rect_contains(&layout.next_btn, pt.0, pt.1) {
                            self.step_frame(1);
                        } else if self.cursor_y >= layout.bar_rect.top as f64
                            && self.cursor_y >= layout.timeline_rect.top as f64 - 10.0
                            && self.cursor_x >= layout.timeline_rect.left as f64
                            && self.cursor_x <= layout.timeline_rect.right as f64
                        {
                            self.timeline_dragging = true;
                            self.playing = false;
                            let frame = timeline_x_to_frame(
                                self.cursor_x,
                                layout.timeline_rect.left as f64,
                                layout.timeline_rect.right as f64,
                                self.total_frames,
                            );
                            self.go_to_frame(frame);
                        }
                    }
                    ElementState::Released => {
                        self.timeline_dragging = false;
                    }
                }
            }

            WindowEvent::RedrawRequested => {
                self.blit();
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Poll render responses
        while let Ok(response) = self.render_rx.try_recv() {
            self.rendered_width = response.pixel_width;
            self.rendered_height = response.pixel_height;
            self.frame_cache.insert(response.frame, response.rgba);
            if self.pending_frame == Some(response.frame) {
                self.pending_frame = None;
            }
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }

        // Poll reload responses
        while let Ok(response) = self.reload_rx.try_recv() {
            match response {
                ReloadResponse::Ready {
                    total_frames,
                    fps,
                    width,
                    height,
                } => {
                    self.total_frames = total_frames;
                    self.fps = fps;
                    self.video_width = width;
                    self.video_height = height;
                    self.frame_duration = Duration::from_secs_f64(1.0 / fps.max(1) as f64);
                    self.frame_cache.clear();
                    self.last_displayed_frame = None;
                    if self.current_frame >= total_frames {
                        self.current_frame = total_frames.saturating_sub(1);
                    }
                    self.request_frame(self.current_frame);
                    self.update_title();
                }
            }
        }

        // Poll file changes
        if let Some(ref rx) = self.file_change_rx {
            if rx.try_recv().is_ok() {
                while rx.try_recv().is_ok() {}
                self.reload_scenario();
            }
        }

        // Poll export progress
        if let Some(ref rx) = self.export_progress_rx {
            while let Ok((progress, label)) = rx.try_recv() {
                self.export_progress = progress;
                self.export_progress_label = label;
            }
        }

        // Poll export completion
        if let Some(ref rx) = self.export_done_rx {
            if let Ok(result) = rx.try_recv() {
                match result {
                    Ok(path) => {
                        eprintln!("Export done: {}", path);
                        self.export_state = ExportState::Done(Instant::now());
                        send_notification("Rustmotion", &format!("Export terminé : {}", path));
                    }
                    Err(e) => {
                        eprintln!("Export error: {}", e);
                        self.export_state = ExportState::Error(Instant::now());
                        send_notification("Rustmotion", "Erreur lors de l'export");
                    }
                }
                self.export_done_rx = None;
                self.export_progress_rx = None;
                self.export_progress = 0.0;
                self.export_progress_label.clear();
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
        }

        // Revert Done/Error to Idle after 2 seconds
        match self.export_state {
            ExportState::Done(t) | ExportState::Error(t) => {
                if t.elapsed() >= Duration::from_secs(2) {
                    self.export_state = ExportState::Idle;
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }
            _ => {}
        }

        // Advance playback
        let needs_animation = matches!(self.export_state,
            ExportState::Exporting | ExportState::Done(_) | ExportState::Error(_));

        if self.playing {
            let now = Instant::now();
            let frame_ready = self.frame_cache.contains_key(&self.current_frame);
            if frame_ready && now.duration_since(self.last_frame_time) >= self.frame_duration {
                self.last_frame_time = now;
                if self.current_frame < self.total_frames.saturating_sub(1) {
                    self.current_frame += 1;
                    self.request_frame(self.current_frame);
                    self.update_title();
                } else {
                    self.current_frame = 0;
                    self.request_frame(0);
                    self.update_title();
                }
            } else if !frame_ready {
                self.last_frame_time = now;
                self.request_frame(self.current_frame);
            }
            event_loop.set_control_flow(ControlFlow::Poll);
        } else if needs_animation
            || self.pending_frame.is_some()
            || self.frame_cache.len() < self.total_frames as usize
        {
            if needs_animation {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(16),
            ));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }
}
