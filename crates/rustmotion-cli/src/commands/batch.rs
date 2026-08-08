//! Batch rendering: render one video per line of a JSONL data file.
//!
//! Design invariants:
//! - All input is validated (JSONL parse + name template + variable checks +
//!   the same schema/geometry pass `render` runs, see `validation::run_checks`)
//!   BEFORE any render starts. A corrupt or overflowing line does not waste
//!   render time, and does not silently produce a bad video either.
//! - Each data line is a JSON object; its fields become variable overrides.
//! - `{field}` in the name template is replaced by the field's JSON-rendered value;
//!   `{index}` by the 0-based line number. The resolved name may create
//!   subdirectories under `--output-dir` (e.g. `{lang}/{id}.mp4`) but may not
//!   escape it: a `..` component or an absolute path is rejected in preflight.
//! - Unknown variables (when the template has a `config` block) produce an actionable
//!   error listing declared variables — same as the single-file path.
//! - Exit code is non-zero if any render failed *or panicked* (`--jobs > 1`
//!   dispatches renders across worker threads; a panic in one is caught and
//!   counted as a failure, never silently dropped); partial success is reported.

use rustmotion::error::{Result, RustmotionError};
use rustmotion::loader::load_input_with_vars;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::commands::render::cmd_render;
use crate::commands::validation::{self, ValidationSource};

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

/// Reject a resolved output name that would escape `--output-dir` once joined
/// onto it. `Path::join` replaces the base entirely when the joined path is
/// absolute, and a `..` component walks back out of it regardless — either
/// way `output_dir.join(name)` can land anywhere on disk. `name` comes from
/// `{field}` substitution of the (untrusted) JSONL data file, so this check
/// runs in preflight, before any render starts, same as every other
/// preflight check in this module.
///
/// A plain relative path — including one that creates subdirectories, e.g.
/// `"en/abc.mp4"` — remains legal: only `ParentDir` (`..`), `RootDir`
/// (a leading `/`), and `Prefix` (a Windows drive/UNC root) are rejected.
fn reject_escaping_name(name: &str) -> std::result::Result<(), String> {
    for component in Path::new(name).components() {
        match component {
            Component::ParentDir => {
                return Err(format!(
                    "resolved output name '{name}' contains a '..' component, \
                     which would escape --output-dir"
                ))
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!(
                    "resolved output name '{name}' is an absolute path, \
                     which would escape --output-dir"
                ))
            }
            Component::CurDir | Component::Normal(_) => {}
        }
    }
    Ok(())
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
        if let Err(msg) = reject_escaping_name(&name) {
            preflight_errors.push(format!("line {}: {}", index + 1, msg));
            continue;
        }
        let output_path = output_dir.join(&name);

        // Validate overrides against the template: parse, resolve variables,
        // then run the same schema + geometry pass `render` runs before it
        // renders a single-file scenario (`validation::run_checks`, wired
        // with the same defaults `render` uses with no flags: blocking
        // geometry violations, no animated-frame sampling). A batch row is
        // exactly the situation the module doc warns about — N videos
        // produced in one shot — so it must not be the one path that skips
        // the "unwrappable_text_overflow" / viewport-overflow gate CLAUDE.md
        // requires. The loaded scenario itself is discarded here: `render_row`
        // reloads it below, right before the actual render.
        let loaded = match validation::load_with_vars(
            ValidationSource::File(template_path),
            Some(&overrides),
        ) {
            Ok(l) => l,
            Err(e) => {
                preflight_errors.push(format!("line {}: {}", index + 1, e));
                continue;
            }
        };
        let report = validation::run_checks(&loaded, false);
        if report.is_blocking(false) {
            preflight_errors.push(format!("line {}: {}", index + 1, report.to_error()));
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
            // `h.join()` returns `Err` when the worker thread panicked instead
            // of returning normally (e.g. a codec assertion on an odd frame
            // dimension). That row was neither counted in `success_arc` nor
            // pushed to `failures` by the closure above — it *only* runs its
            // bookkeeping on the `Ok`/`Err` return path, which a panic skips
            // entirely. Left unhandled, `_ = h.join()` was true to its
            // comment ("panics ... are surfaced as failures below") in name
            // only: nothing below ever inspected the join result, so a batch
            // where every worker panicked reported "0/N succeeded" and still
            // returned `Ok(())` — exit code 0 for a batch that rendered
            // nothing. Record it as a failure explicitly instead.
            if let Err(payload) = h.join() {
                failures.lock().unwrap().push(format!(
                    "worker thread panicked: {}",
                    panic_message(&payload)
                ));
            }
        }
        success_count = *success_arc.lock().unwrap();
    }

    let failure_list = failures.lock().unwrap().clone();
    let fail_count = failure_list.len();

    // Defense in depth: every row must end up counted as either a success or
    // a failure. This should already hold given the panic handling above,
    // but if it doesn't — a future refactor drops a bookkeeping update, a
    // panic happens somewhere neither counter is touched — fail loudly
    // rather than silently report a batch as complete when it wasn't.
    if success_count + fail_count != total {
        return Err(RustmotionError::Generic(format!(
            "batch accounting mismatch: {success_count} succeeded + {fail_count} failed \
             != {total} total item(s) — {} item(s) neither succeeded nor were reported as failed",
            total.saturating_sub(success_count + fail_count)
        )));
    }

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

