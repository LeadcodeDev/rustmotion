# Non-goals

rustmotion renders motion-design videos from JSON scenarios with a single Rust binary — Skia for drawing, taffy for layout, ffmpeg/openh264 for encoding. That focus rules out whole categories of feature that a browser-based or SaaS video tool would otherwise be expected to have. This page names them explicitly, so they're discoverable without reading source or asking.

## Browser, React, DOM

rustmotion has no browser and no JavaScript runtime anywhere in its render path — that absence is the whole point (see the README's opening line). Consequently, out of scope by construction:

- **An embeddable Player component.** There is no `<Player>` to drop into a React/Vue/Svelte app for in-browser preview. Preview happens by rendering a frame (`rustmotion render --frame N`) or a still (`rustmotion still`), or through the standalone `rustmotion-studio` desktop app.
- **A JS/webpack/Vite bundler pipeline.** Scenarios are JSON (or the HTML dialect that transpiles to JSON); there's no module graph to bundle.
- **Tailwind, SCSS, or any CSS preprocessor.** The `style` object is a flat, typed `CssStyle` deserialized by serde — no cascade resolution beyond the deliberately small color/font-* inheritance rustmotion itself implements, no class names, no preprocessing step.
- **three.js or any WebGL/WebGPU scene graph.** 3D transforms (`rotate_x`, `rotate_y`, `perspective`) are real Skia M44 matrix math on 2D layers, not a 3D renderer — there's no mesh, camera, or lighting model to speak of.
- **An in-browser visual studio.** `rustmotion-studio` is a native desktop app (Dioxus, its own binary), not a web app — there is no hosted or local-webserver editing UI.
- **`delayRender()` / async-render-blocking primitives.** rustmotion has no concept of a component signaling "not ready yet" mid-render, because there's no async component tree to block: all assets (images, fonts, icons) are resolved before a frame is painted, synchronously, by the loader.

## Commercial

**No licence key, no telemetry, no per-render billing.** rustmotion is MIT-licensed (see the badge at the top of the README) and that is a complete description of its commercial model — not "free while we build a paid tier." The binary makes exactly three categories of outbound network call, every one of them opt-in by virtue of what's in your scenario: fetching an Iconify icon by id, fetching a Google Font by family name (`FontEntry.source = "google"`), and resolving an `include` entry given as a URL instead of a local path. Neither call reports usage, render count, or any other telemetry, and nothing about this project's licensing or pricing is decided by what a scenario contains. This is worth stating plainly next to the MIT badge because an automated pipeline (CI job, agent, batch renderer) that shells out to `rustmotion render` should not need to reason about license enforcement, seat counts, or metered usage — there is none.

## Vendor cloud

rustmotion doesn't deploy anywhere, and doesn't know what "anywhere" means:

- No AWS Lambda / Google Cloud Run / Vercel deployment target, packaging, or adapter — nothing sizes the binary for a serverless runtime.
- No S3 (or any object-storage) output — `render`/`batch`/`still` write to a local path; shipping the result somewhere else is left to whatever calls the CLI.
- No IAM roles, cloud credentials, or cloud-specific configuration of any kind.
- No pricing model tied to render minutes, resolution, or concurrency — see [Commercial](#commercial) above.

If you need cloud rendering, `rustmotion render --frames a-b` plus `rustmotion concat` is the building block a distributed render pipeline is expected to be built *on top of* — rustmotion renders frames; orchestrating that across machines is out of scope.

## Known gaps (not exclusions)

These are real absences today, not deliberate non-goals — each is natively reachable given the existing dependency stack or architecture, and could reasonably be added without a redesign:

- **SkSL runtime effects.** `skia-safe` (the Skia binding rustmotion is built on) exposes `RuntimeEffect` for custom pixel shaders, and the workspace has zero uses of it today. A `style` field that compiles and runs an SkSL shader is plausible future work, not something the architecture excludes.
- **A public path toolkit.** Bezier/SVG-path math already exists internally (`arrow`'s curve fields, `connector`'s routing, `motion_path`'s SVG path parsing) but isn't exposed as a reusable, documented primitive a scenario author can compose with directly.
- **A seeded `random`/`noise` value.** `wiggle`'s procedural noise and `particle`'s seeded layouts are deterministic and reproducible *for those two components specifically*, but there's no general-purpose `{ "$random": { "seed": ..., "min": ..., "max": ... } }`-style value provider a scenario can use anywhere a number is expected.
