//! [`VarScope`]: the [`Scope`] implementation an expression actually reads
//! `$name` through, composing a scenario-wide [`VarTable`] with an optional
//! scene-level one that shadows it.

use crate::expr::Scope;

use super::track::VarTable;

/// Reads declared scenario/scene variables for one absolute instant `t`.
///
/// Scene-level shadows scenario-level, by name: a scene that redeclares a
/// scenario variable's name gets its own value for that name inside that
/// scene, and the scenario's is invisible there (not summed, not merged —
/// entirely replaced).
///
/// # Scene isolation
///
/// A [`VarScope`] built for scene B never holds scene A's table, so an
/// expression in scene A that names a variable declared only in scene B's
/// `vars` gets `None` back from [`VarScope::var`] — exactly what it would
/// get for a name that was never declared anywhere.
/// [`Scope::var`](crate::expr::Scope::var)'s own doc already treats those
/// two cases as indistinguishable ("`None` means not defined in this
/// scope" turns into the same [`crate::expr::ExprError::UnknownIdent`] a
/// genuinely unknown name produces), which is what makes a cross-scene
/// variable reference surface as the same error a cross-scene node
/// reference does — this module does not need to special-case it, only to
/// never construct a [`VarScope`] that can see another scene's table.
///
/// # Composing with `node_prop`
///
/// This type answers [`Scope::var`] only; [`Scope::node_prop`] keeps its
/// default `None`. A caller that also needs node references (see
/// `engine::deps`) does not wrap a [`VarScope`] inside another `Scope` impl
/// — a `Scope` is consumed behind `&dyn Scope`, and trait objects don't
/// compose that way — it instead holds a [`VarScope`] (or the two
/// [`VarTable`]s and a `t`, if that is more convenient for its own
/// lifetimes) as a field alongside its node lookups, and its own `var`
/// implementation tries [`VarScope::resolve`] first, falling back to
/// whatever else it answers for. [`VarScope::resolve`] is exposed as a
/// plain method — not only reachable through the `Scope` impl — precisely
/// so it can be called that way without going through a trait object:
///
/// ```
/// use rustmotion_core::expr::Scope;
/// use rustmotion_core::vars::VarScope;
///
/// struct EngineScope<'a> {
///     vars: VarScope<'a>,
///     // ... node lookups owned elsewhere ...
/// }
///
/// impl Scope for EngineScope<'_> {
///     fn var(&self, name: &str) -> Option<f64> {
///         self.vars.resolve(name) /* .or_else(|| self.node_derived(name)) */
///     }
///
///     fn node_prop(&self, id: &str, prop: &str) -> Option<f64> {
///         let _ = (id, prop);
///         None // delegate to the node-dependency graph here
///     }
/// }
/// ```
pub struct VarScope<'a> {
    scenario: &'a VarTable,
    scene: Option<&'a VarTable>,
    t: f64,
}

impl<'a> VarScope<'a> {
    /// `scenario` is visible everywhere; `scene`, when given, shadows it by
    /// name for expressions evaluated inside that one scene. `t` is the
    /// absolute scenario time (seconds) this scope answers for.
    pub fn new(scenario: &'a VarTable, scene: Option<&'a VarTable>, t: f64) -> Self {
        VarScope { scenario, scene, t }
    }

    /// Resolve `name`, scene table first. Usable directly, without going
    /// through the [`Scope`] trait object — see the composing note above.
    pub fn resolve(&self, name: &str) -> Option<f64> {
        self.scene
            .and_then(|scene| scene.value_at(name, self.t))
            .or_else(|| self.scenario.value_at(name, self.t))
    }
}

impl Scope for VarScope<'_> {
    fn var(&self, name: &str) -> Option<f64> {
        self.resolve(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::Expr;
    use crate::schema::animation::EasingType;
    use crate::schema::time::{TimeCtx, TimePoint};
    use crate::vars::schema::{VarDef, VarKeyframe, VarSet};

    fn ctx() -> TimeCtx {
        TimeCtx {
            bpm: None,
            beat_offset: 0.0,
            scene_start: 0.0,
        }
    }

    fn table_with(name: &str, def: VarDef) -> VarTable {
        let mut vars = VarSet::new();
        vars.insert(name.to_string(), def);
        VarTable::compile(&vars, &ctx()).unwrap()
    }

    #[test]
    fn expression_reads_a_scenario_level_variable() {
        let scenario = table_with(
            "keyDraw",
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
        let scope = VarScope::new(&scenario, None, 0.5);

        // `Expr::is_static` only special-cases the fixed `t`/`T`/`beat`/
        // `duration` names — it has no notion of a declared `vars` set, so
        // in isolation it still says `true` here. That is expected, not a
        // bug: a caller must additionally consult
        // `crate::vars::dynamic_names` before folding, which is exactly
        // what makes `keyDraw` land on the dynamic side — see this
        // module's doc and `mod.rs`'s "What is not here" section.
        let expr = Expr::parse("= $keyDraw * 360").unwrap();
        assert!(expr.is_static());
        assert_eq!(expr.eval(&scope).unwrap(), 180.0);
    }

    #[test]
    fn scene_level_variable_shadows_scenario_level() {
        let scenario = table_with(
            "speed",
            VarDef {
                default: 1.0,
                animation: Vec::new(),
            },
        );
        let scene = table_with(
            "speed",
            VarDef {
                default: 9.0,
                animation: Vec::new(),
            },
        );

        let shadowed = VarScope::new(&scenario, Some(&scene), 0.0);
        assert_eq!(shadowed.resolve("speed"), Some(9.0));

        let unshadowed = VarScope::new(&scenario, None, 0.0);
        assert_eq!(unshadowed.resolve("speed"), Some(1.0));
    }

    #[test]
    fn scene_local_variable_is_invisible_outside_its_scene() {
        // Scene B declares `onlyInB`; a scope built without scene B's
        // table (as if evaluating an expression in scene A) must not see
        // it — same `None` a genuinely unknown name produces.
        let scenario = VarTable::compile(&VarSet::new(), &ctx()).unwrap();
        let scene_b = table_with(
            "onlyInB",
            VarDef {
                default: 5.0,
                animation: Vec::new(),
            },
        );
        let _ = &scene_b; // would be passed as `scene` only while evaluating scene B

        let scope_in_scene_a = VarScope::new(&scenario, None, 0.0);
        assert_eq!(scope_in_scene_a.resolve("onlyInB"), None);

        let expr = Expr::parse("= $onlyInB").unwrap();
        let err = expr.eval(&scope_in_scene_a).unwrap_err();
        assert_eq!(
            err,
            crate::expr::ExprError::UnknownIdent("onlyInB".to_string())
        );
    }
}
