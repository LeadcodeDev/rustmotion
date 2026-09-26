use std::path::Path;

use serde_json::Value;

const PATH_KEYS: &[&str] = &["src", "track"];

fn is_remote(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://") || s.starts_with("data:")
}

pub fn rebase_relative_paths(value: &mut Value, base_dir: &Path) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if PATH_KEYS.contains(&key.as_str()) {
                    if let Value::String(s) = child {
                        if let Some(abs) = rebased(s, base_dir) {
                            *s = abs;
                            continue;
                        }
                    }
                }
                rebase_relative_paths(child, base_dir);
            }
        }
        Value::Array(items) => {
            for item in items {
                rebase_relative_paths(item, base_dir);
            }
        }
        _ => {}
    }
}

fn rebased(src: &str, base_dir: &Path) -> Option<String> {
    if src.is_empty() || is_remote(src) {
        return None;
    }
    let path = Path::new(src);
    if path.is_absolute() {
        return None;
    }
    let candidate = base_dir.join(path);
    if !candidate.is_file() {
        return None;
    }
    let resolved = std::fs::canonicalize(&candidate).unwrap_or(candidate);
    Some(resolved.to_str()?.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scratch() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rustmotion_assets_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(dir.join("assets")).expect("scratch dir");
        dir
    }

    #[test]
    fn a_relative_src_next_to_the_scenario_becomes_absolute() {
        let dir = scratch();
        std::fs::write(dir.join("assets/logo.png"), b"x").expect("fixture");

        let mut v =
            json!({"scenes": [{"children": [{"type": "image", "src": "assets/logo.png"}]}]});
        rebase_relative_paths(&mut v, &dir);

        let got = v["scenes"][0]["children"][0]["src"].as_str().expect("src");
        assert!(Path::new(got).is_absolute(), "not rewritten: {got}");
        assert!(Path::new(got).is_file(), "rewritten to a non-file: {got}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_result_does_not_depend_on_the_working_directory() {
        let dir = scratch();
        std::fs::write(dir.join("assets/logo.png"), b"x").expect("fixture");

        let mut a = json!({"src": "assets/logo.png"});
        let mut b = json!({"src": "assets/logo.png"});
        rebase_relative_paths(&mut a, &dir);
        rebase_relative_paths(&mut b, &dir);
        assert_eq!(a, b);
        assert_ne!(a["src"], json!("assets/logo.png"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_file_is_left_untouched() {
        let dir = scratch();
        let mut v = json!({"src": "assets/absent.png"});
        rebase_relative_paths(&mut v, &dir);
        assert_eq!(v["src"], json!("assets/absent.png"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn absolute_and_remote_sources_are_left_untouched() {
        let dir = scratch();
        let mut v = json!({
            "a": {"src": "/etc/hosts"},
            "b": {"src": "https://example.com/x.png"},
            "c": {"src": "data:image/png;base64,AAAA"}
        });
        let before = v.clone();
        rebase_relative_paths(&mut v, &dir);
        assert_eq!(v, before);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn track_is_rebased_like_src_so_the_analysis_lookup_still_matches() {
        let dir = scratch();
        std::fs::write(dir.join("assets/t.wav"), b"x").expect("fixture");

        let mut v = json!({
            "audio": [{"src": "assets/t.wav"}],
            "scenes": [{"children": [{"type": "waveform", "track": "assets/t.wav"}]}]
        });
        rebase_relative_paths(&mut v, &dir);

        assert_eq!(
            v["audio"][0]["src"], v["scenes"][0]["children"][0]["track"],
            "src and track must resolve to the same string"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unrelated_keys_are_not_rewritten() {
        let dir = scratch();
        std::fs::write(dir.join("assets/logo.png"), b"x").expect("fixture");
        let mut v = json!({"content": "assets/logo.png", "title": "assets/logo.png"});
        let before = v.clone();
        rebase_relative_paths(&mut v, &dir);
        assert_eq!(v, before);
        std::fs::remove_dir_all(&dir).ok();
    }
}
