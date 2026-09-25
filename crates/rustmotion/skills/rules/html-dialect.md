# Rule: The `rustmotion-html` Dialect — Syntax and Its Real Limits

`rustmotion-html` transpiles a restricted HTML+inline-CSS document straight to the same scenario JSON every other rule in this skill describes — no browser, no JS, no layout engine of its own. It is a syntax choice, not a different feature set: everything it emits still goes through the JSON schema, so a component or field HTML cannot express is not "different in HTML", it is **absent**. Read [SKILL.md](../SKILL.md)'s "Output format" section first for when to offer this format at all.

---

## Document shape

```html
<rustmotion width="1080" height="1920" fps="30" background="#0f172a">
  <font family="Inter" path="fonts/Inter-Regular.ttf">
  <font family="JetBrainsMono" src="https://.../JetBrainsMono-Regular.ttf" weights="400,700">

  <scene duration="4" background="#1a1a2e" transition="fade" transition-duration="0.6">
    <div style="flex-direction:column; align-items:center; gap:32">
      <h1 style="font-size:96; color:#ffffff; text-align:center">Ship Faster</h1>
      <p style="font-size:42; color:#cbd5e1">Built in Rust. No browser.</p>
      <rm-counter from="0" to="1250" suffix="+" style="font-size:96; color:#38bdf8"></rm-counter>
    </div>
  </scene>
</rustmotion>
```

`<rustmotion>` attributes map to `video` (`width`, `height`, `fps`, `background`). `<font>` maps to a `fonts` entry (`family` + `path`/`src`, plus optional `weights="400,700"` CSV).

