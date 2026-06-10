use std::path::PathBuf;

use dioxus::prelude::*;
use dioxus_icons::lucide::{Folder, Import, Plus, Search};

use crate::library::{ScenarioEntry, SharedLibrary};
use crate::scenario::{empty_scenario, Shared, StudioModel, View};

const NEW_SCENARIO: &str = r##"{
  "video": { "width": 1920, "height": 1080, "background": "#0f172a" },
  "scenes": [
    { "duration": 3.0, "children": [
      { "type": "text", "content": "New scenario", "style": { "font-size": 64, "color": "#ffffff", "text-align": "center" } }
    ] }
  ]
}
"##;

/// The library home page: a sidebar of folder groups + recents, and a grid of
/// scenario cards (frame-0 thumbnails). Clicking a card opens it in the editor.
#[component]
pub fn Library(view: Signal<View>) -> Element {
    let library = use_context::<SharedLibrary>();
    let shared = use_context::<Shared>();

    let mut selected = use_signal(|| 0usize);
    let mut query = use_signal(String::new);

    // Build sections: a "Recent" pseudo-group first, then the workspace groups.
    let sections: Vec<(String, Vec<ScenarioEntry>)> = {
        let lib = library.lock().unwrap();
        let mut s = Vec::new();
        if !lib.recents.is_empty() {
            s.push(("Recent".to_string(), lib.recents.clone()));
        }
        for g in &lib.groups {
            s.push((g.name.clone(), g.entries.clone()));
        }
        if s.is_empty() {
            s.push(("Default".to_string(), Vec::new()));
        }
        s
    };
    let sel = selected().min(sections.len().saturating_sub(1));
    let (section_name, entries) = sections[sel].clone();

    let q = query().to_lowercase();
    let visible: Vec<_> = entries
        .into_iter()
        .filter(|e| q.is_empty() || e.name.to_lowercase().contains(&q))
        .collect();

    rsx! {
        div { style: "display:flex; height:100vh; overflow:hidden;",
            // ── Sidebar ──────────────────────────────────────────────
            div { style: "width:280px; flex:none; background:var(--rm-surface); border-right:1px solid var(--rm-border); display:flex; flex-direction:column; padding:14px;",
                div { style: "display:flex; align-items:center; justify-content:space-between; margin-bottom:16px;",
                    div { style: "font-weight:700; color:var(--rm-text-strong);", "Rustmotion" }
                    button {
                        style: "display:flex; align-items:center; gap:4px; padding:4px 8px; cursor:pointer; background:var(--rm-surface-3); color:var(--rm-text); border:1px solid var(--rm-border-2); border-radius:6px;",
                        onclick: {
                            let shared = shared.clone();
                            let library = library.clone();
                            move |_| new_scenario(&shared, &library, view)
                        },
                        Plus { size: 14 }
                        "New"
                    }
                }
                button {
                    style: "display:flex; align-items:center; gap:6px; padding:6px 8px; margin-bottom:12px; cursor:pointer; background:none; color:var(--rm-text); border:1px solid var(--rm-border-2); border-radius:6px;",
                    onclick: {
                        let shared = shared.clone();
                        let library = library.clone();
                        move |_| import_scenario(shared.clone(), library.clone(), view)
                    },
                    Import { size: 14 }
                    "Import file…"
                }
                div { style: "color:var(--rm-text-muted); font-size:11px; text-transform:uppercase; letter-spacing:0.5px; margin:8px 0;", "Folders" }
                div { style: "overflow:auto; display:flex; flex-direction:column; gap:2px;",
                    for (i, (name, group_entries)) in sections.iter().enumerate() {
                        button {
                            key: "{name}-{i}",
                            style: format!(
                                "display:flex; align-items:center; gap:8px; padding:7px 8px; cursor:pointer; border:none; border-radius:6px; text-align:left; color:{}; background:{};",
                                if i == sel { "var(--rm-text-strong)" } else { "var(--rm-text)" },
                                if i == sel { "var(--rm-surface-3)" } else { "transparent" }
                            ),
                            onclick: move |_| selected.set(i),
                            Folder { size: 16 }
                            div { style: "flex:1;", "{name}" }
                            div { style: "color:var(--rm-text-muted); font-size:11px;", "{group_entries.len()}" }
                        }
                    }
                }
            }
            // ── Main area ────────────────────────────────────────────
            div { style: "flex:1; display:flex; flex-direction:column; overflow:hidden;",
                div { style: "display:flex; align-items:center; justify-content:space-between; padding:20px 28px; gap:16px;",
                    div { style: "font-size:24px; font-weight:700; color:var(--rm-text-strong);", "{section_name}" }
                    div { style: "display:flex; align-items:center; gap:6px; background:var(--rm-surface); border:1px solid var(--rm-border-2); border-radius:8px; padding:6px 10px; min-width:240px;",
                        Search { size: 15 }
                        input {
                            style: "flex:1; background:none; border:none; outline:none; color:var(--rm-text); font:inherit;",
                            placeholder: "Search",
                            value: "{query}",
                            oninput: move |e| query.set(e.value()),
                        }
                    }
                }
                div { style: "flex:1; overflow:auto; padding:0 28px 28px;",
                    if visible.is_empty() {
                        div { style: "color:var(--rm-text-muted); padding:24px;", "No scenarios here." }
                    }
                    div { style: "display:grid; grid-template-columns:repeat(auto-fill, minmax(240px, 1fr)); gap:18px;",
                        for entry in visible.iter().cloned() {
                            div {
                                key: "{entry.path.display()}",
                                style: "cursor:pointer; display:flex; flex-direction:column; gap:8px;",
                                onclick: {
                                    let shared = shared.clone();
                                    let library = library.clone();
                                    let path = entry.path.clone();
                                    move |_| open_scenario(&shared, &library, view, path.clone())
                                },
                                div { style: "aspect-ratio:16/9; background:var(--rm-surface-2); border:1px solid var(--rm-border); border-radius:10px; overflow:hidden; display:flex; align-items:center; justify-content:center;",
                                    img {
                                        src: "/thumb/{entry.flat_index}",
                                        style: "width:100%; height:100%; object-fit:cover; display:block;",
                                    }
                                }
                                div { style: "color:var(--rm-text); font-size:13px;", "{entry.name}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Load a scenario into the shared model and switch to the editor.
fn open_scenario(shared: &Shared, library: &SharedLibrary, mut view: Signal<View>, path: PathBuf) {
    let (scenario, error) = match rustmotion::loader::load_input(&path) {
        Ok(s) => (s, None),
        Err(e) => (empty_scenario(), Some(e.to_string())),
    };
    {
        let mut m = shared.lock().unwrap();
        let g = m.generation.wrapping_add(1);
        *m = StudioModel::new(scenario, error, Some(path.clone()));
        m.generation = g;
    }
    {
        let mut lib = library.lock().unwrap();
        lib.note_opened(&path);
        lib.retarget_watch(&path);
    }
    view.set(View::Editor);
}

/// Create a fresh scenario file in the workspace and open it.
fn new_scenario(shared: &Shared, library: &SharedLibrary, view: Signal<View>) {
    let workspace = { library.lock().unwrap().workspace.clone() };
    let mut path = workspace.join("untitled.json");
    let mut n = 1;
    while path.exists() {
        path = workspace.join(format!("untitled-{n}.json"));
        n += 1;
    }
    if std::fs::write(&path, NEW_SCENARIO).is_ok() {
        open_scenario(shared, library, view, path);
    }
}

/// Open a native file picker (off the UI thread) and open the chosen scenario.
fn import_scenario(shared: Shared, library: SharedLibrary, view: Signal<View>) {
    spawn(async move {
        if let Some(handle) = rfd::AsyncFileDialog::new()
            .add_filter("scenario", &["json", "html", "htm"])
            .pick_file()
            .await
        {
            open_scenario(&shared, &library, view, handle.path().to_path_buf());
        }
    });
}
