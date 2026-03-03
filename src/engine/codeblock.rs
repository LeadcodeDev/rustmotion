use anyhow::Result;
use similar::{ChangeTag, TextDiff};
use skia_safe::{Canvas, Font, FontMgr, FontStyle, Paint, PaintStyle, Rect, TextBlob};
use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{
    Color as SynColor, FontStyle as SynFontStyle, ScopeSelectors, StyleModifier, Theme,
    ThemeItem, ThemeSet, ThemeSettings,
};
use syntect::parsing::SyntaxSet;

use super::renderer::paint_from_hex;
use crate::engine::animator::{ease, AnimatedProperties};
use crate::schema::{
    CodeblockHighlight, CodeblockLayer, CodeblockPadding, EasingType, RevealMode, VideoConfig,
};

// ─── Syntect caches ──────────────────────────────────────────────────────────

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn load_theme_from_str(xml: &str) -> Option<Theme> {
    let mut cursor = std::io::Cursor::new(xml.as_bytes());
    ThemeSet::load_from_reader(&mut cursor).ok()
}

/// Parse a hex color string (#RGB, #RGBA, #RRGGBB, #RRGGBBAA) into a syntect Color
fn parse_syn_color(hex: &str) -> Option<SynColor> {
    let hex = hex.trim_start_matches('#');
    let (r, g, b, a) = match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
            (r, g, b, 255u8)
        }
        4 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
            let a = u8::from_str_radix(&hex[3..4], 16).ok()? * 17;
            (r, g, b, a)
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            (r, g, b, 255u8)
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            (r, g, b, a)
        }
        _ => return None,
    };
    Some(SynColor { r, g, b, a })
}

/// Parse a VS Code fontStyle string ("italic", "bold", "italic bold", "underline") into syntect FontStyle
fn parse_font_style(s: &str) -> Option<SynFontStyle> {
    let s = s.trim();
    if s.is_empty() || s == "normal" {
        return Some(SynFontStyle::empty());
    }
    let mut style = SynFontStyle::empty();
    for part in s.split_whitespace() {
        match part {
            "italic" => style |= SynFontStyle::ITALIC,
            "bold" => style |= SynFontStyle::BOLD,
            "underline" => style |= SynFontStyle::UNDERLINE,
            _ => {}
        }
    }
    Some(style)
}

/// Load a VS Code JSON theme and convert it to a syntect Theme
fn load_vscode_theme(json: &str) -> Option<Theme> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;

    let name = v.get("name").and_then(|n| n.as_str()).map(|s| s.to_string());

    // Parse ThemeSettings from "colors" object
    let mut settings = ThemeSettings::default();
    if let Some(colors) = v.get("colors").and_then(|c| c.as_object()) {
        settings.foreground = colors.get("editor.foreground").and_then(|v| v.as_str()).and_then(parse_syn_color);
        settings.background = colors.get("editor.background").and_then(|v| v.as_str()).and_then(parse_syn_color);
        settings.caret = colors.get("editorCursor.foreground").and_then(|v| v.as_str()).and_then(parse_syn_color);
        settings.line_highlight = colors.get("editor.lineHighlightBackground").and_then(|v| v.as_str()).and_then(parse_syn_color);
        settings.selection = colors.get("editor.selectionBackground").and_then(|v| v.as_str()).and_then(parse_syn_color);
        settings.selection_foreground = colors.get("editor.selectionForeground").and_then(|v| v.as_str()).and_then(parse_syn_color);
        settings.gutter = colors.get("editorGutter.background").and_then(|v| v.as_str()).and_then(parse_syn_color);
        settings.gutter_foreground = colors.get("editorLineNumber.foreground").and_then(|v| v.as_str()).and_then(parse_syn_color);
        settings.find_highlight = colors.get("editor.findMatchHighlightBackground").and_then(|v| v.as_str()).and_then(parse_syn_color);
    }

    // Parse scopes from "tokenColors" array
    let mut scopes = Vec::new();
    if let Some(token_colors) = v.get("tokenColors").and_then(|t| t.as_array()) {
        for tc in token_colors {
            let scope_str = match tc.get("scope") {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(serde_json::Value::Array(arr)) => arr
                    .iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                None => {
                    // Global settings entry (no scope) — apply to foreground/background
                    if let Some(s) = tc.get("settings").and_then(|s| s.as_object()) {
                        if settings.foreground.is_none() {
                            settings.foreground = s.get("foreground").and_then(|v| v.as_str()).and_then(parse_syn_color);
                        }
                        if settings.background.is_none() {
                            settings.background = s.get("background").and_then(|v| v.as_str()).and_then(parse_syn_color);
                        }
                    }
                    continue;
                }
                _ => continue,
            };

            let scope = match scope_str.parse::<ScopeSelectors>() {
                Ok(s) => s,
                Err(_) => continue,
            };

            let tc_settings = match tc.get("settings").and_then(|s| s.as_object()) {
                Some(s) => s,
                None => continue,
            };

            let style = StyleModifier {
                foreground: tc_settings.get("foreground").and_then(|v| v.as_str()).and_then(parse_syn_color),
                background: tc_settings.get("background").and_then(|v| v.as_str()).and_then(parse_syn_color),
                font_style: tc_settings.get("fontStyle").and_then(|v| v.as_str()).and_then(parse_font_style),
            };

            scopes.push(ThemeItem { scope, style });
        }
    }

    Some(Theme { name, author: None, settings, scopes })
}

