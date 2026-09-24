use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use rustmotion::encode::video::FrameTask;
use rustmotion::schema::ResolvedScenario;

use crate::scenario::{baseline_slot, get_baseline};

use super::diff_panel::DiffSide;
use super::frames::{baseline_arcs, render_frame};

pub const CACHE_CAP: usize = 120;
pub const WINDOW_AHEAD: u32 = 30;
pub const WINDOW_BEHIND: u32 = 5;

pub const DEFAULT_PREVIEW_SCALE_PCT: u16 = 50;
pub const PREVIEW_SCALE_CHOICES: [u16; 4] = [100, 75, 50, 25];

static PREVIEW_SCALE_PCT: AtomicU16 = AtomicU16::new(DEFAULT_PREVIEW_SCALE_PCT);

pub fn preview_scale_pct() -> u16 {
    PREVIEW_SCALE_PCT.load(Ordering::Relaxed)
}

pub fn set_preview_scale_pct(pct: u16) {
    PREVIEW_SCALE_PCT.store(pct.clamp(10, 100), Ordering::Relaxed);
}

pub fn scale_factor(pct: u16) -> f32 {
    pct as f32 / 100.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameKey {
    pub generation: u64,
    pub side: DiffSide,
    pub frame: u32,
    pub scale_pct: u16,
}

pub fn prefetch_window(current: u32, playing: bool, total: u32) -> Vec<u32> {
    if total == 0 {
        return Vec::new();
    }
    let current = current.min(total - 1);
    let behind = if playing { 0 } else { WINDOW_BEHIND };
    let lo = current.saturating_sub(behind);
    let hi = current.saturating_add(WINDOW_AHEAD).min(total - 1);

    let mut out = Vec::with_capacity((hi - lo + 1) as usize);
    out.push(current);
    for d in 1..=(WINDOW_AHEAD.max(behind)) {
        let fwd = current.saturating_add(d);
        if fwd <= hi {
            out.push(fwd);
        }
        if d <= behind && current >= d && current - d >= lo {
            out.push(current - d);
        }
    }
    out
}

pub fn select_evictions(
    keys: &[FrameKey],
    gen_b: Option<u64>,
    gen_a: Option<u64>,
    head: u32,
    cap: usize,
    scale_pct: u16,
) -> Vec<FrameKey> {
    let is_stale = |k: &FrameKey| {
        k.scale_pct != scale_pct
            || match k.side {
                DiffSide::B => gen_b.is_some_and(|g| k.generation != g),
                DiffSide::A => gen_a.is_some_and(|g| k.generation != g),
            }
    };

    let mut evict: Vec<FrameKey> = keys.iter().filter(|k| is_stale(k)).copied().collect();

    let mut fresh: Vec<FrameKey> = keys.iter().filter(|k| !is_stale(k)).copied().collect();
    if fresh.len() > cap {
        fresh.sort_by_key(|k| std::cmp::Reverse(k.frame.abs_diff(head)));
        evict.extend(fresh.drain(..fresh.len() - cap));
    }
    evict
}

#[derive(Default)]
pub struct FrameCache {
    map: HashMap<FrameKey, Arc<Vec<u8>>>,
}

impl FrameCache {
    pub fn get(&self, key: &FrameKey) -> Option<Arc<Vec<u8>>> {
        self.map.get(key).cloned()
    }

    pub fn contains(&self, key: &FrameKey) -> bool {
        self.map.contains_key(key)
    }

    #[cfg(test)]
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn insert(
        &mut self,
        key: FrameKey,
        bytes: Vec<u8>,
        gen_b: Option<u64>,
        gen_a: Option<u64>,
        head: u32,
        scale_pct: u16,
    ) {
        self.map.insert(key, Arc::new(bytes));
        let keys: Vec<FrameKey> = self.map.keys().copied().collect();
        for k in select_evictions(&keys, gen_b, gen_a, head, CACHE_CAP, scale_pct) {
            self.map.remove(&k);
        }
    }
}

pub type SharedFrameCache = Arc<Mutex<FrameCache>>;

pub fn frame_cache() -> SharedFrameCache {
    static SLOT: OnceLock<SharedFrameCache> = OnceLock::new();
    SLOT.get_or_init(|| Arc::new(Mutex::new(FrameCache::default())))
        .clone()
}

#[derive(Default)]
pub struct ClaimSet {
    set: HashSet<FrameKey>,
}

impl ClaimSet {
    pub fn try_claim(&mut self, key: FrameKey) -> bool {
        self.set.insert(key)
    }

    pub fn release(&mut self, key: &FrameKey) {
        self.set.remove(key);
    }
}

fn claims() -> &'static Mutex<ClaimSet> {
    static SLOT: OnceLock<Mutex<ClaimSet>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(ClaimSet::default()))
}

