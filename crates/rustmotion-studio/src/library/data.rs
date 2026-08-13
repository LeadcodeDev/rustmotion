//! Library data layer: workspace scanning, recent files, and thumbnail cache.
//! Pure logic (no UI). The home page reads a `SharedLibrary` from context.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

/// Control message to the single re-targetable file watcher.
pub enum WatchMsg {
    Retarget(PathBuf),
    Changed,
}

/// A single scenario in the library.
#[derive(Clone, PartialEq)]
pub struct ScenarioEntry {
    pub path: PathBuf,
    pub name: String,
    /// Index into `LibraryState.index` — used for the `/thumb/{i}` URL.
    pub flat_index: usize,
}

/// A folder group (the workspace root = "Default"; subdirs = their name).
#[derive(Clone, PartialEq)]
pub struct Group {
    pub name: String,
    pub entries: Vec<ScenarioEntry>,
}

/// The full library state, shared between the home UI and the thumbnail handler.
pub struct LibraryState {
    pub workspace: PathBuf,
    pub groups: Vec<Group>,
    pub recents: Vec<ScenarioEntry>,
    /// Flat, authoritative path list; `/thumb/{i}` maps to `index[i]`.
    pub index: Vec<PathBuf>,
    pub thumb_cache: HashMap<PathBuf, Arc<Vec<u8>>>,
    /// When launched with `--file`, start directly in the editor.
    pub start_in_editor: bool,
    /// Channel to the file watcher (set by `run_preview_root`).
    pub watch_tx: Option<Sender<WatchMsg>>,
}

pub type SharedLibrary = Arc<Mutex<LibraryState>>;

impl LibraryState {
    pub fn new(workspace: PathBuf, start_in_editor: bool) -> Self {
        let mut s = Self {
            workspace,
            groups: Vec::new(),
            recents: Vec::new(),
            index: Vec::new(),
            thumb_cache: HashMap::new(),
            start_in_editor,
            watch_tx: None,
        };
        s.refresh();
        s
    }

    /// Tell the watcher to follow `path` (so editing it hot-reloads the editor).
    pub fn retarget_watch(&self, path: &Path) {
        if let Some(tx) = &self.watch_tx {
            let _ = tx.send(WatchMsg::Retarget(path.to_path_buf()));
        }
    }

    /// Rescan the workspace + reload recents + rebuild the flat index.
    pub fn refresh(&mut self) {
        self.groups = scan_workspace(&self.workspace);
        self.recents = load_recents()
            .into_iter()
            .map(|p| ScenarioEntry {
                name: file_title(&p),
                path: p,
                flat_index: 0,
            })
            .collect();
        self.reindex();
    }

    /// Ensure a freshly-opened path is visible (recents + index) even if it
    /// lives outside the workspace (e.g. an imported file).
    pub fn note_opened(&mut self, path: &Path) {
        push_recent(path);
        self.refresh();
        // If still absent (e.g. canonicalization mismatch), append it.
        if !self.index.iter().any(|p| p == path) {
            self.index.push(path.to_path_buf());
        }
    }

    pub fn path_at(&self, i: usize) -> Option<PathBuf> {
        self.index.get(i).cloned()
    }

    fn reindex(&mut self) {
        let mut index: Vec<PathBuf> = Vec::new();
        let mut pos: HashMap<PathBuf, usize> = HashMap::new();
        for g in &self.groups {
            for e in &g.entries {
                pos.entry(e.path.clone()).or_insert_with(|| {
                    index.push(e.path.clone());
                    index.len() - 1
                });
            }
        }
        for e in &self.recents {
            pos.entry(e.path.clone()).or_insert_with(|| {
                index.push(e.path.clone());
                index.len() - 1
            });
        }
        for g in &mut self.groups {
            for e in &mut g.entries {
                e.flat_index = pos[&e.path];
            }
        }
        for e in &mut self.recents {
            e.flat_index = pos[&e.path];
        }
        self.index = index;
    }
}

/// Render a scenario's frame 0 to a small JPEG thumbnail. No lock held by design
/// (the caller renders this off-lock, then re-locks to cache the result).
pub fn render_thumbnail(path: &Path) -> Option<Vec<u8>> {
    let scenario = rustmotion::loader::load_input(&path.to_path_buf()).ok()?;
    let tasks = rustmotion::encode::build_frame_tasks(&scenario);
    if tasks.is_empty() {
        return None;
    }
    Some(crate::editor::frames::render_frame_deep(
        &scenario, &tasks, 0, 0.25,
    ))
}

/// Cheap check: is this JSON a Rustmotion scenario? Avoids the heavier
/// `load_scenario` (includes/variables) during a directory scan.
pub fn is_scenario_json(v: &serde_json::Value) -> bool {
    v.get("video").map(|x| x.is_object()).unwrap_or(false)
        && (v.get("scenes").map(|x| x.is_array()).unwrap_or(false)
            || v.get("composition").is_some())
}

