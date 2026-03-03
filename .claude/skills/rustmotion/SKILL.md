---
name: rustmotion
description: Best practices for Rustmotion - Video creation in Rust
metadata:
  tags: motion, video, rust, animation, composition
---

# Skill: Generate rustmotion JSON Scenarios

## What is rustmotion?

rustmotion is a CLI tool that renders motion design videos from JSON scenario files. It uses Skia for 2D rendering and supports MP4, WebM, MOV, GIF, and PNG sequence outputs.

## JSON Scenario Structure

A scenario is a JSON object with:

```json
{
  "version": "1.0",
  "video": { ... },
  "audio": [ ... ],
  "scenes": [ ... ]
}
```

### `video` (required)

| Field        | Type   | Default     | Description                                             |
| ------------ | ------ | ----------- | ------------------------------------------------------- |
| `width`      | u32    | required    | Video width in pixels. **Must be even for H.264.**      |
| `height`     | u32    | required    | Video height in pixels. **Must be even for H.264.**     |
| `fps`        | u32    | `30`        | Frames per second                                       |
| `background` | string | `"#000000"` | Default background color (hex `#RRGGBB` or `#RRGGBBAA`) |
| `codec`      | string | `null`      | `"h264"`, `"h265"`, `"vp9"`, `"prores"`                 |
| `crf`        | u8     | `null`      | Constant Rate Factor (0-51, lower = better quality)     |

### `audio` (optional array)

| Field      | Type   | Default  | Description                                   |
| ---------- | ------ | -------- | --------------------------------------------- |
| `src`      | string | required | Path to audio file (wav, mp3, ogg, flac, aac) |
| `start`    | f64    | `0`      | Start time in seconds                         |
| `end`      | f64    | `null`   | End time (null = full duration)               |
| `volume`   | f32    | `1.0`    | Volume multiplier                             |
| `fade_in`  | f64    | `null`   | Fade in duration in seconds                   |
| `fade_out` | f64    | `null`   | Fade out duration in seconds                  |

### `scenes` (required array)

| Field        | Type   | Default  | Description                                    |
| ------------ | ------ | -------- | ---------------------------------------------- |
| `duration`   | f64    | required | Scene duration in seconds (must be > 0)        |
| `background` | string | `null`   | Override video background for this scene       |
| `layers`     | array  | `[]`     | Layers rendered in order (first = back)        |
| `transition` | object | `null`   | Transition to this scene from the previous one |
| `freeze_at`  | f64    | `null`   | Freeze the scene at this time (seconds)        |

### Transitions

```json
{ "type": "fade", "duration": 0.5 }
```

**13 transition types:** `fade`, `wipe_left`, `wipe_right`, `wipe_up`, `wipe_down`, `zoom_in`, `zoom_out`, `flip`, `clock_wipe`, `iris`, `slide`, `dissolve`, `none`

Default transition duration: `0.5` seconds.

---

## Layer Types

All layers are discriminated by the `"type"` field. Layers are rendered in array order (first = bottom).

### Common optional fields (on most layers)

| Field           | Type   | Default | Description                                      |
| --------------- | ------ | ------- | ------------------------------------------------ |
| `opacity`       | f32    | `1.0`   | 0.0 to 1.0                                       |
| `animations`    | array  | `[]`    | Custom keyframe animations                       |
| `preset`        | string | `null`  | Animation preset name                            |
| `preset_config` | object | `null`  | `{ "delay": 0, "duration": 0.8, "loop": false }` |
| `start_at`      | f64    | `null`  | Show layer starting at this time (seconds)       |
| `end_at`        | f64    | `null`  | Hide layer after this time (seconds)             |
| `wiggle`        | array  | `null`  | Procedural noise-based animation                 |
| `motion_blur`   | f32    | `null`  | Motion blur intensity                            |

### 1. `text`

```json
{
  "type": "text",
  "content": "Hello World",
  "position": { "x": 100, "y": 100 },
  "font_size": 48,
  "color": "#FFFFFF",
  "font_family": "Arial",
  "font_weight": "bold",
  "align": "center",
  "max_width": 800,
  "line_height": 1.2,
  "letter_spacing": 2.0
}
```