fn theme_set() -> &'static ThemeSet {
    THEME_SET.get_or_init(|| {
        let mut ts = ThemeSet::load_defaults();

        // Catppuccin themes (tmTheme format)
        let catppuccin_themes: &[(&str, &str)] = &[
            ("catppuccin-latte", include_str!("../../themes/Catppuccin Latte.tmTheme")),
            ("catppuccin-frappe", include_str!("../../themes/Catppuccin Frappe.tmTheme")),
            ("catppuccin-macchiato", include_str!("../../themes/Catppuccin Macchiato.tmTheme")),
            ("catppuccin-mocha", include_str!("../../themes/Catppuccin Mocha.tmTheme")),
        ];
        for (name, xml) in catppuccin_themes {
            if let Some(theme) = load_theme_from_str(xml) {
                ts.themes.insert(name.to_string(), theme);
            }
        }

        // VS Code / Shiki themes (JSON format)
        let vscode_themes: &[(&str, &str)] = &[
            ("andromeeda", include_str!("../../themes/vscode/andromeeda.json")),
            ("aurora-x", include_str!("../../themes/vscode/aurora-x.json")),
            ("ayu-dark", include_str!("../../themes/vscode/ayu-dark.json")),
            ("ayu-light", include_str!("../../themes/vscode/ayu-light.json")),
            ("ayu-mirage", include_str!("../../themes/vscode/ayu-mirage.json")),
            ("dark-plus", include_str!("../../themes/vscode/dark-plus.json")),
            ("dracula", include_str!("../../themes/vscode/dracula.json")),
            ("dracula-soft", include_str!("../../themes/vscode/dracula-soft.json")),
            ("everforest-dark", include_str!("../../themes/vscode/everforest-dark.json")),
            ("everforest-light", include_str!("../../themes/vscode/everforest-light.json")),
            ("github-dark", include_str!("../../themes/vscode/github-dark.json")),
            ("github-dark-default", include_str!("../../themes/vscode/github-dark-default.json")),
            ("github-dark-dimmed", include_str!("../../themes/vscode/github-dark-dimmed.json")),
            ("github-dark-high-contrast", include_str!("../../themes/vscode/github-dark-high-contrast.json")),
            ("github-light", include_str!("../../themes/vscode/github-light.json")),
            ("github-light-default", include_str!("../../themes/vscode/github-light-default.json")),
            ("github-light-high-contrast", include_str!("../../themes/vscode/github-light-high-contrast.json")),
            ("gruvbox-dark-hard", include_str!("../../themes/vscode/gruvbox-dark-hard.json")),
            ("gruvbox-dark-medium", include_str!("../../themes/vscode/gruvbox-dark-medium.json")),
            ("gruvbox-dark-soft", include_str!("../../themes/vscode/gruvbox-dark-soft.json")),
            ("gruvbox-light-hard", include_str!("../../themes/vscode/gruvbox-light-hard.json")),
            ("gruvbox-light-medium", include_str!("../../themes/vscode/gruvbox-light-medium.json")),
            ("gruvbox-light-soft", include_str!("../../themes/vscode/gruvbox-light-soft.json")),
            ("horizon", include_str!("../../themes/vscode/horizon.json")),
            ("horizon-bright", include_str!("../../themes/vscode/horizon-bright.json")),
            ("houston", include_str!("../../themes/vscode/houston.json")),
            ("kanagawa-dragon", include_str!("../../themes/vscode/kanagawa-dragon.json")),
            ("kanagawa-lotus", include_str!("../../themes/vscode/kanagawa-lotus.json")),
            ("kanagawa-wave", include_str!("../../themes/vscode/kanagawa-wave.json")),
            ("laserwave", include_str!("../../themes/vscode/laserwave.json")),
            ("light-plus", include_str!("../../themes/vscode/light-plus.json")),
            ("material-theme", include_str!("../../themes/vscode/material-theme.json")),
            ("material-theme-darker", include_str!("../../themes/vscode/material-theme-darker.json")),
            ("material-theme-lighter", include_str!("../../themes/vscode/material-theme-lighter.json")),
            ("material-theme-ocean", include_str!("../../themes/vscode/material-theme-ocean.json")),
            ("material-theme-palenight", include_str!("../../themes/vscode/material-theme-palenight.json")),
            ("min-dark", include_str!("../../themes/vscode/min-dark.json")),
            ("min-light", include_str!("../../themes/vscode/min-light.json")),
            ("monokai", include_str!("../../themes/vscode/monokai.json")),
            ("night-owl", include_str!("../../themes/vscode/night-owl.json")),
            ("night-owl-light", include_str!("../../themes/vscode/night-owl-light.json")),
            ("nord", include_str!("../../themes/vscode/nord.json")),
            ("one-dark-pro", include_str!("../../themes/vscode/one-dark-pro.json")),
            ("one-light", include_str!("../../themes/vscode/one-light.json")),
            ("plastic", include_str!("../../themes/vscode/plastic.json")),
            ("poimandres", include_str!("../../themes/vscode/poimandres.json")),
            ("red", include_str!("../../themes/vscode/red.json")),
            ("rose-pine", include_str!("../../themes/vscode/rose-pine.json")),
            ("rose-pine-dawn", include_str!("../../themes/vscode/rose-pine-dawn.json")),
            ("rose-pine-moon", include_str!("../../themes/vscode/rose-pine-moon.json")),
            ("slack-dark", include_str!("../../themes/vscode/slack-dark.json")),
            ("slack-ochin", include_str!("../../themes/vscode/slack-ochin.json")),
            ("snazzy-light", include_str!("../../themes/vscode/snazzy-light.json")),
            ("solarized-dark", include_str!("../../themes/vscode/solarized-dark.json")),
            ("solarized-light", include_str!("../../themes/vscode/solarized-light.json")),
            ("synthwave-84", include_str!("../../themes/vscode/synthwave-84.json")),
            ("tokyo-night", include_str!("../../themes/vscode/tokyo-night.json")),
            ("vesper", include_str!("../../themes/vscode/vesper.json")),
            ("vitesse-black", include_str!("../../themes/vscode/vitesse-black.json")),
            ("vitesse-dark", include_str!("../../themes/vscode/vitesse-dark.json")),
            ("vitesse-light", include_str!("../../themes/vscode/vitesse-light.json")),
        ];
        for (name, json) in vscode_themes {
            if let Some(theme) = load_vscode_theme(json) {
                ts.themes.insert(name.to_string(), theme);
            }
        }

        ts
    })
}

