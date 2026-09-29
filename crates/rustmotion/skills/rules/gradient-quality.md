# Rule: Gradient Quality and Encoding

Dark gradients are prone to color banding (visible steps instead of smooth transitions). Rustmotion mitigates this with:

1. **Linear color space interpolation** for animated background gradients (smoother dark tones)
2. **Subdivided color stops** (16 intermediate stops between each color pair)
3. **Optional 10-bit H.264** encoding (`yuv420p10le`, `high10` profile) behind `--codec h264_10bit`
4. **Dithering** enabled on all gradient paints

## Encoding recommendations

| Scenario | Recommendation |
|---|---|
| Dark gradient backgrounds | `--codec prores` for best quality, or `--codec h264_10bit` for a small file |
| General use | Default H.264, 8-bit, plays everywhere |
| No ffmpeg available | Built-in openh264 (8-bit, may show banding on dark gradients) |

10-bit is not the default, and the reason is not quality. `yuv420p10le` in the
`high10` profile is refused outright by QuickTime and by Safari, so a file that
looked better was a file a lot of people could not open. `--codec h264_10bit` is
the same encode it used to do, asked for on purpose.

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
    "dark_radial": { "preset": "gradient_shift", "colors": ["#0f172a", "#1e1b4b", "#0f172a"], "speed": 20, "gradient_type": "radial" }
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