/// Scan `root`: `.json` scenarios directly in it → a "Default" group; each
/// immediate subdirectory with scenarios → its own group.
pub fn scan_workspace(root: &Path) -> Vec<Group> {
    let mut default = Vec::new();
    let mut subgroups: Vec<Group> = Vec::new();

    if let Ok(rd) = std::fs::read_dir(root) {
        let mut dirs: Vec<PathBuf> = Vec::new();
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                dirs.push(p);
            } else if is_scenario_file(&p) {
                default.push(entry_for(&p));
            }
        }
        dirs.sort();
        for dir in dirs {
            let mut entries = Vec::new();
            if let Ok(sub) = std::fs::read_dir(&dir) {
                for e in sub.flatten() {
                    let p = e.path();
                    if is_scenario_file(&p) {
                        entries.push(entry_for(&p));
                    }
                }
            }
            if !entries.is_empty() {
                entries.sort_by(|a, b| a.name.cmp(&b.name));
                subgroups.push(Group {
                    name: file_title(&dir),
                    entries,
                });
            }
        }
    }

    let mut groups = Vec::new();
    if !default.is_empty() {
        default.sort_by(|a, b| a.name.cmp(&b.name));
        groups.push(Group {
            name: "Default".to_string(),
            entries: default,
        });
    }
    groups.extend(subgroups);
    groups
}

fn is_scenario_file(p: &Path) -> bool {
    match p.extension().and_then(|e| e.to_str()) {
        Some("json") => std::fs::read_to_string(p)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .map(|v| is_scenario_json(&v))
            .unwrap_or(false),
        // Cheap HTML-dialect check: a scenario has a <rustmotion> root element.
        Some("html") | Some("htm") => std::fs::read_to_string(p)
            .map(|s| s.contains("<rustmotion"))
            .unwrap_or(false),
        _ => false,
    }
}

fn entry_for(p: &Path) -> ScenarioEntry {
    ScenarioEntry {
        name: file_title(p),
        path: p.to_path_buf(),
        flat_index: 0,
    }
}

fn file_title(p: &Path) -> String {
    p.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("untitled")
        .to_string()
}

// ── Recents ──────────────────────────────────────────────────────────

const RECENTS_CAP: usize = 15;

fn recents_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("rustmotion").join("recent.json"))
}

pub fn load_recents() -> Vec<PathBuf> {
    let path = match recents_path() {
        Some(p) => p,
        None => return Vec::new(),
    };
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let list: Vec<String> = serde_json::from_str(&raw).unwrap_or_default();
    list.into_iter()
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .collect()
}

fn save_recents(list: &[PathBuf]) {
    if let Some(path) = recents_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let strs: Vec<String> = list.iter().map(|p| p.display().to_string()).collect();
        if let Ok(s) = serde_json::to_string_pretty(&strs) {
            let _ = std::fs::write(&path, s);
        }
    }
}

pub fn push_recent(path: &Path) {
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let list = mru_insert(load_recents(), abs, RECENTS_CAP);
    save_recents(&list);
}

/// Pure MRU insert: move `path` to front, dedup, cap length.
pub fn mru_insert(mut list: Vec<PathBuf>, path: PathBuf, cap: usize) -> Vec<PathBuf> {
    list.retain(|p| p != &path);
    list.insert(0, path);
    list.truncate(cap);
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn is_scenario_json_accepts_and_rejects() {
        assert!(is_scenario_json(&json!({ "video": {}, "scenes": [] })));
        assert!(is_scenario_json(&json!({ "video": {}, "composition": [] })));
        assert!(!is_scenario_json(&json!({ "foo": 1 })));
        assert!(!is_scenario_json(&json!({ "video": {} })));
    }

    #[test]
    fn mru_insert_moves_to_front_dedups_caps() {
        let a = PathBuf::from("/a");
        let b = PathBuf::from("/b");
        let c = PathBuf::from("/c");
        let list = mru_insert(vec![a.clone(), b.clone()], c.clone(), 15);
        assert_eq!(list, vec![c.clone(), a.clone(), b.clone()]);
        let list = mru_insert(list, a.clone(), 15);
        assert_eq!(list, vec![a.clone(), c.clone(), b.clone()]);
        let list = mru_insert(vec![a.clone(), b.clone()], c.clone(), 2);
        assert_eq!(list, vec![c, a]);
    }

    #[test]
    fn scan_workspace_groups_examples() {
        // The repo's examples/ dir holds demo.json + component-showcase.json.
        let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        let groups = scan_workspace(&examples);
        let default = groups
            .iter()
            .find(|g| g.name == "Default")
            .expect("a Default group with the example scenarios");
        assert!(
            default.entries.iter().any(|e| e.name == "demo"),
            "expected demo.json in the Default group"
        );
    }
}
