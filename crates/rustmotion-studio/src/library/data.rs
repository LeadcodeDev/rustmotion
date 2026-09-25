use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

pub enum WatchMsg {
    Retarget(PathBuf),
    Changed,
}

#[derive(Clone, PartialEq)]
pub struct ScenarioEntry {
    pub path: PathBuf,
    pub name: String,
}

#[derive(Clone, PartialEq)]
pub struct Group {
    pub name: String,
    pub entries: Vec<ScenarioEntry>,
}

pub struct LibraryState {
    pub workspace: PathBuf,
    pub groups: Vec<Group>,
    pub recents: Vec<ScenarioEntry>,
    pub watch_tx: Option<Sender<WatchMsg>>,
}

pub type SharedLibrary = Arc<Mutex<LibraryState>>;

impl LibraryState {
    pub fn new(workspace: PathBuf) -> Self {
        let mut s = Self {
            workspace,
            groups: Vec::new(),
            recents: Vec::new(),
            watch_tx: None,
        };
        s.refresh();
        s
    }

    pub fn retarget_watch(&self, path: &Path) {
        if let Some(tx) = &self.watch_tx {
            let _ = tx.send(WatchMsg::Retarget(path.to_path_buf()));
        }
    }

    pub fn refresh(&mut self) {
        self.groups = scan_workspace(&self.workspace);
        self.recents = load_recents()
            .into_iter()
            .map(|p| ScenarioEntry {
                name: file_title(&p),
                path: p,
            })
            .collect();
    }

    pub fn note_opened(&mut self, path: &Path) {
        push_recent(path);
        self.refresh();
    }
}

pub fn is_scenario_json(v: &serde_json::Value) -> bool {
    v.get("video").map(|x| x.is_object()).unwrap_or(false)
        && (v.get("scenes").map(|x| x.is_array()).unwrap_or(false)
            || v.get("composition").is_some())
}

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
    }
}

fn file_title(p: &Path) -> String {
    p.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("untitled")
        .to_string()
}

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
