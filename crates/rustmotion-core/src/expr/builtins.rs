#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Builtin {
    Sin,
    Cos,
    Tan,
    Atan2,
    Sqrt,
    Pow,
    Abs,
    Min,
    Max,
    Clamp,
    Floor,
    Ceil,
    Round,
    Sign,
    Exp,
    Log,
    Lerp,
    Smoothstep,
    Rand,
    Noise,
}

impl Builtin {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "sin" => Self::Sin,
            "cos" => Self::Cos,
            "tan" => Self::Tan,
            "atan2" => Self::Atan2,
            "sqrt" => Self::Sqrt,
            "pow" => Self::Pow,
            "abs" => Self::Abs,
            "min" => Self::Min,
            "max" => Self::Max,
            "clamp" => Self::Clamp,
            "floor" => Self::Floor,
            "ceil" => Self::Ceil,
            "round" => Self::Round,
            "sign" => Self::Sign,
            "exp" => Self::Exp,
            "log" => Self::Log,
            "lerp" => Self::Lerp,
            "smoothstep" => Self::Smoothstep,
            "rand" => Self::Rand,
            "noise" => Self::Noise,
            _ => return None,
        })
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Sin => "sin",
            Self::Cos => "cos",
            Self::Tan => "tan",
            Self::Atan2 => "atan2",
            Self::Sqrt => "sqrt",
            Self::Pow => "pow",
            Self::Abs => "abs",
            Self::Min => "min",
            Self::Max => "max",
            Self::Clamp => "clamp",
            Self::Floor => "floor",
            Self::Ceil => "ceil",
            Self::Round => "round",
            Self::Sign => "sign",
            Self::Exp => "exp",
            Self::Log => "log",
            Self::Lerp => "lerp",
            Self::Smoothstep => "smoothstep",
            Self::Rand => "rand",
            Self::Noise => "noise",
        }
    }

    pub(crate) fn arity(self) -> usize {
        match self {
            Self::Sin
            | Self::Cos
            | Self::Tan
            | Self::Sqrt
            | Self::Abs
            | Self::Floor
            | Self::Ceil
            | Self::Round
            | Self::Sign
            | Self::Exp
            | Self::Log
            | Self::Rand => 1,
            Self::Atan2 | Self::Pow | Self::Min | Self::Max | Self::Noise => 2,
            Self::Clamp | Self::Lerp | Self::Smoothstep => 3,
        }
    }

    pub(crate) fn call(self, args: &[f64]) -> f64 {
        match self {
            Self::Sin => args[0].sin(),
            Self::Cos => args[0].cos(),
            Self::Tan => args[0].tan(),
            Self::Atan2 => args[0].atan2(args[1]),
            Self::Sqrt => args[0].sqrt(),
            Self::Pow => args[0].powf(args[1]),
            Self::Abs => args[0].abs(),
            Self::Min => args[0].min(args[1]),
            Self::Max => args[0].max(args[1]),
            Self::Clamp => clamp(args[0], args[1], args[2]),
            Self::Floor => args[0].floor(),
            Self::Ceil => args[0].ceil(),
            Self::Round => args[0].round(),
            Self::Sign => {
                if args[0] > 0.0 {
                    1.0
                } else if args[0] < 0.0 {
                    -1.0
                } else {
                    0.0
                }
            }
            Self::Exp => args[0].exp(),
            Self::Log => args[0].ln(),
            Self::Lerp => lerp(args[0], args[1], args[2]),
            Self::Smoothstep => smoothstep(args[0], args[1], args[2]),
            Self::Rand => rand(args[0]),
            Self::Noise => noise(args[0], args[1]),
        }
    }
}

fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    x.max(lo).min(hi)
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = if edge0 == edge1 {
        if x < edge0 {
            0.0
        } else {
            1.0
        }
    } else {
        ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0)
    };
    t * t * (3.0 - 2.0 * t)
}

pub(crate) fn constant(name: &str) -> Option<f64> {
    match name {
        "PI" => Some(std::f64::consts::PI),
        "TAU" => Some(std::f64::consts::TAU),
        "E" => Some(std::f64::consts::E),
        _ => None,
    }
}

fn splitmix64(x: u64) -> u64 {
    let x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn unit_interval(bits: u64) -> f64 {
    (bits >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
}

fn rand(seed: f64) -> f64 {
    unit_interval(splitmix64(seed.to_bits()))
}

fn lattice(i: i64, seed: f64) -> f64 {
    let mixed = splitmix64((i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ seed.to_bits());
    unit_interval(mixed)
}

fn smootherstep(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn noise(x: f64, seed: f64) -> f64 {
    let i0 = x.floor() as i64;
    let i1 = i0 + 1;
    let t = x - i0 as f64;
    let v0 = lattice(i0, seed);
    let v1 = lattice(i1, seed);
    let tt = smootherstep(t);
    v0 + (v1 - v0) * tt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rand_is_pure_and_deterministic() {
        assert_eq!(rand(42.0), rand(42.0));
        assert_eq!(Builtin::Rand.call(&[42.0]), Builtin::Rand.call(&[42.0]));
    }

    #[test]
    fn rand_differs_across_seeds_and_stays_in_unit_interval() {
        let a = rand(1.0);
        let b = rand(2.0);
        assert_ne!(a, b);
        for seed in [0.0, 1.0, -5.0, 1e6, 0.0001] {
            let v = rand(seed);
            assert!((0.0..1.0).contains(&v), "rand({seed}) = {v} out of range");
        }
    }

    #[test]
    fn noise_is_pure_and_deterministic() {
        assert_eq!(noise(3.25, 7.0), noise(3.25, 7.0));
    }

    #[test]
    fn noise_is_continuous_at_lattice_points() {
        let seed = 11.0;
        assert_eq!(noise(4.0, seed), lattice(4, seed));
    }

    #[test]
    fn clamp_tolerates_swapped_bounds() {
        assert_eq!(clamp(5.0, 10.0, 0.0), 5.0);
        assert_eq!(clamp(-5.0, 10.0, 0.0), 0.0);
        assert_eq!(clamp(50.0, 10.0, 0.0), 10.0);
    }

    #[test]
    fn lerp_and_smoothstep_basic() {
        assert_eq!(lerp(0.0, 10.0, 0.5), 5.0);
        assert_eq!(smoothstep(0.0, 1.0, -1.0), 0.0);
        assert_eq!(smoothstep(0.0, 1.0, 2.0), 1.0);
        assert_eq!(smoothstep(0.0, 1.0, 0.5), 0.5);
    }
}
