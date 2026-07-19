//! Undo/redo history for studio edits.
//!
//! One history entry = one effective disk write (post-debounce), capturing the
//! full file text as it was BEFORE the write — uniform for JSON scenarios and
//! HTML sources. Undo/redo rewrite the source file; the watcher reload then
//! refreshes the model (reloads never push history, so an undo-induced reload
//! is not captured as a new edit).
//!
//! The stacks live in an app-global slot ([`history_slot`]) separate from
//! `StudioModel`, so they survive the model replacement a watcher reload
//! performs. Switching to a different file resets them lazily (the slot tracks
//! which path it belongs to).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use super::Shared;

/// Maximum retained undo entries; the oldest is evicted beyond this.
pub const HISTORY_CAP: usize = 64;

/// Pure bounded undo/redo stacks over full-file snapshots.
#[derive(Default)]
pub struct History {
    undo: Vec<String>,
    redo: Vec<String>,
}

impl History {
    /// Record the pre-write state of the file as a new undo entry. Clears the
    /// redo stack (a new edit after an undo invalidates the redone future) and
    /// evicts the oldest entry beyond [`HISTORY_CAP`].
    pub fn record(&mut self, snapshot: String) {
        self.undo.push(snapshot);
        if self.undo.len() > HISTORY_CAP {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Pop the previous state, pushing `current` onto the redo stack. Returns
    /// `None` (and leaves both stacks untouched) when there is nothing to undo.
    pub fn undo(&mut self, current: String) -> Option<String> {
        let prev = self.undo.pop()?;
        self.redo.push(current);
        Some(prev)
    }

    /// Pop the next state, pushing `current` onto the undo stack. Returns
    /// `None` (and leaves both stacks untouched) when there is nothing to redo.
    pub fn redo(&mut self, current: String) -> Option<String> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

/// The slot state: which file the stacks belong to, the stacks, and whether a
/// debounced write is pending ("Saving…" indicator).
#[derive(Default)]
pub struct HistoryState {
    pub path: Option<PathBuf>,
    pub history: History,
    pub saving: bool,
}

impl HistoryState {
    /// Reset the stacks when the edited file changes (lazy file-switch reset).
    pub fn ensure_path(&mut self, path: &Path) {
        if self.path.as_deref() != Some(path) {
            self.path = Some(path.to_path_buf());
            self.history = History::default();
        }
    }
}

pub type SharedHistory = Arc<Mutex<HistoryState>>;

/// The app-global history slot. Global (not in `StudioModel`) so the stacks
/// survive watcher reloads, which replace the model wholesale.
pub fn history_slot() -> SharedHistory {
    static SLOT: OnceLock<SharedHistory> = OnceLock::new();
    SLOT.get_or_init(|| Arc::new(Mutex::new(HistoryState::default())))
        .clone()
}

/// Record a pre-write snapshot for `path` (called by the write paths after a
/// successful, effective disk write).
pub fn record_edit(slot: &SharedHistory, path: &Path, snapshot: String) {
    let mut st = slot.lock().unwrap_or_else(|e| e.into_inner());
    st.ensure_path(path);
    st.history.record(snapshot);
}

/// Set the pending-write ("Saving…") indicator.
pub fn set_saving(slot: &SharedHistory, saving: bool) {
    let mut st = slot.lock().unwrap_or_else(|e| e.into_inner());
    st.saving = saving;
}

/// Undo the last edit: write the previous file state back to disk. The watcher
/// reload does the rest. Write failures are surfaced via the model's
/// `write_error` (same guarantees as every studio write) and roll the stacks
/// back so the step isn't lost.
pub fn undo(shared: &Shared, slot: &SharedHistory) {
    step(shared, slot, true)
}

/// Redo the last undone edit (see [`undo`]).
pub fn redo(shared: &Shared, slot: &SharedHistory) {
    step(shared, slot, false)
}

fn step(shared: &Shared, slot: &SharedHistory, is_undo: bool) {
    // Lock discipline: the model lock and the slot lock are never held
    // together (model → path, then slot → stacks + disk, then model → report).
    let path = {
        let m = shared.lock().unwrap_or_else(|e| e.into_inner());
        m.path.clone()
    };
    let Some(path) = path else {
        return;
    };

    // Ok(true) = a state was written; Ok(false) = nothing to undo/redo.
    let outcome: Result<bool, String> = {
        let mut st = slot.lock().unwrap_or_else(|e| e.into_inner());
        st.ensure_path(&path);
        (|| {
            let current = std::fs::read_to_string(&path).map_err(|e| format!("read: {e}"))?;
            let target = if is_undo {
                st.history.undo(current)
            } else {
                st.history.redo(current)
            };
            let Some(target) = target else {
                return Ok(false);
            };
            match std::fs::write(&path, &target) {
                Ok(()) => {
                    // Record the self-write so the watcher skips the reload,
                    // and adopt the restored state in memory ourselves. If
                    // adoption fails (shouldn't: disk states were valid when
                    // captured), clear the note so the watcher reloads
                    // normally instead of leaving stale memory.
                    super::optimistic::note_self_write(
                        &super::optimistic::self_write_slot(),
                        &path,
                        &target,
                    );
                    Ok(true)
                }
                Err(e) => {
                    // Roll the stacks back so the failed step isn't lost: the
                    // inverse operation with `target` restores both stacks
                    // exactly (the popped entry returns, `current` comes back
                    // off the other stack and is discarded).
                    if is_undo {
                        let _ = st.history.redo(target);
                    } else {
                        let _ = st.history.undo(target);
                    }
                    Err(format!("write: {e}"))
                }
            }
        })()
    };

    match outcome {
        Ok(true) => {
            // Adopt the restored state in memory (the watcher will skip the
            // self-write). Failure → clear the note so the watcher reloads.
            let disk = std::fs::read_to_string(&path).unwrap_or_default();
            if super::optimistic::adopt_source(shared, &path, &disk).is_err() {
                super::optimistic::clear_self_write(&super::optimistic::self_write_slot(), &path);
            }
            let mut m = shared.lock().unwrap_or_else(|e| e.into_inner());
            m.write_error = None;
        }
        Ok(false) => {}
        Err(e) => {
            let mut m = shared.lock().unwrap_or_else(|e2| e2.into_inner());
            m.write_error = Some(e);
            m.generation = m.generation.wrapping_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{empty_scenario, StudioModel};

    // ── Pure stack behavior ─────────────────────────────────────────────

    #[test]
    fn record_caps_at_64_evicting_oldest() {
        let mut h = History::default();
        for i in 0..(HISTORY_CAP + 5) {
            h.record(format!("s{i}"));
        }
        // Cap holds; undoing all the way lands on the oldest KEPT entry (s5).
        let mut last = None;
        let mut current = "top".to_string();
        while let Some(s) = h.undo(current.clone()) {
            current = s.clone();
            last = Some(s);
        }
        assert_eq!(last.as_deref(), Some("s5"));
    }

    #[test]
    fn record_clears_redo() {
        let mut h = History::default();
        h.record("s0".into());
        assert!(h.undo("s1".into()).is_some());
        assert!(h.can_redo());
        h.record("s2".into());
        assert!(!h.can_redo(), "a new edit after undo clears redo");
    }

    #[test]
    fn undo_empty_is_noop_and_does_not_touch_redo() {
        let mut h = History::default();
        assert_eq!(h.undo("current".into()), None);
        assert!(!h.can_redo(), "failed undo must not push to redo");
        assert_eq!(h.redo("current".into()), None);
        assert!(!h.can_undo(), "failed redo must not push to undo");
    }

    #[test]
    fn undo_redo_roundtrip_restores() {
        let mut h = History::default();
        h.record("v1".into());
        let prev = h.undo("v2".into()).unwrap();
        assert_eq!(prev, "v1");
        assert!(h.can_redo());
        let next = h.redo(prev).unwrap();
        assert_eq!(next, "v2");
        assert!(h.can_undo());
        assert!(!h.can_redo());
    }

    #[test]
    fn ensure_path_resets_on_file_switch() {
        let mut st = HistoryState::default();
        st.ensure_path(Path::new("/a.json"));
        st.history.record("s".into());
        assert!(st.history.can_undo());
        // Same path: stacks kept.
        st.ensure_path(Path::new("/a.json"));
        assert!(st.history.can_undo());
        // Different path: stacks reset.
        st.ensure_path(Path::new("/b.json"));
        assert!(!st.history.can_undo());
        assert!(!st.history.can_redo());
    }

    // ── Model-level integration (no UI) ─────────────────────────────────

    fn temp_file(tag: &str, content: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("rm_history_{tag}_{}.json", std::process::id()));
        std::fs::write(&p, content).unwrap();
        p
    }

    fn model_for(path: &Path) -> Shared {
        Arc::new(Mutex::new(StudioModel::new(
            empty_scenario(),
            None,
            Some(path.to_path_buf()),
        )))
    }

    fn local_slot() -> SharedHistory {
        Arc::new(Mutex::new(HistoryState::default()))
    }

    #[test]
    fn undo_rewrites_file_and_redo_restores() {
        let path = temp_file("roundtrip", "STATE_B");
        let shared = model_for(&path);
        let slot = local_slot();

        // The edit that produced STATE_B recorded the pre-write state.
        record_edit(&slot, &path, "STATE_A".into());

        undo(&shared, &slot);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "STATE_A");
        assert!(slot.lock().unwrap().history.can_redo());

        redo(&shared, &slot);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "STATE_B");
        assert!(slot.lock().unwrap().history.can_undo());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn undo_with_empty_history_is_noop() {
        let path = temp_file("noop", "UNTOUCHED");
        let shared = model_for(&path);
        let slot = local_slot();

        undo(&shared, &slot);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "UNTOUCHED");
        let m = shared.lock().unwrap();
        assert!(m.write_error.is_none());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn record_edit_resets_on_path_change() {
        let a = temp_file("switch_a", "A");
        let b = temp_file("switch_b", "B");
        let slot = local_slot();

        record_edit(&slot, &a, "old_a".into());
        assert!(slot.lock().unwrap().history.can_undo());

        // Recording for another file drops the previous file's stacks.
        record_edit(&slot, &b, "old_b".into());
        let st = slot.lock().unwrap();
        assert_eq!(st.path.as_deref(), Some(b.as_path()));
        // Only b's single entry remains: one undo possible, then empty.
        drop(st);
        let shared = model_for(&b);
        undo(&shared, &slot);
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "old_b");
        undo(&shared, &slot); // empty now → no-op
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "old_b");

        let _ = std::fs::remove_file(&a);
        let _ = std::fs::remove_file(&b);
    }
}