// ─── Types ───────────────────────────────────────────────────────────────────

struct ColoredSpan {
    text: String,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

struct HighlightedLine {
    spans: Vec<ColoredSpan>,
}

#[derive(Debug)]
enum LineDiffOp {
    Equal {
        #[allow(dead_code)]
        line: String,
        old_idx: usize,
        new_idx: usize,
    },
    Delete {
        #[allow(dead_code)]
        line: String,
        old_idx: usize,
    },
    Insert {
        #[allow(dead_code)]
        line: String,
        new_idx: usize,
    },
    Replace {
        old_line: String,
        new_line: String,
        old_idx: usize,
        new_idx: usize,
    },
}

#[derive(Debug)]
struct FragmentEdit {
    col: usize,
    delete: String,
    insert: String,
}

/// Computed dimensions for a code block
#[allow(dead_code)]
struct CodeDimensions {
    line_count: usize,
    max_line_width: f32,
    gutter_width: f32,
    total_width: f32,
    total_height: f32,
}

// ─── Main render entry point ─────────────────────────────────────────────────

pub fn render_codeblock(
    canvas: &Canvas,
    layer: &CodeblockLayer,
    _config: &VideoConfig,
    time: f64,
    _props: &AnimatedProperties,
) -> Result<()> {
    let font = resolve_monospace_font(&layer.font_family, layer.font_size, Some(layer.font_weight));
    let actual_line_height = layer.font_size * layer.line_height;
    let padding = layer.padding.as_ref().cloned().unwrap_or_default();
    let theme = get_theme(&layer.theme);

    let (current_code, transition) = determine_active_state(layer, time);

    let chrome_enabled = layer.chrome.as_ref().map_or(false, |c| c.enabled);
    let chrome_height = if chrome_enabled { 36.0 } else { 0.0 };

    // Compute dimensions — interpolate during transitions
    let (total_width, total_height, gutter_width) = if let Some(ref trans) = transition {
        let dims_a = compute_code_dimensions(&trans.code_a, &font, &padding, chrome_height, layer);
        let dims_b = compute_code_dimensions(&trans.code_b, &font, &padding, chrome_height, layer);
        let p = trans.progress as f32;
        let gutter = f32::max(dims_a.gutter_width, dims_b.gutter_width);
        match &layer.size {
            Some(s) => (s.width, s.height, gutter),
            None => (
                lerp(dims_a.total_width, dims_b.total_width, p),
                lerp(dims_a.total_height, dims_b.total_height, p),
                gutter,
            ),
        }
    } else {
        let dims = compute_code_dimensions(&current_code, &font, &padding, chrome_height, layer);
        match &layer.size {
            Some(s) => (s.width, s.height, dims.gutter_width),
            None => (dims.total_width, dims.total_height, dims.gutter_width),
        }
    };

    let x = layer.position.x;
    let y = layer.position.y;

    // Background
    let bg_color = layer.background.as_deref().unwrap_or("#2b303b");
    let bg_paint = paint_from_hex(bg_color);
    let bg_rect = Rect::from_xywh(x, y, total_width, total_height);
    let rrect = skia_safe::RRect::new_rect_xy(bg_rect, layer.corner_radius, layer.corner_radius);
    canvas.draw_rrect(rrect, &bg_paint);

    // Chrome (title bar)
    if chrome_enabled {
        draw_chrome(canvas, layer, x, y, total_width, layer.corner_radius);
    }

    // Code area
    let code_x = x + padding.left + gutter_width;
    let code_y = y + chrome_height + padding.top;

    // Clip to content area
    canvas.save();
    let clip_rect = Rect::from_xywh(x, y + chrome_height, total_width, total_height - chrome_height);
    canvas.clip_rect(clip_rect, skia_safe::ClipOp::Intersect, true);

    if let Some(ref trans) = transition {
        render_diff_transition(
            canvas, layer, &font, theme, code_x, code_y, actual_line_height,
            gutter_width, &padding, x, trans,
        )?;
    } else {
        let highlighted = highlight_code(&current_code, &layer.language, theme);
        let (visible_lines, visible_chars, last_line_opacity) =
            compute_reveal(layer, time, &highlighted);

        if layer.show_line_numbers {
            draw_line_numbers(canvas, &font, x + padding.left, code_y, actual_line_height, visible_lines);
        }

        draw_highlights(
            canvas, &layer.highlights, time, x + padding.left, code_y,
            actual_line_height, total_width - padding.left - padding.right,
        );

        draw_highlighted_lines(
            canvas, &highlighted, &font, code_x, code_y, actual_line_height,
            visible_lines, visible_chars, last_line_opacity,
        );
    }

    canvas.restore();
    Ok(())
}

// ─── Dimension computation ───────────────────────────────────────────────────

fn compute_code_dimensions(
    code: &str,
    font: &Font,
    padding: &CodeblockPadding,
    chrome_height: f32,
    layer: &CodeblockLayer,
) -> CodeDimensions {
    let actual_line_height = layer.font_size * layer.line_height;
    let lines: Vec<&str> = code.lines().collect();
    let line_count = lines.len().max(1);

    let gutter_width = if layer.show_line_numbers {
        let digits = format!("{}", line_count).len();
        let digit_width = font.measure_str("0", None).0;
        (digits as f32 * digit_width) + 24.0
    } else {
        0.0
    };

    let max_line_width = lines
        .iter()
        .map(|l| font.measure_str(l, None).0)
        .fold(0.0f32, f32::max);

    let content_width = max_line_width + gutter_width + padding.left + padding.right;
    let content_height = line_count as f32 * actual_line_height + padding.top + padding.bottom;

    CodeDimensions {
        line_count,
        max_line_width,
        gutter_width,
        total_width: content_width,
        total_height: content_height + chrome_height,
    }
}

// ─── State management ────────────────────────────────────────────────────────

struct TransitionInfo {
    code_a: String,
    code_b: String,
    progress: f64,
    #[allow(dead_code)]
    easing: EasingType,
    cursor_config: Option<crate::schema::CodeblockCursor>,
}

fn determine_active_state(layer: &CodeblockLayer, time: f64) -> (String, Option<TransitionInfo>) {
    if layer.states.is_empty() {
        return (layer.code.clone(), None);
    }

    let mut current_code = layer.code.clone();

    for state in &layer.states {
        let end = state.at + state.duration;
        if time < state.at {
            return (current_code, None);
        } else if time < end {
            let raw_progress = (time - state.at) / state.duration;
            let progress = ease(raw_progress, &state.easing);
            return (
                current_code.clone(),
                Some(TransitionInfo {
                    code_a: current_code,
                    code_b: state.code.clone(),
                    progress,
                    easing: state.easing.clone(),
                    cursor_config: state.cursor.clone(),
                }),
            );
        } else {
            current_code = state.code.clone();
        }
    }

    (current_code, None)
}

// ─── Syntax highlighting ─────────────────────────────────────────────────────

fn get_theme(name: &str) -> &'static Theme {
    let ts = theme_set();
    ts.themes.get(name).unwrap_or_else(|| ts.themes.values().next().unwrap())
}

