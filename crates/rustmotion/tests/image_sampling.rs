use rustmotion::encode::{build_frame_tasks, render_frame_task_scaled};
use rustmotion::schema::ResolvedScenario;

const WIDTH: u32 = 320;
const HEIGHT: u32 = 160;
const SOURCE_SIDE: u32 = 640;
const BACKGROUND: [u8; 3] = [255, 0, 0];

fn write_checkerboard() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "rm_image_sampling_checker_{}.png",
        std::process::id()
    ));
    let mut buf = image::ImageBuffer::<image::Rgb<u8>, Vec<u8>>::new(SOURCE_SIDE, SOURCE_SIDE);
    for (x, y, pixel) in buf.enumerate_pixels_mut() {
        let v = if (x + y) % 2 == 0 { 255 } else { 0 };
        *pixel = image::Rgb([v, v, v]);
    }
    buf.save(&path).expect("write the checkerboard source");
    path
}

fn scenario(src: &std::path::Path) -> ResolvedScenario {
    let src = src.display();
    let json = format!(
        r##"{{
          "version": "1.0",
          "video": {{ "width": {WIDTH}, "height": {HEIGHT}, "fps": 30, "background": "#FF0000" }},
          "scenes": [{{
            "duration": 1.0,
            "layout": {{ "direction": "row", "align_items": "center", "justify_content": "center", "gap": 40 }},
            "children": [
              {{ "type": "image", "src": "{src}", "fit": "cover", "style": {{ "width": 64, "height": 64 }} }},
              {{ "type": "image", "src": "{src}", "fit": "cover", "style": {{ "width": 120, "height": 120 }} }}
            ]
          }}]
        }}"##
    );
    rustmotion::loader::load_scenario_from_source(None, Some(&json)).expect("scenario loads")
}

fn first_frame(src: &std::path::Path) -> Vec<u8> {
    let scenario = scenario(src);
    rustmotion::engine::preload::preload_scenario_assets(&scenario).expect("preload");
    let tasks = build_frame_tasks(&scenario);
    render_frame_task_scaled(&scenario.video, &scenario, &tasks[0], 1.0).expect("render")
}

fn is_background(rgba: &[u8], x: u32, y: u32) -> bool {
    let i = ((y * WIDTH + x) * 4) as usize;
    rgba[i] == BACKGROUND[0] && rgba[i + 1] == BACKGROUND[1] && rgba[i + 2] == BACKGROUND[2]
}

fn image_columns(rgba: &[u8]) -> Vec<(u32, u32)> {
    let mut spans = Vec::new();
    let mut start = None;
    for x in 0..WIDTH {
        let painted = (0..HEIGHT).any(|y| !is_background(rgba, x, y));
        match (start, painted) {
            (None, true) => start = Some(x),
            (Some(s), false) => {
                spans.push((s, x));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        spans.push((s, WIDTH));
    }
    spans
}

fn rows_of(rgba: &[u8], (from_x, to_x): (u32, u32)) -> (u32, u32) {
    let painted: Vec<u32> = (0..HEIGHT)
        .filter(|y| (from_x..to_x).any(|x| !is_background(rgba, x, *y)))
        .collect();
    (
        *painted.first().expect("a painted row"),
        painted.last().expect("a painted row") + 1,
    )
}

struct Luma {
    mean: f64,
    min: u8,
    max: u8,
}

fn luma(rgba: &[u8], (x0, x1): (u32, u32), (y0, y1): (u32, u32), inset: u32) -> Luma {
    let mut values = Vec::new();
    for y in y0 + inset..y1 - inset {
        for x in x0 + inset..x1 - inset {
            let i = ((y * WIDTH + x) * 4) as usize;
            let v =
                0.299 * rgba[i] as f64 + 0.587 * rgba[i + 1] as f64 + 0.114 * rgba[i + 2] as f64;
            values.push(v.round() as u8);
        }
    }
    Luma {
        mean: values.iter().map(|v| *v as f64).sum::<f64>() / values.len() as f64,
        min: *values.iter().min().expect("a non-empty region"),
        max: *values.iter().max().expect("a non-empty region"),
    }
}

#[test]
fn a_photo_shrunk_by_the_image_component_is_averaged_not_point_sampled() {
    let src = write_checkerboard();
    let rgba = first_frame(&src);
    let _ = std::fs::remove_file(&src);

    let spans = image_columns(&rgba);
    assert_eq!(
        spans.len(),
        2,
        "expected the 64px and the 120px image, got {spans:?}"
    );

    for (span, drawn) in spans.iter().zip([64u32, 120]) {
        let rows = rows_of(&rgba, *span);
        assert_eq!(
            (span.1 - span.0, rows.1 - rows.0),
            (drawn, drawn),
            "the {drawn}px image is not laid out at its declared size"
        );

        let measured = luma(&rgba, *span, rows, 4);
        assert!(
            (measured.mean - 127.0).abs() < 2.0 && measured.min > 118 && measured.max < 137,
            "a 1px checkerboard drawn at {drawn}px must average to grey, got mean {:.1} \
             min {} max {}; nearest-neighbour sampling reads 255/255/255 at 64px and \
             mean 124 with min 0 max 255 at 120px",
            measured.mean,
            measured.min,
            measured.max
        );
    }
}
