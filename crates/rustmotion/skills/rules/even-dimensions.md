# Rule: Use Even Dimensions for H.264

H.264 requires even width and height. Odd dimensions cause encoding failures.

**GOOD:**
```json
{ "video": { "width": 1080, "height": 1920 } }
```

**BAD:**
```json
{ "video": { "width": 1081, "height": 1921 } }
```

Common safe resolutions: `1080x1920`, `1920x1080`, `1080x1080`, `720x1280`, `1280x720`.
