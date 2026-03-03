# rustmotion

A CLI tool that renders motion design videos from JSON scenarios. No browser, no Node.js — just a single Rust binary.

[![Crates.io](https://img.shields.io/crates/v/rustmotion.svg)](https://crates.io/crates/rustmotion)
[![docs.rs](https://docs.rs/rustmotion/badge.svg)](https://docs.rs/sqlx-gen)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## Install

```bash
cargo install rustmotion
```

**Requirements:** Rust toolchain + C++ compiler (for openh264). Optional: `ffmpeg` CLI for H.265/VP9/ProRes/WebM/GIF output.

## Quick Start

```bash
# Render a video
rustmotion render scenario.json -o video.mp4

# Render with a specific codec
rustmotion render scenario.json -o video.webm --codec vp9 --crf 30

# Export as PNG sequence
rustmotion render scenario.json -o frames/ --format png-seq

# Export as animated GIF
rustmotion render scenario.json -o output.gif --format gif

# Render a single frame for preview
rustmotion render scenario.json --frame 42 -o frame.png

# Validate without rendering
rustmotion validate scenario.json

# Export JSON Schema (for editor autocompletion or LLM prompts)
rustmotion schema -o schema.json

# Show scenario info
rustmotion info scenario.json
```

## CLI Reference

### `rustmotion render`

| Flag | Description | Default |
|---|---|---|
| `input` | Path to the JSON scenario file | (required) |
| `-o, --output` | Output file path | `output.mp4` |
| `--frame <N>` | Render a single frame to PNG (0-indexed) | |
| `--codec <CODEC>` | Video codec: `h264`, `h265`, `vp9`, `prores` | `h264` |
| `--crf <0-51>` | Constant Rate Factor (lower = better quality) | `23` |
| `--format <FMT>` | Output format: `mp4`, `webm`, `mov`, `gif`, `png-seq` | auto from extension |
| `--transparent` | Transparent background (PNG sequence, WebM, ProRes 4444) | `false` |
| `--output-format json` | Machine-readable JSON output for CI pipelines | |
| `-q, --quiet` | Suppress all output except errors | |

---

## JSON Scenario Format

```json
{
  "version": "1.0",
  "video": { ... },
  "audio": [ ... ],
  "scenes": [ ... ]
}
```

### Video Config

```json
{
  "video": {
    "width": 1080,
    "height": 1920,
    "fps": 30,
    "background": "#0f172a",
    "codec": "h264",
    "crf": 23
  }
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `width` | `u32` | (required) | Video width in pixels (must be even) |
| `height` | `u32` | (required) | Video height in pixels (must be even) |
| `fps` | `u32` | `30` | Frames per second |
| `background` | `string` | `"#000000"` | Default background color (hex) |
| `codec` | `string` | `"h264"` | Video codec: `h264`, `h265`, `vp9`, `prores` |
| `crf` | `u8` | `23` | Constant Rate Factor (0-51, lower = better quality) |

### Audio Tracks

```json
{
  "audio": [
    {
      "src": "music.mp3",
      "start": 0.0,
      "end": 10.0,
      "volume": 0.8,
      "fade_in": 1.0,
      "fade_out": 2.0
    }
  ]
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `src` | `string` | (required) | Path to audio file (MP3, WAV, OGG, FLAC, AAC) |
| `start` | `f64` | `0.0` | Start time in the output video (seconds) |
| `end` | `f64` | | End time (omit for full track) |
| `volume` | `f32` | `1.0` | Volume multiplier (0.0 - 1.0) |
| `fade_in` | `f64` | | Fade-in duration (seconds) |
| `fade_out` | `f64` | | Fade-out duration (seconds) |

---

## Scenes

Each scene has a duration, optional background, layers rendered in order, and an optional transition to the next scene.

```json
{
  "scenes": [
    {
      "duration": 3.0,
      "background": "#1a1a2e",
      "freeze_at": 2.5,
      "layers": [ ... ],
      "transition": {
        "type": "fade",
        "duration": 0.5
      }
    }
  ]
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `duration` | `f64` | (required) | Scene duration in seconds |
| `background` | `string` | | Scene background (overrides `video.background`) |
| `freeze_at` | `f64` | | Freeze the scene at this time (seconds). All frames after this point render the frozen state |
| `layers` | `Layer[]` | `[]` | Layers rendered bottom-to-top |
| `transition` | `Transition` | | Transition effect to the next scene |

---

## Transitions

Transitions blend between two consecutive scenes. Set on the **second** scene.

```json
{
  "transition": {
    "type": "clock_wipe",
    "duration": 0.8
  }
}
```

| Type | Description |
|---|---|
| `fade` | Linear crossfade between scenes |
| `wipe_left` | Horizontal wipe revealing scene B from the left |
| `wipe_right` | Horizontal wipe revealing scene B from the right |
| `wipe_up` | Vertical wipe revealing scene B from the top |
| `wipe_down` | Vertical wipe revealing scene B from the bottom |
| `zoom_in` | Scene A zooms in and fades out, revealing scene B |
| `zoom_out` | Scene B zooms out from larger to normal size |
| `flip` | 3D Y-axis flip simulation (scene A folds away, scene B unfolds) |
| `clock_wipe` | Circular clockwise sweep from 12 o'clock |
| `iris` | Expanding circle from the center reveals scene B |
| `slide` | Scene B pushes scene A to the left |
| `dissolve` | Per-pixel noise dissolve (each pixel switches independently) |
| `none` | Hard cut at the midpoint |

| Field | Type | Default | Description |
|---|---|---|---|
| `type` | `string` | (required) | One of the transition types above |
| `duration` | `f64` | `0.5` | Transition duration in seconds |

---

## Layers

All layers share a common set of fields for animation, timing, and effects, plus type-specific fields. Layers are rendered in array order (first = bottom, last = top).

### Common Fields

These fields are available on all layer types (except `group` and `caption` where noted):

| Field | Type | Default | Description |
|---|---|---|---|
| `position` | `{x, y}` | `{x: 0, y: 0}` | Position in pixels |
| `opacity` | `f32` | `1.0` | Layer opacity (0.0 - 1.0) |
| `animations` | `Animation[]` | `[]` | Custom keyframe animations |
| `preset` | `string` | | Preset animation name (see [Animation Presets](#animation-presets)) |
| `preset_config` | `PresetConfig` | | Preset timing configuration |
| `start_at` | `f64` | | Layer appears at this time (seconds within scene) |
| `end_at` | `f64` | | Layer disappears after this time (seconds within scene) |
| `wiggle` | `WiggleConfig[]` | | Procedural noise-based animation (see [Wiggle](#wiggle)) |
| `motion_blur` | `f32` | | Motion blur intensity (0.0 - 1.0). Uses temporal multi-sampling |

---

### Text Layer

```json
{
  "type": "text",
  "content": "Hello World",
  "position": { "x": 540, "y": 960 },
  "font_size": 64,
  "color": "#FFFFFF",
  "font_family": "Inter",
  "font_weight": "bold",
  "align": "center",
  "max_width": 800,
  "line_height": 80,
  "letter_spacing": 2.0,
  "preset": "fade_in_up"
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `content` | `string` | (required) | Text to display. Supports `\n` for line breaks |
| `font_size` | `f32` | `48.0` | Font size in pixels |
| `color` | `string` | `"#FFFFFF"` | Text color (hex). Can be animated via `"color"` property |
| `font_family` | `string` | `"Inter"` | Font family name (uses system fonts) |
| `font_weight` | `string` | `"normal"` | `"normal"` or `"bold"` |
| `align` | `string` | `"left"` | `"left"`, `"center"`, or `"right"` |
| `max_width` | `f32` | | Maximum text width before word-wrapping |
| `line_height` | `f32` | `font_size * 1.3` | Line height in pixels |
| `letter_spacing` | `f32` | `0.0` | Additional spacing between characters |

---

### Shape Layer

```json
{
  "type": "shape",
  "shape": "rounded_rect",
  "position": { "x": 100, "y": 200 },
  "size": { "width": 300, "height": 200 },
  "corner_radius": 16,
  "fill": "#3b82f6",
  "stroke": { "color": "#ffffff", "width": 2 },
  "preset": "scale_in"
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `shape` | `ShapeType` | (required) | Shape type (see below) |
| `size` | `{width, height}` | `{width: 100, height: 100}` | Shape dimensions |
| `fill` | `string \| Gradient` | | Fill color (hex string) or gradient object |
| `stroke` | `Stroke` | | Stroke outline |
| `corner_radius` | `f32` | `8.0` | Corner radius (for `rounded_rect`) |

#### Shape Types

| Type | Description | Extra Fields |
|---|---|---|
| `"rect"` | Rectangle | |
| `"circle"` | Circle (fits inside size rect) | |
| `"rounded_rect"` | Rectangle with rounded corners | `corner_radius` |
| `"ellipse"` | Ellipse (fits inside size rect) | |
| `"triangle"` | Equilateral triangle pointing up | |
| `"star"` | Star polygon | `points` (default: `5`) |
| `"polygon"` | Regular polygon | `sides` (default: `6`) |
| `"path"` | SVG path | `data` (SVG path string, required) |

**Star and polygon examples:**

```json
{ "shape": { "star": { "points": 6 } } }
{ "shape": { "polygon": { "sides": 8 } } }
{ "shape": { "path": { "data": "M 10 80 C 40 10, 65 10, 95 80 S 150 150, 180 80" } } }
```

#### Fill

Fill accepts either a solid hex color string or a gradient object:

```json
"fill": "#ff6b6b"
```

```json
"fill": {
  "type": "linear",
  "colors": ["#667eea", "#764ba2"],
  "angle": 135,
  "stops": [0.0, 1.0]
}
```

| Gradient Field | Type | Default | Description |
|---|---|---|---|
| `type` | `string` | (required) | `"linear"` or `"radial"` |
| `colors` | `string[]` | (required) | Array of hex colors |
| `stops` | `f32[]` | | Color stop positions (0.0 - 1.0) |
| `angle` | `f32` | `0.0` | Angle in degrees (linear gradients only) |

#### Stroke

| Field | Type | Default | Description |
|---|---|---|---|
| `color` | `string` | (required) | Stroke color (hex) |
| `width` | `f32` | `2.0` | Stroke width in pixels |

---

### Image Layer

```json
{
  "type": "image",
  "src": "photo.png",
  "position": { "x": 0, "y": 0 },
  "size": { "width": 1080, "height": 1080 },
  "fit": "cover",
  "preset": "fade_in"
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `src` | `string` | (required) | Path to image file (PNG, JPEG, WebP) |
| `size` | `{width, height}` | | Target size (uses native image size if omitted) |
| `fit` | `string` | `"contain"` | `"cover"`, `"contain"`, or `"fill"` |

---

### SVG Layer

```json
{
  "type": "svg",
  "src": "icon.svg",
  "position": { "x": 100, "y": 100 },
  "size": { "width": 200, "height": 200 }
}
```

Or with inline SVG:

```json
{
  "type": "svg",
  "data": "<svg viewBox='0 0 100 100'><circle cx='50' cy='50' r='40' fill='red'/></svg>",
  "position": { "x": 100, "y": 100 }
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `src` | `string` | | Path to `.svg` file |
| `data` | `string` | | Inline SVG markup |
| `size` | `{width, height}` | | Target size (uses SVG intrinsic size if omitted) |

One of `src` or `data` is required.

---

### Video Layer

Embeds a video clip as a layer. Requires `ffmpeg` on PATH.

```json
{
  "type": "video",
  "src": "clip.mp4",
  "position": { "x": 0, "y": 0 },
  "size": { "width": 1080, "height": 1920 },
  "trim_start": 2.0,
  "trim_end": 8.0,
  "playback_rate": 0.5,
  "fit": "cover",
  "volume": 0.0
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `src` | `string` | (required) | Path to video file |
| `size` | `{width, height}` | (required) | Display size |
| `trim_start` | `f64` | `0.0` | Start offset in the source clip (seconds) |
| `trim_end` | `f64` | | End offset in the source clip (seconds) |
| `playback_rate` | `f64` | `1.0` | Playback speed (0.5 = half speed, 2.0 = double) |
| `fit` | `string` | `"contain"` | `"cover"`, `"contain"`, or `"fill"` |
| `volume` | `f32` | `1.0` | Audio volume (0.0 = mute) |
| `loop_video` | `bool` | | Loop the clip |

---

### GIF Layer

Displays an animated GIF, synced to the scene timeline.

```json
{
  "type": "gif",
  "src": "animation.gif",
  "position": { "x": 100, "y": 100 },
  "size": { "width": 300, "height": 300 },
  "loop_gif": true,
  "fit": "contain"
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `src` | `string` | (required) | Path to `.gif` file |
| `size` | `{width, height}` | | Display size (uses GIF native size if omitted) |
| `fit` | `string` | `"contain"` | `"cover"`, `"contain"`, or `"fill"` |
| `loop_gif` | `bool` | `true` | Loop the GIF animation |

---

### Caption Layer

TikTok-style word-by-word subtitles with active word highlighting.

```json
{
  "type": "caption",
  "words": [
    { "text": "Hello", "start": 0.0, "end": 0.5 },
    { "text": "world!", "start": 0.5, "end": 1.0 }
  ],
  "position": { "x": 540, "y": 1600 },
  "font_size": 56,
  "color": "#FFFFFF",
  "active_color": "#FFFF00",
  "background": "#00000088",
  "style": "highlight",
  "max_width": 900
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `words` | `CaptionWord[]` | (required) | Array of timed words |
| `font_size` | `f32` | `48.0` | Font size |
| `font_family` | `string` | `"Inter"` | Font family |
| `color` | `string` | `"#FFFFFF"` | Default (inactive) word color |
| `active_color` | `string` | `"#FFFF00"` | Active word color |
| `background` | `string` | | Background pill color (hex with alpha, e.g. `"#00000088"`) |
| `style` | `string` | `"highlight"` | Caption style (see below) |
| `max_width` | `f32` | | Maximum width before word-wrapping |

#### CaptionWord

| Field | Type | Description |
|---|---|---|
| `text` | `string` | The word text |
| `start` | `f64` | Start timestamp (seconds, within scene) |
| `end` | `f64` | End timestamp (seconds, within scene) |

#### Caption Styles

| Style | Description |
|---|---|
| `"highlight"` | All words visible, active word changes color |
| `"karaoke"` | All words visible, active word highlighted (same rendering as highlight) |
| `"word_by_word"` | Only the active word is shown at a time |

---

### Group Layer

Groups nested layers with a shared position and opacity.

```json
{
  "type": "group",
  "position": { "x": 100, "y": 100 },
  "opacity": 0.8,
  "layers": [
    { "type": "shape", "shape": "rect", "size": { "width": 400, "height": 300 }, "fill": "#1a1a2e" },
    { "type": "text", "content": "Inside group", "position": { "x": 50, "y": 150 } }
  ]
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `layers` | `Layer[]` | `[]` | Nested layers (positions relative to group) |
| `position` | `{x, y}` | `{x: 0, y: 0}` | Group offset |
| `opacity` | `f32` | `1.0` | Group opacity (applied to all children) |

---

## Animations

### Custom Keyframe Animations

Animate any property over time with keyframes:

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
    },
    {
      "property": "color",
      "keyframes": [
        { "time": 0.0, "value": "#FF0000" },
        { "time": 1.0, "value": "#0000FF" }
      ],
      "easing": "linear"
    }
  ]
}
```

#### Animatable Properties

| Property | Type | Description |
|---|---|---|
| `opacity` | number | Layer opacity (0.0 - 1.0) |
| `position.x` | number | Horizontal offset in pixels |
| `position.y` | number | Vertical offset in pixels |
| `scale` | number | Uniform scale (1.0 = 100%) |
| `scale.x` | number | Horizontal scale |
| `scale.y` | number | Vertical scale |
| `rotation` | number | Rotation in degrees |
| `blur` | number | Gaussian blur radius |
| `visible_chars` | number | Number of visible characters (for text) |
| `visible_chars_progress` | number | Character reveal progress 0.0 - 1.0 (for text) |
| `color` | color | Color interpolation (hex strings, e.g. `"#FF0000"`) |

#### Easing Functions

| Easing | Description |
|---|---|
| `linear` | Constant speed |
| `ease_in` | Cubic ease in (slow start) |
| `ease_out` | Cubic ease out (slow end) — **default** |
| `ease_in_out` | Cubic ease in and out |
| `ease_in_quad` | Quadratic ease in |
| `ease_out_quad` | Quadratic ease out |
| `ease_in_cubic` | Cubic ease in |
| `ease_out_cubic` | Cubic ease out |
| `ease_in_expo` | Exponential ease in |
| `ease_out_expo` | Exponential ease out |
| `spring` | Spring physics (uses `spring` config) |

#### Spring Config

When using `"easing": "spring"`, provide a `spring` object:

```json
{
  "easing": "spring",
  "spring": {
    "damping": 12.0,
    "stiffness": 100.0,
    "mass": 1.0
  }
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `damping` | `f64` | `15.0` | Damping coefficient (higher = less oscillation) |
| `stiffness` | `f64` | `100.0` | Spring stiffness (higher = faster) |
| `mass` | `f64` | `1.0` | Mass (higher = slower, more inertia) |

---

### Animation Presets

Presets are ready-to-use animations. Set `preset` on any layer:

```json
{
  "type": "text",
  "content": "Animated!",
  "preset": "bounce_in",
  "preset_config": {
    "delay": 0.2,
    "duration": 0.8,
    "loop": false
  }
}
```

#### Preset Config

| Field | Type | Default | Description |
|---|---|---|---|
| `delay` | `f64` | `0.0` | Delay before animation starts (seconds) |
| `duration` | `f64` | `0.8` | Animation duration (seconds) |
| `loop` | `bool` | `false` | Loop the animation continuously |

#### Entrance Presets

| Preset | Description |
|---|---|
| `fade_in` | Fade from transparent |
| `fade_in_up` | Fade in + slide up |
| `fade_in_down` | Fade in + slide down |
| `fade_in_left` | Fade in + slide from left |
| `fade_in_right` | Fade in + slide from right |
| `slide_in_left` | Slide in from far left |
| `slide_in_right` | Slide in from far right |
| `slide_in_up` | Slide in from below |
| `slide_in_down` | Slide in from above |
| `scale_in` | Scale up from 0 with spring bounce |
| `bounce_in` | Bouncy scale from small to normal |
| `blur_in` | Fade in from blurred |
| `rotate_in` | Rotate + scale from half size |
| `elastic_in` | Elastic underdamped spring scale |

#### Exit Presets

| Preset | Description |
|---|---|
| `fade_out` | Fade to transparent |
| `fade_out_up` | Fade out + slide up |
| `fade_out_down` | Fade out + slide down |
| `slide_out_left` | Slide out to the left |
| `slide_out_right` | Slide out to the right |
| `slide_out_up` | Slide out upward |
| `slide_out_down` | Slide out downward |
| `scale_out` | Scale down to 0 |
| `bounce_out` | Bouncy scale to small |
| `blur_out` | Fade out with blur |
| `rotate_out` | Rotate + scale to half size |

#### Continuous Presets

These presets loop automatically when `"loop": true` is set in `preset_config`:

| Preset | Description |
|---|---|
| `pulse` | Gentle scale oscillation (1.0 - 1.05) |
| `float` | Vertical floating motion |
| `shake` | Horizontal shake |
| `spin` | 360-degree continuous rotation |

#### Special Presets

| Preset | Description |
|---|---|
| `typewriter` | Progressive character reveal (left to right) |
| `wipe_left` | Slide in from left with fade |
| `wipe_right` | Slide in from right with fade |

---

## Wiggle

Wiggle adds procedural noise-based motion to any animatable property. Unlike keyframe animations, wiggle produces continuous organic movement.

```json
{
  "type": "text",
  "content": "Wobbly",
  "wiggle": [
    { "property": "position.x", "amplitude": 5.0, "frequency": 3.0, "seed": 42 },
    { "property": "rotation", "amplitude": 2.0, "frequency": 2.0, "seed": 99 }
  ]
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `property` | `string` | (required) | Property to wiggle (same as animatable properties) |
| `amplitude` | `f64` | (required) | Maximum deviation (pixels for position, degrees for rotation, etc.) |
| `frequency` | `f64` | (required) | Oscillations per second |
| `seed` | `u64` | `0` | Random seed for reproducible results |

Wiggle offsets are applied **additively** on top of keyframe animations and presets.

---

## Layer Timing

Control when layers appear and disappear within a scene using `start_at` and `end_at`:

```json
{
  "type": "text",
  "content": "Appears at 1s, gone at 3s",
  "start_at": 1.0,
  "end_at": 3.0,
  "preset": "fade_in"
}
```

- `start_at`: the layer is invisible before this time. Animation time is offset so `t=0` in keyframes corresponds to `start_at`
- `end_at`: the layer is invisible after this time
- Both are optional and independent
- `start_at` must be less than `end_at` when both are set

---

## Motion Blur

Adds physically-correct motion blur by rendering multiple sub-frames and compositing them:

```json
{
  "type": "shape",
  "shape": "circle",
  "motion_blur": 0.8,
  "preset": "slide_in_left"
}
```

| Value | Effect |
|---|---|
| `0.0` | No blur |
| `0.5` | Moderate blur |
| `1.0` | Full frame-duration blur |

The renderer samples 5 sub-frames around the current time, each with proportional opacity.

---

## Freeze Frame

Freeze a scene at a specific point in time. All frames after `freeze_at` render the frozen state (animations stop, layers stay in place):

```json
{
  "duration": 5.0,
  "freeze_at": 2.0,
  "layers": [ ... ]
}
```

The scene continues for its full duration but the visual output is frozen from `freeze_at` onward.

---

## Output Formats

| Format | Command | Requires |
|---|---|---|
| **MP4 (H.264)** | `rustmotion render in.json -o out.mp4` | Built-in |
| **MP4 (H.265)** | `rustmotion render in.json -o out.mp4 --codec h265` | ffmpeg |
| **WebM (VP9)** | `rustmotion render in.json -o out.webm --codec vp9` | ffmpeg |
| **MOV (ProRes)** | `rustmotion render in.json -o out.mov --codec prores` | ffmpeg |
| **Animated GIF** | `rustmotion render in.json -o out.gif --format gif` | Built-in |
| **PNG Sequence** | `rustmotion render in.json -o frames/ --format png-seq` | Built-in |
| **Single Frame** | `rustmotion render in.json --frame 0 -o preview.png` | Built-in |

Transparency is supported with `--transparent` for PNG sequences, WebM (VP9), and ProRes 4444.

---

## Full Example

```json
{
  "video": {
    "width": 1080,
    "height": 1920,
    "fps": 30,
    "background": "#0f172a"
  },
  "audio": [
    { "src": "bgm.mp3", "volume": 0.3, "fade_in": 1.0, "fade_out": 2.0 }
  ],
  "scenes": [
    {
      "duration": 4.0,
      "layers": [
        {
          "type": "shape",
          "shape": { "star": { "points": 5 } },
          "position": { "x": 390, "y": 660 },
          "size": { "width": 300, "height": 300 },
          "fill": {
            "type": "radial",
            "colors": ["#fbbf24", "#f59e0b"]
          },
          "preset": "scale_in",
          "wiggle": [
            { "property": "rotation", "amplitude": 3.0, "frequency": 1.5, "seed": 1 }
          ]
        },
        {
          "type": "text",
          "content": "rustmotion",
          "position": { "x": 540, "y": 1100 },
          "font_size": 72,
          "color": "#FFFFFF",
          "align": "center",
          "preset": "fade_in_up",
          "preset_config": { "delay": 0.5 },
          "animations": [
            {
              "property": "color",
              "keyframes": [
                { "time": 1.5, "value": "#FFFFFF" },
                { "time": 3.0, "value": "#fbbf24" }
              ],
              "easing": "ease_in_out"
            }
          ]
        },
        {
          "type": "caption",
          "words": [
            { "text": "Motion", "start": 1.0, "end": 1.5 },
            { "text": "design", "start": 1.5, "end": 2.0 },
            { "text": "from", "start": 2.0, "end": 2.3 },
            { "text": "JSON", "start": 2.3, "end": 3.0 }
          ],
          "position": { "x": 540, "y": 1400 },
          "font_size": 48,
          "color": "#94a3b8",
          "active_color": "#FFFFFF",
          "background": "#1e293b",
          "style": "highlight",
          "max_width": 800
        }
      ],
      "transition": { "type": "iris", "duration": 0.8 }
    },
    {
      "duration": 3.0,
      "background": "#1e293b",
      "layers": [
        {
          "type": "text",
          "content": "No browser needed.",
          "position": { "x": 540, "y": 960 },
          "font_size": 56,
          "color": "#e2e8f0",
          "align": "center",
          "preset": "typewriter",
          "preset_config": { "duration": 1.5 }
        }
      ]
    }
  ]
}
```

## Architecture

- **Rendering:** skia-safe (same engine as Chrome/Flutter)
- **Video encoding:** openh264 (Cisco BSD, compiled from source) + ffmpeg (optional, for H.265/VP9/ProRes)
- **Audio encoding:** AAC via minimp4
- **SVG rendering:** resvg + usvg
- **GIF decoding/encoding:** gif crate
- **MP4 muxing:** minimp4
- **JSON Schema:** schemars (auto-generated from Rust types)
- **Parallelism:** rayon (multi-threaded frame rendering)

## License

MIT
