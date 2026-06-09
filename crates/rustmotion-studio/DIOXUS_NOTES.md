# Dioxus 0.7 API — pinned against installed crates

Resolved: `dioxus 0.7.9`, `dioxus-desktop 0.7.9`, `wry 0.53.5`.

## Launch + root context
- `dioxus::launch(app: fn() -> Element)` — simple.
- `dioxus::LaunchBuilder::desktop() -> LaunchBuilder`
- `LaunchBuilder::with_context(self, state: impl Any + Clone + Send + Sync + 'static) -> Self`
  → injects a root context, read it with `use_context::<T>()`.
- `LaunchBuilder::launch(self, app: fn() -> Element)` — note: a `fn` pointer, so the
  root component must be a zero-arg `#[component] fn App() -> Element`.

## Asset handler (frame transport)
- `dioxus::desktop::use_asset_handler(name: &str, FnMut(AssetRequest, RequestAsyncResponder) + 'static)`
- `AssetRequest = wry::http::Request<Vec<u8>>` (has `.uri()`).
- `RequestAsyncResponder::respond<T: Into<Cow<'static,[u8]>>>(self, Response<T>)`
  → `responder.respond(Response::builder().header("Content-Type","image/png").body(png_vec).unwrap())`.
- Imports: `use dioxus::desktop::{use_asset_handler, wry::http::Response, AssetRequest, RequestAsyncResponder};`
- URL mapping: a handler named `"frame"` serves requests under the `/frame/...` path; the
  request URI path is what we parse for the frame index.

## Hooks
- `use_context::<T>()` (re-exported in prelude from dioxus-hooks).
- `use_signal`, `use_future` in `dioxus::prelude::*`.

## Constraints discovered
- `with_context` requires `Send + Sync` — the shared model is `Arc<Mutex<StudioModel>>`
  (Send+Sync as long as `StudioModel` is Send, which it is: plain data).

## Measurements (1920×1080, `--bench`)

| build | encode/frame | total/frame | max fps |
|-------|--------------|-------------|---------|
| PNG, debug                         | 260 ms | 262 ms | 3.8 |
| JPEG, debug                        | 924 ms | 925 ms | 1.1 |
| JPEG, dev + deps opt-3             | 269 ms | 270 ms | 3.7 |
| JPEG, dev + deps opt-3 + no-checks | 191 ms | 192 ms | 5.2 |
| JPEG, **dev + studio crate opt-3** | **14 ms** | **14.5 ms** | **69** |
| JPEG, `--release`                  | 13.7 ms | 14.2 ms | 70 |

Skia render is ~0.5 ms regardless (precompiled C++). The cost was pure-Rust
image encoding running unoptimized. The `image` JPEG encoder is **generic and
monomorphizes into the calling crate** (`rustmotion-studio`), so a `package."*"`
override alone left it at opt-0; optimizing `rustmotion-studio` in dev fixed it.

## Decision (2026-06-09)
**GO on webview.** With JPEG preview encoding + the dev profile optimization,
a 1080p frame is ~14 ms (≈69 fps) in plain `cargo run`, clearing every spec
threshold (edit <50 ms, scrub <100 ms, playback ≥24–30 fps). No need for
Blitz or a frame cache yet; revisit if 2256-tall real scenarios regress.
