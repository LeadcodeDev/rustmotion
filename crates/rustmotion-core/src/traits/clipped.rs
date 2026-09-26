pub trait Clipped {
    fn clip(&self) -> bool {
        false
    }
}
