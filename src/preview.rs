use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use skia_safe::{surfaces, AlphaType, ColorType, ImageInfo};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::encode::{self, video::FrameTask};
use crate::engine;
use crate::error::RustmotionError;
use crate::schema::ResolvedScenario;

// ── Messages ────────────────────────────────────────────────────────

enum RenderRequest {
    Frame(u32),
    SetScale(f32),
    Reload(ResolvedScenario),
    Shutdown,
}

struct RenderResponse {
    frame: u32,
    rgba: Vec<u8>,
    pixel_width: u32,
    pixel_height: u32,
}

enum ReloadResponse {
    Ready {
        total_frames: u32,
        fps: u32,
        width: u32,
        height: u32,
    },
}

const TIMELINE_HEIGHT: u32 = 40;
const TIMELINE_PADDING: u32 = 12;

fn timeline_x_to_frame(x: f64, window_w: u32, total: u32) -> u32 {
    let bar_x_start = TIMELINE_PADDING as f64;
    let bar_x_end = window_w as f64 - TIMELINE_PADDING as f64;
    let bar_w = bar_x_end - bar_x_start;
    if bar_w <= 0.0 { return 0; }
    let ratio = ((x - bar_x_start) / bar_w).clamp(0.0, 1.0);
    ((ratio * total as f64) as u32).min(total.saturating_sub(1))
}

// ── PreviewApp ──────────────────────────────────────────────────────

struct PreviewApp {
    // Video metadata (kept in sync with render thread)
    total_frames: u32,
    fps: u32,
    video_width: u32,
    video_height: u32,
    input_path: Option<PathBuf>,

    // Playback
    current_frame: u32,
    playing: bool,
    last_frame_time: Instant,
    frame_duration: Duration,

    // Frame cache (stores raw RGBA bytes at rendered resolution)
    frame_cache: HashMap<u32, Vec<u8>>,
    rendered_width: u32,
    rendered_height: u32,

    // Render thread
    render_tx: Sender<RenderRequest>,
    render_rx: Receiver<RenderResponse>,
    reload_rx: Receiver<ReloadResponse>,
    pending_frame: Option<u32>,

    // File watch
    file_change_rx: Option<Receiver<()>>,

    // Window
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    display_width: u32,
    display_height: u32,
    scale: f64,

    // Mouse
    modifiers: ModifiersState,
    timeline_dragging: bool,
    cursor_y: f64,
}

impl PreviewApp {
    fn request_frame(&mut self, frame: u32) {
        if frame >= self.total_frames { return; }
        if self.frame_cache.contains_key(&frame) {
            if self.pending_frame == Some(frame) {
                self.pending_frame = None;
            }
            if let Some(window) = &self.window {
                window.request_redraw();
            }
            return;
        }
        if self.pending_frame == Some(frame) { return; }
        self.pending_frame = Some(frame);
        let _ = self.render_tx.send(RenderRequest::Frame(frame));
    }

