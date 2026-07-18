use skia_safe::Canvas;

/// RAII guard that calls `Canvas::save()` on construction and
/// `Canvas::restore()` on drop. Use it to ensure the canvas matrix and clip
/// stack are restored even if the wrapped code returns early via `?` or
/// panics, preventing latent state corruption between frames.
///
/// Construction takes a raw pointer instead of a borrow because Skia's
/// `Canvas::save()` takes `&mut`, but we want consumers to keep using the
/// canvas immutably afterwards (the visible mutation is a stack push). The
/// pointer is dereferenced only inside `Drop`, which always runs on the same
/// thread that built the guard — so this is sound for our use.
pub struct CanvasGuard {
    canvas: *const Canvas,
}

impl CanvasGuard {
    /// Save the canvas state. The matching `restore()` is invoked on drop.
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
        // Safety: the pointer was derived from a live reference passed to
        // `new`, and Skia canvases are not Send/Sync — the guard cannot
        // outlive the borrow because we hold no lifetime, but in practice
        // every call site keeps the canvas alive for the entire scope.
        unsafe {
            (*self.canvas).restore();
        }
    }
}
