# Rule: Module Structure & File Organization

## Monolithic files have been split into sub-modules

### schema/ (data models)

`schema/video.rs` (2167 lines) has been split into 5 modules:

| Module | Contents |
|---|---|
| `schema/scenario.rs` | Scenario, ResolvedScenario, View, ResolvedView, Scene, VideoConfig, Transition, Camera, AudioTrack, FontEntry |
| `schema/style.rs` | LayerStyle, TimelineStep, Spacing, FontWeight, TextAlign, CardDirection/Align/Justify, CardBorder, CardShadow, SizeDimension, FilterConfig, BlendMode |
| `schema/background.rs` | AnimatedBackground, BackgroundPreset, BackgroundValue, ResolvedBackground, HaloZone, ScrollDirection |
| `schema/codeblock_types.rs` | CodeblockChrome, CodeblockState, CodeblockReveal, CodeblockCursor |
| `schema/video.rs` | AnimationEffect, AnimationTiming, Size, ShapeType, Fill, Gradient, GradientType, Stroke, ImageFit, GlowConfig, OrbitConfig, WiggleConfig |

All types are re-exported via `pub use *` in `schema/mod.rs`, so `crate::schema::LayerStyle` works regardless of which sub-module the type lives in.

### components/chart/ (chart rendering)

`chart.rs` (1376 lines) has been split into 10 sub-modules:

| Module | Contents |
|---|---|
| `chart/mod.rs` | Chart struct, ChartType enum, data types, Widget impl, progress(), helper types |
| `chart/bar.rs` | render_bar, render_horizontal_bar, render_stacked_bar |
| `chart/line.rs` | render_line, render_area |
| `chart/pie.rs` | render_pie, render_donut |
| `chart/radar.rs` | render_radar |
| `chart/scatter.rs` | render_scatter |
| `chart/radial.rs` | render_radial_bar |
| `chart/funnel.rs` | render_funnel, render_funnel_vertical, render_funnel_horizontal |
| `chart/waterfall.rs` | render_waterfall |
| `chart/axes.rs` | draw_axes, format_number, contrast_text_color |

Each sub-module extends `Chart` via `impl Chart { ... }` blocks.

### engine/render/ (render pipeline)

`render_v2.rs` (1899 lines) has been split into 5 sub-modules:

| Module | Contents |
|---|---|
| `render/mod.rs` | render_component, render_component_inner, render_children, render_children_with_stagger, render_overlays |
| `render/scene.rs` | render_frame_v2, render_scene_frame_scaled, compute_root_layout, prepare_scene, camera transforms |
| `render/background.rs` | draw_animated_background, gradient_shift, halo, concentric_circles, grid_dots, heropattern |
| `render/transforms.rs` | draw_3d_shadow, draw_inner_shadow |
| `render/effects.rs` | Reserved for future effect extraction |

**Important:** The old `render_v2::` path is now `render::`. All references use `crate::engine::render::*`.

### engine/codeblock/ (codeblock rendering)

`codeblock.rs` (1713 lines) has been split into 6 sub-modules:

| Module | Contents |
|---|---|
| `codeblock/mod.rs` | render_codeblock, render_codeblock_v2 |
| `codeblock/highlight.rs` | Syntect integration, theme loading, syntax highlighting |
| `codeblock/chrome.rs` | macOS title bar chrome drawing |
| `codeblock/reveal.rs` | Typewriter, line-by-line reveal, line numbers |
| `codeblock/diff.rs` | State diff transitions, word diff, cursor editing |
| `codeblock/dimensions.rs` | compute_code_dimensions |

## Convention for new splits

When splitting a file into sub-modules:
1. Create a directory with the same name as the file
2. Put shared types and the main entry point in `mod.rs`
3. Use `pub(super)` for functions shared between sub-modules
4. Use `super::*` imports within sub-modules
5. Re-export public items from `mod.rs` so external callers are unaffected
