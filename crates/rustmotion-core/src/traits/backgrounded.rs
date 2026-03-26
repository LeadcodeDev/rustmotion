/// Trait for components that support a background color.
pub trait Backgrounded {
    fn background(&self) -> Option<&str>;
}

/// Mutable access to background — needed by builder traits.
pub trait BackgroundedMut: Backgrounded {
    fn set_background(&mut self, bg: Option<String>);
}

/// Builder API for background.
pub trait BackgroundedExt: BackgroundedMut + Sized {
    fn bg(mut self, color: impl Into<String>) -> Self {
        self.set_background(Some(color.into()));
        self
    }

    fn bg_none(mut self) -> Self {
        self.set_background(None);
        self
    }
}

impl<T: BackgroundedMut + Sized> BackgroundedExt for T {}
