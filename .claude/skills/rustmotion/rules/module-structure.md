# Rule: Module Structure & File Organization

## Workspace crate split

The workspace is split into three crates:

| Crate | Role |
|---|---|
| `rustmotion-core` | Engine, CSS model, schema types, Painter trait |
| `rustmotion-components` | 57 component structs + Painter impls + box builder |
| `rustmotion-cli` | CLI commands (render, validate, schema, info) |

---

## rustmotion-core

```
src/
├── css/
│   ├── style.rs          # CssStyle — all CSS properties (kebab-case, serde)
│   ├── units.rs          # Length, LengthPercentage (px, %, em, rem, vw, vh, fr, auto)
│   ├── cascade.rs        # Inherit color/font-* from parent to children
│   ├── taffy_bridge.rs   # CssStyle → taffy::Style conversion
│   └── animation.rs      # Resolve animations → CssStyle overrides at time t
├── engine/
│   ├── box_tree.rs       # BoxNode { id, kind, css: CssStyle, children, intrinsic }
│   ├── layout_pass.rs    # Taffy orchestration → BoxLayout { x, y, width, height } per node
│   ├── paint_pass.rs     # Walk top-down: transform, clip, bg, border, dispatch Painter, children
│   ├── animator.rs       # Animation resolution, easing, spring solver, AnimatedProperties
│   ├── transition.rs     # Scene transition rendering (fade, slide, wipe, etc.)
│   ├── renderer/         # Skia drawing primitives
│   │   ├── mod.rs        # asset_cache, fetch_icon_svg
│   │   ├── colors.rs     # paint_from_hex, parse_hex_color
│   │   ├── fonts.rs      # font loading, emoji_typeface, draw_text_with_fallback
│   │   ├── shapes.rs     # rounded_rect, circle, arrow paths
│   │   └── text.rs       # Skia text measurement (line metrics, wrapping)
│   └── text/
│       └── cosmic.rs     # cosmic-text FontSystem global + Skia glyph bridge
├── schema/               # JSON-serializable data models
│   ├── scenario.rs       # Scenario, ResolvedScenario, View, ResolvedView, Scene, VideoConfig
│   ├── style.rs          # Specialized types: CardBorder, CardShadow, Fill, Gradient, etc.
│   ├── background.rs     # AnimatedBackground, BackgroundPreset
│   ├── animation.rs      # EasingType, AnimationPreset, PresetConfig, AnimationEffect
│   ├── codeblock_types.rs# CodeblockChrome, CodeblockState, CodeblockReveal
│   └── video.rs          # Size, ShapeType, Stroke, ImageFit, GlowConfig, OrbitConfig
└── traits/
    ├── painter.rs        # Painter trait + PaintCtx + AvailableSize + MeasureCtx
    ├── animatable.rs     # Animatable trait (animation field)
    ├── timed.rs          # Timed trait + TimingConfig (start_at, end_at)
    └── styled.rs         # Styled trait (style field accessor)
```

### Key types

- **`CssStyle`** (`css/style.rs`) — replaces the old `LayerStyle`. All fields are optional; names are CSS kebab-case (`flex-direction`, `border-radius`, `font-size`). Serde uses `rename_all = "kebab-case"`.
- **`BoxLayout`** (`engine/layout_pass.rs`) — final resolved geometry: `x, y, width, height, padding_box, content_box`.
- **`AnimatedProperties`** (`engine/animator.rs`) — per-component animation state (draw_progress, char animation, visible_chars, font_size override, color override, stroke_width, 3D rotate_x/rotate_y, etc.).
- **`PaintCtx`** (`traits/painter.rs`) — frame-level context: `time`, `scene_duration`, `fps`, `frame_index`, `video_width`, `video_height`, `stagger_offset`.

---

## rustmotion-components

```
src/
├── lib.rs              # Enum Component + dispatch methods (as_painter, as_animatable, etc.)
├── box_builder.rs      # build_scene(children) → BoxBuilderResult; component_size() for fixed-size leaves
├── intrinsic.rs        # IntrinsicMeasure impls: TextIntrinsic, BadgeIntrinsic, CounterIntrinsic,
│                       #   GradientTextIntrinsic, CaptionIntrinsic, KbdIntrinsic, …
├── legacy_dispatch.rs  # LegacyPaintDispatcher: maps NodeId payload → Component → Painter::paint_content
├── chart/              # Chart component (12 types)
│   ├── mod.rs          # Chart struct, ChartType enum, data types, Painter impl, progress()
│   ├── bar.rs          # render_bar, render_horizontal_bar, render_stacked_bar
│   ├── line.rs         # render_line, render_area
│   ├── pie.rs          # render_pie, render_donut
│   ├── radar.rs        # render_radar
│   ├── scatter.rs      # render_scatter
│   ├── radial.rs       # render_radial_bar
│   ├── funnel.rs       # render_funnel (horizontal + vertical)
│   ├── waterfall.rs    # render_waterfall
│   └── axes.rs         # draw_axes(categorical: bool), format_number, contrast_text_color
├── codeblock/          # Codeblock component
│   ├── mod.rs          # render_codeblock_v2, Painter impl
│   ├── highlight.rs    # Syntect integration, syntax highlighting
│   ├── chrome.rs       # macOS title bar chrome
│   ├── reveal.rs       # Typewriter, line-by-line reveal
│   ├── diff.rs         # State transitions, word diff, cursor editing
│   └── dimensions.rs   # compute_code_dimensions
└── *.rs                # One file per component (impl Painter)
```

### chart/axes.rs — categorical flag

`draw_axes` has a `categorical: bool` parameter that controls x-label placement:
- `categorical = true` (bar, stacked_bar, waterfall): labels at slot centers `(i + 0.5) / n * chart_w`
- `categorical = false` (line, area, scatter): labels at point positions `i / (n-1) * chart_w`

---

## Convention for new component splits

When splitting a large file into sub-modules:
1. Create a directory with the same name as the file
2. Put shared types and the main entry point in `mod.rs`
3. Use `pub(super)` for functions shared between sub-modules
4. Use `super::*` imports within sub-modules
5. Re-export public items from `mod.rs` so external callers are unaffected