/// Extract a human-readable message from a caught thread panic payload.
/// `std::thread::Result`'s `Err` variant is `Box<dyn Any + Send>`; panics
/// raised via `panic!("...")` / `.unwrap()` / `.expect(...)` box either a
/// `&'static str` or a `String`, which covers the vast majority of real
/// panics (including the openh264 `assert_eq!` this fix was written for).
/// Anything else (a custom payload via `std::panic::panic_any`) still
/// produces a readable, if generic, message instead of losing the row.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
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
mod escaping_name_tests {
    use super::*;

    #[test]
    fn plain_relative_name_is_allowed() {
        assert!(reject_escaping_name("abc.mp4").is_ok());
    }

    /// `{lang}/{id}.mp4`-style names are a documented, legitimate feature
    /// (`field_placeholder_resolved` above): rejecting every non-`Normal`
    /// path component would also reject this, which is why only
    /// `ParentDir`/`RootDir`/`Prefix` are rejected, not every multi-segment
    /// name.
    #[test]
    fn relative_subdirectory_name_is_allowed() {
        assert!(reject_escaping_name("en/abc.mp4").is_ok());
    }

    #[test]
    fn parent_dir_component_is_rejected() {
        let err = reject_escaping_name("../../escaped.mp4").unwrap_err();
        assert!(
            err.contains(".."),
            "error must call out the '..' component: {err}"
        );
    }

