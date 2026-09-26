pub trait Backgrounded {
    fn background(&self) -> Option<&str>;
}

pub trait BackgroundedMut: Backgrounded {
    fn set_background(&mut self, bg: Option<String>);
}

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
