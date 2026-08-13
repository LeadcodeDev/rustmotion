//! A deeply nested scenario must render on the stacks the app actually uses.
//!
//! The CLI renders on the main thread (8 MB on macOS) and never noticed. The
//! studio renders thumbnails inside a webview asset-handler callback, which runs
//! on an OS-provided thread with a far smaller stack — so a scenario that is
//! merely *deep* aborts the whole process with a stack overflow there while
//! rendering fine from the terminal.
use std::path::PathBuf;

fn render_on(stack_kb: usize, path: PathBuf) -> bool {
    std::thread::Builder::new()
        .stack_size(stack_kb * 1024)
        .spawn(move || {
            let scenario = rustmotion::loader::load_input(&path).expect("load");
            let tasks = rustmotion::encode::build_frame_tasks(&scenario);
            rustmotion::encode::render_frame_task_scaled(
                &scenario.video,
                &scenario,
                &tasks[0],
                0.25,
            )
            .expect("render");
        })
        .unwrap()
        .join()
        .is_ok()
}

#[test]
#[ignore = "probe: needs PROBE_FILE=<scenario.json>"]
fn report_stack_needed() {
    // One size per process: a stack overflow aborts, it cannot be caught, so a
    // loop inside a single run would stop at the first failure.
    let p = PathBuf::from(std::env::var("PROBE_FILE").expect("PROBE_FILE"));
    let kb: usize = std::env::var("PROBE_STACK_KB").unwrap().parse().unwrap();
    println!(
        "stack {kb} KiB -> {}",
        if render_on(kb, p) { "ok" } else { "panic" }
    );
}
