use rustmotion::error::Result;
use rustmotion::loader::load_scenario;
use rustmotion::schema;
use std::path::PathBuf;

pub fn cmd_info(input: &PathBuf) -> Result<()> {
    let scenario = load_scenario(input)?;
    let fps = scenario.video.fps;
    let all_scenes: Vec<_> = scenario.all_scenes().collect();
    let total_duration: f64 = all_scenes.iter().map(|s| s.duration).sum();
    let total_frames: u32 = all_scenes
        .iter()
        .map(|s| (s.duration * fps as f64).round() as u32)
        .sum();

    let total_layers: usize = all_scenes.iter().map(|s| s.children.len()).sum();

    println!("File: {}", input.display());
    println!("Resolution: {}x{}", scenario.video.width, scenario.video.height);
    println!("FPS: {}", fps);
    println!("Duration: {:.1}s ({} frames)", total_duration, total_frames);
    println!("Views: {}", scenario.views.len());
    println!("Scenes: {}", all_scenes.len());
    println!("Total layers: {}", total_layers);
    println!("Audio tracks: {}", scenario.audio.len());

    for (vi, view) in scenario.views.iter().enumerate() {
        let vtype = match view.view_type {
            schema::ViewType::Slide => "Slide",
            schema::ViewType::World => "World",
        };
        println!("  View {}: {} ({} scenes)", vi + 1, vtype, view.scenes.len());
        for (si, scene) in view.scenes.iter().enumerate() {
            let scene_frames = (scene.duration * fps as f64).round() as u32;
            println!(
                "    Scene {}: {:.1}s ({} frames, {} layers{})",
                si + 1,
                scene.duration,
                scene_frames,
                scene.children.len(),
                scene
                    .transition
                    .as_ref()
                    .map(|t| format!(", transition: {:?} {:.1}s", t.transition_type, t.duration))
                    .unwrap_or_default()
            );
        }
    }

    Ok(())
}
