use rustmotion::engine::render::render_scene_hits;
use rustmotion::schema::{Scene, VideoConfig};

fn config() -> VideoConfig {
    serde_json::from_value(serde_json::json!({
        "width": 1920,
        "height": 1080,
        "fps": 30
    }))
    .expect("video config is schema-valid")
}

fn resizing_card_scene() -> Scene {
    serde_json::from_value(serde_json::json!({
        "duration": 2.0,
        "children": [{
            "type": "card",
            "style": {
                "width": 330,
                "height": 132,
                "background": "#1E293B",
                "justify-content": "center",
                "align-items": "center",
                "animation": [{
                    "name": "keyframes",
                    "duration": 1.0,
                    "keyframes": [
                        {
                            "property": "width",
                            "easing": "linear",
                            "keyframes": [
                                { "time": 0.0, "value": 330 },
                                { "time": 1.0, "value": 560 }
                            ]
                        },
                        {
                            "property": "height",
                            "easing": "linear",
                            "keyframes": [
                                { "time": 0.0, "value": 132 },
                                { "time": 1.0, "value": 210 }
                            ]
                        }
                    ]
                }]
            },
            "children": [
                { "type": "text", "content": "Detail", "style": { "font-size": 24, "color": "#FFFFFF" } }
            ]
        }]
    }))
    .expect("scene is schema-valid")
}

fn card_rect(frame: u32) -> (f32, f32) {
    let hits = render_scene_hits(&config(), &resizing_card_scene(), frame);
    let card = hits
        .iter()
        .find(|h| h.kind == "div")
        .expect("card hit present in render_scene_hits output");
    (card.rect.w, card.rect.h)
}

#[test]
fn animated_width_and_height_resize_the_laid_out_box() {
    let (w0, h0) = card_rect(0);
    let (w_mid, h_mid) = card_rect(15);
    let (w_end, h_end) = card_rect(30);

    assert!(
        (w0 - 330.0).abs() < 1.0 && (h0 - 132.0).abs() < 1.0,
        "at t=0 the card should still be at its authored 330×132, got {w0}×{h0}"
    );
    assert!(
        (w_end - 560.0).abs() < 1.0 && (h_end - 210.0).abs() < 1.0,
        "at t=1s the card should have reached 560×210, got {w_end}×{h_end}"
    );
    assert!(
        w_mid > w0 + 1.0 && w_mid < w_end - 1.0,
        "mid-animation width should sit strictly between 330 and 560, got {w_mid}"
    );
    assert!(
        h_mid > h0 + 1.0 && h_mid < h_end - 1.0,
        "mid-animation height should sit strictly between 132 and 210, got {h_mid}"
    );
}

#[test]
fn the_child_reflows_inside_the_resized_card() {
    let hits_start = render_scene_hits(&config(), &resizing_card_scene(), 0);
    let hits_end = render_scene_hits(&config(), &resizing_card_scene(), 30);

    let child_rect = |hits: &[rustmotion_core::engine::paint_pass::EnrichedHit]| {
        hits.iter()
            .find(|h| h.kind == "text")
            .expect("text child hit present")
            .rect
    };

    let start = child_rect(&hits_start);
    let end = child_rect(&hits_end);

    assert!(
        (start.x - end.x).abs() > 1.0,
        "the centred text child should have been pushed right by the card's extra width, but its \
         x is unchanged: start={start:?} end={end:?}"
    );
    assert!(
        (start.y - end.y).abs() > 1.0,
        "the centred text child should have been pushed down by the card's extra height, but its \
         y is unchanged: start={start:?} end={end:?}"
    );
}
