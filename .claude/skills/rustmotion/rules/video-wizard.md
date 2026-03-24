# Rule: Video Creation Wizard Best Practices

## Description
Guidelines for the assisted video creation wizard flow.

## Rules

1. **Never generate the entire video at once.** Build scene by scene, validating each one before moving to the next. This lets the user course-correct early.

2. **Always validate each scene individually** with `rustmotion validate` before presenting it to the user. Fix any errors before asking for feedback.

3. **Propose alternatives when the user isn't satisfied.** If a scene doesn't match expectations, suggest 2-3 concrete variations (different layout, animation style, or component choice) rather than asking "what would you prefer?".

4. **Use presets over custom keyframes.** rustmotion has 39+ built-in animation presets. Always prefer them for consistency and reliability. Only use custom keyframes when no preset fits.

5. **Adapt style to the brief answers.** The tone/style chosen in Phase 1 should influence every decision:
   - **Corporate** → subtle animations (fade_in, slide_in_up), neutral backgrounds, clean layouts
   - **Playful** → bouncy presets (bounce_in, scale_in with overshoot), bright colors, particles
   - **Minimal** → few elements per scene, lots of whitespace, char animations only
   - **Tech/Dark** → dark gradients, concentric_circles, glow effects, monospace fonts; use `$ref` templates with `transition` for smooth bg evolution
   - **Colorful** → multi-color gradients, confetti particles, varied icon colors; use `$ref` templates to keep consistent bg across scenes

6. **Suggest previews for complex scenes.** When a scene has overlapping elements, custom positioning, or intricate layouts, render a single frame with `--frame` so the user can verify placement before moving on.

7. **Name files meaningfully.** Use kebab-case derived from the video subject (e.g., `product-launch-intro.json`, not `video.json` or `output.json`).

8. **Keep the conversation flowing.** After each scene validation, briefly recap progress ("Scene 3/6 done, next: feature showcase") to maintain context.

## BAD: Dumping everything at once
```
Here's your complete 6-scene video JSON...
[500 lines of JSON]
```

## GOOD: Iterative construction
```
Let's start with Scene 1 (Intro). Here's the JSON:
[scene JSON]
✓ Validated successfully.
Want me to render a preview frame, or shall we move to Scene 2?
```