| Field            | Type     | Default                                               |
| ---------------- | -------- | ----------------------------------------------------- |
| `content`        | string   | required                                              |
| `position`       | `{x, y}` | `{0, 0}`                                              |
| `font_size`      | f32      | `24.0`                                                |
| `color`          | string   | `"#FFFFFF"`                                           |
| `font_family`    | string   | `"Arial"`                                             |
| `font_weight`    | enum     | `"normal"` — options: `"normal"`, `"bold"`, `"light"` |
| `align`          | enum     | `"left"` — options: `"left"`, `"center"`, `"right"`   |
| `max_width`      | f32      | `null`                                                |
| `line_height`    | f32      | `null`                                                |
| `letter_spacing` | f32      | `null`                                                |

### 2. `shape`

```json
{
  "type": "shape",
  "shape": "rounded_rect",
  "position": { "x": 50, "y": 50 },
  "size": { "width": 200, "height": 100 },
  "fill": "#FF5733",
  "corner_radius": 16,
  "stroke": { "color": "#FFFFFF", "width": 2 }
}
```

| Field           | Type               | Default                            |
| --------------- | ------------------ | ---------------------------------- |
| `shape`         | enum               | required                           |
| `position`      | `{x, y}`           | `{0, 0}`                           |
| `size`          | `{width, height}`  | `{100, 100}`                       |
| `fill`          | string or gradient | `null`                             |
| `stroke`        | `{color, width}`   | `null`                             |
| `corner_radius` | f32                | `null` (for `rounded_rect`)        |
| `text`          | object             | `null` — embedded text (see below) |

**Shape types:** `rect`, `circle`, `rounded_rect`, `ellipse`, `triangle`, `star` (with `points`, default 5), `polygon` (with `sides`, default 6), `path` (with `data` SVG path string)

**Fill can be a gradient:**

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

Gradient types: `linear`, `radial`.

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
    "vertical_align": "center"
  }
}
```

`vertical_align`: `"top"`, `"center"`, `"bottom"` (default: `"center"`).

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

### 5. `video`

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

### 6. `gif`

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

### 7. `caption`

Timed word-by-word captions with active word highlighting.

```json
{
  "type": "caption",
  "words": [
    { "text": "Hello", "start": 0.0, "end": 0.5 },
    { "text": "World", "start": 0.5, "end": 1.0 }
  ],
  "position": { "x": 540, "y": 1600 },
  "font_size": 48,
  "color": "#FFFFFF",
  "active_color": "#FFD700",
  "style": "highlight",
  "max_width": 900
}
```

| Field          | Type     | Default                                                                    |
| -------------- | -------- | -------------------------------------------------------------------------- |
| `words`        | array    | required — `[{ "text", "start", "end" }]`                                  |
| `position`     | `{x, y}` | `{0, 0}`                                                                   |
| `font_size`    | f32      | `24.0`                                                                     |
| `font_family`  | string   | `null`                                                                     |
| `color`        | string   | `"#FFFFFF"`                                                                |
| `active_color` | string   | `"#FFD700"`                                                                |
| `background`   | string   | `null`                                                                     |
| `style`        | enum     | `"default"` — options: `"default"`, `"highlight"`, `"karaoke"`, `"bounce"` |
| `max_width`    | f32      | `null`                                                                     |

### 8. `counter`

Animated number counter that interpolates from a start value to an end value.

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
  "font_size": 72,
  "color": "#FFFFFF",
  "font_weight": "bold",
  "align": "center",
  "start_at": 0.5,
  "end_at": 2.5,
  "preset": "fade_in_up"
}
```

