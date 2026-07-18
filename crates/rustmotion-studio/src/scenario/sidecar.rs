//! Annotations sidecar for HTML-dialect scenarios.
//!
//! HTML sources can't carry an `annotations` array like JSON scenarios do, so
//! the studio persists comments in a sidecar file next to the source:
//! `foo.html` → `foo.annotations.json`, holding `{"annotations": [...]}` with
//! exactly the same annotation object format as the JSON scenarios' field.
//!
//! The HTML file itself is never touched by annotations. The sidecar is
//! created lazily on the first comment and deleted when the last one is
//! removed. A present-but-corrupt sidecar is always an error — it is never
//! silently overwritten.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// Sidecar path for an HTML source: `foo.html` → `foo.annotations.json`.
pub fn sidecar_path(html_path: &Path) -> PathBuf {
    html_path.with_extension("annotations.json")
}

/// Merge sidecar annotations into a scenario raw value by appending them to
/// `raw["annotations"]` (created if absent). Pure; used at model load so
/// `list_annotations` and the comments panel work unchanged for HTML sources.
pub fn merge_annotations(mut raw: Value, annotations: Vec<Value>) -> Value {
    if annotations.is_empty() {
        return raw;
    }
    if let Some(obj) = raw.as_object_mut() {
        let arr = obj
            .entry("annotations")
            .or_insert_with(|| Value::Array(vec![]));
        if let Value::Array(a) = arr {
            a.extend(annotations);
        }
    }
    raw
}

/// Read and parse the sidecar for an HTML source. A missing sidecar is
/// `Ok(vec![])`; an unreadable or malformed one is `Err(reason)` so callers
/// surface it instead of clobbering the file.
pub fn read_sidecar(html_path: &Path) -> Result<Vec<Value>, String> {
    let p = sidecar_path(html_path);
    let text = match std::fs::read_to_string(&p) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(format!("read {}: {e}", p.display())),
    };
    let doc: Value =
        serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", p.display()))?;
    doc.get("annotations")
        .and_then(|a| a.as_array())
        .cloned()
        .ok_or_else(|| format!("{}: missing \"annotations\" array", p.display()))
}

/// Append one annotation to the sidecar, creating the file lazily on first
/// use. Refuses to overwrite a corrupt sidecar (returns its parse error).
pub fn append_sidecar_annotation(html_path: &Path, annotation: Value) -> Result<(), String> {
    let mut annotations = read_sidecar(html_path)?;
    annotations.push(annotation);
    write_sidecar(html_path, &annotations)
}

/// Remove the annotation with `id` from the sidecar. Deletes the file when it
/// becomes empty (no ghost `{"annotations": []}` files).
pub fn remove_sidecar_annotation(html_path: &Path, id: &str) -> Result<(), String> {
    let mut annotations = read_sidecar(html_path)?;
    annotations.retain(|x| x.get("id").and_then(|v| v.as_str()) != Some(id));
    if annotations.is_empty() {
        let p = sidecar_path(html_path);
        match std::fs::remove_file(&p) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("remove {}: {e}", p.display())),
        }
    } else {
        write_sidecar(html_path, &annotations)
    }
}

/// Serialize `{"annotations": [...]}` (pretty) to the sidecar path.
fn write_sidecar(html_path: &Path, annotations: &[Value]) -> Result<(), String> {
    let doc = serde_json::json!({ "annotations": annotations });
    let text = serde_json::to_string_pretty(&doc).map_err(|e| format!("json: {e}"))?;
    let p = sidecar_path(html_path);
    std::fs::write(&p, text).map_err(|e| format!("write {}: {e}", p.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Unique per-test HTML path in the OS temp dir (no tempfile dependency).
    fn temp_html(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir();
        let p = dir.join(format!("rm_sidecar_{}_{}.html", tag, std::process::id()));
        // Clean any leftover sidecar from a previous crashed run.
        let _ = std::fs::remove_file(sidecar_path(&p));
        p
    }

    fn ann(id: &str) -> Value {
        json!({
            "id": id, "note": "make it smaller", "status": "open", "frame": 12,
            "view": 0, "scene": 0,
            "target": { "pointer": "/scenes/0/children/1", "kind": "text" }
        })
    }

    #[test]
    fn sidecar_path_derives_next_to_source() {
        assert_eq!(
            sidecar_path(Path::new("/work/demo/foo.html")),
            PathBuf::from("/work/demo/foo.annotations.json")
        );
        assert_eq!(
            sidecar_path(Path::new("/work/demo/bar.htm")),
            PathBuf::from("/work/demo/bar.annotations.json")
        );
    }

    #[test]
    fn merge_appends_into_raw_annotations() {
        let raw = json!({ "video": { "width": 1, "height": 1 }, "scenes": [] });
        let merged = merge_annotations(raw, vec![ann("a1"), ann("a2")]);
        let arr = merged["annotations"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["id"], "a1");

        // Existing annotations are preserved, sidecar ones appended after.
        let raw = json!({ "annotations": [ { "id": "pre" } ] });
        let merged = merge_annotations(raw, vec![ann("post")]);
        let arr = merged["annotations"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["id"], "pre");
        assert_eq!(arr[1]["id"], "post");
    }

    #[test]
    fn missing_sidecar_reads_as_empty() {
        let html = temp_html("missing");
        assert_eq!(read_sidecar(&html), Ok(vec![]));
    }

    #[test]
    fn append_creates_sidecar_with_expected_format() {
        let html = temp_html("append");
        append_sidecar_annotation(&html, ann("an_1")).unwrap();

        let text = std::fs::read_to_string(sidecar_path(&html)).unwrap();
        let doc: Value = serde_json::from_str(&text).unwrap();
        let arr = doc["annotations"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0], ann("an_1"));
        // Exactly one top-level key: the same `annotations` field JSON
        // scenarios use.
        assert_eq!(doc.as_object().unwrap().len(), 1);

        let _ = std::fs::remove_file(sidecar_path(&html));
    }

    #[test]
    fn remove_keeps_remaining_and_deletes_when_empty() {
        let html = temp_html("remove");
        append_sidecar_annotation(&html, ann("a1")).unwrap();
        append_sidecar_annotation(&html, ann("a2")).unwrap();

        remove_sidecar_annotation(&html, "a1").unwrap();
        let left = read_sidecar(&html).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0]["id"], "a2");

        // Removing the last annotation deletes the file (no ghost sidecar).
        remove_sidecar_annotation(&html, "a2").unwrap();
        assert!(!sidecar_path(&html).exists());

        // Removing from a missing sidecar is a no-op, not an error.
        assert_eq!(remove_sidecar_annotation(&html, "a2"), Ok(()));
    }

    #[test]
    fn corrupt_sidecar_errors_and_is_never_overwritten() {
        let html = temp_html("corrupt");
        std::fs::write(sidecar_path(&html), "{ not json").unwrap();

        assert!(read_sidecar(&html).is_err());
        assert!(append_sidecar_annotation(&html, ann("x")).is_err());
        assert!(remove_sidecar_annotation(&html, "x").is_err());

        // The corrupt content is still intact — nothing clobbered it.
        let text = std::fs::read_to_string(sidecar_path(&html)).unwrap();
        assert_eq!(text, "{ not json");

        // A sidecar without an `annotations` array is also an error.
        std::fs::write(sidecar_path(&html), r#"{ "foo": 1 }"#).unwrap();
        assert!(read_sidecar(&html).is_err());

        let _ = std::fs::remove_file(sidecar_path(&html));
    }
}