`<scene>` known attributes: `duration` (required), `align`/`justify`/`direction`/`gap`/`padding` (become the scene's implicit-flex `layout`, `align`/`justify` default to `center`), `background`, `effects`, `transition`/`transition-duration`/`transition-easing`, `freeze_at`, `world-position` (parses, but see the gaps below — it never affects rendering in this dialect), `animated-background`. Anything else on `<scene>` is a named-attribute error, not a silent drop.

---

## Element mapping

| Tag | Becomes |
|---|---|
| `div`, `section`, `main`, `header`, `footer`, `article` | `{"type": "div"}` |
| `p`, `span`, `h1`–`h6`, `strong`, `em`, `label` | `{"type": "text", "content": "<flattened text>"}` |
| `img`, `video`, `svg` | **Refused outright**, naming the real tag to use: `rm-image`, `rm-video`, `rm-svg` |
| `rm-<name>` | `{"type": "<name>"}` — any of the 60 component types |
| anything else unrecognized | **Silently becomes a `div`.** A typo'd tag (`<h7>`, `<dvi>`) does not error — `validate` sees a legal `div`, so there is nothing to catch the mistake on. The visible effect: a `div`'s children each become their own block-level component (column flex by default), where a real `h1`–`h6`/`p`/`span` would have flattened its descendants into one inline text run. Demonstrated: `<h7>Hello <strong>World</strong></h7>` renders **"Hello" and "World" on two separate lines** — under the intended tag it would have been a single line reading "Hello World". Double-check tag spelling. |

`<style>` is refused (not ignored) — it would have a real visual effect the transpiler cannot honor. `<script>`, `<title>`, `<noscript>`, `<template>`, `<head>` are skipped like real HTML.

### `rm-*` custom elements → any component

`<rm-badge text="New" icon="lucide:zap" pulse></rm-badge>` becomes `{"type": "badge", "text": "New", "icon": "lucide:zap", "pulse": true}`. Every non-`style`/`class`/`anim` attribute becomes a JSON field, scalar-coerced (see below); a bare boolean attribute (`<rm-codeblock diff>`, no `="..."`) becomes `true`, matching HTML's own boolean-attribute convention. Nested elements become `children`.

### Scalar coercion — the ceiling of what an attribute can hold

Every non-`style` attribute value goes through the same coercion `style="..."` uses: `"true"`/`"false"` → JSON bool, a bare number or `<n>px` → JSON number, everything else → a JSON string. There is **no way to write a JSON array or object as a plain attribute** (`<rm-chart data="...">` cannot work) and no quoting escape for a string that happens to look numeric: `<rm-text content="2024">` or `<rm-kbd key="1">` hard-error, because `content`/`key` are typed as JSON strings but the value coerces to a number first. If the data you need to pass is naturally numeric-looking text, or is an array/object, that field needs JSON — see the capability gaps below.

---

## Inline `style="prop:value; prop:value"`

Mirrors `CssStyle`, but only the flat half of it:

- `padding`, `margin` — accept the CSS 1/2/3/4-value box shorthand, expanded into `{top,right,bottom,left}`.
- `border-radius` — same shorthand, expanded into the four corners.
- `grid-template-columns` / `grid-template-rows` — accept a track list (`repeat()`/`minmax()` included).
- Every other property whose value is more than one (paren-aware) token is **refused**, not passed through as an opaque string. This is the actual boundary, and it removes a lot: `box-shadow`, `text-shadow`, `transform`, `filter`/`backdrop-filter`, `border` (as the `{width,style,color}` object — a single-token `border: 1px` alone doesn't express color+style), `clip-path`, `fill`/`background` as a gradient object, `font-family` stacks (`"Inter, sans-serif"`), `grid-column`/`grid-row` span objects, `transition`.
- A declaration missing its colon (`"font-size 400"` — typo'd, no `:`) is **silently dropped**, not an error. Nothing else in this dialect fails silently; this is the one hole.

There is no `style.animation` written through `style=`. It is only ever set through `anim=`.

## `anim="..."` — the animation attribute

Two forms, on the same attribute:

```html
<h1 anim="fade-in-up duration:0.8">…</h1>
<div anim="scale-in duration:0.6; float-3d loop:true">…</div>
<h2 anim='{"name":"pulse","duration":1.5,"loop":true}'>…</h2>
<h2 anim='[{"name":"scale_in","duration":0.7},{"name":"float_3d","loop":true}]'>…</h2>
```

- **Compact DSL**: `preset-name key:value key:value` (kebab preset names, converted to `snake_case`), multiple effects separated by `;`. `spring` only accepts `spring:true` (all-default `SpringConfig`) or `spring:false` (absent) in this form — fine-grained damping/stiffness/mass needs the JSON form.
- **JSON escape hatch**: a value starting with `{` or `[` is parsed as raw JSON and placed into `style.animation` directly (wrapped in an array if it was a single object). This is the one field-level escape hatch that exists for `anim`.

`background`, `effects`, `world-position`, and `animated-background` on `<scene>` get the **same kind of escape hatch** — each accepts a value starting with `{`/`[` as inline JSON matching that field's real schema shape (`background='{"preset":"halo","halo":{...},"speed":0}'`, as in the shipped example below). `world-position` additionally accepts a `"x,y"` CSV shorthand. **These five are the only attributes with a JSON-string escape hatch.** No other component field — not `chart.data`, not `table.rows`, not a `timeline` step list — has one; those need JSON authoring, full stop.

---

## What the dialect cannot express — check before offering HTML

- Any array/object **component field** beyond `style`/`anim`/the five scene attributes above: `chart` (all 12 types), `table`, `list`, `stepper`, the `timeline` *component*, `tag_cloud`, `avatar_group`, `pill_nav`, and the per-component `timeline` field (state-transition steps) on any of the 60 components.
- The structured half of `style` listed above (shadows, transforms, filters, gradients, clip-path, font stacks, grid spans, transitions).
- `audio` tracks at all — so `waveform`, `audio_spectrum`, and `style.audio-reactive` have nothing to bind to.
- `composition`/`world` views. `world-position` is accepted and validated but never read: the dialect only ever emits a flat `scenes` list, i.e. one implicit `slide` view. A world-view video (the one mechanism for beat-to-beat continuity — see [rules/world-view.md](world-view.md)) cannot be written in HTML.
- Root-level `config`/variables, named `backgrounds` templates, `version`.

## Footguns worth knowing even when everything above is in scope

- **Whitespace is not collapsed.** Indenting the HTML source changes the rendered output — a pretty-printed document can render with the source's leading spaces as literal content under `nowrap`. Keep text content on one line inside its tag.
- **No inline text formatting.** `<b>`, `<i>`, `<br>`, `<code>`, `<a>`, `<u>`, `<small>` are all refused inside `p`/`span`/`h1`–`h6`/etc. — there is no way to bold a word mid-sentence or force a line break inside one text component. Split into separate elements (or a `\n` in a JSON `content` string, which isn't available here either — use two stacked elements instead).
- **Don't nest hundreds of levels deep.** There is no recursion depth guard; pathological nesting can abort the process rather than produce a validate-able error. Ordinary generated markup never gets close to this, but it's not caught and turned into a message if something goes wrong upstream and produces runaway nesting.

## Worked reference

`examples/html/showcase.html` (fonts, transitions, both `anim=` forms, an inline JSON `background=` on `<scene>`, a staggered `rm-card`) and `examples/html/hello.html` (minimal) are real, validating files — read them alongside this rule rather than inventing syntax from the summary above.
