/// Trait for components that support rounded corners.
pub trait Rounded {
    fn corner_radius(&self) -> f32;
}

/// Mutable access to corner radius — needed by builder traits.
pub trait RoundedMut: Rounded {
    fn set_corner_radius(&mut self, radius: f32);
}

/// Builder API for corner radius.
pub trait RoundedExt: RoundedMut + Sized {
    fn rounded(mut self, radius: f32) -> Self {
        self.set_corner_radius(radius);
        self
    }

    fn rounded_none(self) -> Self { self.rounded(0.0) }
    fn rounded_sm(self) -> Self { self.rounded(4.0) }
    fn rounded_md(self) -> Self { self.rounded(8.0) }
    fn rounded_lg(self) -> Self { self.rounded(12.0) }
    fn rounded_xl(self) -> Self { self.rounded(16.0) }
    fn rounded_2xl(self) -> Self { self.rounded(24.0) }
    fn rounded_full(self) -> Self { self.rounded(9999.0) }
}

impl<T: RoundedMut + Sized> RoundedExt for T {}