pub const MAX_RENDER_ATTEMPTS: u8 = 2;

#[derive(Default)]
pub struct FailLedger {
    map: HashMap<FrameKey, u8>,
}

impl FailLedger {
    pub fn record_failure(&mut self, key: FrameKey) {
        if self.map.len() > 4096 {
            self.map.clear();
        }
        *self.map.entry(key).or_insert(0) += 1;
    }

    pub fn exhausted(&self, key: &FrameKey) -> bool {
        self.map.get(key).is_some_and(|&n| n >= MAX_RENDER_ATTEMPTS)
    }
}

pub(crate) fn fail_ledger() -> &'static Mutex<FailLedger> {
    static SLOT: OnceLock<Mutex<FailLedger>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(FailLedger::default()))
}

pub fn worker_count(cores: usize) -> usize {
    cores.saturating_sub(2).clamp(2, 6)
}

#[derive(Clone)]
pub struct PrefetchTarget {
    pub current: u32,
    pub playing: bool,
    pub generation: u64,
    pub side: DiffSide,
    pub scenario: Option<Arc<ResolvedScenario>>,
    pub tasks: Option<Arc<Vec<FrameTask>>>,
    pub path: Option<PathBuf>,
}

impl Default for PrefetchTarget {
    fn default() -> Self {
        Self {
            current: 0,
            playing: false,
            generation: 0,
            side: DiffSide::B,
            scenario: None,
            tasks: None,
            path: None,
        }
    }
}

fn prefetch_slot() -> Arc<Mutex<PrefetchTarget>> {
    static SLOT: OnceLock<Arc<Mutex<PrefetchTarget>>> = OnceLock::new();
    SLOT.get_or_init(|| Arc::new(Mutex::new(PrefetchTarget::default())))
        .clone()
}

pub fn publish_target(target: PrefetchTarget) {
    *prefetch_slot().lock().unwrap_or_else(|e| e.into_inner()) = target;
}

fn target_fingerprint(t: &PrefetchTarget, scale_pct: u16) -> (u32, bool, u64, DiffSide, u16) {
    (t.current, t.playing, t.generation, t.side, scale_pct)
}

pub fn ensure_prefetcher() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        for _ in 0..worker_count(cores) {
            let _ = std::thread::Builder::new()
                .stack_size(crate::editor::frames::RENDER_STACK)
                .spawn(prefetch_loop);
        }
    });
}

