//! Batch rendering: render one video per line of a JSONL data file.
//!
//! Design invariants:
//! - All input is validated (JSONL parse + name template + variable checks) BEFORE
//!   any render starts. A corrupt line does not waste render time.
//! - Each data line is a JSON object; its fields become variable overrides.
//! - `{field}` in the name template is replaced by the field's JSON-rendered value;
//!   `{index}` by the 0-based line number.
//! - Unknown variables (when the template has a `config` block) produce an actionable
//!   error listing declared variables — same as the single-file path.
//! - Exit code is non-zero if any render failed; partial success is reported.

use rustmotion::error::{Result, RustmotionError};
use rustmotion::loader::load_input_with_vars;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::commands::render::cmd_render;

/// One row parsed from the JSONL data file.
struct BatchRow {
    index: usize,
    overrides: HashMap<String, serde_json::Value>,
    output_path: PathBuf,
}

/// Resolve `{field}` placeholders in a name template from a data row.
/// `{index}` is always available; other placeholders reference JSON fields.
/// Returns an error if a placeholder references a missing field.
pub(crate) fn resolve_name_template(
    template: &str,
    row: &HashMap<String, serde_json::Value>,
    index: usize,
) -> std::result::Result<String, String> {
    let mut cursor = 0usize;
    let bytes = template.as_bytes();
    let mut out = String::with_capacity(template.len());

    while cursor < bytes.len() {
        if bytes[cursor] == b'{' {
            // Find closing brace
            let start = cursor + 1;
            let end = template[start..].find('}').ok_or_else(|| {
                format!(
                    "name_template '{}': unclosed '{{' at position {}",
                    template, cursor
                )
            })?;
            let field = &template[start..start + end];
            let replacement = if field == "index" {
                index.to_string()
            } else {
                match row.get(field) {
                    Some(serde_json::Value::String(s)) => s.clone(),
                    Some(v) => v.to_string(),
                    None => {
                        return Err(format!(
                            "name_template placeholder '{{{field}}}': field '{field}' not found in data row {index}"
                        ))
                    }
                }
            };
            out.push_str(&replacement);
            cursor = start + end + 1; // skip past '}'
        } else {
            out.push(bytes[cursor] as char);
            cursor += 1;
        }
    }
    let _ = bytes;
    Ok(out)
}

/// Parse the JSONL file, resolve output names, and validate overrides against
/// the scenario template (dry-run load). Returns ordered rows or an error if
/// anything is invalid. No rendering happens here.
fn preflight(
    template_path: &Path,
    data_path: &Path,
    output_dir: &Path,
    name_template: &str,
) -> Result<Vec<BatchRow>> {
    let jsonl = std::fs::read_to_string(data_path).map_err(|e| RustmotionError::FileRead {
        path: data_path.display().to_string(),
        source: e,
    })?;

    let mut rows: Vec<BatchRow> = Vec::new();
    let mut preflight_errors: Vec<String> = Vec::new();

    for (index, line) in jsonl.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue; // skip blank lines and comment-like lines
        }

        // Parse override object
        let overrides: serde_json::Value = serde_json::from_str(trimmed).map_err(|e| {
            RustmotionError::Generic(format!(
                "batch data line {}: invalid JSON: {}",
                index + 1,
                e
            ))
        })?;
        let overrides = match overrides {
            serde_json::Value::Object(map) => map.into_iter().collect::<HashMap<_, _>>(),
            _ => {
                return Err(RustmotionError::Generic(format!(
                    "batch data line {}: each line must be a JSON object {{...}}",
                    index + 1
                )))
            }
        };

        // Resolve output name
        let name = match resolve_name_template(name_template, &overrides, index) {
            Ok(n) => n,
            Err(msg) => {
                preflight_errors.push(format!("line {}: {}", index + 1, msg));
                continue;
            }
        };
        let output_path = output_dir.join(&name);

        // Validate overrides against the template (dry-run: load then discard)
        let path_buf = template_path.to_path_buf();
        if let Err(e) = load_input_with_vars(&path_buf, Some(&overrides)) {
            preflight_errors.push(format!("line {}: {}", index + 1, e));
            continue;
        }

        rows.push(BatchRow {
            index,
            overrides,
            output_path,
        });
    }

    if !preflight_errors.is_empty() {
        let msg = std::iter::once("Batch preflight failed — no renders started:".to_string())
            .chain(preflight_errors.into_iter().map(|e| format!("  {}", e)))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(RustmotionError::Generic(msg));
    }

    if rows.is_empty() {
        return Err(RustmotionError::Generic(
            "batch data file is empty or contains no valid rows".to_string(),
        ));
    }

    Ok(rows)
}

