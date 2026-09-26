use crate::expr::Scope;

use super::track::VarTable;

pub struct VarScope<'a> {
    scenario: &'a VarTable,
    scene: Option<&'a VarTable>,
    t: f64,
}

impl<'a> VarScope<'a> {
    pub fn new(scenario: &'a VarTable, scene: Option<&'a VarTable>, t: f64) -> Self {
        VarScope { scenario, scene, t }
    }

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
        let scenario = VarTable::compile(&VarSet::new(), &ctx()).unwrap();
        let scene_b = table_with(
            "onlyInB",
            VarDef {
                default: 5.0,
                animation: Vec::new(),
            },
        );
        let _ = &scene_b;

        let scope_in_scene_a = VarScope::new(&scenario, None, 0.0);
        assert_eq!(scope_in_scene_a.resolve("onlyInB"), None);

        let expr = Expr::parse("= $onlyInB").unwrap();
        let err = expr.eval(&scope_in_scene_a).unwrap_err();
        assert_eq!(
            err,
            crate::expr::ExprError::UnknownIdent("onlyInB".to_string())
        );
    }

    #[test]
    fn a_host_scope_can_hold_a_var_scope_and_delegate_to_resolve() {
        struct EngineScope<'a> {
            vars: VarScope<'a>,
        }

        impl crate::expr::Scope for EngineScope<'_> {
            fn var(&self, name: &str) -> Option<f64> {
                self.vars.resolve(name)
            }

            fn node_prop(&self, id: &str, prop: &str) -> Option<f64> {
                let _ = (id, prop);
                None
            }
        }

        let scenario = table_with(
            "W",
            VarDef {
                default: 1080.0,
                animation: Vec::new(),
            },
        );
        let host = EngineScope {
            vars: VarScope::new(&scenario, None, 0.0),
        };
        assert_eq!(crate::expr::Scope::var(&host, "W"), Some(1080.0));
        assert_eq!(crate::expr::Scope::node_prop(&host, "n", "x"), None);
    }
}