| Field       | Type   | Default     |
| ----------- | ------ | ----------- |
| `from`      | f64    | required    |
| `to`        | f64    | required    |
| `decimals`  | u8     | `0`         |
| `separator` | string | `null` — thousands separator (e.g. `" "`, `","`) |
| `prefix`    | string | `null` — text before the number (e.g. `"$"`, `"+"`) |
| `suffix`    | string | `null` — text after the number (e.g. `"%"`, `"€"`) |
| `easing`    | enum   | `"linear"` — easing for the counter interpolation |
| `position`  | `{x, y}` | `{0, 0}` |
| `font_size` | f32    | `48.0`      |
| `color`     | string | `"#FFFFFF"`  |
| `font_family` | string | `"Inter"` |
| `font_weight` | enum | `"normal"` — `"normal"`, `"bold"` |
| `align`     | enum   | `"left"` — `"left"`, `"center"`, `"right"` |
| `letter_spacing` | f32 | `null`   |
| `shadow`    | object | `null` — `{ "color": "#00000080", "offset_x": 2, "offset_y": 2, "blur": 4 }` |
| `stroke`    | object | `null` — `{ "color": "#000", "width": 2 }` |

The counter animates over the layer's visible duration (`start_at` to `end_at`, or full scene). The `easing` controls the number interpolation curve, independent from visual animation presets.

### 9. `group`

Groups multiple layers with shared position and animations.

```json
{
  "type": "group",
  "position": { "x": 100, "y": 100 },
  "layers": [ ... ],
  "preset": "fade_in"
}
```

| Field      | Type     | Default              |
| ---------- | -------- | -------------------- |
| `position` | `{x, y}` | `{0, 0}`             |
| `layers`   | array    | `[]` — nested layers |

### 10. `card`

Visual container with flexbox layout. Unlike `group` (absolute positioning, no style), `card` auto-positions children and supports background, border, shadow, padding, corner radius.

```json
{
  "type": "card",
  "position": { "x": 100, "y": 200 },
  "size": { "width": 400 },
  "background": "#FFFFFF",
  "corner_radius": 16,
  "padding": 24,
  "gap": 16,
  "direction": "column",
  "align": "start",
  "shadow": { "color": "#00000040", "offset_x": 0, "offset_y": 4, "blur": 12 },
  "border": { "color": "#E5E7EB", "width": 1 },
  "layers": [
    { "type": "text", "content": "Title", "font_size": 32, "font_weight": "bold", "color": "#111827" },
    { "type": "text", "content": "Description", "font_size": 18, "color": "#6B7280" }
  ],
  "preset": "fade_in_up"
}
```

| Field | Type | Default |
| --- | --- | --- |
| `position` | `{x, y}` | `{0, 0}` |
| `size` | `{width, height}` | `null` (auto-calculated from children) |
| `background` | string | `null` — hex background color |
| `corner_radius` | f32 | `12.0` |
| `border` | object | `null` — `{ "color": "#E5E7EB", "width": 1 }` |
| `shadow` | object | `null` — `{ "color": "#00000040", "offset_x": 0, "offset_y": 4, "blur": 12 }` |
| `padding` | f32 or object | `null` — uniform `24` or `{ "top": 24, "right": 24, "bottom": 24, "left": 24 }` |
| `direction` | enum | `"column"` — `"column"` (vertical) or `"row"` (horizontal) |
| `wrap` | bool | `false` — wrap children to next line |
| `align` | enum | `"start"` — cross-axis: `"start"`, `"center"`, `"end"` |
| `justify` | enum | `"start"` — main-axis: `"start"`, `"center"`, `"end"`, `"space_between"`, `"space_around"` |
| `gap` | f32 | `0` — spacing between children in pixels |
| `layers` | array | `[]` — child layers (positioned automatically, `position` field ignored) |

Children `position` is ignored — the card computes layout from `direction`, `align`, `justify`, `gap`, and `wrap`. Supports all common fields (animations, presets, timing, wiggle, motion_blur).

### 11. `codeblock`

Code block with syntax highlighting, carbon.now.sh chrome, reveal animations, and animated diff transitions.

