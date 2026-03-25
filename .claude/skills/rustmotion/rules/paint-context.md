# Rule: Widget paint() API with PaintContext

All components implement the `Widget` trait with three methods:

```rust
trait Widget {
    fn paint(&self, canvas: &Canvas, ctx: &PaintContext) -> Result<()>;
    fn measure(&self, constraints: &Constraints) -> (f32, f32);
    fn layout(&self, constraints: &Constraints) -> LayoutNode;
}
```

## PaintContext (Flutter-inspired)

`PaintContext` replaces the old 4-parameter signature `(canvas, layout, ctx, props)` with a single rich context:

```rust
// Access layout dimensions
let w = ctx.width();    // shorthand for ctx.layout.width
let h = ctx.height();   // shorthand for ctx.layout.height

// Access child layouts (for containers)
let child_layout = &ctx.layout.children[i];

// Access resolved animation properties
let opacity = ctx.props.opacity;
let scale = ctx.props.scale_x;

// Access timing
let t = ctx.time;
let duration = ctx.scene_duration;

// Access video dimensions
let vw = ctx.video_width;
```

## Inside paint() — Common Patterns

**Leaf component:**
```rust
fn paint(&self, canvas: &Canvas, ctx: &PaintContext) -> Result<()> {
    let layout = ctx.layout;
    let props = ctx.props;
    // Draw using layout.width, layout.height, props.opacity, ctx.time, etc.
    Ok(())
}
```

**Container component** (renders children via the render pipeline):
```rust
fn paint(&self, canvas: &Canvas, ctx: &PaintContext) -> Result<()> {
    let layout = ctx.layout;
    let render_ctx = ctx.as_render_context();
    // Draw background, border...
    crate::engine::render::render_children_with_stagger(
        canvas, &self.children, layout, &render_ctx, self.style.stagger
    )?;
    Ok(())
}
```

**Component using self.progress() or other helpers that take &RenderContext:**
```rust
fn paint(&self, canvas: &Canvas, ctx: &PaintContext) -> Result<()> {
    let layout = ctx.layout;
    let render_ctx = ctx.as_render_context();
    let progress = self.progress(&render_ctx);
    // ...
}
```

## Key Conversions

- `ctx.as_render_context()` — converts `PaintContext` → `RenderContext` (for container children and helper methods)
- `PaintContext::from_legacy(layout, render_ctx, props)` — constructs from old-style params (used by render pipeline)
