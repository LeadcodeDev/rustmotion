# Rule: Always Validate Generated JSON

Every generated JSON scenario MUST be validated with `rustmotion validate` before presenting to the user. The validator runs both **schema** and **geometry** checks — geometry violations fail the build by default.

1. Write JSON to a temporary file (e.g. `/tmp/scenario.json`)
2. Run `rustmotion validate -f /tmp/scenario.json`
3. If validation fails: correct errors and re-validate
4. If validation succeeds: present to the user

## Geometry violations

The validator detects five overflow conditions:
- `viewport_overflow` — absolute bbox crosses the device edge
- `unwrappable_text_overflow` — `style.white-space: "nowrap"`/`"pre"` but natural width > box (there is no `wrap` field)
- `content_overflows_box` — wrapping text needs more room than its own box, e.g. a paragraph in a fixed-height card. Fires even when the box stays inside the frame
- `auto_scroll_disabled_overflow` — `auto_scroll: false` on a codeblock/terminal with content > box
- `animated_text_overflow` — an animated transform pushes the bbox out of the viewport at some sampled time (`--strict-anim` only)

See [rules/geometry-safety.md](geometry-safety.md) for the underlying mechanisms.

## Useful flags

```bash
rustmotion validate -f /tmp/scenario.json --fix             # safe auto-fixes
rustmotion validate -f /tmp/scenario.json --report r.json   # JSON report
rustmotion validate -f /tmp/scenario.json --strict-anim     # per-frame checks
rustmotion validate -f /tmp/scenario.json --lenient         # warnings only
```

`--fix` handles three mechanical cases: it sets `auto_scroll: true` on `auto_scroll_disabled_overflow`; it removes `style.white-space` on `unwrappable_text_overflow` (falling back to the `normal` default, i.e. wrapping); and on `content_overflows_box` for `text`/`gradient_text` specifically, it sets `style.text-autofit: true`, which shrinks the font until the content fits (down to a calibrated readability floor — if that's not enough, the violation is still reported). `content_overflows_box` on every other component, and every `viewport_overflow`, is never auto-fixed — those need a layout decision only you can make. Do not hand-resize a box `--fix` would have handled instead; but also don't reach for `text-autofit` to paper over a layout you could simply size correctly — it's for content whose length you can't know ahead of time.

**FORBIDDEN:** Presenting JSON that has not been validated by `rustmotion validate`.