fn highlight_code(code: &str, language: &str, theme: &Theme) -> Vec<HighlightedLine> {
    let ss = syntax_set();
    let syntax = ss
        .find_syntax_by_token(language)
        .or_else(|| ss.find_syntax_by_name(language))
        .unwrap_or_else(|| ss.find_syntax_plain_text());

    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut result = Vec::new();

    for line in syntect::util::LinesWithEndings::from(code) {
        let ranges = highlighter.highlight_line(line, ss).unwrap_or_default();
        let spans: Vec<ColoredSpan> = ranges
            .into_iter()
            .map(|(style, text)| ColoredSpan {
                text: text.trim_end_matches('\n').to_string(),
                r: style.foreground.r,
                g: style.foreground.g,
                b: style.foreground.b,
                a: style.foreground.a,
            })
            .collect();
        result.push(HighlightedLine { spans });
    }

    result
}

// ─── Chrome (title bar) ──────────────────────────────────────────────────────

fn draw_chrome(canvas: &Canvas, layer: &CodeblockLayer, x: f32, y: f32, width: f32, corner_radius: f32) {
    let chrome = layer.chrome.as_ref().unwrap();
    let chrome_height = 36.0;

    let bar_color = chrome.color.as_deref().unwrap_or("#343d46");
    let bar_paint = paint_from_hex(bar_color);
    let bar_rect = Rect::from_xywh(x, y, width, chrome_height);
    let radii = [
        skia_safe::Point::new(corner_radius, corner_radius),
        skia_safe::Point::new(corner_radius, corner_radius),
        skia_safe::Point::new(0.0, 0.0),
        skia_safe::Point::new(0.0, 0.0),
    ];
    let rrect = skia_safe::RRect::new_rect_radii(bar_rect, &radii);
    canvas.draw_rrect(rrect, &bar_paint);

    let dot_y = y + chrome_height / 2.0;
    let dot_radius = 6.0;
    let dot_start_x = x + 16.0;
    let dot_spacing = 20.0;
    let dot_colors = ["#FF5F56", "#FFBD2E", "#27C93F"];
    for (i, color) in dot_colors.iter().enumerate() {
        let dot_x = dot_start_x + i as f32 * dot_spacing;
        canvas.draw_circle((dot_x, dot_y), dot_radius, &paint_from_hex(color));
    }

    if let Some(ref title) = chrome.title {
        let font_mgr = FontMgr::default();
        let typeface = font_mgr
            .match_family_style("Inter", FontStyle::normal())
            .or_else(|| font_mgr.match_family_style("Helvetica", FontStyle::normal()))
            .or_else(|| font_mgr.match_family_style("Arial", FontStyle::normal()))
            .unwrap_or_else(|| font_mgr.match_family_style("sans-serif", FontStyle::normal()).unwrap());
        let title_font = Font::from_typeface(typeface, 13.0);
        let (title_width, _) = title_font.measure_str(title, None);
        let title_x = x + width / 2.0 - title_width / 2.0;
        let title_y = dot_y + 4.0;
        let mut title_paint = paint_from_hex("#999999");
        title_paint.set_anti_alias(true);
        if let Some(blob) = TextBlob::new(title, &title_font) {
            canvas.draw_text_blob(&blob, (title_x, title_y), &title_paint);
        }
    }
}

