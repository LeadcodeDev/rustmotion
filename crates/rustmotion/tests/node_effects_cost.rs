use std::path::PathBuf;
use std::time::Instant;

use rustmotion::engine::render::post_effects::apply_post_effects;
use rustmotion::engine::render::{render_scene_frame_scaled, render_scene_hits};
use rustmotion::loader::load_scenario;
use rustmotion::schema::{BlurDirection, PostEffect, ResolvedScenario};

fn load_mega_showcase() -> ResolvedScenario {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join("mega-showcase.json");
    load_scenario(&path).expect("load examples/mega-showcase.json")
}

fn realistic_effects() -> Vec<PostEffect> {
    vec![
        PostEffect::Grain {
            intensity: 0.15,
            seed: 42,
            animated: true,
        },
        PostEffect::Vignette {
            intensity: 0.5,
            radius: 0.75,
        },
        PostEffect::Pixelate { size: 8 },
        PostEffect::ProgressiveBlur {
            direction: BlurDirection::Bottom,
            start: 0.5,
            max_radius: 12.0,
        },
    ]
}

#[test]
#[ignore = "prints timing numbers for the node-effects cost report; not a correctness check"]
fn bench_pixel_effects_full_frame_vs_node_box() {
    let scenario = load_mega_showcase();
    let config = &scenario.video;
    let scene = scenario.all_scenes_vec()[0];
    let full_w = config.width;
    let full_h = config.height;

    let hits = render_scene_hits(config, scene, 0);
    let viewport_area = (full_w * full_h) as f32;
    let mut sizes: Vec<(f32, f32)> = hits.iter().map(|h| (h.rect.w, h.rect.h)).collect();
    sizes.sort_by(|a, b| (a.0 * a.1).partial_cmp(&(b.0 * b.1)).unwrap());
    let node = hits
        .iter()
        .filter(|h| h.rect.w * h.rect.h < 0.9 * viewport_area)
        .max_by(|a, b| {
            (a.rect.w * a.rect.h)
                .partial_cmp(&(b.rect.w * b.rect.h))
                .unwrap()
        })
        .expect("at least one non-full-bleed component in scene 0");
    let box_w = node.rect.w.round().max(1.0) as u32;
    let box_h = node.rect.h.round().max(1.0) as u32;
    eprintln!(
        "[node-effects cost] all {} component hit-rect sizes in scene 0 (w x h): {:?}",
        sizes.len(),
        sizes
            .iter()
            .map(|(w, h)| format!("{}x{}", w.round(), h.round()))
            .collect::<Vec<_>>()
    );

    let effects = realistic_effects();
    const FRAMES: u32 = 30;

    let mut full_buf = vec![0u8; (full_w * full_h * 4) as usize];
    let t0 = Instant::now();
    for f in 0..FRAMES {
        apply_post_effects(&mut full_buf, full_w, full_h, &effects, f, 0.0);
    }
    let full_elapsed = t0.elapsed();

    let mut box_buf = vec![0u8; (box_w * box_h * 4) as usize];
    let t0 = Instant::now();
    for f in 0..FRAMES {
        apply_post_effects(&mut box_buf, box_w, box_h, &effects, f, 0.0);
    }
    let box_elapsed = t0.elapsed();

    let area_ratio = (full_w * full_h) as f64 / (box_w * box_h) as f64;
    let time_ratio = full_elapsed.as_secs_f64() / box_elapsed.as_secs_f64().max(1e-9);

    eprintln!(
        "\n[node-effects cost] full-frame {full_w}x{full_h} vs node-box {box_w}x{box_h} \
         (largest non-full-bleed component in mega-showcase scene 0), {FRAMES} frames, 4 effects (grain+vignette+pixelate+progressive_blur):\n\
         \x20 full-frame : {full_elapsed:?}  ({:.3} ms/frame)\n\
         \x20 node-box   : {box_elapsed:?}  ({:.3} ms/frame)\n\
         \x20 area ratio : {area_ratio:.2}x   time ratio: {time_ratio:.2}x\n",
        full_elapsed.as_secs_f64() * 1000.0 / FRAMES as f64,
        box_elapsed.as_secs_f64() * 1000.0 / FRAMES as f64,
    );
}

#[test]
#[ignore = "prints timing numbers for the node-effects cost report; not a correctness check"]
fn bench_duplicate_paint_via_render_scene_hits() {
    let scenario = load_mega_showcase();
    let config = &scenario.video;
    let scene = scenario.all_scenes_vec()[0];
    const FRAMES: u32 = 30;

    let t0 = Instant::now();
    for f in 0..FRAMES {
        let _ = render_scene_frame_scaled(config, scene, f, f as f64 / 30.0, FRAMES, 1.0)
            .expect("render");
    }
    let normal_elapsed = t0.elapsed();

    let t0 = Instant::now();
    for f in 0..FRAMES {
        let _ = render_scene_hits(config, scene, f);
    }
    let hits_elapsed = t0.elapsed();

    let combined = normal_elapsed + hits_elapsed;
    let overhead_ratio = combined.as_secs_f64() / normal_elapsed.as_secs_f64();

    eprintln!(
        "\n[node-effects cost] mega-showcase scene 0 ({}x{}@{}fps), {FRAMES} frames:\n\
         \x20 normal render (paid today)         : {normal_elapsed:?}  ({:.3} ms/frame)\n\
         \x20 + render_scene_hits (rejected path) : {hits_elapsed:?}  ({:.3} ms/frame)\n\
         \x20 combined vs normal-only             : {overhead_ratio:.2}x\n",
        config.width,
        config.height,
        config.fps,
        normal_elapsed.as_secs_f64() * 1000.0 / FRAMES as f64,
        hits_elapsed.as_secs_f64() * 1000.0 / FRAMES as f64,
    );
}
