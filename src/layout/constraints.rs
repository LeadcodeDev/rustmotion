#![allow(dead_code)]
/// Layout constraints passed from parent to child (Flutter-style).
#[derive(Debug, Clone, Copy)]
pub struct Constraints {
    pub min_width: f32,
    pub max_width: f32,
    pub min_height: f32,
    pub max_height: f32,
}

impl Constraints {
    /// No constraints — child can be any size.
    pub fn unbounded() -> Self {
        Self {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: 0.0,
            max_height: f32::INFINITY,
        }
    }

    /// Exact size — child must be exactly this size.
    pub fn tight(width: f32, height: f32) -> Self {
        Self {
            min_width: width,
            max_width: width,
            min_height: height,
            max_height: height,
        }
    }

    /// Bounded — child can be between 0 and max.
    pub fn loose(max_width: f32, max_height: f32) -> Self {
        Self {
            min_width: 0.0,
            max_width,
            min_height: 0.0,
            max_height,
        }
    }

    /// Constrain a size to fit within these constraints.
    pub fn constrain(&self, width: f32, height: f32) -> (f32, f32) {
        (
            width.clamp(self.min_width, self.max_width),
            height.clamp(self.min_height, self.max_height),
        )
    }

    /// Remove the minimum constraints (make them 0).
    pub fn loosen(&self) -> Self {
        Self {
            min_width: 0.0,
            max_width: self.max_width,
            min_height: 0.0,
            max_height: self.max_height,
        }
    }

    pub fn has_bounded_width(&self) -> bool {
        self.max_width.is_finite()
    }

    pub fn has_bounded_height(&self) -> bool {
        self.max_height.is_finite()
    }
}