```json
{
  "type": "codeblock",
  "code": "fn main() {\n    println!(\"Hello\");\n}",
  "language": "rust",
  "theme": "base16-ocean.dark",
  "position": { "x": 200, "y": 150 },
  "font_size": 18,
  "show_line_numbers": true,
  "chrome": { "enabled": true, "title": "main.rs" },
  "reveal": { "mode": "typewriter", "start": 0, "duration": 2.5 },
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

| Field | Type | Default |
| --- | --- | --- |
| `code` | string | required |
| `language` | string | `"plain"` — e.g. `"rust"`, `"javascript"`, `"python"`, `"go"`, `"typescript"` |
| `theme` | string | `"base16-ocean.dark"` — 72 themes available (see below) |
| `position` | `{x, y}` | `{0, 0}` |
| `size` | `{width, height}` | `null` (auto-calculated) |
| `font_family` | string | `"JetBrains Mono"` |
| `font_size` | f32 | `16.0` |
| `font_weight` | u16 | `400` — 100=Thin, 300=Light, 400=Normal, 500=Medium, 600=SemiBold, 700=Bold, 900=Black |
| `line_height` | f32 | `1.5` (multiplier) |
| `background` | string | `null` (uses theme bg) |
| `show_line_numbers` | bool | `false` |
| `corner_radius` | f32 | `12.0` |
| `padding` | `{top, right, bottom, left}` | `{16, 16, 16, 16}` |
| `chrome` | object | `null` — `{ "enabled": true, "title": "file.rs", "color": "#343d46" }` |
| `highlights` | array | `[]` — `[{ "lines": [2], "color": "#FFFF0033", "start": 1.0, "end": 3.0 }]` |
| `reveal` | object | `null` — `{ "mode": "typewriter"|"line_by_line", "start": 0, "duration": 1.0, "easing": "linear" }` |
| `states` | array | `[]` — code mutations with diff transitions |

**States (diff transitions):**

```json
{
  "states": [{
    "code": "new code...",
    "at": 5.0,
    "duration": 0.6,
    "easing": "ease_in_out",
    "cursor": { "enabled": true, "color": "#FFFFFF", "width": 2.0, "blink": true },
    "highlights": [{ "lines": [1], "color": "#FFFF0022" }]
  }]
}
```

Unchanged lines slide to new positions, deleted lines fade out, inserted lines fade in, modified lines show a cursor editing effect (backspace then type new text).

**Available themes (72):** `base16-ocean.dark`, `base16-ocean.light`, `base16-eighties.dark`, `base16-mocha.dark`, `InspiredGitHub`, `Solarized (dark)`, `Solarized (light)`, `catppuccin-latte`, `catppuccin-frappe`, `catppuccin-macchiato`, `catppuccin-mocha`, `andromeeda`, `aurora-x`, `ayu-dark`, `ayu-light`, `ayu-mirage`, `dark-plus`, `dracula`, `dracula-soft`, `everforest-dark`, `everforest-light`, `github-dark`, `github-dark-default`, `github-dark-dimmed`, `github-dark-high-contrast`, `github-light`, `github-light-default`, `github-light-high-contrast`, `gruvbox-dark-hard`, `gruvbox-dark-medium`, `gruvbox-dark-soft`, `gruvbox-light-hard`, `gruvbox-light-medium`, `gruvbox-light-soft`, `horizon`, `horizon-bright`, `houston`, `kanagawa-dragon`, `kanagawa-lotus`, `kanagawa-wave`, `laserwave`, `light-plus`, `material-theme`, `material-theme-darker`, `material-theme-lighter`, `material-theme-ocean`, `material-theme-palenight`, `min-dark`, `min-light`, `monokai`, `night-owl`, `night-owl-light`, `nord`, `one-dark-pro`, `one-light`, `plastic`, `poimandres`, `red`, `rose-pine`, `rose-pine-dawn`, `rose-pine-moon`, `slack-dark`, `slack-ochin`, `snazzy-light`, `solarized-dark`, `solarized-light`, `synthwave-84`, `tokyo-night`, `vesper`, `vitesse-black`, `vitesse-dark`, `vitesse-light`

---

## Animations

### Custom Keyframe Animations

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

### Animation Presets

Use `"preset"` instead of manual keyframes:

```json
{
  "type": "text",
  "content": "Hello",
  "position": { "x": 540, "y": 500 },
  "preset": "fade_in_up",
  "preset_config": { "delay": 0.2, "duration": 0.8, "loop": false }
}
```

**31 presets:**

| Category   | Presets                                                                                                                                                                                                    |
| ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Entrances  | `fade_in`, `fade_in_up`, `fade_in_down`, `fade_in_left`, `fade_in_right`, `slide_in_left`, `slide_in_right`, `slide_in_up`, `slide_in_down`, `scale_in`, `bounce_in`, `blur_in`, `rotate_in`, `elastic_in` |
| Exits      | `fade_out`, `fade_out_up`, `fade_out_down`, `slide_out_left`, `slide_out_right`, `slide_out_up`, `slide_out_down`, `scale_out`, `bounce_out`, `blur_out`, `rotate_out`                                     |
| Continuous | `pulse`, `float`, `shake`, `spin` (use `"loop": true` in preset_config)                                                                                                                                    |
| Special    | `typewriter`, `wipe_left`, `wipe_right`                                                                                                                                                                    |

### Wiggle (Procedural Noise)

```json
{
  "wiggle": [
    { "property": "translate_x", "amplitude": 5, "frequency": 3, "seed": 42 }
  ]
}
```

---

## Minimal Complete Example

```json
{
  "video": {
    "width": 1080,
    "height": 1920,
    "fps": 30,
    "background": "#0f172a"
  },
  "scenes": [
    {
      "duration": 3.0,
      "layers": [
        {
          "type": "shape",
          "shape": "rounded_rect",
          "position": { "x": 140, "y": 760 },
          "size": { "width": 800, "height": 400 },
          "fill": {
            "type": "linear",
            "colors": ["#6366f1", "#8b5cf6"],
            "angle": 135
          },
          "corner_radius": 24,
          "preset": "scale_in"
        },
        {
          "type": "text",
          "content": "Hello rustmotion!",
          "position": { "x": 540, "y": 940 },
          "font_size": 56,
          "color": "#FFFFFF",
          "font_weight": "bold",
          "align": "center",
          "preset": "fade_in_up",
          "preset_config": { "delay": 0.3, "duration": 0.8 }
        }
      ]
    },
    {
      "duration": 2.0,
      "transition": { "type": "fade", "duration": 0.5 },
      "layers": [
        {
          "type": "text",
          "content": "Powered by Rust + Skia",
          "position": { "x": 540, "y": 960 },
          "font_size": 36,
          "color": "#94a3b8",
          "align": "center",
          "preset": "fade_in"
        }
      ]
    }
  ]
}
```

---

## Validation obligatoire du JSON généré

**Après chaque génération de JSON**, tu DOIS valider le scénario avant de le présenter à l'utilisateur :

1. **Écrire** le JSON dans un fichier temporaire (ex: `/tmp/scenario.json`)
2. **Exécuter** `rustmotion validate /tmp/scenario.json` pour vérifier la validité
3. **Si la validation échoue** : corriger les erreurs et re-valider jusqu'à ce que ça passe
4. **Si la validation réussit** : présenter le JSON à l'utilisateur

Ne jamais proposer un JSON qui n'a pas été validé par `rustmotion validate`.

---

## CLI Commands

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

---

## Important Constraints

1. **Even dimensions**: `width` and `height` must be even numbers for H.264 encoding
2. **Common resolutions**: 1080x1920 (portrait 9:16), 1920x1080 (landscape 16:9), 1080x1080 (square)
3. **Timing**: `start_at` must be < `end_at` when both are specified
4. **Layer order**: Layers render bottom-to-top (first layer in array = behind)
5. **File paths**: Image, video, GIF, SVG `src` paths are relative to the working directory
6. **SVG layers**: Must have either `src` or `data`, not both empty
7. **At least one scene** is required, each with `duration > 0`
8. **Colors**: Use hex format `#RRGGBB` or `#RRGGBBAA`
9. **Presets vs animations**: `preset` is a shorthand; explicit `animations` override preset animations on the same property
10. **Continuous presets** (`pulse`, `float`, `shake`, `spin`) should use `"loop": true` in `preset_config`
