//! Scenario and scene variables (issue #329): a scalar declared once,
//! animated on the scenario's absolute timeline independently of any one
//! scene, and readable from any [`crate::expr`] expression as an ordinary
//! `$name`.
//!
//! # Why this exists
//!
//! An [`crate::expr::Expr`] has no memory of its own — every evaluation
//! starts from the same free variables the caller's [`crate::expr::Scope`]
//! happens to answer for that frame. That is fine for `$t`-driven motion
//! (a wiggle, a fade) but it cannot produce a value that keeps moving
//! *across* scene boundaries, or that several unrelated expressions need to
//! agree on simultaneously — a rotation every edge of a wireframe reads its
//! own opacity from, a draw-on progress a dozen strokes share, a counter
//! driving both a number and a bar's width. Asked how one of the reels this
//! chantier is chasing was built, its author described exactly this
//! indirection: GSAP animates a plain `{x, y, s, r}` object, and a function
//! reads it on every frame to produce attributes. This module is that
//! object: a named, animated scalar, resolved once per frame into a
//! [`crate::expr::Scope`] every expression can read.
//!
//! # Declaring a variable
//!
//! ```json
//! "vars": {
//!   "keyDraw": { "default": 0,
//!     "animation": [{ "at": "@4.85s", "to": 1, "duration": "3b", "easing": "ease_in_out" }] }
//! }
//! ```
//!
//! See [`VarDef`] and [`VarKeyframe`] for the full shape. `at` and
//! `duration` are [`crate::schema::time::TimePoint`] — the scenario's beat
//! grid, if it declares one (`bpm`/`beat_offset`), so a variable's tween
//! can land on a beat exactly like a scene cut can. A variable with an
//! empty (or absent) `animation` is a constant — see [`VarDef::is_static`]
//! and [`dynamic_names`] for how a loader is expected to fold it away
//! entirely, the same as any other literal.
//!
//! # Resolving it
//!
//! [`VarTable::compile`] turns a whole [`VarSet`] into cheap-to-sample
//! numbers once, at load; [`VarTable::value_at`] then answers any absolute
//! time in O(keyframes). See [`track`]'s module doc for the exact
//! before-first/after-last/gap rule and why a `"b"`-unit `duration` is
//! resolved differently from an `at`.
//!
//! # Reading it from an expression
//!
//! [`VarScope`] implements [`crate::expr::Scope`] over one or two compiled
//! [`VarTable`]s (scenario-level, and an optional scene-level one that
//! shadows it) at one instant `t`. See [`scope`]'s module doc for how it is
//! meant to compose with a node-reference-answering `Scope` (the
//! `engine::deps` side of this chantier) inside a single composite context,
//! and for why a scene-local variable referenced from another scene surfaces
//! as the ordinary "unknown identifier" error rather than a dedicated one.
//!
//! # What is *not* here
//!
//! This module owns none of: the `Scenario`/`Scene` fields that carry a
//! [`VarSet`] (`schema::scenario::Scenario`/`Scene` — see this crate's
//! `vars`-partition notes for the exact line to add), the per-frame
//! dependency graph that decides *when* to re-resolve a [`VarTable`] and
//! feeds its output into [`VarScope`] (`engine::deps`), or the loader change
//! that keeps an expression naming a [`dynamic_names`] variable from being
//! folded at load — `crates/rustmotion/src/loader.rs`'s
//! `fold_static_expressions` only knows the fixed `t`/`T`/`beat`/`duration`
//! list today (see [`crate::expr::Expr::is_static`]'s doc) and needs to
//! additionally consult [`dynamic_names`] before folding any expression, or
//! `"= $keyDraw * 360"` fails to load instead of surviving to the per-frame
//! tier. This module only guarantees the *information* — [`dynamic_names`]
//! — is there to consult.

pub mod schema;
pub mod scope;
pub mod track;