// ─── Line numbers ────────────────────────────────────────────────────────────

fn draw_line_numbers(canvas: &Canvas, font: &Font, x: f32, y: f32, line_height: f32, visible_lines: usize) {
    let mut paint = paint_from_hex("#65737E");
    paint.set_anti_alias(true);
    let (_sw, metrics) = font.metrics();
    let ascent = -metrics.ascent;

    for i in 0..visible_lines {
        let num_str = format!("{}", i + 1);
        let num_y = y + i as f32 * line_height + ascent;
        if let Some(blob) = TextBlob::new(&num_str, font) {
            canvas.draw_text_blob(&blob, (x + 12.0, num_y), &paint);
        }
    }
}

/// Draw a single line number at an arbitrary Y with given opacity
fn draw_line_number_at(canvas: &Canvas, font: &Font, x: f32, y: f32, num: usize, opacity: f32) {
    let num_str = format!("{}", num);
    let mut paint = paint_from_hex("#65737E");
    paint.set_anti_alias(true);
    paint.set_alpha_f(opacity);
    if let Some(blob) = TextBlob::new(&num_str, font) {
        canvas.draw_text_blob(&blob, (x + 12.0, y), &paint);
    }
}

// ─── Highlights ──────────────────────────────────────────────────────────────

fn draw_highlights(
    canvas: &Canvas, highlights: &[CodeblockHighlight], time: f64,
    x: f32, y: f32, line_height: f32, width: f32,
) {
    for hl in highlights {
        if let Some(start) = hl.start { if time < start { continue; } }
        if let Some(end) = hl.end { if time > end { continue; } }
        let hl_paint = paint_from_hex(&hl.color);
        for &line_num in &hl.lines {
            if line_num == 0 { continue; }
            let line_idx = line_num - 1;
            let hl_rect = Rect::from_xywh(x, y + line_idx as f32 * line_height, width, line_height);
            canvas.draw_rect(hl_rect, &hl_paint);
        }
    }
}

// ─── Reveal ──────────────────────────────────────────────────────────────────

fn compute_reveal(
    layer: &CodeblockLayer, time: f64, highlighted: &[HighlightedLine],
) -> (usize, Option<usize>, f32) {
    let total_lines = highlighted.len();
    if total_lines == 0 { return (0, None, 1.0); }

    match &layer.reveal {
        None => (total_lines, None, 1.0),
        Some(reveal) => {
            if time < reveal.start { return (0, None, 1.0); }
            let raw_progress = ((time - reveal.start) / reveal.duration).clamp(0.0, 1.0);
            let progress = ease(raw_progress, &reveal.easing);

            match reveal.mode {
                RevealMode::Typewriter => {
                    let total_chars: usize = highlighted.iter()
                        .map(|l| l.spans.iter().map(|s| s.text.len()).sum::<usize>())
                        .sum();
                    let visible_chars = (total_chars as f64 * progress).round() as usize;
                    let mut chars_remaining = visible_chars;
                    let mut visible_lines = 0;
                    let mut last_line_chars = None;
                    for line in highlighted {
                        let line_chars: usize = line.spans.iter().map(|s| s.text.len()).sum();
                        if chars_remaining >= line_chars {
                            chars_remaining -= line_chars;
                            visible_lines += 1;
                        } else {
                            visible_lines += 1;
                            last_line_chars = Some(chars_remaining);
                            break;
                        }
                    }
                    (visible_lines, last_line_chars, 1.0)
                }
                RevealMode::LineByLine => {
                    let visible_f = total_lines as f64 * progress;
                    let full_lines = visible_f.floor() as usize;
                    let fractional = (visible_f - full_lines as f64) as f32;
                    if full_lines >= total_lines {
                        (total_lines, None, 1.0)
                    } else {
                        (full_lines + 1, None, fractional.max(0.01))
                    }
                }
            }
        }
    }
}

// ─── Draw highlighted lines ──────────────────────────────────────────────────

fn draw_highlighted_lines(
    canvas: &Canvas, highlighted: &[HighlightedLine], font: &Font,
    x: f32, y: f32, line_height: f32, visible_lines: usize,
    visible_chars_last_line: Option<usize>, last_line_opacity: f32,
) {
    let (_sw, metrics) = font.metrics();
    let ascent = -metrics.ascent;

    for (i, line) in highlighted.iter().enumerate() {
        if i >= visible_lines { break; }
        let is_last_visible = i == visible_lines - 1;
        let line_y = y + i as f32 * line_height + ascent;
        let char_limit = if is_last_visible { visible_chars_last_line } else { None };
        let opacity = if is_last_visible && last_line_opacity < 1.0 { last_line_opacity } else { 1.0 };
        draw_single_highlighted_line_partial(canvas, line, font, x, line_y, opacity, char_limit);
    }
}

