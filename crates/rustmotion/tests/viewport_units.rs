use rustmotion::engine::render::render_scene_hits;
use rustmotion::schema::{Scene, VideoConfig};

fn vertical_config() -> VideoConfig {
    serde_json::from_value(serde_json::json!({
        "width": 1080,
        "height": 1920,
        "fps": 30
    }))
    .expect("video config is schema-valid")
}

fn scene_with_vw_vh_shape() -> Scene {
    serde_json::from_value(serde_json::json!({
        "duration": 1.0,
        "children": [
            {
                "type": "shape",
                "shape": "rect",
                "style": { "width": "50vw", "height": "50vh" }
            }
        ]
    }))
    .expect("scene is schema-valid")
}

#[test]
fn vw_and_vh_resolve_against_the_real_video_viewport() {
    let config = vertical_config();
    let scene = scene_with_vw_vh_shape();

    let hits = render_scene_hits(&config, &scene, 0);
    let shape_hit = hits
        .iter()
        .find(|h| h.kind == "shape")
        .expect("shape hit present in render_scene_hits output");

    assert!(
        (shape_hit.rect.w - 540.0).abs() < 1.0,
        "expected 50vw on a 1080px-wide viewport to resolve to ~540px, got {}",
        shape_hit.rect.w
    );
    assert!(
        (shape_hit.rect.h - 960.0).abs() < 1.0,
        "expected 50vh on a 1920px-tall viewport to resolve to ~960px, got {}",
        shape_hit.rect.h
    );
}
