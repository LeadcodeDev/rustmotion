/// Trait for components that clip their content to their bounds.
pub trait Clipped {
    fn clip(&self) -> bool {
        false
    }
}
