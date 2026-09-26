use rustmotion::engine::render::post_effects::apply_post_effects;
use rustmotion::engine::render::{render_scene_frame_scaled, render_scene_hits};
use rustmotion::schema::{PostEffect, Scene, VideoConfig};

fn background_and_overlapping_foreground() -> (VideoConfig, Scene) {
    let config: VideoConfig =
        serde_json::from_value(serde_json::json!({ "width": 800, "height": 800, "fps": 30 }))
            .expect("config");
    let scene: Scene = serde_json::from_value(serde_json::json!({
        "duration": 1.0,
        "children": [
            {
                "type": "shape", "shape": "rect", "fill": "#ff0000",
                "position": "absolute", "x": 0, "y": 0,
                "style": { "width": "800px", "height": "800px" }
            },
            {
                "type": "shape", "shape": "rect", "fill": "#0000ff",
                "position": "absolute", "x": 300, "y": 300,
                "style": { "width": "100px", "height": "100px" }
            }
        ]
    }))
    .expect("scene");
    (config, scene)
}

fn pixel_at(buf: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let base = ((y * w + x) * 4) as usize;
    [buf[base], buf[base + 1], buf[base + 2], buf[base + 3]]
}

#[test]
fn buffer_crop_strategy_leaks_effect_onto_overlapping_sibling() {
    let (config, scene) = background_and_overlapping_foreground();

    let rendered = render_scene_frame_scaled(&config, &scene, 0, 0.0, 30, 1.0).expect("render");

    let before = pixel_at(&rendered, config.width, 350, 350);
    assert_eq!(
        before,
        [0, 0, 255, 255],
        "foreground square must render as pure blue before any effect"
    );

    let hits = render_scene_hits(&config, &scene, 0);
    let background_hit = hits
        .first()
        .expect("background shape is the first painted component");
    assert!(
        (background_hit.rect.w - 800.0).abs() < 1.0 && (background_hit.rect.h - 800.0).abs() < 1.0,
        "background hit rect should cover the full 800x800 canvas, got {:?}",
        background_hit.rect
    );

    let mut buf = rendered.clone();
    let effects = vec![PostEffect::Pixelate { size: 32 }];
    apply_post_effects(&mut buf, config.width, config.height, &effects, 0, 0.0);

    let mut changed = 0u32;
    let mut worst: Option<(u32, u32, [u8; 4], [u8; 4])> = None;
    for y in 300..400 {
        for x in 300..400 {
            let a = pixel_at(&rendered, config.width, x, y);
            let b = pixel_at(&buf, config.width, x, y);
            if a != b {
                changed += 1;
                worst.get_or_insert((x, y, a, b));
            }
        }
    }
    let (sx, sy, sample_before, sample_after) =
        worst.expect("at least one changed pixel to report");

    eprintln!(
        "\n[node-effects leak proof] pixelate(size=32) applied to the background node's \
         render_scene_hits rect (0,0,800,800) via buffer-crop: {changed}/10000 pixels inside the \
         *foreground* sibling's own 100x100 box changed (sample at ({sx},{sy}): {sample_before:?} \
         -> {sample_after:?}; centre pixel (350,350) unaffected by coincidence: {before:?})\n"
    );

    assert!(
        changed > 0,
        "buffer-crop strategy must NOT change any of the foreground sibling's pixels — but it \
         changes {changed}/10000 of them, which is exactly why it was rejected in favour of an \
         isolated Skia layer inside paint_pass.rs (frozen for this chantier)"
    );
    assert!(
        changed > 5_000,
        "expected the leak to affect the majority of the sibling's box, got {changed}/10000"
    );
}