fn draw_single_highlighted_line_partial(
    canvas: &Canvas, line: &HighlightedLine, font: &Font,
    x: f32, y: f32, opacity: f32, char_limit: Option<usize>,
) {
    let mut cursor_x = x;
    let mut chars_drawn = 0usize;

    for span in &line.spans {
        let text_to_draw = if let Some(limit) = char_limit {
            let remaining = limit.saturating_sub(chars_drawn);
            if remaining == 0 { break; }
            let chars: Vec<char> = span.text.chars().collect();
            let take = remaining.min(chars.len());
            chars[..take].iter().collect::<String>()
        } else {
            span.text.clone()
        };

        if text_to_draw.is_empty() {
            chars_drawn += span.text.len();
            continue;
        }

        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_color4f(
            skia_safe::Color4f::new(
                span.r as f32 / 255.0, span.g as f32 / 255.0,
                span.b as f32 / 255.0, (span.a as f32 / 255.0) * opacity,
            ),
            None,
        );

        if let Some(blob) = TextBlob::new(&text_to_draw, font) {
            canvas.draw_text_blob(&blob, (cursor_x, y), &paint);
        }
        let (w, _) = font.measure_str(&text_to_draw, None);
        cursor_x += w;
        chars_drawn += text_to_draw.len();

        if let Some(limit) = char_limit {
            if chars_drawn >= limit { break; }
        }
    }
}

// ─── Diff transitions ────────────────────────────────────────────────────────

/// Describes how to render a single line during a diff transition.
struct AnimatedLinePlacement {
    /// Interpolated Y position (in line-index space, multiply by line_height + add code_y later)
    old_y_idx: f32,
    new_y_idx: f32,
    /// Opacity at start (progress=0) and end (progress=1)
    opacity_start: f32,
    opacity_end: f32,
    /// Line numbers for old and new state (1-indexed)
    old_line_number: usize,
    new_line_number: usize,
    /// What kind of content to render
    content: AnimatedLineContent,
}

enum AnimatedLineContent {
    /// Render from highlighted_b at this index
    FromB { idx: usize },
    /// Render from highlighted_a at this index (for delete)
    FromA { idx: usize },
    /// Cursor-edited line (replace)
    CursorEdit {
        old_line: String,
        new_line: String,
        old_idx: usize,
        new_idx: usize,
    },
}

fn render_diff_transition(
    canvas: &Canvas,
    layer: &CodeblockLayer,
    font: &Font,
    theme: &Theme,
    code_x: f32,
    code_y: f32,
    line_height: f32,
    _gutter_width: f32,
    padding: &CodeblockPadding,
    block_x: f32,
    trans: &TransitionInfo,
) -> Result<()> {
    let progress = trans.progress as f32;
    let (_sw, metrics) = font.metrics();
    let ascent = -metrics.ascent;

    let diff_ops = compute_line_diff(&trans.code_a, &trans.code_b);
    let highlighted_a = highlight_code(&trans.code_a, &layer.language, theme);
    let highlighted_b = highlight_code(&trans.code_b, &layer.language, theme);

    let cursor_enabled = trans.cursor_config.as_ref().map_or(true, |c| c.enabled);
    let cursor_color = trans.cursor_config.as_ref().map_or("#FFFFFF", |c| c.color.as_str());
    let cursor_width = trans.cursor_config.as_ref().map_or(2.0, |c| c.width);
    let cursor_blink = trans.cursor_config.as_ref().map_or(true, |c| c.blink);

    // Build animated line placements with proper interpolated positions.
    // Track "virtual cursors" for old and new index space so that
    // Insert/Delete lines get smooth starting/ending positions.
    let mut placements: Vec<AnimatedLinePlacement> = Vec::new();
    let mut _old_cursor: f32 = 0.0;
    let mut _new_cursor: f32 = 0.0;

    for op in &diff_ops {
        match op {
            LineDiffOp::Equal { old_idx, new_idx, .. } => {
                placements.push(AnimatedLinePlacement {
                    old_y_idx: *old_idx as f32,
                    new_y_idx: *new_idx as f32,
                    opacity_start: 1.0,
                    opacity_end: 1.0,
                    old_line_number: old_idx + 1,
                    new_line_number: new_idx + 1,
                    content: AnimatedLineContent::FromB { idx: *new_idx },
                });
                _old_cursor = *old_idx as f32 + 1.0;
                _new_cursor = *new_idx as f32 + 1.0;
            }
            LineDiffOp::Delete { old_idx, .. } => {
                // Line fades out at its old position (no Y movement)
                placements.push(AnimatedLinePlacement {
                    old_y_idx: *old_idx as f32,
                    new_y_idx: *old_idx as f32,
                    opacity_start: 1.0,
                    opacity_end: 0.0,
                    old_line_number: old_idx + 1,
                    new_line_number: old_idx + 1,
                    content: AnimatedLineContent::FromA { idx: *old_idx },
                });
                _old_cursor = *old_idx as f32 + 1.0;
                // _new_cursor does NOT advance for deletes
            }
            LineDiffOp::Insert { new_idx, .. } => {
                // Line fades in at its final position (no Y movement)
                placements.push(AnimatedLinePlacement {
                    old_y_idx: *new_idx as f32,
                    new_y_idx: *new_idx as f32,
                    opacity_start: 0.0,
                    opacity_end: 1.0,
                    old_line_number: new_idx + 1,
                    new_line_number: new_idx + 1,
                    content: AnimatedLineContent::FromB { idx: *new_idx },
                });
                _new_cursor = *new_idx as f32 + 1.0;
                // _old_cursor does NOT advance for inserts
            }
            LineDiffOp::Replace { old_line, new_line, old_idx, new_idx } => {
                placements.push(AnimatedLinePlacement {
                    old_y_idx: *old_idx as f32,
                    new_y_idx: *new_idx as f32,
                    opacity_start: 1.0,
                    opacity_end: 1.0,
                    old_line_number: old_idx + 1,
                    new_line_number: new_idx + 1,
                    content: AnimatedLineContent::CursorEdit {
                        old_line: old_line.clone(),
                        new_line: new_line.clone(),
                        old_idx: *old_idx,
                        new_idx: *new_idx,
                    },
                });
                _old_cursor = *old_idx as f32 + 1.0;
                _new_cursor = *new_idx as f32 + 1.0;
            }
        }
    }

    // Pre-compute fragment edits for Replace ops
    let mut fragment_edits: Vec<(usize, usize, Vec<FragmentEdit>)> = Vec::new();
    for op in &diff_ops {
        if let LineDiffOp::Replace { old_line, new_line, old_idx, new_idx } = op {
            let edits = compute_word_diff(old_line, new_line);
            fragment_edits.push((*old_idx, *new_idx, edits));
        }
    }

    // Render all placements
    let gutter_x = block_x + padding.left;

    for pl in &placements {
        let y_pos = code_y + lerp(pl.old_y_idx, pl.new_y_idx, progress) * line_height;
        let opacity = lerp(pl.opacity_start, pl.opacity_end, progress);

        // Skip fully transparent lines
        if opacity < 0.005 {
            continue;
        }

        // Draw line number — use old number at start, new number at end
        if layer.show_line_numbers {
            let line_num = if progress < 0.5 { pl.old_line_number } else { pl.new_line_number };
            draw_line_number_at(canvas, font, gutter_x, y_pos + ascent, line_num, opacity);
        }

        // Draw content
        match &pl.content {
            AnimatedLineContent::FromB { idx } => {
                if let Some(line) = highlighted_b.get(*idx) {
                    draw_single_highlighted_line(canvas, line, font, code_x, y_pos + ascent, opacity);
                }
            }
            AnimatedLineContent::FromA { idx } => {
                if let Some(line) = highlighted_a.get(*idx) {
                    draw_single_highlighted_line(canvas, line, font, code_x, y_pos + ascent, opacity);
                }
            }
            AnimatedLineContent::CursorEdit { old_line, new_line, old_idx, new_idx } => {
                if let Some((_oi, _ni, edits)) =
                    fragment_edits.iter().find(|(oi, ni, _)| oi == old_idx && ni == new_idx)
                {
                    draw_cursor_edited_line(
                        canvas, font, old_line, new_line, edits,
                        code_x, y_pos + ascent, trans.progress,
                        cursor_enabled, cursor_color, cursor_width, cursor_blink,
                        &layer.language, theme,
                    );
                }
            }
        }
    }

    Ok(())
}

