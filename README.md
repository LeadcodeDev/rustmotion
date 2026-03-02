# rustmotion

A CLI tool that renders motion design videos from JSON scenarios. No browser, no Node.js — just a single Rust binary.

[![Crates.io](https://img.shields.io/crates/v/rustmotion.svg)](https://crates.io/crates/rustmotion)
[![docs.rs](https://docs.rs/rustmotion/badge.svg)](https://docs.rs/sqlx-gen)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## Install

```bash
cargo install rustmotion
```

**Requirements:** Rust toolchain + C++ compiler (for openh264).

## Quick Start

```bash
# Render a video
rustmotion render scenario.json -o video.mp4

# Validate without rendering
rustmotion validate scenario.json

# Export JSON Schema (for LLM prompts)
rustmotion schema -o schema.json

# Show scenario info
rustmotion info scenario.json

# Render a single frame for debugging
rustmotion render scenario.json --frame 42 -o frame.png
```

## JSON Scenario Format

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
          "type": "text",
          "content": "Hello World",
          "position": { "x": 540, "y": 960 },
          "font_size": 64,
          "color": "#FFFFFF",
          "align": "center",
          "preset": "fade_in_up"
        }
      ]
    }
  ]
}
```

## Features

- **Layers:** text, shape (rect, circle, rounded_rect, ellipse), image (PNG/JPEG/WebP/SVG), group
- **Animations:** keyframes with 10 easing functions + spring physics
- **Presets:** fade_in, fade_in_up, slide_in_left, scale_in, typewriter, bounce_in, blur_in, and more
- **Transitions:** fade, wipe (4 directions), zoom between scenes
- **Audio:** MP3, WAV, OGG, FLAC, AAC — with volume, fade in/out, multi-track mixing
- **Output:** H.264 + AAC in MP4 (LinkedIn/social media compatible)
- **Performance:** Multi-threaded rendering via rayon
- **Machine-friendly:** `--output-format json` for CI/n8n integration

## Architecture

- **Rendering:** skia-safe (same engine as Chrome/Flutter)
- **Video encoding:** openh264 (Cisco BSD, compiled from source)
- **Audio encoding:** fdk-aac (AAC-LC)
- **MP4 muxing:** minimp4
- **JSON Schema:** schemars (auto-generated from Rust types)

## License

MIT
