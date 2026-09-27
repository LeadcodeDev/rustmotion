use rustmotion_core::engine::renderer::{interpolate_path_data, trim_path_between};
use skia_safe::Path;

fn horizontal_line(len: f32) -> Path {
    Path::from_svg(format!("M0 0 L{len} 0")).unwrap()
}

#[test]
fn trim_at_zero_and_progress_at_one_keeps_the_whole_path() {
    let path = horizontal_line(100.0);
    let trimmed = trim_path_between(&path, 0.0, 1.0);
    let mut measure = skia_safe::PathMeasure::new(&trimmed, false, None);
    assert!(
        (measure.length() - 100.0).abs() < 0.5,
        "start=0, end=1 must keep the full path, got length {}",
        measure.length()
    );
}

#[test]
fn trim_start_at_half_removes_the_first_half() {
    let path = horizontal_line(100.0);
    let trimmed = trim_path_between(&path, 0.5, 1.0);
    let mut measure = skia_safe::PathMeasure::new(&trimmed, false, None);
    assert!(
        (measure.length() - 50.0).abs() < 0.5,
        "start=0.5 must leave half the path visible, got length {}",
        measure.length()
    );

    let bounds = trimmed.bounds();
    assert!(
        bounds.left >= 49.0,
        "the first half of the path must have no ink; visible bounds start at {}",
        bounds.left
    );
}

#[test]
fn trim_end_at_half_removes_the_second_half() {
    let path = horizontal_line(100.0);
    let trimmed = trim_path_between(&path, 0.0, 0.5);
    let bounds = trimmed.bounds();
    assert!(
        bounds.right <= 51.0,
        "the second half of the path must have no ink; visible bounds end at {}",
        bounds.right
    );
}

#[test]
fn trim_start_after_end_yields_nothing_instead_of_swapping_silently() {
    let path = horizontal_line(100.0);
    let inverted = trim_path_between(&path, 0.8, 0.8);
    let mut measure = skia_safe::PathMeasure::new(&inverted, false, None);
    assert!(
        measure.length() < 0.5,
        "a zero-width window must paint nothing, got length {}",
        measure.length()
    );
}

#[test]
fn trim_reversed_start_and_end_is_treated_as_the_same_window() {
    let path = horizontal_line(100.0);
    let forward = trim_path_between(&path, 0.2, 0.6);
    let backward = trim_path_between(&path, 0.6, 0.2);
    let mut mf = skia_safe::PathMeasure::new(&forward, false, None);
    let mut mb = skia_safe::PathMeasure::new(&backward, false, None);
    assert!(
        (mf.length() - mb.length()).abs() < 0.5,
        "trim_path_between(0.2, 0.6) and trim_path_between(0.6, 0.2) must describe the same \
         window, got {} and {}",
        mf.length(),
        mb.length()
    );
}

#[test]
fn interpolate_matching_structure_paths_blends_the_points() {
    let from = "M0 0 L10 0 L10 10 Z";
    let to = "M0 0 L20 0 L20 20 Z";
    let mid = interpolate_path_data(from, to, 0.5).expect("same command structure must succeed");
    let bounds = mid.bounds();
    assert!(
        (bounds.right - 15.0).abs() < 0.01,
        "halfway between a 10-wide and a 20-wide triangle must be 15 wide, got {}",
        bounds.right
    );
    assert!(
        (bounds.bottom - 15.0).abs() < 0.01,
        "halfway between a 10-tall and a 20-tall triangle must be 15 tall, got {}",
        bounds.bottom
    );
}

#[test]
fn interpolate_at_t_zero_matches_the_first_keyframe_exactly() {
    let from = "M0 0 L10 0 L10 10 Z";
    let to = "M0 0 L40 0 L40 40 Z";
    let start = interpolate_path_data(from, to, 0.0).unwrap();
    let reference = Path::from_svg(from).unwrap();
    assert_eq!(*start.bounds(), *reference.bounds());
}

#[test]
fn interpolate_at_t_one_matches_the_second_keyframe_exactly() {
    let from = "M0 0 L10 0 L10 10 Z";
    let to = "M0 0 L40 0 L40 40 Z";
    let end = interpolate_path_data(from, to, 1.0).unwrap();
    let reference = Path::from_svg(to).unwrap();
    assert_eq!(*end.bounds(), *reference.bounds());
}

#[test]
fn interpolate_mismatched_command_counts_is_reported_and_returns_none() {
    let triangle = "M0 0 L10 0 L10 10 Z";
    let pentagon = "M0 0 L10 0 L10 10 L5 15 L0 10 Z";
    assert!(
        interpolate_path_data(triangle, pentagon, 0.5).is_none(),
        "a triangle and a pentagon do not share a command structure; interpolation must refuse \
         rather than snap or panic"
    );
}

#[test]
fn interpolate_mismatched_command_kind_at_the_same_position_is_reported_and_returns_none() {
    let with_a_line = "M0 0 L10 0 L10 10 Z";
    let with_a_curve = "M0 0 Q5 5 10 0 L10 10 Z";
    assert!(
        interpolate_path_data(with_a_line, with_a_curve, 0.5).is_none(),
        "a line and a quadratic curve at the same position in the command list must refuse to \
         interpolate rather than silently reinterpreting the control point"
    );
}

#[test]
fn interpolate_unparseable_svg_returns_none_instead_of_panicking() {
    assert!(interpolate_path_data("not a path", "M0 0 L10 0", 0.5).is_none());
    assert!(interpolate_path_data("M0 0 L10 0", "not a path", 0.5).is_none());
}
