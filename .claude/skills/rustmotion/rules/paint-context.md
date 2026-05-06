# Rule: Painter Trait API

All components implement the `Painter` trait (replaces the old `Widget` trait):

```rust
pub trait Painter {
    /// Paint the component's content. Canvas is already translated to the
    /// content-box origin and clipped if `overflow: hidden`. Background,
    /// border, and shadow are already drawn by the engine before this call.
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        props: &AnimatedProperties,
        ctx: &PaintCtx,
    );

    /// Optional intrinsic measurement for taffy's measure_fn (text, image,
    /// codeblock). Return None to let taffy size the node from CssStyle alone.
    fn intrinsic_size(&self, available: AvailableSize, ctx: &MeasureCtx) -> Option<(f32, f32)> {
        None
    }
}
```

## BoxLayout — resolved geometry

```rust
pub struct BoxLayout {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    // border-box, padding-box, content-box rectangles also available
}
```

All coordinates are absolute within the scene canvas (no parent offset arithmetic needed — the engine translates the canvas before calling `paint_content`).

## PaintCtx — frame-level context

```rust
pub struct PaintCtx {
    pub time: f64,            // seconds elapsed in the current scene
    pub scene_duration: f64,
    pub frame_index: u32,
    pub fps: u32,
    pub video_width: u32,
    pub video_height: u32,
    pub stagger_offset: f64,  // stagger delay from parent container
}
```

## AnimatedProperties — per-component state

`props` carries animation-resolved values for the current frame. Outer transforms (translate, scale, rotate, opacity, filter) are already applied to the canvas by the engine. `props` exposes internal-only fields used by component painters:

| Field | Type | Used by |
|---|---|---|
| `draw_progress` | f32 0–1 | chart, gauge, sparkline, progress |
| `visible_chars` / `visible_words` | usize | text char animations |
| `char_animation` | Option<CharAnim> | text reveal effects |
| `font_size` | Option<f32> | text size override |
| `color` | Option<String> | text/icon color override |
| `stroke_width` | Option<f32> | shape stroke override |
| `border_radius` | Option<f32> | shape radius override |
| `rotate_x`, `rotate_y`, `perspective` | f32 | 3D tilt (applied by engine) |

## Inside paint_content — Common Patterns

**Leaf component (uses layout.width / layout.height):**
```rust
fn paint_content(&self, canvas: &Canvas, layout: &BoxLayout, props: &AnimatedProperties, ctx: &PaintCtx) {
    let w = layout.width;
    let h = layout.height;
    let progress = props.draw_progress;
    // Draw using Skia primitives on `canvas`
}
```

**Reading timing:**
```rust
fn paint_content(&self, canvas: &Canvas, layout: &BoxLayout, props: &AnimatedProperties, ctx: &PaintCtx) {
    let t = ctx.time;
    let progress = (t / ctx.scene_duration).clamp(0.0, 1.0) as f32;
    // ...
}
```

**Containers** do not need to call render_children manually — the engine recurses on children after `paint_content` returns. `paint_content` for containers is a no-op or draws a background overlay:
```rust
fn paint_content(&self, canvas: &Canvas, layout: &BoxLayout, _props: &AnimatedProperties, _ctx: &PaintCtx) {
    // Background / border already drawn by engine.
    // Children painted automatically by paint_pass after this call.
}
```

## Key differences from old Widget/PaintContext

| Old (Widget) | New (Painter) |
|---|---|
| `fn paint(&self, canvas, ctx: &PaintContext)` | `fn paint_content(&self, canvas, layout, props, ctx: &PaintCtx)` |
| `fn measure(&self, constraints)` + `fn layout(&self, constraints)` | `fn intrinsic_size(&self, available, ctx)` |
| `ctx.layout.width`, `ctx.width()` | `layout.width` |
| `ctx.props.opacity`, `ctx.props.scale_x` | applied by engine to canvas (not in props) |
| `ctx.as_render_context()` + `render_children(...)` | engine recurses automatically |
| `ctx.time`, `ctx.scene_duration` | `ctx.time`, `ctx.scene_duration` (same) |
