# Rule: Continuous Presets Need loop: true

The presets `pulse`, `float`, `shake`, and `spin` are continuous animations. Without `"loop": true`, they play once and stop.

**GOOD:**
```json
{ "animation": [{ "name": "float", "loop": true }], "style": {} }
```

**BAD:**
```json
{ "animation": [{ "name": "float" }], "style": {} }
```

Continuous presets: `pulse`, `float`, `shake`, `spin`.
