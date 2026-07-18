//! `rustmotion captions` — generate word-level caption timings.
//!
//! Two modes:
//! - **Transcription** (default): shells out to a whisper.cpp binary
//!   (`whisper-cli`, `whisper-cpp` or `main` in PATH), following the same
//!   philosophy as ffmpeg: auto-detection, actionable error when absent,
//!   no compiled-in C++ dependency.
//! - **Import**: `--from-srt` / `--from-vtt` parse subtitle files offline
//!   with a hand-rolled parser (no dependency). Word timing inside a cue is
//!   spread uniformly over the cue duration (approximation).
//!
//! Output shape (stdout or `-o`): `{"words":[{"text","start","end"}]}` —
//! directly usable as a `--props` file with a `"words": "$words"` variable
//! reference in the scenario.

use rustmotion::error::{Result, RustmotionError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

/// A single timed word, as emitted in the output JSON.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TimedWord {
    pub text: String,
    pub start: f64,
    pub end: f64,
}

/// Output file shape: `{"words": [...]}`.
#[derive(Debug, Serialize)]
struct WordsFile<'a> {
    words: &'a [TimedWord],
}

/// A subtitle cue (SRT/VTT): a time window and its (already cleaned) text.
#[derive(Debug, Clone, PartialEq)]
struct Cue {
    start: f64,
    end: f64,
    text: String,
}

// ---------------------------------------------------------------------------
// Subtitle parsing (SRT / VTT)
// ---------------------------------------------------------------------------

fn parse_srt(input: &str) -> Result<Vec<Cue>> {
    parse_cues(input, false)
}

fn parse_vtt(input: &str) -> Result<Vec<Cue>> {
    parse_cues(input, true)
}

/// Shared SRT/VTT cue parser: blocks separated by blank lines, each with a
/// `start --> end` timing line followed by (possibly multi-line) text.
fn parse_cues(input: &str, vtt: bool) -> Result<Vec<Cue>> {
    let input = input
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n");

    let mut cues = Vec::new();
    for block in input.split("\n\n") {
        let lines: Vec<&str> = block
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        let Some(first) = lines.first() else { continue };
        if vtt
            && (first.starts_with("WEBVTT")
                || first.starts_with("NOTE")
                || first.starts_with("STYLE")
                || first.starts_with("REGION"))
        {
            continue;
        }
        // The timing line is usually line 0 (VTT) or line 1 (SRT index /
        // VTT cue identifier before it).
        let Some(timing_idx) = lines.iter().position(|l| l.contains("-->")) else {
            continue;
        };
        let Some((start, end)) = parse_timing_line(lines[timing_idx]) else {
            continue;
        };
        let text = strip_tags(&lines[timing_idx + 1..].join(" "));
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            continue;
        }
        cues.push(Cue { start, end, text });
    }

    if cues.is_empty() {
        return Err(RustmotionError::Generic(format!(
            "no cues found in {} input — expected \"HH:MM:SS{}mmm --> HH:MM:SS{}mmm\" timing lines followed by text",
            if vtt { "VTT" } else { "SRT" },
            if vtt { "." } else { "," },
            if vtt { "." } else { "," },
        )));
    }
    Ok(cues)
}

/// Parses `start --> end [cue settings...]` into a `(start, end)` pair.
fn parse_timing_line(line: &str) -> Option<(f64, f64)> {
    let (left, right) = line.split_once("-->")?;
    let start = parse_timestamp(left)?;
    // The end timestamp may be followed by VTT cue settings.
    let end = parse_timestamp(right.split_whitespace().next()?)?;
    Some((start, end))
}

/// Parses `HH:MM:SS,mmm` (SRT), `HH:MM:SS.mmm` (VTT) or `MM:SS.mmm`
/// (VTT short form) into seconds.
fn parse_timestamp(s: &str) -> Option<f64> {
    let s = s.trim();
    let (clock, frac) = match s.rsplit_once([',', '.']) {
        Some((clock, millis)) => (clock, format!("0.{millis}").parse::<f64>().ok()?),
        None => (s, 0.0),
    };
    let parts: Vec<&str> = clock.split(':').collect();
    let (h, m, sec): (u64, u64, u64) = match parts.as_slice() {
        [h, m, s] => (h.parse().ok()?, m.parse().ok()?, s.parse().ok()?),
        [m, s] => (0, m.parse().ok()?, s.parse().ok()?),
        _ => return None,
    };
    Some(h as f64 * 3600.0 + m as f64 * 60.0 + sec as f64 + frac)
}

