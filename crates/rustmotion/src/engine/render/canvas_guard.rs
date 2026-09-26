use skia_safe::Canvas;

pub struct CanvasGuard {
    canvas: *const Canvas,
}

impl CanvasGuard {
    #[inline]
    pub fn new(canvas: &Canvas) -> Self {
        canvas.save();
        Self {
            canvas: canvas as *const Canvas,
        }
    }
}

impl Drop for CanvasGuard {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            (*self.canvas).restore();
        }
    }
}
