# Rule: Always Validate Generated JSON

Every generated JSON scenario MUST be validated with `rustmotion validate` before presenting to the user. The validator runs both **schema** and **geometry** checks — geometry violations fail the build by default.

1. Write JSON to a temporary file (e.g. `/tmp/scenario.json`)
2. Run `rustmotion validate -f /tmp/scenario.json`
3. If validation fails: correct errors and re-validate
4. If validation succeeds: present to the user

## Geometry violations

The validator detects three viewport-overflow conditions:
- `viewport_overflow` — absolute bbox crosses the device edge
- `unwrappable_text_overflow` — `style.wrap: false` but natural width > box
- `auto_scroll_disabled_overflow` — `auto_scroll: false` on a codeblock/terminal with content > box

See [rules/geometry-safety.md](geometry-safety.md) for the underlying mechanisms.

## Useful flags

```bash
rustmotion validate -f /tmp/scenario.json --fix             # safe auto-fixes
rustmotion validate -f /tmp/scenario.json --report r.json   # JSON report
rustmotion validate -f /tmp/scenario.json --strict-anim     # per-frame checks
rustmotion validate -f /tmp/scenario.json --lenient         # warnings only
```

`--fix` may set `wrap: true` and `auto_scroll: true` automatically. Position/size issues are never auto-fixed.

**FORBIDDEN:** Presenting JSON that has not been validated by `rustmotion validate`.