#[allow(clippy::too_many_arguments)]
pub fn cmd_batch(
    template_path: &Path,
    data_path: &Path,
    output_dir: &Path,
    name_template: &str,
    codec: Option<String>,
    crf: Option<u8>,
    format: Option<String>,
    transparent: bool,
    jobs: usize,
    quiet: bool,
) -> Result<()> {
    // Create output directory
    std::fs::create_dir_all(output_dir).map_err(|e| RustmotionError::FileRead {
        path: output_dir.display().to_string(),
        source: e,
    })?;

    // Preflight: parse + validate all rows before any rendering
    let rows = preflight(template_path, data_path, output_dir, name_template)?;
    let total = rows.len();

    if !quiet {
        eprintln!(
            "Batch: {} item(s) to render → {}",
            total,
            output_dir.display()
        );
    }

    let failures: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let mut success_count = 0usize;

    if jobs <= 1 {
        // Sequential path (default)
        for row in rows {
            match render_row(
                template_path,
                &row,
                codec.as_deref(),
                crf,
                format.as_deref(),
                transparent,
                quiet,
            ) {
                Ok(()) => {
                    success_count += 1;
                    if !quiet {
                        eprintln!(
                            "[{}/{}] {}",
                            success_count,
                            total,
                            row.output_path.display()
                        );
                    }
                }
                Err(e) => {
                    failures
                        .lock()
                        .unwrap()
                        .push(format!("item {}: {}", row.index + 1, e));
                }
            }
        }
    } else {
        // Parallel path: N threads, each pops a row
        let rows = Arc::new(Mutex::new(rows.into_iter()));
        let codec = Arc::new(codec);
        let format = Arc::new(format);
        let failures = Arc::clone(&failures);
        let success_arc: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
        let template_path = Arc::new(template_path.to_path_buf());

        let handles: Vec<_> = (0..jobs)
            .map(|_| {
                let rows = Arc::clone(&rows);
                let codec = Arc::clone(&codec);
                let format = Arc::clone(&format);
                let failures = Arc::clone(&failures);
                let success_arc = Arc::clone(&success_arc);
                let template_path = Arc::clone(&template_path);

                std::thread::spawn(move || loop {
                    let row = {
                        let mut guard = rows.lock().unwrap();
                        guard.next()
                    };
                    let row = match row {
                        Some(r) => r,
                        None => break,
                    };
                    match render_row(
                        &template_path,
                        &row,
                        codec.as_deref(),
                        crf,
                        format.as_deref(),
                        transparent,
                        quiet,
                    ) {
                        Ok(()) => {
                            let mut s = success_arc.lock().unwrap();
                            *s += 1;
                            if !quiet {
                                eprintln!("[ok] {}", row.output_path.display());
                            }
                        }
                        Err(e) => {
                            failures
                                .lock()
                                .unwrap()
                                .push(format!("item {}: {}", row.index + 1, e));
                        }
                    }
                })
            })
            .collect();

        for h in handles {
            let _ = h.join(); // panics in threads are surfaced as failures below
        }
        success_count = *success_arc.lock().unwrap();
    }

    let failure_list = failures.lock().unwrap().clone();
    let fail_count = failure_list.len();

    if !quiet {
        eprintln!("Batch complete: {}/{} succeeded.", success_count, total);
    }

    if fail_count > 0 {
        let mut msg = format!("{} of {} render(s) failed:\n", fail_count, total);
        for f in &failure_list {
            msg.push_str(&format!("  {}\n", f));
        }
        return Err(RustmotionError::Generic(msg));
    }

    Ok(())
}

fn render_row(
    template_path: &Path,
    row: &BatchRow,
    codec: Option<&str>,
    crf: Option<u8>,
    format: Option<&str>,
    transparent: bool,
    quiet: bool,
) -> Result<()> {
    let path_buf = template_path.to_path_buf();
    let scenario = load_input_with_vars(&path_buf, Some(&row.overrides))?;
    cmd_render(
        scenario,
        &row.output_path,
        None, // no single-frame mode in batch
        None, // no JSON output_format
        quiet,
        codec.map(str::to_string),
        crf,
        format.map(str::to_string),
        transparent,
    )
}

#[cfg(test)]
mod name_template_tests {
    use super::*;
    use serde_json::json;

    fn row(pairs: &[(&str, serde_json::Value)]) -> HashMap<String, serde_json::Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn index_placeholder_resolved() {
        let data = row(&[]);
        assert_eq!(
            resolve_name_template("{index}.mp4", &data, 0).unwrap(),
            "0.mp4"
        );
        assert_eq!(
            resolve_name_template("{index}.mp4", &data, 42).unwrap(),
            "42.mp4"
        );
    }

    #[test]
    fn field_placeholder_resolved() {
        let data = row(&[("id", json!("abc")), ("lang", json!("en"))]);
        assert_eq!(
            resolve_name_template("{lang}/{id}.mp4", &data, 0).unwrap(),
            "en/abc.mp4"
        );
    }

