# Rule: Gradient Quality and Encoding

Dark gradients are prone to color banding (visible steps instead of smooth transitions). Rustmotion mitigates this with:

1. **Linear color space interpolation** for animated background gradients (smoother dark tones)
2. **Subdivided color stops** (16 intermediate stops between each color pair)
3. **10-bit H.264** encoding (`yuv420p10le`, `high10` profile) when ffmpeg is available
4. **Dithering** enabled on all gradient paints

## Encoding recommendations

| Scenario | Recommendation |
|---|---|
| Dark gradient backgrounds | Use `--codec prores` for best quality |
| General use | Default H.264 10-bit (requires ffmpeg) |
| No ffmpeg available | Built-in openh264 (8-bit, may show banding on dark gradients) |

**GOOD:** Use `gradient_type: "radial"` with at least 3 colors for smooth transitions:
```json
{
  "background": {
    "colors": ["#0f172a", "#1e1b4b", "#0f172a"],
    "speed": 20,
    "gradient_type": "radial"
  }
}
```

Or use a named template with `$ref` for reuse across scenes:
```json
{
  "backgrounds": {
    "dark_radial": { "colors": ["#0f172a", "#1e1b4b", "#0f172a"], "speed": 20, "gradient_type": "radial" }
  },
  "scenes": [
    { "duration": 5, "background": { "$ref": "dark_radial" } }
  ]
}
```

**BAD:** Only 2 very similar dark colors (minimal contrast = worst banding):
```json
{
  "background": {
    "colors": ["#0a0a0a", "#0b0b0b"],
    "gradient_type": "radial"
  }
}
```

## ffmpeg auto-detection

When ffmpeg is installed, rustmotion uses it automatically for all MP4 output (10-bit H.264). Without ffmpeg, it falls back to the built-in openh264 encoder (8-bit). No flag needed.
