//! The `shimmer` effect lights the element's own pixels, not its box.
//!
//! A sweeping band painted over a node's *rectangle* reads as a lit rectangle
//! sliding past the element. Painted against the node's own alpha it reads as
//! light catching the element itself — which is the effect anyone asking for a
//! shimmer actually wants. `engine::paint_pass` gets that by isolating the
//! node in its own layer and stamping the band with `BlendMode::SrcATop`.
//!
//! The proof below is a circle: the corners of its box are inside the
//! rectangle a naive implementation would light, and outside the shape.

use rustmotion::engine::render::render_scene_frame_scaled;
use rustmotion::schema::{Scene, VideoConfig};

const W: u32 = 400;
const H: u32 = 400;

fn config() -> VideoConfig {
    serde_json::from_value(serde_json::json!({ "width": W, "height": H, "fps": 30 }))
        .expect("config")
}

/// A white circle inscribed in a 200×200 box at (100, 100), on black.
/// `shimmer` is attached only when `with_shimmer` — the two renders are
/// otherwise byte-identical, so any difference is the band and nothing else.
fn circle_scene(with_shimmer: bool) -> Scene {
    let animation = if with_shimmer {
        serde_json::json!([{
            "name": "shimmer",
            "delay": 0.0,
            "duration": 1.0,
            "color": "#FF0000",
            "intensity": 1.0,
            "width": 0.9,
            "angle": 0.0
        }])
    } else {
        serde_json::json!([])
    };
    serde_json::from_value(serde_json::json!({
        "duration": 2.0,
        "background": "#000000",
        "children": [{
            "type": "shape",
            "shape": "circle",
            "fill": "#FFFFFF",
            "position": "absolute",
            "x": 100,
            "y": 100,
            "style": { "width": "200px", "height": "200px", "animation": animation }
        }]
    }))
    .expect("scene is schema-valid")
}

fn pixel_at(buf: &[u8], x: u32, y: u32) -> [u8; 4] {
    let base = ((y * W + x) * 4) as usize;
    [buf[base], buf[base + 1], buf[base + 2], buf[base + 3]]
}

/// The scene is 2s at 30fps; scene-local time comes from the frame index.
const SCENE_FRAMES: u32 = 60;

fn render(with_shimmer: bool, time: f64) -> Vec<u8> {
    let frame = (time * 30.0).round() as u32;
    render_scene_frame_scaled(
        &config(),
        &circle_scene(with_shimmer),
        frame,
        time,
        SCENE_FRAMES,
        1.0,
    )
    .expect("render")
}

#[test]
fn the_band_lights_the_shape_but_never_the_empty_corners_of_its_box() {
    // Mid-sweep, with a band wide enough to span the whole box.
    let plain = render(false, 0.5);
    let shimmered = render(true, 0.5);

    // Inside the circle: the red band must have visibly tinted it.
    let centre_plain = pixel_at(&plain, 200, 200);
    let centre_lit = pixel_at(&shimmered, 200, 200);
    assert_eq!(
        centre_plain,
        [255, 255, 255, 255],
        "test setup: the circle's centre should be pure white without a shimmer"
    );
    assert!(
        centre_lit[1] < 230 || centre_lit[2] < 230,
        "the band should have visibly tinted the circle's centre, got {centre_lit:?}"
    );

    // The corners of the same box are inside the rectangle a box-wide band
    // would light, but outside the shape — they must be untouched.
    for (x, y) in [(105, 105), (295, 105), (105, 295), (295, 295)] {
        assert_eq!(
            pixel_at(&plain, x, y),
            pixel_at(&shimmered, x, y),
            "({x},{y}) is inside the shape's box but outside the shape — the band must not \
             reach it"
        );
    }
}

#[test]
fn the_sweep_moves_across_the_element_over_time() {
    // A narrow band at two instants must light two different parts of the
    // circle — otherwise it is a static wash, not a sweep.
    let narrow = |time: f64| -> Vec<u8> {
        let scene: Scene = serde_json::from_value(serde_json::json!({
            "duration": 2.0,
            "background": "#000000",
            "children": [{
                "type": "shape",
                "shape": "circle",
                "fill": "#FFFFFF",
                "position": "absolute",
                "x": 100,
                "y": 100,
                "style": {
                    "width": "200px", "height": "200px",
                    "animation": [{
                        "name": "shimmer", "duration": 1.0, "color": "#FF0000",
                        "intensity": 1.0, "width": 0.15, "angle": 0.0
                    }]
                }
            }]
        }))
        .expect("scene is schema-valid");
        let frame = (time * 30.0).round() as u32;
        render_scene_frame_scaled(&config(), &scene, frame, time, SCENE_FRAMES, 1.0)
            .expect("render")
    };

    // Sample a row through the circle's centre and find where the band is
    // darkest in green (i.e. most saturated red).
    let band_centre = |buf: &[u8]| -> Option<u32> {
        (130..270)
            .filter(|&x| pixel_at(buf, x, 200)[3] == 255)
            .min_by_key(|&x| pixel_at(buf, x, 200)[1])
    };

    let early = band_centre(&narrow(0.3)).expect("band visible at 30%");
    let late = band_centre(&narrow(0.7)).expect("band visible at 70%");

    assert!(
        late > early + 20,
        "the band should have travelled measurably across the circle between 30% and 70% of \
         the sweep (early={early}, late={late})"
    );
}

#[test]
fn a_finished_sweep_leaves_the_element_exactly_as_it_was() {
    // Past its duration a non-looping shimmer must be gone — not parked at
    // the far edge still tinting the last column of pixels.
    let plain = render(false, 1.5);
    let after = render(true, 1.5);
    assert_eq!(
        plain, after,
        "after its duration, a non-looping shimmer must leave no trace on the frame"
    );
}
