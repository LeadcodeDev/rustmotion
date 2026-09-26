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
    let p = PathBuf::from(std::env::var("PROBE_FILE").expect("PROBE_FILE"));
    let kb: usize = std::env::var("PROBE_STACK_KB").unwrap().parse().unwrap();
    println!(
        "stack {kb} KiB -> {}",
        if render_on(kb, p) { "ok" } else { "panic" }
    );
}
