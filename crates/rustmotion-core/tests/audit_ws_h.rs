use rustmotion_core::engine::renderer::{extract_video_frame, http_agent};

#[test]
fn shared_http_agent_has_finite_global_and_connect_timeouts() {
    let timeouts = http_agent().config().timeouts();
    assert!(
        timeouts.global.is_some(),
        "ureq 3.x's default Timeouts::global is None — the shared agent must override it \
         so a stalled host cannot hang a render forever"
    );
    assert!(
        timeouts.connect.is_some(),
        "a hung TCP handshake must not hang forever either"
    );
}

#[test]
fn extract_video_frame_rejects_a_remote_src_before_it_ever_reaches_ffmpeg() {
    let err = extract_video_frame(
        "http://169.254.169.254/latest/meta-data/iam/security-credentials/",
        0.0,
        16,
        16,
    )
    .expect_err("a scheme-prefixed src must be rejected outright, not handed to ffmpeg");
    let msg = err.to_string();
    assert!(
        msg.contains("does not fetch video over the network"),
        "expected the scheme-rejection error, not an ffmpeg spawn/exit failure: {msg}"
    );
}

#[test]
fn extract_video_frame_does_not_reject_a_plain_local_path() {
    let result = extract_video_frame("/no/such/file/on/disk.mp4", 0.0, 16, 16);
    let err = result.expect_err("a missing local file is still an error");
    assert!(
        !err.to_string()
            .contains("does not fetch video over the network"),
        "a plain local path must fail on ffmpeg/the missing file, not on the scheme check: {err}"
    );
}

#[test]
fn for_each_node_budget_rejects_a_declared_product_that_exceeds_the_cap() {
    fn items(n: usize) -> serde_json::Value {
        serde_json::Value::Array(
            (0..n)
                .map(|i| serde_json::json!({ "v": i }))
                .collect::<Vec<_>>(),
        )
    }

    let mut doc = serde_json::json!({
        "video": { "width": 100, "height": 100 },
        "scenes": [{
            "duration": 1.0,
            "children": [{
                "for-each": items(200),
                "template": {
                    "type": "card",
                    "children": [{
                        "for-each": items(200),
                        "template": {
                            "type": "card",
                            "children": [{
                                "for-each": items(200),
                                "template": { "type": "text", "content": "$v" }
                            }]
                        }
                    }]
                }
            }]
        }]
    });

    let err = rustmotion_core::expand::expand_directives(&mut doc, "test.json")
        .expect_err("a declared product this far past the cap must be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("budget"),
        "error should name the node-budget ceiling it exceeded: {msg}"
    );
}
