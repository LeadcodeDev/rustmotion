# Rule: Continuous Presets Need loop: true

The presets `pulse`, `float`, `shake`, and `spin` are continuous animations. Without `"loop": true` in `preset_config`, they play once and stop.

**GOOD:**
```json
{ "preset": "float", "preset_config": { "loop": true } }
```

**BAD:**
```json
{ "preset": "float" }
```

Continuous presets: `pulse`, `float`, `shake`, `spin`.