/// Removes `<i>`, `<b>`, `<font ...>` and any other angle-bracket tags.
fn strip_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// Spreads each cue's duration uniformly across its words (approximation:
/// every word in a cue gets `duration / word_count` seconds).
fn distribute_words(cues: &[Cue]) -> Vec<TimedWord> {
    let mut words = Vec::new();
    for cue in cues {
        let tokens: Vec<&str> = cue.text.split_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }
        let per_word = (cue.end - cue.start).max(0.0) / tokens.len() as f64;
        for (i, token) in tokens.iter().enumerate() {
            words.push(TimedWord {
                text: (*token).to_string(),
                start: round_ms(cue.start + i as f64 * per_word),
                end: round_ms(cue.start + (i + 1) as f64 * per_word),
            });
        }
    }
    words
}

fn round_ms(t: f64) -> f64 {
    (t * 1000.0).round() / 1000.0
}

/// Serializes the words to the `{"words": [...]}` JSON shape.
fn words_to_json(words: &[TimedWord]) -> String {
    serde_json::to_string_pretty(&WordsFile { words }).expect("words serialize to JSON")
}

// ---------------------------------------------------------------------------
// whisper.cpp subprocess
// ---------------------------------------------------------------------------

/// Actionable error when no whisper.cpp binary is found in PATH.
fn missing_binary_error() -> RustmotionError {
    RustmotionError::Generic(
        "No whisper.cpp binary found in PATH (tried `whisper-cli`, `whisper-cpp`, `main`).\n\
         Install it with: brew install whisper-cpp\n\
         Or build it from source: https://github.com/ggml-org/whisper.cpp\n\
         Alternatively, import existing subtitles with --from-srt / --from-vtt."
            .to_string(),
    )
}

/// Actionable error when the requested model cannot be resolved.
fn missing_model_error(name: &str, searched: &[PathBuf]) -> RustmotionError {
    let searched_list = searched
        .iter()
        .map(|p| format!("  - {}", p.display()))
        .collect::<Vec<_>>()
        .join("\n");
    RustmotionError::Generic(format!(
        "Whisper model '{name}' not found (looked for ggml-{name}.bin in):\n{searched_list}\n\
         Download it with:\n  \
         mkdir -p ~/.cache/whisper && curl -L -o ~/.cache/whisper/ggml-{name}.bin \\\n    \
         https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{name}.bin"
    ))
}

/// Searches PATH for an executable file with the given name.
fn find_in_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// Looks for a whisper.cpp binary in PATH (`whisper-cli`, `whisper-cpp`,
/// `main`), probing `--help` and requiring "whisper" in the help text (this
/// guards against an unrelated binary that happens to be called `main`).
fn detect_whisper_binary() -> Option<PathBuf> {
    ["whisper-cli", "whisper-cpp", "main"]
        .iter()
        .filter_map(|name| find_in_path(name))
        .find(|path| {
            Command::new(path)
                .arg("--help")
                .output()
                .map(|out| {
                    let help = format!(
                        "{}{}",
                        String::from_utf8_lossy(&out.stdout),
                        String::from_utf8_lossy(&out.stderr)
                    );
                    help.to_lowercase().contains("whisper")
                })
                .unwrap_or(false)
        })
}

/// Resolves `--model`: a direct `.bin` path, or a name searched as
/// `ggml-<name>.bin` in `~/.cache/whisper` and next to the binary.
fn resolve_model(model: &str, binary: &Path) -> Result<PathBuf> {
    if model.ends_with(".bin") || model.contains('/') {
        let as_path = Path::new(model);
        if as_path.is_file() {
            return Ok(as_path.to_path_buf());
        }
        return Err(RustmotionError::Generic(format!(
            "Whisper model file not found: {model}"
        )));
    }
    let file = format!("ggml-{model}.bin");
    let mut searched = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        searched.push(Path::new(&home).join(".cache").join("whisper").join(&file));
    }
    if let Some(dir) = binary.parent() {
        if !dir.as_os_str().is_empty() {
            searched.push(dir.join(&file));
        }
    }
    searched
        .iter()
        .find(|p| p.is_file())
        .cloned()
        .ok_or_else(|| missing_model_error(model, &searched))
}