// ─── Line diff computation ───────────────────────────────────────────────────

fn compute_line_diff(code_a: &str, code_b: &str) -> Vec<LineDiffOp> {
    let diff = TextDiff::from_lines(code_a, code_b);
    let mut ops = Vec::new();
    let mut old_idx = 0usize;
    let mut new_idx = 0usize;

    for change in diff.iter_all_changes() {
        let text = change.value().trim_end_matches('\n').to_string();
        match change.tag() {
            ChangeTag::Equal => {
                ops.push(LineDiffOp::Equal { line: text, old_idx, new_idx });
                old_idx += 1;
                new_idx += 1;
            }
            ChangeTag::Delete => {
                ops.push(LineDiffOp::Delete { line: text, old_idx });
                old_idx += 1;
            }
            ChangeTag::Insert => {
                let merged = matches!(ops.last(), Some(LineDiffOp::Delete { .. }));
                if merged {
                    if let Some(LineDiffOp::Delete { line: old_line, old_idx: oi }) = ops.pop() {
                        ops.push(LineDiffOp::Replace {
                            old_line, new_line: text, old_idx: oi, new_idx,
                        });
                    }
                } else {
                    ops.push(LineDiffOp::Insert { line: text, new_idx });
                }
                new_idx += 1;
            }
        }
    }

    ops
}

// ─── Word-level diff for cursor animation ────────────────────────────────────

fn compute_word_diff(old_line: &str, new_line: &str) -> Vec<FragmentEdit> {
    let diff = TextDiff::from_words(old_line, new_line);
    let mut edits = Vec::new();
    let mut col = 0usize;
    let mut pending_delete = String::new();

    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Equal => {
                if !pending_delete.is_empty() {
                    edits.push(FragmentEdit { col, delete: pending_delete.clone(), insert: String::new() });
                    pending_delete.clear();
                }
                col += change.value().len();
            }
            ChangeTag::Delete => {
                pending_delete.push_str(change.value());
            }
            ChangeTag::Insert => {
                edits.push(FragmentEdit {
                    col, delete: pending_delete.clone(), insert: change.value().to_string(),
                });
                pending_delete.clear();
                col += change.value().len();
            }
        }
    }

    if !pending_delete.is_empty() {
        edits.push(FragmentEdit { col, delete: pending_delete, insert: String::new() });
    }

    edits
}

// ─── Cursor-animated line editing ────────────────────────────────────────────

