---
name: rustmotion
description: Best practices for Rustmotion - Video creation in Rust
metadata:
  tags: motion, video, rust, animation, composition
---

# Skill: Generate rustmotion JSON Scenarios

## What is rustmotion?

rustmotion is a CLI tool that renders motion design videos from JSON scenario files. It uses Skia for 2D rendering and supports MP4, WebM, MOV, GIF, and PNG sequence outputs.

## Quick Reference

**Common resolutions:** 1080x1920 (portrait 9:16), 1920x1080 (landscape 16:9), 1080x1080 (square)

**Essential CLI:**
```bash
rustmotion validate scenario.json          # Validate without rendering
rustmotion render scenario.json -o out.mp4 # Render to MP4
rustmotion render scenario.json -o f.png --frame 0  # Single frame
rustmotion schema                          # Print JSON Schema
```

**JSON skeleton:**
```json
{
  "version": "1.0",
  "video": { "width": 1080, "height": 1920, "fps": 30, "background": "#0f172a" },
  "scenes": [
    { "duration": 3.0, "children": [ ... ] }
  ]
}
```

---

## Rules

Read individual rule files for detailed explanations, GOOD/BAD examples, and constraints:

- [rules/validate-json.md](rules/validate-json.md) - Always validate generated JSON with `rustmotion validate` before presenting
- [rules/even-dimensions.md](rules/even-dimensions.md) - Use even width/height for H.264 encoding
- [rules/counter-standalone.md](rules/counter-standalone.md) - Counter must be standalone (no baseline correction inside cards)
- [rules/vertical-align.md](rules/vertical-align.md) - Shape text vertical_align: use "top"/"middle"/"bottom" (NOT "center")
- [rules/stagger-animations.md](rules/stagger-animations.md) - Stagger animations with increasing preset_config.delay
- [rules/layer-order.md](rules/layer-order.md) - Layer order matters: first in array = behind, last = front
- [rules/card-flex-layout.md](rules/card-flex-layout.md) - Scene = implicit flex container; use card/flex for nested layout
- [rules/continuous-presets.md](rules/continuous-presets.md) - Continuous presets (pulse, float, shake, spin) need loop: true
- [rules/timing-constraints.md](rules/timing-constraints.md) - Timing: start_at must be < end_at, duration > 0
- [rules/icon-format.md](rules/icon-format.md) - Icon format must be "prefix:name" (e.g. "lucide:home")
- [rules/wiggle-additive.md](rules/wiggle-additive.md) - Wiggle is additive on top of presets and keyframes
- [rules/prefer-presets.md](rules/prefer-presets.md) - Prefer presets over manual keyframes (31 built-in presets)
- [rules/hex-colors.md](rules/hex-colors.md) - Colors in hex format only (#RRGGBB or #RRGGBBAA)
- [rules/easing-guidelines.md](rules/easing-guidelines.md) - Easing guidelines for motion design

---

## Complete Examples

### Example 1: Marketing Card (Portrait)

```json
{
  "version": "1.0",
  "video": { "width": 1080, "height": 1920, "fps": 30, "background": "#0f172a" },
  "scenes": [
    {
      "duration": 4.0,
      "children": [
        {
          "type": "shape",
          "shape": "rounded_rect",
          "position": { "x": 90, "y": 700 },
          "size": { "width": 900, "height": 520 },
          "preset": "scale_in",
          "preset_config": { "duration": 0.6 },
          "style": {
            "fill": {
              "type": "linear",
              "colors": ["#6366f1", "#8b5cf6"],
              "angle": 135
            },
            "border-radius": 32
          }
        },
        {
          "type": "icon",
          "icon": "lucide:rocket",
          "position": { "x": 490, "y": 800 },
          "size": { "width": 80, "height": 80 },
          "preset": "fade_in_up",
          "preset_config": { "delay": 0.3, "duration": 0.6 },
          "style": { "color": "#FFFFFF" }
        },
        {
          "type": "text",
          "content": "Ship Faster",
          "position": { "x": 540, "y": 940 },
          "preset": "fade_in_up",
          "preset_config": { "delay": 0.5, "duration": 0.6 },
          "style": {
            "font-size": 64,
            "color": "#FFFFFF",
            "font-weight": "bold",
            "text-align": "center"
          }
        },
        {
          "type": "text",
          "content": "Build motion videos in Rust.\nNo browser needed.",
          "position": { "x": 540, "y": 1060 },
          "max_width": 700,
          "preset": "fade_in_up",
          "preset_config": { "delay": 0.7, "duration": 0.6 },
          "style": {
            "font-size": 32,
            "color": "#CBD5E1",
            "text-align": "center",
            "line-height": 1.5
          }
        }
      ]
    }
  ]
}
```

### Example 2: Code Tutorial (Landscape)

```json
{
  "version": "1.0",
  "video": { "width": 1920, "height": 1080, "fps": 30, "background": "#1a1b26" },
  "scenes": [
    {
      "duration": 10.0,
      "children": [
        {
          "type": "text",
          "content": "Getting Started with Rust",
          "position": { "x": 960, "y": 80 },
          "preset": "fade_in",
          "preset_config": { "duration": 0.5 },
          "style": {
            "font-size": 40,
            "color": "#7AA2F7",
            "font-weight": "bold",
            "text-align": "center"
          }
        },
        {
          "type": "codeblock",
          "code": "fn main() {\n    println!(\"Hello, world!\");\n}",
          "language": "rust",
          "theme": "tokyo-night",
          "position": { "x": 260, "y": 160 },
          "size": { "width": 1400, "height": 400 },
          "show_line_numbers": true,
          "chrome": { "enabled": true, "title": "src/main.rs" },
          "reveal": { "mode": "typewriter", "start": 0.5, "duration": 3.0 },
          "style": { "font-size": 22, "padding": 24, "border-radius": 16 },
          "states": [
            {
              "code": "fn main() {\n    let name = \"rustmotion\";\n    println!(\"Hello, {}!\", name);\n}",
              "at": 5.0,
              "duration": 2.5,
              "cursor": { "enabled": true, "blink": true }
            }
          ]
        },
        {
          "type": "text",
          "content": "Variables are immutable by default",
          "position": { "x": 960, "y": 650 },
          "start_at": 5.0,
          "preset": "fade_in_up",
          "preset_config": { "delay": 0.0, "duration": 0.6 },
          "style": {
            "font-size": 28,
            "color": "#9ECE6A",
            "text-align": "center"
          }
        }
      ]
    }
  ]
}
```

### Example 3: Multi-Scene with Transitions

```json
{
  "version": "1.0",
  "video": { "width": 1080, "height": 1920, "fps": 30, "background": "#0a0a14" },
  "scenes": [
    {
      "duration": 3.0,
      "children": [
        {
          "type": "text",
          "content": "2024 Results",
          "position": { "x": 540, "y": 800 },
          "preset": "fade_in_up",
          "style": { "font-size": 72, "color": "#FFFFFF", "font-weight": "bold", "text-align": "center" }
        },
        {
          "type": "text",
          "content": "Year in Review",
          "position": { "x": 540, "y": 900 },
          "preset": "fade_in_up",
          "preset_config": { "delay": 0.3, "duration": 0.6 },
          "style": { "font-size": 36, "color": "#94A3B8", "text-align": "center" }
        }
      ]
    },
    {
      "duration": 4.0,
      "transition": { "type": "slide", "duration": 0.6 },
      "children": [
        {
          "type": "counter",
          "from": 0,
          "to": 12500,
          "separator": ",",
          "easing": "ease_out",
          "position": { "x": 540, "y": 750 },
          "start_at": 0.3,
          "end_at": 3.5,
          "style": { "font-size": 96, "color": "#38BDF8", "font-weight": "bold", "text-align": "center" }
        },
        {
          "type": "text",
          "content": "Users Reached",
          "position": { "x": 540, "y": 880 },
          "preset": "fade_in_up",
          "preset_config": { "delay": 0.5, "duration": 0.6 },
          "style": { "font-size": 36, "color": "#CBD5E1", "text-align": "center" }
        },
        {
          "type": "card",
          "position": { "x": 90, "y": 1050 },
          "size": { "width": 900, "height": "auto" },
          "preset": "fade_in_up",
          "preset_config": { "delay": 0.8, "duration": 0.6 },
          "style": {
            "flex-direction": "row",
            "gap": 16,
            "padding": 24,
            "background": "#1E293B",
            "border-radius": 20
          },
          "children": [
            {
              "type": "icon",
              "icon": "lucide:trending-up",
              "size": { "width": 48, "height": 48 },
              "style": { "color": "#22C55E" }
            },
            {
              "type": "text",
              "content": "+340% growth YoY",
              "style": { "font-size": 32, "color": "#FFFFFF", "font-weight": "bold" }
            }
          ]
        }
      ]
    },
    {
      "duration": 3.0,
      "transition": { "type": "fade", "duration": 0.5 },
      "children": [
        {
          "type": "text",
          "content": "Thank You",
          "position": { "x": 540, "y": 900 },
          "preset": "scale_in",
          "preset_config": { "duration": 0.8 },
          "wiggle": [
            { "property": "translate_y", "amplitude": 4, "frequency": 1.5, "seed": 7 }
          ],
          "style": { "font-size": 80, "color": "#FFFFFF", "font-weight": "bold", "text-align": "center" }
        },
        {
          "type": "icon",
          "icon": "lucide:heart",
          "position": { "x": 508, "y": 1020 },
          "size": { "width": 64, "height": 64 },
          "preset": "fade_in",
          "preset_config": { "delay": 0.5, "loop": false },
          "wiggle": [
            { "property": "scale", "amplitude": 0.1, "frequency": 2, "seed": 42 }
          ],
          "style": { "color": "#F43F5E" }
        }
      ]
    }
  ]
}
```

---

## Reference

### JSON Scenario Structure

```json
{
  "version": "1.0",
  "video": { ... },
  "audio": [ ... ],
  "scenes": [ ... ]
}
```

#### `video` (required)

| Field        | Type   | Default     | Description                                             |
| ------------ | ------ | ----------- | ------------------------------------------------------- |
| `width`      | u32    | required    | Video width in pixels. **Must be even for H.264.**      |
| `height`     | u32    | required    | Video height in pixels. **Must be even for H.264.**     |
| `fps`        | u32    | `30`        | Frames per second                                       |
| `background` | string | `"#000000"` | Default background color (hex `#RRGGBB` or `#RRGGBBAA`) |
| `codec`      | string | `null`      | `"h264"`, `"h265"`, `"vp9"`, `"prores"`                 |
| `crf`        | u8     | `null`      | Constant Rate Factor (0-51, lower = better quality)     |

#### `audio` (optional array)

| Field      | Type   | Default  | Description                                   |
| ---------- | ------ | -------- | --------------------------------------------- |
| `src`      | string | required | Path to audio file (wav, mp3, ogg, flac, aac) |
| `start`    | f64    | `0`      | Start time in seconds                         |
| `end`      | f64    | `null`   | End time (null = full duration)               |
| `volume`   | f32    | `1.0`    | Volume multiplier                             |
| `fade_in`  | f64    | `null`   | Fade in duration in seconds                   |
| `fade_out` | f64    | `null`   | Fade out duration in seconds                  |

#### `scenes` (required array)

| Field        | Type   | Default  | Description                                    |
| ------------ | ------ | -------- | ---------------------------------------------- |
| `duration`   | f64    | required | Scene duration in seconds (must be > 0)        |
| `background` | string | `null`   | Override video background for this scene       |
| `children`   | array  | `[]`     | Components rendered in order (first = back)    |
| `layout`     | object | `null`   | Scene-level flex layout (see below)            |
| `transition` | object | `null`   | Transition to this scene from the previous one |
| `freeze_at`  | f64    | `null`   | Freeze the scene at this time (seconds)        |

Each scene is an **implicit flex container** at video dimensions (like a full-screen web page). Children without `position` participate in flex flow; children with `position` are absolutely positioned. Default direction: `column`.

**`layout` options:** `direction` (column/row), `gap`, `align_items` (start/center/end/stretch), `justify_content` (start/center/end/space_between/space_around/space_evenly), `padding`

#### Include (Composable Scenarios)

Scene entries can reference external scenario files to inject their scenes inline:

```json
{
  "scenes": [
    { "include": "shared/intro.json" },
    { "duration": 5.0, "children": [...] },
    { "include": "shared/credits.json", "scenes": [0, 2] }
  ]
}
```

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `include` | string | required | Path (relative to parent) or URL to a scenario JSON file |
| `scenes` | array of usize | `null` | Only include scenes at these 0-based indices |

- The included file's `video` config is ignored
- Audio tracks from included files are merged
- Includes can be nested (max depth: 8)

#### Transitions

```json
{ "type": "fade", "duration": 0.5 }
```

**13 types:** `fade`, `wipe_left`, `wipe_right`, `wipe_up`, `wipe_down`, `zoom_in`, `zoom_out`, `flip`, `clock_wipe`, `iris`, `slide`, `dissolve`, `none`

Default duration: `0.5` seconds.

---

### Component Types

All components are discriminated by `"type"`. Rendered in array order (first = bottom). See Rule 7.

#### Common Optional Fields (root level)

| Field           | Type   | Default | Description                                      |
| --------------- | ------ | ------- | ------------------------------------------------ |
| `animations`    | array  | `[]`    | Custom keyframe animations                       |
| `preset`        | string | `null`  | Animation preset name                            |
| `preset_config` | object | `null`  | `{ "delay": 0, "duration": 0.8, "loop": false }` |
| `start_at`      | f64    | `null`  | Show component starting at this time (seconds)   |
| `end_at`        | f64    | `null`  | Hide component after this time (seconds)         |
| `wiggle`        | array  | `null`  | Procedural noise-based animation                 |
| `motion_blur`   | f32    | `null`  | Motion blur intensity                            |

#### Common Style Fields (inside `"style"`)

| Style field | Type | Default | Description |
| --- | --- | --- | --- |
| `opacity` | f32 | `1.0` | 0.0 to 1.0 |
| `padding` | f32 or {top,right,bottom,left} | `null` | Inner spacing |
| `margin` | f32 or {top,right,bottom,left} | `null` | Outer spacing |
| `glow` | object | `null` | Luminous halo effect |

#### Glow Effect

```json
{
  "style": {
    "glow": {
      "color": "#5C39EE",
      "radius": 20,
      "intensity": 2.0
    }
  }
}
```

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `color` | string | `"#FFFFFF"` | Glow color (hex) |
| `radius` | f32 | `10.0` | Blur radius |
| `intensity` | f32 | `1.0` | Brightness multiplier |

---

### 1. `text`

```json
{
  "type": "text",
  "content": "Hello World",
  "position": { "x": 100, "y": 100 },
  "max_width": 800,
  "style": {
    "font-size": 48,
    "color": "#FFFFFF",
    "font-family": "Arial",
    "font-weight": "bold",
    "text-align": "center",
    "line-height": 1.2,
    "letter-spacing": 2.0
  }
}
```

**Root fields:** `content` (required), `position`, `max_width`

| Style field       | Type     | Default    |
| ----------------- | -------- | ---------- |
| `font-size`       | f32      | `48.0`     |
| `color`           | string   | `"#FFFFFF"` |
| `font-family`     | string   | `"Inter"`  |
| `font-weight`     | enum     | `"normal"` — `"normal"`, `"bold"` |
| `font-style`      | enum     | `"normal"` — `"normal"`, `"italic"`, `"oblique"` |
| `text-align`      | enum     | `"left"` — `"left"`, `"center"`, `"right"` |
| `line-height`     | f32      | `null`     |
| `letter-spacing`  | f32      | `null`     |
| `text-shadow`     | object   | `null` — `{ "color": "#000", "offset_x": 2, "offset_y": 2, "blur": 4 }` |
| `stroke`          | object   | `null` — `{ "color": "#000", "width": 2 }` |
| `text-background` | object   | `null` — `{ "color": "#000", "padding": 4, "corner_radius": 4 }` |

### 2. `shape`

```json
{
  "type": "shape",
  "shape": "rounded_rect",
  "position": { "x": 50, "y": 50 },
  "size": { "width": 200, "height": 100 },
  "style": {
    "fill": "#FF5733",
    "border-radius": 16,
    "stroke": { "color": "#FFFFFF", "width": 2 }
  }
}
```

**Root fields:** `shape` (required), `position`, `size`, `text`

| Style field     | Type               | Default     |
| --------------- | ------------------ | ----------- |
| `fill`          | string or gradient | `null`      |
| `stroke`        | `{color, width}`   | `null`      |
| `border-radius` | f32                | `null`      |

**Shape types:** `rect`, `circle`, `rounded_rect`, `ellipse`, `triangle`, `star` (with `points`, default 5), `polygon` (with `sides`, default 6), `path` (with `data` SVG path string)

**Gradient fill:**
```json
{
  "fill": {
    "type": "linear",
    "colors": ["#FF0000", "#0000FF"],
    "angle": 45,
    "stops": [0.0, 1.0]
  }
}
```

Types: `linear`, `radial`.

**Embedded text in shapes (`text` field):**
```json
{
  "text": {
    "content": "Click me",
    "font_size": 16,
    "color": "#FFFFFF",
    "font_family": "Arial",
    "font_weight": "bold",
    "align": "center",
    "vertical_align": "middle"
  }
}
```

`vertical_align`: `"top"`, `"middle"`, `"bottom"` (default: `"middle"`). See Rule 5.

### 3. `image`

```json
{
  "type": "image",
  "src": "path/to/image.png",
  "position": { "x": 0, "y": 0 },
  "size": { "width": 400, "height": 300 },
  "fit": "cover"
}
```

| Field      | Type              | Default                                                         |
| ---------- | ----------------- | --------------------------------------------------------------- |
| `src`      | string            | required — path to image file                                   |
| `position` | `{x, y}`          | `{0, 0}`                                                        |
| `size`     | `{width, height}` | `null` (uses image dimensions)                                  |
| `fit`      | enum              | `"cover"` — options: `"cover"`, `"contain"`, `"fill"`, `"none"` |

### 4. `svg`

```json
{
  "type": "svg",
  "data": "<svg>...</svg>",
  "position": { "x": 0, "y": 0 },
  "size": { "width": 200, "height": 200 }
}
```

| Field      | Type              | Default                                                     |
| ---------- | ----------------- | ----------------------------------------------------------- |
| `src`      | string            | `null` — path to SVG file (either `src` or `data` required) |
| `data`     | string            | `null` — inline SVG markup                                  |
| `position` | `{x, y}`          | `{0, 0}`                                                    |
| `size`     | `{width, height}` | `null`                                                      |

### 5. `icon`

Renders an icon from the Iconify library. See Rule 11.

```json
{
  "type": "icon",
  "icon": "lucide:home",
  "position": { "x": 540, "y": 400 },
  "size": { "width": 64, "height": 64 },
  "style": { "color": "#38bdf8" }
}
```

| Field      | Type              | Default                                                  |
| ---------- | ----------------- | -------------------------------------------------------- |
| `icon`     | string            | required — Iconify id `"prefix:name"` (e.g. `"lucide:home"`) |
| `position` | `{x, y}`          | `{0, 0}`                                                 |
| `size`     | `{width, height}` | `{24, 24}`                                               |

Style: `color` (default `"#FFFFFF"`)

### 6. `video`

```json
{
  "type": "video",
  "src": "path/to/video.mp4",
  "position": { "x": 0, "y": 0 },
  "size": { "width": 1920, "height": 1080 },
  "trim_start": 2.0,
  "trim_end": 10.0
}
```

| Field           | Type              | Default   |
| --------------- | ----------------- | --------- |
| `src`           | string            | required  |
| `position`      | `{x, y}`          | `{0, 0}`  |
| `size`          | `{width, height}` | required  |
| `trim_start`    | f64               | `null`    |
| `trim_end`      | f64               | `null`    |
| `playback_rate` | f64               | `null`    |
| `fit`           | enum              | `"cover"` |
| `volume`        | f32               | `1.0`     |
| `loop_video`    | bool              | `null`    |

### 7. `gif`

```json
{
  "type": "gif",
  "src": "path/to/animation.gif",
  "position": { "x": 100, "y": 100 },
  "size": { "width": 200, "height": 200 }
}
```

| Field      | Type              | Default   |
| ---------- | ----------------- | --------- |
| `src`      | string            | required  |
| `position` | `{x, y}`          | `{0, 0}`  |
| `size`     | `{width, height}` | `null`    |
| `fit`      | enum              | `"cover"` |
| `loop_gif` | bool              | `true`    |

### 8. `caption`

Timed word-by-word captions with active word highlighting.

```json
{
  "type": "caption",
  "words": [
    { "text": "Hello", "start": 0.0, "end": 0.5 },
    { "text": "World", "start": 0.5, "end": 1.0 }
  ],
  "position": { "x": 540, "y": 1600 },
  "mode": "highlight",
  "max_width": 900,
  "style": { "font-size": 48, "color": "#FFFFFF" }
}
```

| Field          | Type     | Default                                                                    |
| -------------- | -------- | -------------------------------------------------------------------------- |
| `words`        | array    | required — `[{ "text", "start", "end" }]`                                  |
| `position`     | `{x, y}` | `{0, 0}`                                                                   |
| `mode`         | enum     | `"default"` — `"default"`, `"highlight"`, `"karaoke"`, `"bounce"` |
| `active_color` | string   | `"#FFD700"`                                                                |
| `max_width`    | f32      | `null`                                                                     |

Style: `font-size` (48.0), `font-family`, `color` (#FFFFFF), `background`

### 9. `counter`

Animated number counter. See Rule 4: must be standalone.

```json
{
  "type": "counter",
  "from": 0,
  "to": 1250,
  "decimals": 0,
  "separator": " ",
  "suffix": "€",
  "easing": "ease_out",
  "position": { "x": 540, "y": 960 },
  "start_at": 0.5,
  "end_at": 2.5,
  "style": { "font-size": 72, "color": "#FFFFFF", "font-weight": "bold", "text-align": "center" }
}
```

**Root fields:** `from`, `to`, `decimals`, `separator`, `prefix`, `suffix`, `easing`, `position`

**Easing options:** `linear`, `ease_in`, `ease_out`, `ease_in_out`, `ease_in_quad`, `ease_out_quad`, `ease_in_cubic`, `ease_out_cubic`, `ease_in_expo`, `ease_out_expo`, `spring`

Style: `font-size` (48.0), `color` (#FFFFFF), `font-family` (Inter), `font-weight`, `text-align`, `letter-spacing`, `text-shadow`, `stroke`

### 10. `card` / `flex`

Visual container with CSS-like flex & grid layout. `flex` is an alias for `card`. See Rule 8.

Each dimension of `size` can be a number or `"auto"`.

**Flex example:**
```json
{
  "type": "card",
  "size": { "width": 800, "height": 100 },
  "style": { "flex-direction": "row", "gap": 16 },
  "children": [
    { "type": "shape", "shape": "rect", "size": { "width": 100, "height": 100 }, "style": { "fill": "#FF0000" } },
    { "type": "shape", "shape": "rect", "size": { "width": 100, "height": 100 }, "style": { "fill": "#00FF00", "flex-grow": 1 } },
    { "type": "shape", "shape": "rect", "size": { "width": 100, "height": 100 }, "style": { "fill": "#0000FF" } }
  ]
}
```

**Grid example (2x2):**
```json
{
  "type": "card",
  "size": { "width": 600, "height": 400 },
  "style": {
    "display": "grid",
    "grid-template-columns": [{ "fr": 1 }, { "fr": 1 }],
    "grid-template-rows": [{ "fr": 1 }, { "fr": 1 }],
    "gap": 16,
    "padding": 24,
    "background": "#1a1a2e"
  },
  "children": [
    { "type": "text", "content": "Cell 1", "style": { "color": "#FFFFFF" } },
    { "type": "text", "content": "Cell 2", "style": { "color": "#FFFFFF" } },
    { "type": "text", "content": "Cell 3", "style": { "color": "#FFFFFF" } },
    { "type": "text", "content": "Cell 4", "style": { "color": "#FFFFFF" } }
  ]
}
```

**Style fields:**

| Style field              | Type        | Default    |
| ------------------------ | ----------- | ---------- |
| `display`                | enum        | `"flex"` — `"flex"` or `"grid"` |
| `background`             | string      | `null`     |
| `border-radius`          | f32         | `12.0`     |
| `border`                 | object      | `null` — `{ "color": "#E5E7EB", "width": 1 }` |
| `box-shadow`             | object      | `null` — `{ "color": "#00000040", "offset_x": 0, "offset_y": 4, "blur": 12 }` |
| `padding`                | f32 or obj  | `null`     |
| `flex-direction`         | enum        | `"column"` — `"column"`, `"row"`, `"column_reverse"`, `"row_reverse"` |
| `flex-wrap`              | bool        | `false`    |
| `align-items`            | enum        | `"start"` — `"start"`, `"center"`, `"end"`, `"stretch"` |
| `justify-content`        | enum        | `"start"` — `"start"`, `"center"`, `"end"`, `"space_between"`, `"space_around"`, `"space_evenly"` |
| `gap`                    | f32         | `0`        |
| `grid-template-columns`  | array       | `null` — `[{"px": N}, {"fr": N}, "auto"]` |
| `grid-template-rows`     | array       | `null`     |

**Per-child layout properties** (in child `"style"`):
- `flex-grow` (f32) — default 0
- `flex-shrink` (f32) — default 1
- `flex-basis` (f32) — defaults to natural size
- `align-self` (enum) — `"start"`, `"center"`, `"end"`, `"stretch"`
- `grid-column` (object) — `{ "start": 1, "span": 2 }` (1-indexed)
- `grid-row` (object) — `{ "start": 1, "span": 2 }` (1-indexed)

Children `position` is ignored in flex flow — the card computes layout from style properties.

### 12. `codeblock`

Code block with syntax highlighting, chrome, reveal animations, and animated diff transitions.

```json
{
  "type": "codeblock",
  "code": "fn main() {\n    println!(\"Hello\");\n}",
  "language": "rust",
  "theme": "base16-ocean.dark",
  "position": { "x": 200, "y": 150 },
  "show_line_numbers": true,
  "chrome": { "enabled": true, "title": "main.rs" },
  "reveal": { "mode": "typewriter", "start": 0, "duration": 2.5 },
  "style": { "font-size": 18, "border-radius": 12, "padding": 16 },
  "states": [
    {
      "code": "fn main() {\n    println!(\"Hello, world!\");\n}",
      "at": 5.0,
      "duration": 2.0,
      "cursor": { "enabled": true }
    }
  ]
}
```

**Root fields:** `code` (required), `language`, `theme`, `position`, `size`, `show_line_numbers`, `chrome`, `highlights`, `reveal`, `states`

| Style field     | Type   | Default              |
| --------------- | ------ | -------------------- |
| `font-family`   | string | `"JetBrains Mono"`   |
| `font-size`     | f32    | `14.0`               |
| `font-weight`   | enum   | `"normal"`           |
| `line-height`   | f32    | `1.5` (multiplier)   |
| `background`    | string | `null` (uses theme)  |
| `border-radius` | f32    | `12.0`               |
| `padding`       | f32 or obj | `16`             |

**Available themes (72):** `base16-ocean.dark`, `base16-ocean.light`, `base16-eighties.dark`, `base16-mocha.dark`, `InspiredGitHub`, `Solarized (dark)`, `Solarized (light)`, `catppuccin-latte`, `catppuccin-frappe`, `catppuccin-macchiato`, `catppuccin-mocha`, `andromeeda`, `aurora-x`, `ayu-dark`, `ayu-light`, `ayu-mirage`, `dark-plus`, `dracula`, `dracula-soft`, `everforest-dark`, `everforest-light`, `github-dark`, `github-dark-default`, `github-dark-dimmed`, `github-dark-high-contrast`, `github-light`, `github-light-default`, `github-light-high-contrast`, `gruvbox-dark-hard`, `gruvbox-dark-medium`, `gruvbox-dark-soft`, `gruvbox-light-hard`, `gruvbox-light-medium`, `gruvbox-light-soft`, `horizon`, `horizon-bright`, `houston`, `kanagawa-dragon`, `kanagawa-lotus`, `kanagawa-wave`, `laserwave`, `light-plus`, `material-theme`, `material-theme-darker`, `material-theme-lighter`, `material-theme-ocean`, `material-theme-palenight`, `min-dark`, `min-light`, `monokai`, `night-owl`, `night-owl-light`, `nord`, `one-dark-pro`, `one-light`, `plastic`, `poimandres`, `red`, `rose-pine`, `rose-pine-dawn`, `rose-pine-moon`, `slack-dark`, `slack-ochin`, `snazzy-light`, `solarized-dark`, `solarized-light`, `synthwave-84`, `tokyo-night`, `vesper`, `vitesse-black`, `vitesse-dark`, `vitesse-light`

---

### Animations

#### Custom Keyframe Animations

```json
{
  "animations": [
    {
      "property": "opacity",
      "keyframes": [
        { "time": 0.0, "value": 0.0 },
        { "time": 0.5, "value": 1.0 }
      ],
      "easing": "ease_out"
    }
  ]
}
```

**Animatable properties:** `opacity`, `translate_x`, `translate_y`, `scale_x`, `scale_y`, `scale` (both axes), `rotation`, `blur`, `color`

**11 easing functions:** `linear`, `ease_in`, `ease_out`, `ease_in_out`, `ease_in_quad`, `ease_out_quad`, `ease_in_cubic`, `ease_out_cubic`, `ease_in_expo`, `ease_out_expo`, `spring`

**Spring physics** (when easing is `spring`):
```json
{
  "easing": "spring",
  "spring": { "damping": 15, "stiffness": 100, "mass": 1 }
}
```

#### Animation Presets

See Rule 13 for usage guidance.

```json
{
  "preset": "fade_in_up",
  "preset_config": { "delay": 0.2, "duration": 0.8, "loop": false }
}
```

**31 presets:**

| Category   | Presets                                                                                                                                                                                                    |
| ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Entrances  | `fade_in`, `fade_in_up`, `fade_in_down`, `fade_in_left`, `fade_in_right`, `slide_in_left`, `slide_in_right`, `slide_in_up`, `slide_in_down`, `scale_in`, `bounce_in`, `blur_in`, `rotate_in`, `elastic_in` |
| Exits      | `fade_out`, `fade_out_up`, `fade_out_down`, `slide_out_left`, `slide_out_right`, `slide_out_up`, `slide_out_down`, `scale_out`, `bounce_out`, `blur_out`, `rotate_out`                                     |
| Continuous | `pulse`, `float`, `shake`, `spin` (use `"loop": true` — see Rule 9)                                                                                                                                       |
| Special    | `typewriter`, `wipe_left`, `wipe_right`                                                                                                                                                                    |

#### Wiggle (Procedural Noise)

See Rule 12 for combining with presets.

```json
{
  "wiggle": [
    { "property": "translate_x", "amplitude": 5, "frequency": 3, "seed": 42 },
    { "property": "rotation", "amplitude": 8, "frequency": 4, "seed": 13, "decay": 0.6 }
  ]
}
```

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `property` | string | required | Property to wiggle (same as animatable properties) |
| `amplitude` | f64 | required | Maximum deviation |
| `frequency` | f64 | required | Oscillations per second |
| `seed` | u64 | `0` | Random seed for reproducible results |
| `octaves` | u32 | `3` | Noise complexity |
| `phase` | f64 | `0.0` | Phase offset |
| `decay` | f64 | `null` | Exponential decay rate |
| `easing` | string | `null` | Remap noise through an easing curve |

Wiggle offsets are applied **additively** on top of keyframe animations and presets.

---

### CLI Commands

```bash
# Render a scenario file to MP4
rustmotion render scenario.json -o output.mp4

# Render from inline JSON
rustmotion render --json '{ ... }' -o output.mp4

# Validate a scenario without rendering
rustmotion validate scenario.json

# Print the JSON Schema
rustmotion schema

# Show scenario info
rustmotion info scenario.json

# Render a single frame (0-indexed) as PNG
rustmotion render scenario.json -o frame.png --frame 0

# Render with specific codec/format
rustmotion render scenario.json -o output.webm --codec vp9 --format webm

# Render as GIF
rustmotion render scenario.json -o output.gif --format gif

# Render as PNG sequence
rustmotion render scenario.json -o frames/ --format png-seq

# Machine-readable output
rustmotion render scenario.json -o output.mp4 --output-format json
```
