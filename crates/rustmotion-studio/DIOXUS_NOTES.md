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

## Measurements
(filled in by Task 3 `--bench` runs)

## Decision
(dated decision after the perf gate)
