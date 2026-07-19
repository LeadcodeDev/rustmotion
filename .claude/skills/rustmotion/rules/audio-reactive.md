# Audio-Reactive Components

Rustmotion supports two built-in audio-reactive painters and a general-purpose `audio_reactive` CSS binding for any component.

## How it works

Before rendering frames, `analyze_scenario_audio` decodes each audio track (via symphonia), computes per-frame RMS amplitude and 16 log-spaced frequency bands (20 Hz–16 kHz) using a Hann-windowed FFT, normalizes the values to 0..1 across the whole track, and stores the result in a global `AudioAnalysis` cache keyed by `src`. Graceful degradation: if a track cannot be decoded (file not found, corrupt), it is silently skipped and all audio bindings fall back to their `min` value.

## audio_spectrum component

A frequency spectrum visualizer using the 16 internal bands.

```json
{
  "type": "audio_spectrum",
  "track": "music.mp3",
  "bars": 32,
  "mode": "bars",
  "color": "#38bdf8",
  "bar_gap": 3,
  "min_height": 2,
  "style": { "width": "600px", "height": "160px" }
}
```

Fields:
- `track` (optional): src path of the audio track. When absent, the first cached track is used.
- `bars` (default 16): number of bars displayed. Bands are resampled by linear interpolation from the 16 internal bands.
- `mode`: `"bars"` (vertical bars from bottom) or `"radial"` (circular spoke pattern).
- `color`: hex color string.
- `bar_gap` (default 2.0): gap in pixels between bars.
- `min_height` (default 2.0): minimum bar height in pixels (so bars are always visible).
- Default size: 400×120 px (override via `style.width`/`style.height`).

## waveform component

A time-domain waveform of the amplitude signal, centered on the current playhead.

```json
{
  "type": "waveform",
  "track": "music.mp3",
  "color": "#38bdf8",
  "draw_style": "filled",
  "window": 3.0,
  "style": { "width": "800px", "height": "100px" }
}
```

Fields:
- `track` (optional): src path. Falls back to first cached track.
- `color`: hex color.
- `draw_style`: `"line"` (single stroke path) or `"filled"` (semi-transparent fill + outline).
- `window` (default 2.0): time window in seconds, centered on the current frame time. 
- Default size: 400×80 px.

When no cached analysis exists (graceful degradation), both components render a flat line / empty bars at minimum height rather than panicking.

## audio_reactive CSS binding

Any component's `style` block can include an `audio-reactive` binding (kebab-case key — `audio_reactive` is rejected by the schema) that lerps a CSS property between `min` and `max` based on audio data:

```json
{
  "type": "shape",
  "shape": "rect",
  "fill": "#ff3366",
  "style": {
    "width": "100px",
    "height": "100px",
    "audio-reactive": {
      "track": "music.mp3",
      "source": "amplitude",
      "property": "opacity",
      "min": 0.2,
      "max": 1.0,
      "smoothing_frames": 3
    }
  }
}
```

### Fields

| Field | Type | Description |
|---|---|---|
| `track` | string? | Audio track src key. Falls back to first cached track when absent. |
| `source` | `"amplitude"` or `{"band": N}` | `"amplitude"` → overall RMS; `{"band": N}` → N-th frequency band (0..15). |
| `property` | string | CSS property to drive: `"opacity"`, `"scale"`, `"translate_y"`, `"rotation"`. |
| `min` | f64 | Value at audio=0. |
| `max` | f64 | Value at audio=1. |
| `smoothing_frames` | u32 (default 0) | Number of past frames to average (temporal smoothing). |

### Supported properties

- **`opacity`**: multiplied with the base opacity (0..1 range, clamped).
- **`scale`**: uniform 2D scale applied via CSS transform. `min: 0.5, max: 1.5` pulses the element.
- **`translate_y`**: vertical translation in px (negative = up). Useful for bouncing elements.
- **`rotation`**: rotation in degrees.

### Notes

- The binding is applied at box-tree build time (before taffy layout), so `opacity` and `transform` flow through the standard CSS paint pass.
- `scale` and `transform`-based properties are appended to the existing transform list (they compose).
- `smoothing_frames: 3` at 30 fps ≈ 100ms smoothing, which removes most percussive jitter while keeping reactivity.
- Cache miss (no audio or track not found) → all reactive values fall back to `min` (deterministic, no flicker).

## band indices

The 16 bands span 20 Hz to 16 kHz, log-spaced. Approximate centers:

| Index | ~Center Hz |
|---|---|
| 0 | 24 |
| 1 | 36 |
| 2 | 55 |
| 3 | 82 |
| 4 | 124 |
| 5 | 186 |
| 6 | 279 |
| 7 | 419 |
| 8 | 629 |
| 9 | 944 |
| 10 | 1416 |
| 11 | 2125 |
| 12 | 3190 |
| 13 | 4790 |
| 14 | 7190 |
| 15 | 10795 |

Use bands 0–3 for bass, 4–7 for low-mid, 8–11 for mid, 12–15 for high.