pub use schema::{dynamic_names, VarDef, VarKeyframe, VarSet};
pub use scope::VarScope;
pub use track::{VarTable, VarTrack, VarsError};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::Expr;
    use crate::schema::animation::EasingType;
    use crate::schema::time::{TimeCtx, TimePoint};
    use std::f64::consts::TAU;

    /// The acceptance test named in this workstream's brief: a `for-each`
    /// over edges, each edge's opacity an expression over a rotation
    /// variable, rendering as something that actually rotates. The
    /// `for-each`/JSON side of this can't be exercised yet — this crate's
    /// `vars` partition does not own `Scenario.vars` (see the module doc's
    /// "What is not here") — so this builds the same shape directly: a
    /// [`VarSet`] with one `rotY` variable, and eight per-edge opacity
    /// expressions, exactly what a `for-each` over 8 items would expand to
    /// once its own `$i`/`$count` are folded and only `$rotY` is left live.
    #[test]
    fn wireframe_edges_rotate_via_a_shared_variable() {
        let mut vars = VarSet::new();
        vars.insert(
            "rotY".to_string(),
            VarDef {
                default: 0.0,
                animation: vec![VarKeyframe {
                    at: TimePoint::Seconds(0.0),
                    to: TAU,
                    duration: TimePoint::Seconds(4.0),
                    easing: EasingType::Linear,
                }],
            },
        );
        let ctx = TimeCtx {
            bpm: None,
            beat_offset: 0.0,
            scene_start: 0.0,
        };
        let scenario = VarTable::compile(&vars, &ctx).unwrap();

        const EDGE_COUNT: usize = 8;
        // Per-edge opacity by (simulated) depth: a Y-rotation wireframe's
        // classic "front edges brighter than back edges" shading, exactly
        // the kind of `for-each`-generated, per-edge-static, cos(rotY + angle)
        // expression the issue's crystal example describes.
        // `rotY` is animated, so it belongs in `dynamic_names` — the signal
        // a loader must consult before folding any expression that reads
        // it (see this module's "What is not here" section: `Expr::is_static`
        // alone cannot tell `$rotY` apart from a genuine constant).
        assert_eq!(dynamic_names(&vars).collect::<Vec<_>>(), vec!["rotY"]);

        let edge_exprs: Vec<Expr> = (0..EDGE_COUNT)
            .map(|i| {
                let angle = i as f64 / EDGE_COUNT as f64 * TAU;
                Expr::parse(&format!("= (cos($rotY + {angle}) + 1) / 2")).unwrap()
            })
            .collect();

        let sample_times = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0];
        let mut frames: Vec<Vec<f64>> = Vec::new();
        for &t in &sample_times {
            let scope = VarScope::new(&scenario, None, t);

            let rot_y = scope.resolve("rotY").unwrap();
            let expected_rot_y = TAU * (t / 4.0).min(1.0);
            assert!((rot_y - expected_rot_y).abs() < 1e-9);

            let opacities: Vec<f64> = edge_exprs.iter().map(|e| e.eval(&scope).unwrap()).collect();
            for (i, &opacity) in opacities.iter().enumerate() {
                let angle = i as f64 / EDGE_COUNT as f64 * TAU;
                let expected = (f64::cos(rot_y + angle) + 1.0) / 2.0;
                assert!((opacity - expected).abs() < 1e-9);
            }
            frames.push(opacities);
        }

        // It actually rotates: consecutive frames inside the tween must
        // differ — a frozen wireframe would repeat the same opacities.
        for (i, pair) in frames.windows(2).enumerate() {
            if i < 4 {
                assert_ne!(
                    pair[0],
                    pair[1],
                    "wireframe did not move between sampled frames {i} and {}",
                    i + 1
                );
            }
        }
        // After the last keyframe (t=4) it holds at TAU, matching "Value
        // outside the keyframes" in `track`'s module doc — not a wrap-around
        // back to `default`.
        assert_eq!(
            frames[4], frames[5],
            "must hold, not reset, past the last keyframe"
        );
    }

    /// Deliverable 5: an un-animated variable folds like any other literal.
    /// `Expr::is_static` only special-cases the fixed `t`/`T`/`beat`/`duration`
    /// names (frozen in `expr::eval::is_dynamic_var_name`) — an arbitrary
    /// `$name` it has never heard of is, by its own contract, eligible to
    /// fold already. `dynamic_names` is the other half: the set a loader
    /// must additionally treat as non-foldable for the *animated* ones,
    /// since `Expr::is_static` alone cannot tell `$constant` and `$rotY`
    /// apart.
    #[test]
    fn unanimated_variable_is_classified_static_and_folds() {
        let mut vars = VarSet::new();
        vars.insert(
            "constant".to_string(),
            VarDef {
                default: 7.0,
                animation: Vec::new(),
            },
        );
        vars.insert(
            "rotY".to_string(),
            VarDef {
                default: 0.0,
                animation: vec![VarKeyframe {
                    at: TimePoint::Seconds(0.0),
                    to: 1.0,
                    duration: TimePoint::Seconds(1.0),
                    easing: EasingType::Linear,
                }],
            },
        );

        let dynamic: Vec<&str> = dynamic_names(&vars).collect();
        assert_eq!(dynamic, vec!["rotY"]);

        // `Expr::is_static` itself, unaware of `vars`, already agrees a
        // bare `$constant` reference is fold-eligible.
        let constant_expr = Expr::parse("= $constant * 2").unwrap();
        assert!(constant_expr.is_static());

        let ctx = TimeCtx {
            bpm: None,
            beat_offset: 0.0,
            scene_start: 0.0,
        };
        let table = VarTable::compile(&vars, &ctx).unwrap();
        let scope = VarScope::new(&table, None, 0.0);
        assert_eq!(constant_expr.eval(&scope).unwrap(), 14.0);
    }
}