/// whisper.cpp `-oj` output (documented format): segments under
/// `transcription`, with `offsets.{from,to}` in milliseconds.
#[derive(Deserialize)]
struct WhisperOutput {
    #[serde(default)]
    transcription: Vec<WhisperSegment>,
}

#[derive(Deserialize)]
struct WhisperSegment {
    offsets: WhisperOffsets,
    text: String,
}

#[derive(Deserialize)]
struct WhisperOffsets {
    from: u64,
    to: u64,
}

/// Parses whisper.cpp `-oj` JSON output (`transcription[].offsets.{from,to}`
/// in milliseconds, `.text`) into timed words. Punctuation-only segments
/// (produced by `-ml 1 -sow`) are glued to the previous word.
fn parse_whisper_json(json: &str) -> Result<Vec<TimedWord>> {
    let parsed: WhisperOutput = serde_json::from_str(json)?;
    let mut words: Vec<TimedWord> = Vec::new();
    for seg in parsed.transcription {
        let text = seg.text.trim().to_string();
        if text.is_empty() {
            continue;
        }
        let end = round_ms(seg.offsets.to as f64 / 1000.0);
        if !text.chars().any(|c| c.is_alphanumeric()) {
            // Punctuation-only segment: attach to the previous word
            // (drop it when there is no previous word).
            if let Some(prev) = words.last_mut() {
                prev.text.push_str(&text);
                prev.end = end;
            }
            continue;
        }
        words.push(TimedWord {
            text,
            start: round_ms(seg.offsets.from as f64 / 1000.0),
            end,
        });
    }
    Ok(words)
}

