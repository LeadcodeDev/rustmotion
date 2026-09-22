//! Regression tests for the workstream H (untrusted scenario ingestion)
//! audit finding that lives in the `rustmotion` crate:.
//!
//! Remote fetching for `include: "https://..."` is deliberate design; the
//! finding is the absence of any control around it. These tests exercise
//! `include::resolve_includes`/`resolve_includes_with_policy` directly
//! rather than through `rustmotion::loader`, so they do not depend on the
//! CLI wiring a sibling workstream owns.

use rustmotion::include::{
    resolve_includes, resolve_includes_with_policy, IncludeSource, RemoteIncludePolicy,
};
use rustmotion::schema::Scenario;

fn scenario_with_remote_include(url: &str) -> Scenario {
    serde_json::from_value(serde_json::json!({
        "video": { "width": 100, "height": 100 },
        "scenes": [{ "include": url }]
    }))
    .expect("scenario with a remote include parses")
}

// ----, part 1: remote includes are denied by default ----

#[test]
fn remote_include_is_denied_by_default() {
    let scenario = scenario_with_remote_include("https://example.invalid/scenario.json");
    let err = resolve_includes(scenario, &IncludeSource::Inline)
        .expect_err("a remote include must be refused unless explicitly opted into");
    let msg = err.to_string();
    assert!(
        msg.contains("disabled by default"),
        "the denial must explain that remote includes are opt-in: {msg}"
    );
}

#[test]
fn remote_include_denial_names_the_opt_in() {
    let scenario = scenario_with_remote_include("https://example.invalid/scenario.json");
    let err =
        resolve_includes_with_policy(scenario, &IncludeSource::Inline, RemoteIncludePolicy::Deny)
            .expect_err("Deny must refuse the same way the default does");
    assert!(
        err.to_string().contains("--allow-remote-include"),
        "the error should name the flag that opts in, for a CLI to surface: {err}"
    );
}

// ----, part 2: once opted in, resolved addresses are still checked
// against loopback/link-local/private ranges before any request is made ----

#[test]
fn opted_in_remote_include_still_refuses_the_cloud_metadata_endpoint() {
    // A literal IP in the URL resolves without any DNS/network I/O (`(&str,
    // u16): ToSocketAddrs` parses a numeric host directly), so this is a
    // deterministic, offline test of the SSRF check itself.
    let scenario = scenario_with_remote_include(
        "http://169.254.169.254/latest/meta-data/iam/security-credentials/",
    );
    let err =
        resolve_includes_with_policy(scenario, &IncludeSource::Inline, RemoteIncludePolicy::Allow)
            .expect_err(
                "the cloud metadata address must be refused even when remote includes are allowed",
            );
    let msg = err.to_string();
    assert!(
        msg.contains("169.254.169.254"),
        "the error should name the disallowed address: {msg}"
    );
}

#[test]
fn opted_in_remote_include_still_refuses_loopback() {
    let scenario = scenario_with_remote_include("http://127.0.0.1:8500/scenario.json");
    let err =
        resolve_includes_with_policy(scenario, &IncludeSource::Inline, RemoteIncludePolicy::Allow)
            .expect_err("loopback must be refused even when remote includes are allowed");
    assert!(err.to_string().contains("127.0.0.1"), "{err}");
}

#[test]
fn opted_in_remote_include_still_refuses_rfc1918_private_ranges() {
    let scenario = scenario_with_remote_include("http://10.0.0.5/scenario.json");
    let err =
        resolve_includes_with_policy(scenario, &IncludeSource::Inline, RemoteIncludePolicy::Allow)
            .expect_err(
                "an RFC1918 private address must be refused even when remote includes are allowed",
            );
    assert!(err.to_string().contains("10.0.0.5"), "{err}");
}
