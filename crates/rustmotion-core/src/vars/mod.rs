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
        assert_eq!(
            frames[4], frames[5],
            "must hold, not reset, past the last keyframe"
        );
    }

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
