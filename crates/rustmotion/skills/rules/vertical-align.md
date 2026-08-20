# Rule: Use vertical_align Correctly in Shape Text

The `vertical_align` property in shape embedded text accepts only `"top"`, `"middle"`, or `"bottom"`. The value `"center"` is **NOT valid**.

**BAD:**
```json
{ "text": { "content": "Click me", "vertical_align": "center" } }
```

**GOOD:**
```json
{ "text": { "content": "Click me", "vertical_align": "middle" } }
```

Default is `"middle"`, so you can omit it for centered text.
