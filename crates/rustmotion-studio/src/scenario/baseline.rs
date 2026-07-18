//! Baseline snapshots for the diff/review mode.
//!
//! A baseline is the scenario as it was when first opened in the editor this
//! session (source text + parsed raw JSON), kept in an app-global map keyed by
//! path. Watcher reloads never touch it, and re-opening the same file keeps
//! its original baseline until the app quits or the user re-snapshots via
//! "Set baseline".

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::Value;

/// One snapshot: the source file text (JSON text or HTML markup) and the raw
/// scenario JSON derived from it (used for diffing).
#[derive(Clone)]
pub struct Baseline {
    pub source: String,
    pub raw: Value,
}

pub type SharedBaselines = Arc<Mutex<HashMap<PathBuf, Baseline>>>;

/// The app-global baseline map.
pub fn baseline_slot() -> SharedBaselines {
    static SLOT: OnceLock<SharedBaselines> = OnceLock::new();
    SLOT.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
        .clone()
}

/// Insert a baseline for `path` if none exists yet (first open wins; watcher
/// reloads call this too and are no-ops). Skips null raw (unparseable source).
pub fn ensure_baseline(slot: &SharedBaselines, path: &Path, source: &str, raw: &Value) {
    if raw.is_null() {
        return;
    }
    let mut map = slot.lock().unwrap_or_else(|e| e.into_inner());
    map.entry(path.to_path_buf()).or_insert_with(|| Baseline {
        source: source.to_string(),
        raw: raw.clone(),
    });
}

/// Re-snapshot: overwrite the baseline for `path` ("Set baseline" button).
pub fn set_baseline(slot: &SharedBaselines, path: &Path, source: String, raw: Value) {
    let mut map = slot.lock().unwrap_or_else(|e| e.into_inner());
    map.insert(path.to_path_buf(), Baseline { source, raw });
}

/// Fetch the baseline for `path`, if any.
pub fn get_baseline(slot: &SharedBaselines, path: &Path) -> Option<Baseline> {
    let map = slot.lock().unwrap_or_else(|e| e.into_inner());
    map.get(path).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn local() -> SharedBaselines {
        Arc::new(Mutex::new(HashMap::new()))
    }

    #[test]
    fn ensure_keeps_the_original_snapshot() {
        let slot = local();
        let p = Path::new("/w/a.json");
        ensure_baseline(&slot, p, "v1", &json!({ "v": 1 }));
        // A later ensure (e.g. watcher reload after an edit) must not replace it.
        ensure_baseline(&slot, p, "v2", &json!({ "v": 2 }));
        let b = get_baseline(&slot, p).unwrap();
        assert_eq!(b.source, "v1");
        assert_eq!(b.raw, json!({ "v": 1 }));
    }

    #[test]
    fn set_baseline_overwrites() {
        let slot = local();
        let p = Path::new("/w/a.json");
        ensure_baseline(&slot, p, "v1", &json!(1));
        set_baseline(&slot, p, "v2".into(), json!(2));
        assert_eq!(get_baseline(&slot, p).unwrap().source, "v2");
    }

    #[test]
    fn null_raw_is_not_snapshotted_and_missing_is_none() {
        let slot = local();
        let p = Path::new("/w/broken.json");
        ensure_baseline(&slot, p, "{ bad", &Value::Null);
        assert!(get_baseline(&slot, p).is_none());
    }
}
