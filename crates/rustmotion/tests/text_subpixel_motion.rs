use rustmotion::encode::{build_frame_tasks, render_frame_task_scaled};
use rustmotion::schema::ResolvedScenario;

const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;
const FPS: u32 = 30;
const TRAVEL_PX: f64 = 3.0;

fn scenario() -> ResolvedScenario {
    let json = format!(
        r##"{{
          "version": "1.0",
          "video": {{ "width": {WIDTH}, "height": {HEIGHT}, "fps": {FPS}, "background": "#FFFFFF" }},
          "scenes": [{{
            "duration": 1.0,
            "layout": {{ "align_items": "center", "justify_content": "center" }},
            "children": [{{
              "type": "div",
              "style": {{
                "flex-direction": "column", "gap": 20,
                "animation": [{{ "name": "keyframes", "keyframes": [{{ "property": "translate_y", "easing": "linear",
                  "keyframes": [{{ "time": 0.0, "value": 0.0 }}, {{ "time": 1.0, "value": {TRAVEL_PX} }}] }}] }}]
              }},
              "children": [
                {{ "type": "text", "content": "Chef de projet", "style": {{ "font-size": 40, "font-weight": "bold", "color": "#000000" }} }},
                {{ "type": "shape", "shape": "rect", "fill": "#000000", "style": {{ "width": 300, "height": 4 }} }}
              ]
            }}]
          }}]
        }}"##
    );
    rustmotion::loader::load_scenario_from_source(None, Some(&json)).expect("scenario loads")
}

fn row_ink(rgba: &[u8]) -> Vec<f64> {
    (0..HEIGHT as usize)
        .map(|y| {
            (0..WIDTH as usize)
                .map(|x| {
                    let i = (y * WIDTH as usize + x) * 4;
                    let [r, g, b, a] = [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]];
                    let luma = (0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64) / 255.0;
                    (1.0 - luma) * (a as f64 / 255.0)
                })
                .sum()
        })
        .collect()
}

fn ink_bands(rows: &[f64]) -> Vec<(usize, usize)> {
    let mut bands = Vec::new();
    let mut start = None;
    for (y, ink) in rows.iter().enumerate() {
        match (start, *ink > 0.01) {
            (None, true) => start = Some(y),
            (Some(s), false) => {
                bands.push((s, y));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        bands.push((s, rows.len()));
    }
    bands
}

fn centroid(rows: &[f64], (from, to): (usize, usize)) -> f64 {
    let weight: f64 = rows[from..to].iter().sum();
    let moment: f64 = rows[from..to]
        .iter()
        .enumerate()
        .map(|(i, ink)| (from + i) as f64 * ink)
        .sum();
    moment / weight
}

fn centroids_per_frame() -> Vec<(f64, f64)> {
    let scenario = scenario();
    rustmotion::engine::preload::preload_scenario_assets(&scenario).expect("preload");
    let tasks = build_frame_tasks(&scenario);
    assert_eq!(tasks.len(), FPS as usize, "a 1s scene at {FPS}fps");

    tasks
        .iter()
        .map(|task| {
            let rgba =
                render_frame_task_scaled(&scenario.video, &scenario, task, 1.0).expect("render");
            let rows = row_ink(&rgba);
            let bands = ink_bands(&rows);
            assert_eq!(
                bands.len(),
                2,
                "expected the text band and the bar band, got {bands:?}"
            );
            (centroid(&rows, bands[0]), centroid(&rows, bands[1]))
        })
        .collect()
}

#[test]
fn text_tracks_a_slow_translation_as_closely_as_the_shape_beside_it() {
    let per_frame = centroids_per_frame();
    let (text0, bar0) = per_frame[0];

    let mut worst_text = 0.0_f64;
    let mut worst_gap = 0.0_f64;
    for (i, (text, bar)) in per_frame.iter().enumerate() {
        let expected = TRAVEL_PX * i as f64 / (FPS - 1) as f64;
        worst_text = worst_text.max((text - text0 - expected).abs());
        worst_gap = worst_gap.max(((text - text0) - (bar - bar0)).abs());
    }

    assert!(
        worst_text < 0.25,
        "glyphs snapped to whole pixels: the text centroid strays {worst_text:.3}px from a \
         {TRAVEL_PX}px linear travel, against 0.690px measured with snapping on\n{per_frame:#?}"
    );
    assert!(
        worst_gap < 0.25,
        "the text drifts {worst_gap:.3}px against the bar it shares a translated box with\n\
         {per_frame:#?}"
    );
}