fn draw_cursor_edited_line(
    canvas: &Canvas, font: &Font, old_line: &str, new_line: &str,
    edits: &[FragmentEdit], x: f32, y: f32, progress: f64,
    cursor_enabled: bool, cursor_color: &str, cursor_width: f32, cursor_blink: bool,
    language: &str, theme: &Theme,
) {
    if edits.is_empty() {
        let highlighted = highlight_code(new_line, language, theme);
        if let Some(line) = highlighted.first() {
            draw_single_highlighted_line(canvas, line, font, x, y, 1.0);
        }
        return;
    }

    let total_work: usize = edits.iter().map(|e| e.delete.len() + e.insert.len()).sum();
    if total_work == 0 {
        let highlighted = highlight_code(new_line, language, theme);
        if let Some(line) = highlighted.first() {
            draw_single_highlighted_line(canvas, line, font, x, y, 1.0);
        }
        return;
    }

    let chars_progress = (progress * total_work as f64).round() as usize;
    let mut current_line = old_line.to_string();
    let mut work_done = 0usize;
    let mut cursor_col: Option<usize> = None;
    let mut offset_adjust: i64 = 0;

    for edit in edits {
        let adjusted_col = (edit.col as i64 + offset_adjust).max(0) as usize;
        let delete_len = edit.delete.len();
        let insert_len = edit.insert.len();
        let edit_work = delete_len + insert_len;

        if work_done + edit_work <= chars_progress {
            let end = (adjusted_col + delete_len).min(current_line.len());
            let start = adjusted_col.min(current_line.len());
            current_line.replace_range(start..end, &edit.insert);
            offset_adjust += insert_len as i64 - delete_len as i64;
            work_done += edit_work;
        } else {
            let remaining_progress = chars_progress - work_done;
            if remaining_progress < delete_len {
                let chars_deleted = remaining_progress;
                let del_start = (adjusted_col + delete_len - chars_deleted).min(current_line.len());
                let del_end = (adjusted_col + delete_len).min(current_line.len());
                if del_start < del_end {
                    current_line.replace_range(del_start..del_end, "");
                }
                cursor_col = Some(del_start.min(current_line.len()));
            } else {
                let chars_inserted = remaining_progress - delete_len;
                let start = adjusted_col.min(current_line.len());
                let end = (adjusted_col + delete_len).min(current_line.len());
                let partial_insert = &edit.insert[..chars_inserted.min(edit.insert.len())];
                current_line.replace_range(start..end, partial_insert);
                cursor_col = Some(adjusted_col + chars_inserted);
            }
            break;
        }
    }

    let highlighted = highlight_code(&current_line, language, theme);
    if let Some(line) = highlighted.first() {
        draw_single_highlighted_line(canvas, line, font, x, y, 1.0);
    }

    if cursor_enabled {
        if let Some(col) = cursor_col {
            let should_show = if cursor_blink {
                let blink_time = progress * 10.0;
                (blink_time % 1.06).fract() < 0.53
            } else {
                true
            };

            if should_show {
                let prefix = &current_line[..col.min(current_line.len())];
                let (prefix_width, _) = font.measure_str(prefix, None);
                let cursor_x = x + prefix_width;
                let mut cursor_paint = paint_from_hex(cursor_color);
                cursor_paint.set_style(PaintStyle::Fill);
                let (_sw, metrics) = font.metrics();
                let cursor_top = y - (-metrics.ascent);
                let cursor_bottom = y + metrics.descent;
                let cursor_rect = Rect::from_xywh(cursor_x, cursor_top, cursor_width, cursor_bottom - cursor_top);
                canvas.draw_rect(cursor_rect, &cursor_paint);
            }
        }
    }
}

// ─── Helper: draw a single highlighted line ──────────────────────────────────

fn draw_single_highlighted_line(
    canvas: &Canvas, line: &HighlightedLine, font: &Font,
    x: f32, y: f32, opacity: f32,
) {
    let mut cursor_x = x;
    for span in &line.spans {
        if span.text.is_empty() { continue; }
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_color4f(
            skia_safe::Color4f::new(
                span.r as f32 / 255.0, span.g as f32 / 255.0,
                span.b as f32 / 255.0, (span.a as f32 / 255.0) * opacity,
            ),
            None,
        );
        if let Some(blob) = TextBlob::new(&span.text, font) {
            canvas.draw_text_blob(&blob, (cursor_x, y), &paint);
        }
        let (w, _) = font.measure_str(&span.text, None);
        cursor_x += w;
    }
}

// ─── Font resolution ─────────────────────────────────────────────────────────

fn resolve_monospace_font(family: &str, size: f32, weight: Option<u16>) -> Font {
    let font_mgr = FontMgr::default();
    let w = weight.unwrap_or(400);
    let skia_weight = skia_safe::font_style::Weight::from(w as i32);
    let style = FontStyle::new(skia_weight, skia_safe::font_style::Width::NORMAL, skia_safe::font_style::Slant::Upright);
    let fallbacks = [family, "JetBrains Mono", "Fira Code", "Menlo", "Courier New", "monospace"];
    let typeface = fallbacks.iter()
        .filter_map(|name| font_mgr.match_family_style(name, style))
        .next()
        .unwrap_or_else(|| {
            if font_mgr.count_families() > 0 {
                font_mgr.match_family_style(&font_mgr.family_name(0), style).unwrap()
            } else {
                panic!("No fonts available on this system");
            }
        });
    Font::from_typeface(typeface, size)
}

// ─── Utility ─────────────────────────────────────────────────────────────────

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
