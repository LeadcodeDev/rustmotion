# Rule: Continuous Presets Need loop: true

The presets `pulse`, `float`, `shake`, and `spin` are continuous animations. Without `"loop": true`, they play once and stop.

**GOOD:**
```json
{ "style": { "animation": [{ "name": "float", "loop": true }] } }
```

**BAD:**
```json
{ "style": { "animation": [{ "name": "float" }] } }
```

Continuous presets: `pulse`, `float`, `shake`, `spin`.
