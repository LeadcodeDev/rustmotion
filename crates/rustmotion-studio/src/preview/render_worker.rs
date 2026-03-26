use std::sync::mpsc::{Receiver, Sender};

use rustmotion::encode::{self, video::FrameTask};
use rustmotion::schema::ResolvedScenario;

// ── Messages ────────────────────────────────────────────────────────

pub(super) enum RenderRequest {
    Frame(u32),
    SetScale(f32),
    Reload(ResolvedScenario),
    Shutdown,
}

pub(super) struct RenderResponse {
    pub frame: u32,
    pub rgba: Vec<u8>,
    pub pixel_width: u32,
    pub pixel_height: u32,
}

pub(super) enum ReloadResponse {
    Ready {
        total_frames: u32,
        fps: u32,
        width: u32,
        height: u32,
    },
}

// ── Render thread ───────────────────────────────────────────────────

pub(super) fn render_thread(
    req_rx: Receiver<RenderRequest>,
    resp_tx: Sender<RenderResponse>,
    reload_resp_tx: Sender<ReloadResponse>,
    mut scenario: ResolvedScenario,
) {
    let mut frame_tasks = encode::build_frame_tasks(&scenario);
    let mut rendered = vec![false; frame_tasks.len()];
    let mut bg_cursor: u32 = 0;
    let mut scale: f32 = 1.0;

    let render_one = |fi: u32,
                      tasks: &[FrameTask],
                      sc: &ResolvedScenario,
                      done: &mut Vec<bool>,
                      tx: &Sender<RenderResponse>,
                      sf: f32| {
        if fi >= tasks.len() as u32 {
            return;
        }
        if done[fi as usize] {
            return;
        }
        if let Some(task) = tasks.get(fi as usize) {
            match encode::render_frame_task_scaled(&sc.video, sc, task, sf) {
                Ok(rgba) => {
                    let pw = (sc.video.width as f32 * sf) as u32;
                    let ph = (sc.video.height as f32 * sf) as u32;
                    let _ = tx.send(RenderResponse {
                        frame: fi,
                        rgba,
                        pixel_width: pw,
                        pixel_height: ph,
                    });
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
                          reload_tx: &Sender<ReloadResponse>|
     -> bool {
        match req {
            RenderRequest::Frame(fi) => {
                let total = tasks.len() as u32;
                render_one(fi, tasks, sc, done, tx, *sf);
                if fi + 1 < total {
                    render_one(fi + 1, tasks, sc, done, tx, *sf);
                }
                if fi + 2 < total {
                    render_one(fi + 2, tasks, sc, done, tx, *sf);
                }
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
                    total_frames: new_total,
                    fps,
                    width: w,
                    height: h,
                });
            }
            RenderRequest::Shutdown => return true,
        }
        false
    };

    let Ok(first) = req_rx.recv() else { return };
    if handle_request(
        first,
        &mut frame_tasks,
        &mut scenario,
        &mut rendered,
        &mut bg_cursor,
        &mut scale,
        &resp_tx,
        &reload_resp_tx,
    ) {
        return;
    }

    loop {
        while let Ok(req) = req_rx.try_recv() {
            if handle_request(
                req,
                &mut frame_tasks,
                &mut scenario,
                &mut rendered,
                &mut bg_cursor,
                &mut scale,
                &resp_tx,
                &reload_resp_tx,
            ) {
                return;
            }
        }

        let total = frame_tasks.len() as u32;
        let next_frame = (bg_cursor..total)
            .chain(0..bg_cursor)
            .find(|&i| !rendered[i as usize]);

        if let Some(fi) = next_frame {
            render_one(fi, &frame_tasks, &scenario, &mut rendered, &resp_tx, scale);
            bg_cursor = fi + 1;
        } else {
            match req_rx.recv() {
                Ok(req) => {
                    if handle_request(
                        req,
                        &mut frame_tasks,
                        &mut scenario,
                        &mut rendered,
                        &mut bg_cursor,
                        &mut scale,
                        &resp_tx,
                        &reload_resp_tx,
                    ) {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
    }
}