    fn go_to_frame(&mut self, frame: u32) {
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
                self.request_frame(0);
            }
        }
        self.update_title();
    }

    fn update_title(&self) {
        if let Some(window) = &self.window {
            let name = self.input_path.as_ref()
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

    fn reload_scenario(&mut self) {
        if let Some(ref path) = self.input_path {
            match crate::load_scenario(path) {
                Ok(scenario) => {
                    if !scenario.fonts.is_empty() {
                        engine::renderer::load_custom_fonts(&scenario.fonts);
                    }
                    engine::prefetch_icons(&scenario.scenes);
                    engine::preextract_video_frames(&scenario.scenes, scenario.video.fps);
                    let _ = self.render_tx.send(RenderRequest::Reload(scenario));
                }
                Err(e) => eprintln!("Reload error: {}", e),
            }
        }
    }

    fn blit(&mut self) {
        let Some(surface) = &mut self.surface else { return };
        let Some(window) = &self.window else { return };

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

        let Ok(mut buffer) = surface.buffer_mut() else { return };

        // Wrap softbuffer memory as a Skia surface (BGRA8888 = u32 layout on little-endian)
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
        let Some(mut skia_surface) = surfaces::wrap_pixels(
            &buf_info, byte_buf, width as usize * 4, None,
        ) else {
            let _ = buffer.present();
            return;
        };

        let canvas = skia_surface.canvas();
        canvas.clear(skia_safe::Color::BLACK);

        // Draw video frame (Skia handles scaling + color conversion directly)
        let video_display_h = height.saturating_sub(TIMELINE_HEIGHT);
        let display_frame = if self.frame_cache.contains_key(&self.current_frame) {
            Some(self.current_frame)
        } else {
            self.frame_cache.keys()
                .min_by_key(|&&k| (k as i64 - self.current_frame as i64).unsigned_abs())
                .copied()
        };

        if let Some(frame_id) = display_frame {
            if let Some(rgba) = self.frame_cache.get(&frame_id) {
                let rw = self.rendered_width;
                let rh = self.rendered_height;
                let src_info = ImageInfo::new(
                    (rw as i32, rh as i32),
                    ColorType::RGBA8888,
                    AlphaType::Premul,
                    None,
                );
                let src_data = skia_safe::Data::new_copy(rgba);
                if let Some(img) = skia_safe::images::raster_from_data(
                    &src_info, src_data, rw as usize * 4,
                ) {
                    let dst_rect = skia_safe::Rect::from_wh(width as f32, video_display_h as f32);
                    let mut paint = skia_safe::Paint::default();
                    paint.set_anti_alias(true);
                    canvas.draw_image_rect(&img, None, dst_rect, &paint);
                }
            }
        }

        // Draw timeline directly with Skia
        let tl_y = video_display_h as f32;
        let w = width as f32;
        let h = height as f32;

        // Timeline background
        let mut paint = skia_safe::Paint::default();
        paint.set_color(skia_safe::Color::from_rgb(0x1a, 0x1a, 0x1a));
        canvas.draw_rect(skia_safe::Rect::from_xywh(0.0, tl_y, w, h - tl_y), &paint);

        if self.total_frames > 0 {
            let pad = TIMELINE_PADDING as f32;
            let bar_y = tl_y + 14.0;
            let bar_h = 12.0;
            let bar_w = w - pad * 2.0;
            let progress = self.current_frame as f32 / self.total_frames as f32;
            let filled_w = bar_w * progress;

            // Unfilled bar
            paint.set_color(skia_safe::Color::from_rgb(0x33, 0x33, 0x33));
            canvas.draw_rect(skia_safe::Rect::from_xywh(pad, bar_y, bar_w, bar_h), &paint);

            // Filled bar
            paint.set_color(skia_safe::Color::from_rgb(0x00, 0xbc, 0xd4));
            canvas.draw_rect(skia_safe::Rect::from_xywh(pad, bar_y, filled_w, bar_h), &paint);

            // Playhead
            paint.set_color(skia_safe::Color::WHITE);
            let ph_x = pad + filled_w;
            let ph_y = tl_y + 8.0;
            let ph_h = TIMELINE_HEIGHT as f32 - 16.0;
            canvas.draw_rect(skia_safe::Rect::from_xywh(ph_x, ph_y, 2.0, ph_h), &paint);
        }

        // Drop skia surface before accessing buffer again
        drop(skia_surface);

        // Mask alpha for softbuffer (expects 0x00RRGGBB)
        for p in buffer.iter_mut() {
            *p &= 0x00FFFFFF;
        }

        let _ = buffer.present();
    }
}

impl ApplicationHandler for PreviewApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Calculate scale to fit screen (logical pixels)
        let monitor = event_loop.primary_monitor()
            .or_else(|| event_loop.available_monitors().next());
        let (max_w, max_h) = monitor
            .map(|m| {
                let size = m.size();
                let scale_factor = m.scale_factor();
                (size.width as f64 / scale_factor * 0.85, size.height as f64 / scale_factor * 0.85)
            })
            .unwrap_or((1920.0, 1080.0));

        self.scale = (max_w / self.video_width as f64)
            .min(max_h / (self.video_height as f64 + TIMELINE_HEIGHT as f64))
            .min(1.0);

        let logical_w = ((self.video_width as f64 * self.scale) as u32).max(1);
        let logical_h = ((self.video_height as f64 * self.scale) as u32 + TIMELINE_HEIGHT).max(1);

        let window_attrs = Window::default_attributes()
            .with_title("rustmotion \u{2014} preview")
            .with_inner_size(LogicalSize::new(logical_w, logical_h))
            .with_resizable(false);

        let window = Arc::new(event_loop.create_window(window_attrs).expect("Failed to create window"));

        // Use physical pixel dimensions for the surface buffer (Retina-aware)
        let scale_factor = window.scale_factor();
        self.display_width = ((logical_w as f64 * scale_factor) as u32).max(1);
        self.display_height = ((logical_h as f64 * scale_factor) as u32).max(1);
        let context = softbuffer::Context::new(window.clone()).expect("Failed to create softbuffer context");
        let mut surface = softbuffer::Surface::new(&context, window.clone()).expect("Failed to create surface");

        surface.resize(
            NonZeroU32::new(self.display_width).unwrap(),
            NonZeroU32::new(self.display_height).unwrap(),
        ).expect("Failed to resize surface");

        self.window = Some(window);
        self.surface = Some(surface);

        // Send scale factor to render thread for high-DPI rendering
        // Use .min() to fit within display area, exclude timeline from height
        let video_area_h = self.display_height.saturating_sub(TIMELINE_HEIGHT);
        let render_scale = (self.display_width as f32 / self.video_width as f32)
            .min(video_area_h as f32 / self.video_height as f32);
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
                event: KeyEvent {
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
                self.cursor_y = position.y;
                if self.timeline_dragging {
                    let frame = timeline_x_to_frame(position.x, self.display_width, self.total_frames);
                    self.playing = false;
                    self.go_to_frame(frame);
                }
            }

            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                let video_h = self.display_height - TIMELINE_HEIGHT;
                let in_timeline = self.cursor_y >= video_h as f64;
                match state {
                    ElementState::Pressed if in_timeline => {
                        self.timeline_dragging = true;
                        self.playing = false;
                    }
                    ElementState::Released => {
                        self.timeline_dragging = false;
                    }
                    _ => {}
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
                ReloadResponse::Ready { total_frames, fps, width, height } => {
                    self.total_frames = total_frames;
                    self.fps = fps;
                    self.video_width = width;
                    self.video_height = height;
                    self.frame_duration = Duration::from_secs_f64(1.0 / fps.max(1) as f64);
                    self.frame_cache.clear();
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

        // Advance playback — only advance when current frame is cached (no frame drops)
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
                // Reset timer so we don't instantly skip ahead when the frame arrives
                self.last_frame_time = now;
            }
            event_loop.set_control_flow(ControlFlow::Poll);
        } else if self.pending_frame.is_some()
            || self.frame_cache.len() < self.total_frames as usize
        {
            // Keep polling while render thread is still working
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(16),
            ));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }
}

