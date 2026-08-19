//! Regression test — animated `width`/`height` must reach the layout pass.
//!
//! `engine::animator` has always resolved a `keyframes` effect targeting
//! `width`/`height` into `AnimatedProperties::{width,height}` (animator.rs,
//! the `"width" => props.width = …` arm), but `css::animation::
//! apply_animated_props` deliberately dropped those two on the floor: they
//! were never translated into `CssStyle`, so taffy never saw them and the box
//! kept its authored size for the whole scene. The animation resolved to
//! numbers nobody read.
//!
//! That is the difference between a card *resizing* — its children reflowing
//! inside a new box — and a card being *scaled*, which stretches the pixels it
//! already had, text included. Only the former is expressible with `scale`'s
//! absence here; the latter was the only thing authors could actually get.

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

/// A card that grows 330×132 → 560×210 over the first second, holding a text
/// child whose own box has to follow the parent's new width.
fn resizing_card_scene() -> Scene {
    serde_json::from_value(serde_json::json!({
        "duration": 2.0,
        "children": [{
            "type": "card",
            "style": {
                "width": 330,
                "height": 132,
                "background": "#1E293B",
                // Centred content, so the child's *position* tracks the box's
                // growth on both axes — not just its stretched height.
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
        .find(|h| h.kind == "card")
        .expect("card hit present in render_scene_hits output");
    (card.rect.w, card.rect.h)
}

#[test]
fn animated_width_and_height_resize_the_laid_out_box() {
    let (w0, h0) = card_rect(0); // t = 0.0s
    let (w_mid, h_mid) = card_rect(15); // t = 0.5s
    let (w_end, h_end) = card_rect(30); // t = 1.0s

    assert!(
        (w0 - 330.0).abs() < 1.0 && (h0 - 132.0).abs() < 1.0,
        "at t=0 the card should still be at its authored 330×132, got {w0}×{h0}"
    );
    assert!(
        (w_end - 560.0).abs() < 1.0 && (h_end - 210.0).abs() < 1.0,
        "at t=1s the card should have reached 560×210, got {w_end}×{h_end}"
    );
    // The interesting frame: strictly between the two, which is what fails
    // when the animated size never reaches taffy (it would read 330×132).
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
    // A resize is a layout change, so the child's box has to move with the
    // parent's new content box. A `scale` transform would leave the child's
    // laid-out rect untouched and only stretch its pixels — this assertion is
    // what separates the two.
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