/// Runs the whisper.cpp binary on the audio file and returns timed words.
///
/// Flags used (whisper.cpp CLI):
/// - `-m <model>` / `-f <audio>`: model and input file
/// - `-ml 1 -sow`: max segment length 1 + split on word — whisper.cpp's
///   documented recipe for word-level timestamps (one segment per word)
/// - `-oj -of <base>`: write JSON output to `<base>.json`
/// - `-np`: suppress progress prints
/// - `-l <lang>`: optional language code
fn transcribe(
    audio: &Path,
    model: &str,
    lang: Option<&str>,
    quiet: bool,
) -> Result<Vec<TimedWord>> {
    if !audio.is_file() {
        return Err(RustmotionError::Generic(format!(
            "Audio file not found: {}",
            audio.display()
        )));
    }
    let binary = detect_whisper_binary().ok_or_else(missing_binary_error)?;
    let model_path = resolve_model(model, &binary)?;

    let out_base = std::env::temp_dir().join(format!("rustmotion-captions-{}", std::process::id()));
    let mut cmd = Command::new(&binary);
    cmd.arg("-m")
        .arg(&model_path)
        .arg("-f")
        .arg(audio)
        .arg("-ml")
        .arg("1")
        .arg("-sow")
        .arg("-oj")
        .arg("-of")
        .arg(&out_base)
        .arg("-np");
    if let Some(lang) = lang {
        cmd.arg("-l").arg(lang);
    }

    if !quiet {
        eprintln!(
            "Transcribing {} (binary: {}, model: {})...",
            audio.display(),
            binary.display(),
            model_path.display()
        );
    }
    let output = cmd.output().map_err(|e| {
        RustmotionError::Generic(format!("Failed to run {}: {e}", binary.display()))
    })?;
    let json_path = out_base.with_extension("json");
    if !output.status.success() {
        let _ = std::fs::remove_file(&json_path);
        return Err(RustmotionError::Generic(format!(
            "whisper.cpp failed (exit code {:?}):\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let json = std::fs::read_to_string(&json_path).map_err(|e| {
        RustmotionError::Generic(format!(
            "whisper.cpp did not produce {}: {e}",
            json_path.display()
        ))
    })?;
    let _ = std::fs::remove_file(&json_path);
    parse_whisper_json(&json)
}

// ---------------------------------------------------------------------------
// Command entry point
// ---------------------------------------------------------------------------

pub fn cmd_captions(
    audio: Option<&Path>,
    output: Option<&Path>,
    model: &str,
    lang: Option<&str>,
    from_srt: Option<&Path>,
    from_vtt: Option<&Path>,
    quiet: bool,
) -> Result<()> {
    let words = if let Some(srt) = from_srt {
        distribute_words(&parse_srt(&read_subtitle(srt)?)?)
    } else if let Some(vtt) = from_vtt {
        distribute_words(&parse_vtt(&read_subtitle(vtt)?)?)
    } else {
        let audio = audio.ok_or_else(|| {
            RustmotionError::Generic(
                "Provide an audio file to transcribe, or --from-srt / --from-vtt".to_string(),
            )
        })?;
        transcribe(audio, model, lang, quiet)?
    };
    if words.is_empty() {
        return Err(RustmotionError::Generic(
            "No words produced (empty transcription / subtitle file)".to_string(),
        ));
    }

    let json = words_to_json(&words);
    match output {
        Some(path) => {
            std::fs::write(path, format!("{json}\n"))?;
            if !quiet {
                eprintln!("Wrote {} word(s) to {}", words.len(), path.display());
            }
        }
        None => println!("{json}"),
    }
    Ok(())
}

fn read_subtitle(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|source| RustmotionError::FileRead {
        path: path.display().to_string(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- SRT parsing ---

    #[test]
    fn srt_parses_a_simple_cue() {
        let srt = "1\n00:00:01,500 --> 00:00:03,000\nHello world\n";
        let cues = parse_srt(srt).unwrap();
        assert_eq!(
            cues,
            vec![Cue {
                start: 1.5,
                end: 3.0,
                text: "Hello world".to_string()
            }]
        );
    }

    #[test]
    fn srt_joins_multiline_text_with_spaces() {
        let srt = "1\n00:00:00,000 --> 00:00:02,000\nfirst line\nsecond line\n\n2\n00:00:02,000 --> 00:00:04,000\nnext cue\n";
        let cues = parse_srt(srt).unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "first line second line");
        assert_eq!(cues[1].text, "next cue");
        assert_eq!(cues[1].start, 2.0);
        assert_eq!(cues[1].end, 4.0);
    }

    #[test]
    fn srt_strips_formatting_tags() {
        let srt = "1\n00:00:00,000 --> 00:00:01,000\n<i>Hello</i> <b>bold</b> world\n";
        let cues = parse_srt(srt).unwrap();
        assert_eq!(cues[0].text, "Hello bold world");
    }

    #[test]
    fn srt_handles_leading_bom() {
        let srt = "\u{feff}1\n00:00:00,500 --> 00:00:01,000\nhi\n";
        let cues = parse_srt(srt).unwrap();
        assert_eq!(cues[0].start, 0.5);
        assert_eq!(cues[0].text, "hi");
    }

    #[test]
    fn srt_without_cues_is_an_error() {
        let err = parse_srt("just some text without any timing\n").unwrap_err();
        assert!(err.to_string().contains("no cues"), "got: {err}");
    }

    // --- VTT parsing ---

    #[test]
    fn vtt_parses_header_and_dot_timestamps() {
        let vtt = "WEBVTT\n\n00:00:01.500 --> 00:00:03.000\nHello world\n";
        let cues = parse_vtt(vtt).unwrap();
        assert_eq!(
            cues,
            vec![Cue {
                start: 1.5,
                end: 3.0,
                text: "Hello world".to_string()
            }]
        );
    }

    #[test]
    fn vtt_accepts_short_timestamps_cue_ids_and_settings() {
        // MM:SS.mmm form, an optional cue identifier line, and cue settings
        // after the end timestamp must all be handled.
        let vtt =
            "WEBVTT - title\n\nintro\n01:02.000 --> 01:04.500 position:10%,line-left\nshort form\n";
        let cues = parse_vtt(vtt).unwrap();
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].start, 62.0);
        assert_eq!(cues[0].end, 64.5);
        assert_eq!(cues[0].text, "short form");
    }

    #[test]
    fn vtt_skips_note_blocks() {
        let vtt = "WEBVTT\n\nNOTE\nthis is a comment\n\n00:00:00.000 --> 00:00:01.000\nreal cue\n";
        let cues = parse_vtt(vtt).unwrap();
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "real cue");
    }

    // --- Uniform word distribution ---

    #[test]
    fn distributes_cue_duration_uniformly_across_words() {
        // 3 words over 1.5s → 0.5s each, at the right offsets.
        let cues = vec![Cue {
            start: 1.0,
            end: 2.5,
            text: "one two three".to_string(),
        }];
        let words = distribute_words(&cues);
        assert_eq!(
            words,
            vec![
                TimedWord {
                    text: "one".to_string(),
                    start: 1.0,
                    end: 1.5
                },
                TimedWord {
                    text: "two".to_string(),
                    start: 1.5,
                    end: 2.0
                },
                TimedWord {
                    text: "three".to_string(),
                    start: 2.0,
                    end: 2.5
                },
            ]
        );
    }

    // --- Output JSON shape ---

    #[test]
    fn output_json_has_words_array_shape() {
        let words = vec![TimedWord {
            text: "hi".to_string(),
            start: 0.25,
            end: 0.75,
        }];
        let json = words_to_json(&words);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["words"][0]["text"], "hi");
        assert_eq!(value["words"][0]["start"], 0.25);
        assert_eq!(value["words"][0]["end"], 0.75);
        assert_eq!(value["words"].as_array().unwrap().len(), 1);
    }

    // --- whisper.cpp integration ---

    #[test]
    fn missing_binary_error_contains_install_hint() {
        let msg = missing_binary_error().to_string();
        assert!(
            msg.contains("brew install whisper-cpp"),
            "no install hint in: {msg}"
        );
        assert!(
            msg.contains("whisper.cpp"),
            "no project reference in: {msg}"
        );
    }

    #[test]
    fn missing_model_error_contains_download_hint() {
        let msg = missing_model_error("base", &[PathBuf::from("/tmp/ggml-base.bin")]).to_string();
        assert!(msg.contains("ggml-base.bin"), "no model name in: {msg}");
        assert!(
            msg.contains("huggingface.co/ggerganov/whisper.cpp"),
            "no download hint in: {msg}"
        );
    }

    #[test]
    fn parses_whisper_cpp_json_output() {
        // Documented whisper.cpp -oj format: transcription[].offsets in ms.
        let json = r#"{
            "systeminfo": "x",
            "result": { "language": "en" },
            "transcription": [
                { "timestamps": { "from": "00:00:00,000", "to": "00:00:00,320" },
                  "offsets": { "from": 0, "to": 320 },
                  "text": " Hello" },
                { "timestamps": { "from": "00:00:00,320", "to": "00:00:00,700" },
                  "offsets": { "from": 320, "to": 700 },
                  "text": " world" }
            ]
        }"#;
        let words = parse_whisper_json(json).unwrap();
        assert_eq!(
            words,
            vec![
                TimedWord {
                    text: "Hello".to_string(),
                    start: 0.0,
                    end: 0.32
                },
                TimedWord {
                    text: "world".to_string(),
                    start: 0.32,
                    end: 0.7
                },
            ]
        );
    }

    #[test]
    fn whisper_json_appends_punctuation_only_segments_to_previous_word() {
        // With -ml 1 -sow, punctuation can come out as its own segment; it
        // must be glued to the previous word instead of becoming a "word".
        let json = r#"{
            "transcription": [
                { "offsets": { "from": 0, "to": 300 }, "text": " Hi" },
                { "offsets": { "from": 300, "to": 350 }, "text": "." }
            ]
        }"#;
        let words = parse_whisper_json(json).unwrap();
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].text, "Hi.");
        assert_eq!(words[0].end, 0.35);
    }

    /// Real transcription path — only runs when a whisper.cpp binary is
    /// installed; skips silently otherwise (no binary on CI).
    #[test]
    fn detects_whisper_binary_when_installed() {
        match detect_whisper_binary() {
            Some(path) => {
                assert!(
                    Command::new(&path).arg("--help").output().is_ok(),
                    "detected binary is not runnable: {}",
                    path.display()
                );
            }
            None => eprintln!("skipping: no whisper.cpp binary in PATH"),
        }
    }
}