fn prefetch_loop() {
    loop {
        std::thread::sleep(Duration::from_millis(12));
        let target = prefetch_slot()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();

        let (gen, scenario, tasks) = match target.side {
            DiffSide::B => match (&target.scenario, &target.tasks) {
                (Some(s), Some(t)) => (target.generation, s.clone(), t.clone()),
                _ => continue,
            },
            DiffSide::A => {
                let Some(path) = target.path.as_deref() else {
                    continue;
                };
                let Some(b) = get_baseline(&baseline_slot(), path) else {
                    continue;
                };
                match baseline_arcs(path, &b.source) {
                    Ok((hash, s, t)) => (hash, s, t),
                    Err(_) => continue,
                }
            }
        };

        let total = tasks.len() as u32;
        let window = prefetch_window(target.current, target.playing, total);
        let scale = preview_scale_pct();
        let fingerprint = target_fingerprint(&target, scale);
        let (gen_b, gen_a) = match target.side {
            DiffSide::B => (Some(gen), None),
            DiffSide::A => (Some(target.generation), Some(gen)),
        };

        for frame in window {
            let key = FrameKey {
                generation: gen,
                side: target.side,
                frame,
                scale_pct: scale,
            };
            if frame_cache()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains(&key)
            {
                continue;
            }
            if fail_ledger()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .exhausted(&key)
            {
                continue;
            }
            if !claims()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .try_claim(key)
            {
                continue;
            }
            let already = frame_cache()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains(&key);
            if !already {
                let rendered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    render_frame(&scenario, &tasks, frame, scale_factor(scale))
                }));
                match rendered {
                    Ok(bytes) => frame_cache()
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(key, bytes, gen_b, gen_a, target.current, scale),
                    Err(_) => fail_ledger()
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .record_failure(key),
                }
            }
            claims()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .release(&key);

            let now = prefetch_slot()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            if target_fingerprint(&now, preview_scale_pct()) != fingerprint {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(gen: u64, side: DiffSide, frame: u32) -> FrameKey {
        FrameKey {
            generation: gen,
            side,
            frame,
            scale_pct: 100,
        }
    }

    #[test]
    fn window_playing_is_forward_only_ascending() {
        let w = prefetch_window(10, true, 1000);
        assert_eq!(w.len(), (WINDOW_AHEAD + 1) as usize);
        assert_eq!(w.first(), Some(&10));
        assert_eq!(w.last(), Some(&(10 + WINDOW_AHEAD)));
        assert!(
            w.windows(2).all(|p| p[0] < p[1]),
            "ascending = nearest-first"
        );
    }

    #[test]
    fn window_paused_is_nearest_first_with_backtrack() {
        let w = prefetch_window(10, false, 1000);
        assert_eq!(w.len(), (WINDOW_AHEAD + WINDOW_BEHIND + 1) as usize);
        assert_eq!(&w[0..5], &[10, 11, 9, 12, 8]);
        assert!(w.contains(&5) && w.contains(&40));
        assert!(!w.contains(&4) && !w.contains(&41));
    }

    #[test]
    fn window_clamps_at_start() {
        let w = prefetch_window(2, false, 1000);
        assert!(w.iter().all(|&f| f <= 2 + WINDOW_AHEAD));
        assert!(w.contains(&0) && w.contains(&1) && w.contains(&2));
        assert_eq!(
            w.len(),
            (WINDOW_AHEAD + 1 + 2) as usize,
            "only 2 back frames exist"
        );
    }

    #[test]
    fn window_clamps_at_end() {
        let w = prefetch_window(99, true, 100);
        assert_eq!(w, vec![99]);
        let w = prefetch_window(500, true, 100);
        assert!(w.iter().all(|&f| f < 100));
    }

    #[test]
    fn window_empty_when_no_frames() {
        assert!(prefetch_window(0, true, 0).is_empty());
        assert!(prefetch_window(10, false, 0).is_empty());
    }

    #[test]
    fn evictions_drop_stale_generations_first() {
        let keys = vec![
            key(1, DiffSide::B, 0),
            key(2, DiffSide::B, 5),
            key(7, DiffSide::A, 5),
            key(9, DiffSide::A, 6),
        ];
        let out = select_evictions(&keys, Some(2), Some(9), 5, 100, 100);
        assert!(out.contains(&key(1, DiffSide::B, 0)));
        assert!(out.contains(&key(7, DiffSide::A, 5)));
        assert_eq!(out.len(), 2, "fresh entries stay under cap");
    }

    #[test]
    fn evictions_unknown_side_a_generation_keeps_a_entries() {
        let keys = vec![key(7, DiffSide::A, 5), key(2, DiffSide::B, 5)];
        let out = select_evictions(&keys, Some(2), None, 5, 100, 100);
        assert!(out.is_empty(), "A staleness unknowable without gen_a");
    }

    #[test]
    fn evictions_then_farthest_from_head() {
        let keys: Vec<FrameKey> = [10u32, 11, 9, 30, 50, 12]
            .iter()
            .map(|&f| key(1, DiffSide::B, f))
            .collect();
        let out = select_evictions(&keys, Some(1), None, 10, 4, 100);
        assert_eq!(out.len(), 2);
        assert!(out.contains(&key(1, DiffSide::B, 50)));
        assert!(out.contains(&key(1, DiffSide::B, 30)));
    }

    #[test]
    fn evictions_stale_then_distance_combined() {
        let mut keys: Vec<FrameKey> = (0..5).map(|f| key(1, DiffSide::B, f)).collect();
        keys.extend((0..6).map(|f| key(2, DiffSide::B, f * 10)));
        let out = select_evictions(&keys, Some(2), None, 0, 4, 100);
        assert_eq!(out.len(), 7);
        assert!(out.contains(&key(2, DiffSide::B, 50)));
        assert!(out.contains(&key(2, DiffSide::B, 40)));
        assert!(!out.contains(&key(2, DiffSide::B, 0)));
    }

    #[test]
    fn evictions_drop_other_scale_entries() {
        let old = FrameKey {
            scale_pct: 100,
            ..key(1, DiffSide::B, 5)
        };
        let fresh = FrameKey {
            scale_pct: 50,
            ..key(1, DiffSide::B, 5)
        };
        let out = select_evictions(&[old, fresh], Some(1), None, 5, 100, 50);
        assert_eq!(out, vec![old], "same frame at the old scale is stale");
    }

    #[test]
    fn cache_roundtrip_and_side_separation() {
        let mut c = FrameCache::default();
        c.insert(key(1, DiffSide::B, 3), vec![0xB], Some(1), None, 3, 100);
        c.insert(key(1, DiffSide::A, 3), vec![0xA], None, Some(1), 3, 100);
        assert_eq!(c.get(&key(1, DiffSide::B, 3)).unwrap().as_slice(), &[0xB]);
        assert_eq!(c.get(&key(1, DiffSide::A, 3)).unwrap().as_slice(), &[0xA]);
        assert!(
            c.get(&key(2, DiffSide::B, 3)).is_none(),
            "generation is part of the key"
        );
    }

    #[test]
    fn cache_stays_bounded_and_keeps_nearest() {
        let mut c = FrameCache::default();
        for f in 0..(CACHE_CAP as u32 + 20) {
            c.insert(key(1, DiffSide::B, f), vec![0], Some(1), None, 0, 100);
        }
        assert!(c.len() <= CACHE_CAP);
        assert!(
            c.contains(&key(1, DiffSide::B, 0)),
            "nearest to head survives"
        );
        assert!(
            !c.contains(&key(1, DiffSide::B, CACHE_CAP as u32 + 19)),
            "farthest evicted"
        );
    }

    #[test]
    fn cache_purges_old_generation_on_insert() {
        let mut c = FrameCache::default();
        for f in 0..10 {
            c.insert(key(1, DiffSide::B, f), vec![0], Some(1), None, 0, 100);
        }
        c.insert(key(2, DiffSide::B, 0), vec![0], Some(2), None, 0, 100);
        assert_eq!(c.len(), 1);
        assert!(c.contains(&key(2, DiffSide::B, 0)));
    }

    #[test]
    fn cache_purges_old_scale_on_insert() {
        let mut c = FrameCache::default();
        for f in 0..10 {
            c.insert(key(1, DiffSide::B, f), vec![0], Some(1), None, 0, 100);
        }
        let half = FrameKey {
            scale_pct: 50,
            ..key(1, DiffSide::B, 0)
        };
        c.insert(half, vec![0], Some(1), None, 0, 50);
        assert_eq!(c.len(), 1);
        assert!(c.contains(&half));
    }

    #[test]
    fn claim_is_exclusive_until_released() {
        let mut claims = ClaimSet::default();
        let k = key(1, DiffSide::B, 7);
        assert!(claims.try_claim(k), "first claim wins");
        assert!(!claims.try_claim(k), "second claim loses while held");
        claims.release(&k);
        assert!(claims.try_claim(k), "claimable again after release");
    }

    #[test]
    fn claims_are_per_key() {
        let mut claims = ClaimSet::default();
        assert!(claims.try_claim(key(1, DiffSide::B, 7)));
        assert!(claims.try_claim(key(1, DiffSide::B, 8)), "other frame");
        assert!(claims.try_claim(key(1, DiffSide::A, 7)), "other side");
        let other_scale = FrameKey {
            scale_pct: 50,
            ..key(1, DiffSide::B, 7)
        };
        assert!(claims.try_claim(other_scale), "other scale");
    }

    #[test]
    #[ignore]
    fn soak_full_pipeline_rss() {
        use crate::editor::frames::frame_hits;
        use std::sync::atomic::{AtomicU32, Ordering};

        let Ok(src) = std::fs::read_to_string("../../examples/dynamic-glass.json") else {
            eprintln!("skipped: examples/dynamic-glass.json not present");
            return;
        };
        let scenario =
            Arc::new(rustmotion::loader::load_scenario_from_source(None, Some(&src)).unwrap());
        let tasks = Arc::new(rustmotion::encode::build_frame_tasks(&scenario));
        let total = tasks.len() as u32;
        let rss_mb = || -> u64 {
            let out = std::process::Command::new("ps")
                .args(["-o", "rss=", "-p", &std::process::id().to_string()])
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout)
                .trim()
                .parse::<u64>()
                .unwrap()
                / 1024
        };

        ensure_prefetcher();
        static SERVED: AtomicU32 = AtomicU32::new(0);
        static MISSES: AtomicU32 = AtomicU32::new(0);

        let s2 = scenario.clone();
        let t2 = tasks.clone();
        let sim = std::thread::spawn(move || {
            let mut current = 0u32;
            for _tick in 0..(30 * 60) {
                std::thread::sleep(Duration::from_millis(33));
                current = (current + 1) % total;
                *prefetch_slot().lock().unwrap_or_else(|e| e.into_inner()) = PrefetchTarget {
                    current,
                    playing: true,
                    generation: 1,
                    side: DiffSide::B,
                    scenario: Some(s2.clone()),
                    tasks: Some(t2.clone()),
                    path: None,
                };
                let key = FrameKey {
                    generation: 1,
                    side: DiffSide::B,
                    frame: current,
                    scale_pct: preview_scale_pct(),
                };
                let hit = frame_cache()
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .contains(&key);
                if !hit {
                    MISSES.fetch_add(1, Ordering::Relaxed);
                    let bytes = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        crate::editor::frames::render_frame(
                            &s2,
                            &t2,
                            current,
                            scale_factor(key.scale_pct),
                        )
                    }));
                    if let Ok(b) = bytes {
                        frame_cache()
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .insert(key, b, Some(1), None, current, key.scale_pct);
                    }
                }
                SERVED.fetch_add(1, Ordering::Relaxed);
                let _ = frame_hits(&s2, &t2, current, "/scenes/0");
            }
        });

        while !sim.is_finished() {
            std::thread::sleep(Duration::from_secs(5));
            println!(
                "rss={} MB served={} misses={}",
                rss_mb(),
                SERVED.load(Ordering::Relaxed),
                MISSES.load(Ordering::Relaxed)
            );
        }
        sim.join().unwrap();
        println!(
            "end rss={} MB misses={}",
            rss_mb(),
            MISSES.load(Ordering::Relaxed)
        );
    }

    #[test]
    fn fail_ledger_exhausts_after_max_attempts() {
        let mut ledger = FailLedger::default();
        let k = key(1, DiffSide::B, 7);
        assert!(!ledger.exhausted(&k));
        ledger.record_failure(k);
        assert!(!ledger.exhausted(&k), "one transient failure gets a retry");
        ledger.record_failure(k);
        assert!(ledger.exhausted(&k), "gives up after MAX_RENDER_ATTEMPTS");
        assert!(
            !ledger.exhausted(&key(2, DiffSide::B, 7)),
            "a new generation retries the same frame"
        );
    }

    #[test]
    fn worker_count_leaves_ui_cores_and_stays_bounded() {
        assert_eq!(worker_count(1), 2, "floor: two workers even on tiny CPUs");
        assert_eq!(worker_count(4), 2);
        assert_eq!(worker_count(8), 6);
        assert_eq!(worker_count(12), 6, "cap at six");
    }

    #[test]
    fn publish_target_overwrites_the_slot() {
        let target = PrefetchTarget {
            current: 42,
            playing: true,
            generation: 7,
            side: DiffSide::A,
            scenario: None,
            tasks: None,
            path: Some(PathBuf::from("/tmp/published.json")),
        };
        publish_target(target);
        let published = prefetch_slot()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        assert_eq!(published.current, 42);
        assert!(published.playing);
        assert_eq!(published.generation, 7);
        assert_eq!(published.side, DiffSide::A);
        assert_eq!(published.path, Some(PathBuf::from("/tmp/published.json")));
        publish_target(PrefetchTarget::default());
    }

    #[test]
    fn scale_setter_clamps_and_factor_converts() {
        set_preview_scale_pct(50);
        assert_eq!(preview_scale_pct(), 50);
        set_preview_scale_pct(0);
        assert_eq!(preview_scale_pct(), 10, "clamped to the floor");
        set_preview_scale_pct(200);
        assert_eq!(preview_scale_pct(), 100, "clamped to full resolution");
        assert_eq!(scale_factor(50), 0.5);
        assert_eq!(scale_factor(100), 1.0);
        set_preview_scale_pct(DEFAULT_PREVIEW_SCALE_PCT);
    }
}