// ── Render thread ───────────────────────────────────────────────────

fn render_thread(
    req_rx: Receiver<RenderRequest>,
    resp_tx: Sender<RenderResponse>,
    reload_resp_tx: Sender<ReloadResponse>,
    mut scenario: ResolvedScenario,
) {
    let mut frame_tasks = encode::build_frame_tasks(&scenario);
    let mut rendered = vec![false; frame_tasks.len()];
    let mut bg_cursor: u32 = 0;
    let mut scale: f32 = 1.0;

    // Helper: render a single frame if not already done
    let render_one = |fi: u32,
                      tasks: &[FrameTask],
                      sc: &ResolvedScenario,
                      done: &mut Vec<bool>,
                      tx: &Sender<RenderResponse>,
                      sf: f32| {
        if fi >= tasks.len() as u32 { return; }
        if done[fi as usize] { return; }
        if let Some(task) = tasks.get(fi as usize) {
            match encode::render_frame_task_scaled(&sc.video, &sc.scenes, task, sf) {
                Ok(rgba) => {
                    let pw = (sc.video.width as f32 * sf) as u32;
                    let ph = (sc.video.height as f32 * sf) as u32;
                    let _ = tx.send(RenderResponse { frame: fi, rgba, pixel_width: pw, pixel_height: ph });
                    done[fi as usize] = true;
                }
                Err(e) => eprintln!("Render error frame {}: {}", fi, e),
            }
        }
    };

    let handle_request = |req: RenderRequest,
                          tasks: &mut Vec<FrameTask>,
                          sc: &mut ResolvedScenario,
                          done: &mut Vec<bool>,
                          cursor: &mut u32,
                          sf: &mut f32,
                          tx: &Sender<RenderResponse>,
                          reload_tx: &Sender<ReloadResponse>| -> bool {
        match req {
            RenderRequest::Frame(fi) => {
                let total = tasks.len() as u32;
                render_one(fi, tasks, sc, done, tx, *sf);
                // Prefetch +1 and +2 for smoother playback
                if fi + 1 < total {
                    render_one(fi + 1, tasks, sc, done, tx, *sf);
                }
                if fi + 2 < total {
                    render_one(fi + 2, tasks, sc, done, tx, *sf);
                }
                // Jump bg_cursor ahead so background rendering follows playback
                let ahead = (fi + 3).min(total);
                if ahead > *cursor {
                    *cursor = ahead;
                }
            }
            RenderRequest::SetScale(new_scale) => {
                if (*sf - new_scale).abs() > 0.01 {
                    *sf = new_scale;
                    *done = vec![false; tasks.len()];
                    *cursor = 0;
                }
            }
            RenderRequest::Reload(new_scenario) => {
                let fps = new_scenario.video.fps;
                let (w, h) = (new_scenario.video.width, new_scenario.video.height);
                *tasks = encode::build_frame_tasks(&new_scenario);
                let new_total = tasks.len() as u32;
                *sc = new_scenario;
                *done = vec![false; new_total as usize];
                *cursor = 0;
                let _ = reload_tx.send(ReloadResponse::Ready {
                    total_frames: new_total, fps, width: w, height: h,
                });
            }
            RenderRequest::Shutdown => return true,
        }
        false
    };

    // Wait for the first request
    let Ok(first) = req_rx.recv() else { return };
    if handle_request(first, &mut frame_tasks, &mut scenario, &mut rendered, &mut bg_cursor, &mut scale, &resp_tx, &reload_resp_tx) {
        return;
    }

    // Main loop: handle urgent requests, then background render
    loop {
        while let Ok(req) = req_rx.try_recv() {
            if handle_request(req, &mut frame_tasks, &mut scenario, &mut rendered, &mut bg_cursor, &mut scale, &resp_tx, &reload_resp_tx) {
                return;
            }
        }

        let total = frame_tasks.len() as u32;
        // Find next unrendered frame: first forward from cursor, then wrap around
        let next_frame = (bg_cursor..total)
            .chain(0..bg_cursor)
            .find(|&i| !rendered[i as usize]);

        if let Some(fi) = next_frame {
            render_one(fi, &frame_tasks, &scenario, &mut rendered, &resp_tx, scale);
            bg_cursor = fi + 1;
        } else {
            // All frames rendered — block until a new request arrives
            match req_rx.recv() {
                Ok(req) => {
                    if handle_request(req, &mut frame_tasks, &mut scenario, &mut rendered, &mut bg_cursor, &mut scale, &resp_tx, &reload_resp_tx) {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
    }
}

// ── Entry point ─────────────────────────────────────────────────────

pub fn run_preview(
    scenario: ResolvedScenario,
    input_path: Option<PathBuf>,
    watch: bool,
) -> Result<()> {
    // Prefetch assets
    engine::prefetch_icons(&scenario.scenes);
    engine::preextract_video_frames(&scenario.scenes, scenario.video.fps);
    if !scenario.fonts.is_empty() {
        engine::renderer::load_custom_fonts(&scenario.fonts);
    }

    let fps = scenario.video.fps;
    let video_width = scenario.video.width;
    let video_height = scenario.video.height;
    let total_frames = encode::build_frame_tasks(&scenario).len() as u32;

    // Channels
    let (req_tx, req_rx) = mpsc::channel::<RenderRequest>();
    let (resp_tx, resp_rx) = mpsc::channel::<RenderResponse>();
    let (reload_resp_tx, reload_resp_rx) = mpsc::channel::<ReloadResponse>();

    // Spawn render thread (owns the scenario, produces full-res RGBA)
    std::thread::spawn(move || {
        render_thread(req_rx, resp_tx, reload_resp_tx, scenario);
    });

    // File watcher (optional)
    let file_change_rx = if watch {
        if let Some(ref path) = input_path {
            let (tx, rx) = mpsc::channel();
            let watch_path = path.clone();
            std::thread::spawn(move || {
                use notify::{Watcher, RecursiveMode};
                let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
                    if let Ok(event) = res {
                        if event.kind.is_modify() || event.kind.is_create() {
                            let _ = tx.send(());
                        }
                    }
                }).expect("Failed to create file watcher");
                watcher.watch(watch_path.as_ref(), RecursiveMode::NonRecursive)
                    .expect("Failed to watch file");
                loop { std::thread::sleep(Duration::from_secs(3600)); }
            });
            Some(rx)
        } else {
            None
        }
    } else {
        None
    };

    let event_loop = EventLoop::new()
        .map_err(|e| RustmotionError::PreviewWindow { reason: e.to_string() })?;

    let mut app = PreviewApp {
        total_frames,
        fps,
        video_width,
        video_height,
        input_path,
        current_frame: 0,
        playing: false,
        last_frame_time: Instant::now(),
        frame_duration: Duration::from_secs_f64(1.0 / fps.max(1) as f64),
        frame_cache: HashMap::new(),
        rendered_width: video_width,
        rendered_height: video_height,
        render_tx: req_tx,
        render_rx: resp_rx,
        reload_rx: reload_resp_rx,
        pending_frame: None,
        file_change_rx,
        window: None,
        surface: None,
        display_width: video_width,
        display_height: video_height + TIMELINE_HEIGHT,
        scale: 1.0,
        modifiers: ModifiersState::empty(),
        timeline_dragging: false,
        cursor_y: 0.0,
    };

    event_loop.run_app(&mut app)
        .map_err(|e| RustmotionError::PreviewWindow { reason: e.to_string() })?;

    Ok(())
}