    #[test]
    fn absolute_unix_path_is_rejected() {
        let err = reject_escaping_name("/tmp/scratch/pwned/absolute.mp4").unwrap_err();
        assert!(
            err.contains("absolute"),
            "error must call out the absolute path: {err}"
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

    /// Constat #2/#4: a `{field}` value from the (untrusted) JSONL data file
    /// containing `..` must not be able to walk `output_dir.join(name)` back
    /// out of `--output-dir`. Reproduces the brief's scenario: `--output-dir
    /// <base>/sub/dir`, `--name-template '{name}.png'`, data field
    /// `"../../escaped"` — before the fix this created
    /// `<base>/escaped.png/frame_00000.png`, outside `<base>/sub/dir`.
    #[test]
    fn batch_rejects_name_that_escapes_output_dir_via_parent_dir() {
        let template = write_json(&minimal_template("name"), "tmpl_escape_dotdot");
        let data = write_jsonl(
            &[serde_json::json!({"name": "../../escaped"})],
            "escape_dotdot",
        );
        let base = std::env::temp_dir().join(format!("rm_batch_escdd_{}", std::process::id()));
        let out_dir = base.join("sub").join("dir");
        std::fs::create_dir_all(&out_dir).unwrap();

        let err = cmd_batch(
            &template,
            &data,
            &out_dir,
            "{name}.png",
            None,
            None,
            Some("png-seq".to_string()),
            false,
            1,
            true,
        )
        .expect_err("a name containing '..' must be rejected in preflight");

        assert!(
            err.to_string().contains(".."),
            "error must call out the escaping component: {err}"
        );

        // `<base>/sub/dir/../../escaped.png` lexically resolves to
        // `<base>/escaped.png` — it must never have been created.
        let escaped_target = base.join("escaped.png");
        assert!(
            !escaped_target.exists(),
            "traversal target must not have been created: {}",
            escaped_target.display()
        );

        let _ = std::fs::remove_file(&template);
        let _ = std::fs::remove_file(&data);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Constat #2/#4, absolute-path variant: `Path::join` with an absolute
    /// component discards the base entirely, so a `{field}` value that is
    /// itself an absolute path writes straight to that path, ignoring
    /// `--output-dir` completely.
    #[test]
    fn batch_rejects_name_that_is_an_absolute_path() {
        let template = write_json(&minimal_template("name"), "tmpl_escape_abs");
        let escape_target =
            std::env::temp_dir().join(format!("rm_batch_abs_escape_target_{}", std::process::id()));
        let name_value = escape_target.to_string_lossy().to_string();
        let data = write_jsonl(&[serde_json::json!({"name": name_value})], "escape_abs");
        let out_dir =
            std::env::temp_dir().join(format!("rm_batch_out_escabs_{}", std::process::id()));
        std::fs::create_dir_all(&out_dir).unwrap();

        let target_with_ext = PathBuf::from(format!("{}.png", escape_target.display()));
        let _ = std::fs::remove_dir_all(&target_with_ext);

        let err = cmd_batch(
            &template,
            &data,
            &out_dir,
            "{name}.png",
            None,
            None,
            Some("png-seq".to_string()),
            false,
            1,
            true,
        )
        .expect_err("an absolute output name must be rejected in preflight");

        assert!(
            err.to_string().to_lowercase().contains("absolute"),
            "error must call out the absolute path: {err}"
        );
        assert!(
            !target_with_ext.exists(),
            "absolute-path target must not have been created: {}",
            target_with_ext.display()
        );

        let _ = std::fs::remove_file(&template);
        let _ = std::fs::remove_file(&data);
        let _ = std::fs::remove_dir_all(&out_dir);
    }

    /// The traversal fix must not regress the documented `{field}/{field}`
    /// nested-subdirectory feature (`field_placeholder_resolved` unit test):
    /// only `..`/absolute components are rejected, plain relative
    /// subdirectories still get created under `--output-dir`.
    #[test]
    fn batch_relative_subdirectory_name_still_creates_nested_output() {
        let template = write_json(&minimal_template("lang"), "tmpl_subdir");
        let data = write_jsonl(&[serde_json::json!({"lang": "en"})], "subdir");
        let out_dir =
            std::env::temp_dir().join(format!("rm_batch_out_subdir_{}", std::process::id()));
        std::fs::create_dir_all(&out_dir).unwrap();

        cmd_batch(
            &template,
            &data,
            &out_dir,
            "{lang}/{index}.png",
            None,
            None,
            Some("png-seq".to_string()),
            false,
            1,
            true,
        )
        .expect("a relative subdirectory name must still be allowed");

        let subdir = out_dir.join("en").join("0.png");
        assert!(
            subdir.exists() && subdir.is_dir(),
            "expected nested output dir en/0.png to exist"
        );

        let _ = std::fs::remove_file(&template);
        let _ = std::fs::remove_file(&data);
        let _ = std::fs::remove_dir_all(&out_dir);
    }

    /// Constat #3: `batch` must run the same schema + geometry validation
    /// `render` runs before it renders a single-file scenario — CLAUDE.md's
    /// "schema + geometry, les deux doivent passer" rule applies just as
    /// much to a batch row. Reproduces the brief's scenario: a 320×180
    /// template with 96px `white-space: nowrap` text and a long `$title`
    /// override overflows the viewport; `render` blocks on it, so `batch`
    /// must too, before any render starts.
    #[test]
    fn batch_preflight_rejects_a_geometry_overflow() {
        let template = write_json(
            &serde_json::json!({
                "config": { "title": { "type": "string", "default": "x" } },
                "video": { "width": 320, "height": 180, "fps": 1 },
                "scenes": [{
                    "duration": 1.0,
                    "children": [{
                        "type": "text",
                        "content": "$title",
                        "style": { "font-size": 96, "white-space": "nowrap" }
                    }]
                }]
            }),
            "tmpl_overflow",
        );
        let data = write_jsonl(
            &[serde_json::json!({"title": "A ridiculously long overflowing headline"})],
            "overflow",
        );
        let out_dir =
            std::env::temp_dir().join(format!("rm_batch_out_overflow_{}", std::process::id()));
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
        .expect_err("a scenario that overflows the viewport must fail preflight, like `render`");

        assert!(
            err.to_string().contains("geometry violation"),
            "error must surface the geometry violation, matching `render`'s message: {err}"
        );
        assert!(
            !out_dir.join("0.png").exists(),
            "preflight failure must mean no render started"
        );

        let _ = std::fs::remove_file(&template);
        let _ = std::fs::remove_file(&data);
        let _ = std::fs::remove_dir_all(&out_dir);
    }

    /// Guard that temporarily restricts `PATH` to a minimal, `ffmpeg`-free
    /// set of directories, so `render`'s `Command::new("ffmpeg").arg(
    /// "-version")` availability probe fails and the built-in openh264
    /// fallback encoder runs instead — the codepath the audit's own repro
    /// deliberately forced with `env -i PATH=/usr/bin:/bin ...` to make the
    /// `YUVBuffer` even-dimension assertion panic reachable without a real
    /// `ffmpeg` dependency in CI. Restores the original `PATH` on drop.
    struct NoFfmpegPathGuard {
        original: Option<std::ffi::OsString>,
        _permit: std::sync::MutexGuard<'static, ()>,
    }

    /// Serializes every test in this file that mutates `PATH` — required
    /// because `std::env::set_var`/`remove_var` are `unsafe`: the standard
    /// library only guarantees soundness when nothing else in the process
    /// reads or writes the environment concurrently. This is the only test
    /// file in `rustmotion-cli` whose tests spawn a real (non `png-seq` /
    /// `gif` / `raw`) video encode — every other test in this crate's test
    /// binary never reads `PATH` — so this lock only needs to protect
    /// against concurrent runs of tests within this file.
    static PATH_MUTATION_LOCK: Mutex<()> = Mutex::new(());

    impl NoFfmpegPathGuard {
        fn install() -> Self {
            let permit = PATH_MUTATION_LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let original = std::env::var_os("PATH");
            // SAFETY: `_permit` holds `PATH_MUTATION_LOCK` for this guard's
            // entire lifetime (released on `Drop`, after `PATH` is restored
            // below), and per the lock's doc comment no other thread in this
            // binary reads/writes `PATH` while it is held.
            unsafe { std::env::set_var("PATH", "/usr/bin:/bin") };
            Self {
                original,
                _permit: permit,
            }
        }
    }

    impl Drop for NoFfmpegPathGuard {
        fn drop(&mut self) {
            // SAFETY: see `install`.
            unsafe {
                match &self.original {
                    Some(v) => std::env::set_var("PATH", v),
                    None => std::env::remove_var("PATH"),
                }
            }
        }
    }

    /// Constat #1: with `--jobs > 1`, a worker thread that panics (instead of
    /// returning `Err`) was invisible to `cmd_batch` — `let _ = h.join()`
    /// discarded the panic, so neither `success_arc` nor `failures` was ever
    /// incremented for that row. A batch where *every* worker panicked
    /// reported "Batch complete: 0/N succeeded." and still returned `Ok(())`
    /// — exit code 0 for a batch that rendered nothing.
    ///
    /// The brief's own reproduction (odd frame dimensions, e.g. 321×241,
    /// forcing an `openh264` `assert_eq!(width % 2, 0, ...)` panic) is no
    /// longer reachable *through `batch`* once constat #3 is fixed: schema
    /// validation (`video.width and video.height must be even`, wired in by
    /// the constat #3 fix above) now rejects that scenario in preflight,
    /// before any worker thread is even spawned — a nice side effect, but it
    /// means this test needs a different panic that survives preflight.
    /// `create_encoder` (crates/rustmotion/src/encode/video/h264.rs) computes
    /// `let pixels = width * height;` in plain (checked) `u32` arithmetic
    /// before ever touching a frame buffer; 70000×70000 is even, positive,
    /// and passes every schema/geometry check, but `70000 * 70000 =
    /// 4_900_000_000` overflows `u32::MAX` (4_294_967_295) and panics with
    /// "attempt to multiply with overflow" — deterministically, and before
    /// any expensive Skia canvas allocation happens for the (nonexistent)
    /// video frame. Still requires the openh264 fallback (`create_encoder`
    /// is only reached when `ffmpeg` is unavailable), hence
    /// `NoFfmpegPathGuard`.
    #[test]
    fn batch_parallel_worker_panic_is_reported_as_failure_not_silent_success() {
        let template = write_json(
            &serde_json::json!({
                "video": { "width": 70000, "height": 70000, "fps": 1 },
                "scenes": [{ "duration": 1.0, "children": [] }]
            }),
            "tmpl_overflow_dims",
        );
        let data = write_jsonl(
            &[serde_json::json!({}), serde_json::json!({})],
            "overflow_dims",
        );
        let out_dir =
            std::env::temp_dir().join(format!("rm_batch_out_overflowdims_{}", std::process::id()));
        std::fs::create_dir_all(&out_dir).unwrap();

        let _no_ffmpeg = NoFfmpegPathGuard::install();

        // Run `cmd_batch` on a worker so a hang (e.g. a deadlock introduced
        // by a broken fix) fails the test instead of wedging the whole
        // suite, mirroring `paint_within` in
        // crates/rustmotion-components/tests/degenerate_inputs.rs.
        let (tx, rx) = std::sync::mpsc::channel();
        let t_template = template.clone();
        let t_data = data.clone();
        let t_out_dir = out_dir.clone();
        let worker = std::thread::spawn(move || {
            let result = cmd_batch(
                &t_template,
                &t_data,
                &t_out_dir,
                "{index}.mp4",
                None,
                None,
                None, // default format: the mp4/openh264 path under test
                false,
                2, // jobs: exercises the parallel path constat #1 is about
                true,
            );
            let _ = tx.send(result);
        });

        let result = match rx.recv_timeout(std::time::Duration::from_secs(30)) {
            Ok(r) => {
                worker
                    .join()
                    .expect("outer worker thread must not itself panic");
                r
            }
            Err(_) => {
                panic!("cmd_batch did not return within 30s — possible deadlock in the join/accounting fix")
            }
        };

        assert!(
            result.is_err(),
            "a batch where every worker panicked must not report success"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.to_lowercase().contains("panic"),
            "the failure must surface the panic, not just an opaque render failure: {msg}"
        );

        let _ = std::fs::remove_file(&template);
        let _ = std::fs::remove_file(&data);
        let _ = std::fs::remove_dir_all(&out_dir);
    }
}