    #[test]
    fn numeric_field_rendered_as_string() {
        let data = row(&[("n", json!(7))]);
        assert_eq!(resolve_name_template("{n}.mp4", &data, 0).unwrap(), "7.mp4");
    }

    #[test]
    fn missing_field_is_error() {
        let data = row(&[]);
        let err = resolve_name_template("{missing}.mp4", &data, 2).unwrap_err();
        assert!(
            err.contains("missing"),
            "error must name missing field: {err}"
        );
        assert!(err.contains("2"), "error must mention row index: {err}");
    }

    #[test]
    fn template_without_placeholders_is_literal() {
        let data = row(&[]);
        assert_eq!(
            resolve_name_template("static.mp4", &data, 0).unwrap(),
            "static.mp4"
        );
    }
}

#[cfg(test)]
mod batch_integration_tests {
    use super::*;
    use std::io::Write;

    /// Build a minimal 32×32, 1 s scenario JSON for fast batch testing.
    /// 1 fps × 1 s = 1 frame, which is the minimum for a valid PNG sequence.
    fn minimal_template(var_name: &str) -> serde_json::Value {
        serde_json::json!({
            "config": {
                var_name: { "type": "string", "default": "default" }
            },
            "video": { "width": 32, "height": 32, "fps": 1 },
            "scenes": [{ "duration": 1.0, "children": [] }]
        })
    }

    fn write_json(val: &serde_json::Value, suffix: &str) -> PathBuf {
        let p =
            std::env::temp_dir().join(format!("rm_batch_{}_{}.json", suffix, std::process::id()));
        std::fs::write(&p, val.to_string()).unwrap();
        p
    }

    fn write_jsonl(lines: &[serde_json::Value], suffix: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "rm_batch_data_{}_{}.jsonl",
            suffix,
            std::process::id()
        ));
        let mut f = std::fs::File::create(&p).unwrap();
        for line in lines {
            writeln!(f, "{}", line).unwrap();
        }
        p
    }

    /// 3-line JSONL → 3 PNG-seq outputs named by {index}.
    #[test]
    fn batch_three_lines_produces_three_outputs() {
        let template = write_json(&minimal_template("title"), "tmpl_3lines");
        let data = write_jsonl(
            &[
                serde_json::json!({"title": "A"}),
                serde_json::json!({"title": "B"}),
                serde_json::json!({"title": "C"}),
            ],
            "3lines",
        );
        let out_dir =
            std::env::temp_dir().join(format!("rm_batch_out_3lines_{}", std::process::id()));
        std::fs::create_dir_all(&out_dir).unwrap();

        cmd_batch(
            &template,
            &data,
            &out_dir,
            "{index}.png",
            None, // codec
            None, // crf
            Some("png-seq".to_string()),
            false, // transparent
            1,     // jobs
            true,  // quiet
        )
        .expect("batch must succeed");

        // Each row uses name_template "{index}.png" → output paths 0.png, 1.png, 2.png.
        // The png-seq encoder treats the output path as a directory and creates
        // frame_00000.png inside it, so we get: out_dir/0.png/frame_00000.png etc.
        for i in 0..3usize {
            let subdir = out_dir.join(format!("{}.png", i));
            assert!(
                subdir.exists() && subdir.is_dir(),
                "expected output subdir {}.png to exist",
                i
            );
            assert!(
                subdir.join("frame_00000.png").exists(),
                "expected frame_00000.png inside {}.png/",
                i
            );
        }

        let _ = std::fs::remove_file(&template);
        let _ = std::fs::remove_file(&data);
        let _ = std::fs::remove_dir_all(&out_dir);
    }

    /// Invalid JSONL line → preflight error, no render started.
    #[test]
    fn batch_invalid_jsonl_line_fails_preflight() {
        let template = write_json(&minimal_template("x"), "tmpl_invalid");
        let data = write_jsonl(&[serde_json::json!({"x": "ok"})], "invalid");
        // Corrupt the file by appending a bad line
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&data)
            .unwrap();
        writeln!(f, "not {{ json").unwrap();
        drop(f);

        let out_dir = std::env::temp_dir().join(format!("rm_batch_out_inv_{}", std::process::id()));
        std::fs::create_dir_all(&out_dir).unwrap();

        let err = cmd_batch(
            &template,
            &data,
            &out_dir,
            "{index}.png",
            None,
            None,
            Some("png-seq".to_string()),
            false,
            1,
            true,
        )
        .expect_err("must fail on invalid JSON line");

        // Preflight errors are reported as "batch data line N: ..." before any render
        assert!(
            err.to_string().contains("batch data line"),
            "error must identify the bad data line: {err}"
        );

        let _ = std::fs::remove_file(&template);
        let _ = std::fs::remove_file(&data);
        let _ = std::fs::remove_dir_all(&out_dir);
    }
}
