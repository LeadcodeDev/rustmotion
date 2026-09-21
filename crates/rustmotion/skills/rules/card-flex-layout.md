# Rule: Use card/flex/div for Layout

`card` and `flex` (alias for `card`) use a CSS flexbox engine that auto-positions children. Use `card` for visual containers (background, border, shadow), `div` for invisible grouping and pure layout (no background, no border, no clipping).

## Scene = Implicit Flex Container

Every scene acts as an implicit full-screen flex container (`direction: column` by default). Children without `position` participate in flex flow automatically. Children with `position` are absolutely positioned.

You can customize the scene layout:
```json
{
  "duration": 5.0,
  "layout": {
    "direction": "column",
    "align_items": "center",
    "justify_content": "center",
    "gap": 24,
    "padding": 40
  },
  "children": [
    { "type": "text", "content": "Centered title", "style": { "font-size": 64, "color": "#FFFFFF" } },
    { "type": "text", "content": "Subtitle below", "style": { "font-size": 32, "color": "#94A3B8" } }
  ]
}
```

## Card/Flex Patterns

Key patterns:
- **Horizontal row:** `"flex-direction": "row"` + `"gap"`
- **Vertical stack:** `"flex-direction": "column"` (default) + `"gap"`
- **Centered content:** `"align-items": "center"` + `"justify-content": "center"`
- **Auto-height:** `"style": { "width": 800, "height": "auto" }`
- **Grid:** `"display": "grid"` + `"grid-template-columns"`

Children flow in the flexbox. Use `positioned` container for absolute positioning.

**Grid sizing:** `height: "auto"` on a grid container sizes correctly to content — you don't need an explicit `height` just to avoid stretching. See [rules/grid-card-height.md](rules/grid-card-height.md).

## 23 component types have no *intrinsic* size — they fall back to a documented default

Most components either measure their own content (`text`, `codeblock`, `counter`, `badge`, `table`, `terminal`, `caption`, `kbd`, `gradient_text`, `rich_text`) or get a computed fallback size from their own fields (`icon`-like shapes such as `avatar`, `divider`, `line`, `arrow`, `switch`, `slider`, `progress`, `list`, `timeline`, `notification`, `rating`, `qr_code`, `countdown`, `particle`, `cursor`, `connector`, `waveform`, `audio_spectrum`). The following **23 types have no intrinsic measurement** (they fall through `component_intrinsic`'s `_ => None` arm and don't override `Painter::intrinsic_size`), but every one of them gets a **default `width`/`height` applied by `apply_intrinsic_overrides`** in `crates/rustmotion-components/src/box_builder.rs` whenever the JSON doesn't already set `style.width`/`style.height` — an explicit size is an *override*, not a requirement:

| Component | Default size | Where it comes from |
|---|---|---|
| `sparkline` | 120×40 | fixed convention |
| `stat` | 280×180 | fixed convention |
| `gauge` | square, `2·(88 + track_width/2 + 4)` | derived from its own `track_width` field |
| `dot_map` | 640×320 (2:1) | fixed (equirectangular aspect) |
| `comparison` | 520×280 | fixed convention |
| `treemap` | 416×368 | fixed convention |
| `chart` | 320×320 (pie/donut/radar/radial_bar) or 400×300 (other 8 types) | fixed, by chart shape |
| `mockup` | per `device` (e.g. 320×690 for iphone/android, 640×400 laptop, 640×360 browser) | fixed, by device aspect |
| `icon` | 64×64 | fixed convention |
| `svg` | 200×200 | fixed convention |
| `shape` | 80×80 | fixed convention |
| `image` | 400×300 (4:3) | fixed convention |
| `video`, `gif` | 400×225 (16:9) | fixed convention |
| `lottie` | 300×300 | fixed convention |
| `skeleton` | 400×200 (rectangle) / 64×64 (circle) / `240×(lines·line_height + gaps)` (text) | fixed, or derived from its own `lines`/`line_height`/`line_gap` for the `text` variant |
| `marquee` | 800×`2·font_size` | fixed width, height derived from its own `font_size` |
| `heatmap` | derived from `data` rows/cols and `cell_size`/`cell_gap` | fully content-derived |
| `callout`, `tooltip` | derived from the measured text width + padding + arrow | fully content-derived |
| `pill_nav` | derived from each item's measured label width + padding + gap | fully content-derived |
| `stepper` | derived from `node_size` and each step's label/description width | fully content-derived |
| `tag_cloud` | derived from each tag's weighted font size, wrapped at a conventional content width | fully content-derived |

A default only fills in the axis that's actually missing — `apply_default_size` respects an explicit `width` or `height` (and derives the other one from `style.aspect-ratio` when only one is set). Explicit `style.width`/`style.height` is still worth setting whenever the default doesn't match the layout you want (e.g. a `stat` narrower than 280px in a tight row), but omitting it no longer produces a blank frame — three `stat`s in a flex-row card with no explicit size now lay out at 280×180 each, confirmed by `box_builder.rs`'s own tests.

If a component isn't showing up despite that, look at [rules/component-field-placement.md](rules/component-field-placement.md) first — schema-field misplacement is the more common cause of an invisible component.

**GOOD** (icon + text row):
```json
{
  "type": "card",
  "style": {
    "width": 800,
    "height": "auto",
    "flex-direction": "row",
    "align-items": "center",
    "gap": 16,
    "padding": 24,
    "background": "#1E293B",
    "border-radius": 16
  },
  "children": [
    { "type": "icon", "icon": "lucide:check-circle", "style": { "width": 48, "height": 48, "color": "#22C55E" } },
    { "type": "text", "content": "Feature enabled", "style": { "font-size": 32, "color": "#FFFFFF" } }
  ]
}
```
