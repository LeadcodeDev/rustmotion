use skia_safe::{Font, FontStyle};
use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{
    Color as SynColor, FontStyle as SynFontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem,
    ThemeSet, ThemeSettings,
};
use syntect::parsing::SyntaxSet;

use rustmotion_core::schema::FontWeight;

// ─── Syntect caches ──────────────────────────────────────────────────────────

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();

pub(super) fn syntax_set() -> &'static SyntaxSet {
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

    let name = v
        .get("name")
        .and_then(|n| n.as_str())
        .map(|s| s.to_string());

    // Parse ThemeSettings from "colors" object
    let mut settings = ThemeSettings::default();
    if let Some(colors) = v.get("colors").and_then(|c| c.as_object()) {
        settings.foreground = colors
            .get("editor.foreground")
            .and_then(|v| v.as_str())
            .and_then(parse_syn_color);
        settings.background = colors
            .get("editor.background")
            .and_then(|v| v.as_str())
            .and_then(parse_syn_color);
        settings.caret = colors
            .get("editorCursor.foreground")
            .and_then(|v| v.as_str())
            .and_then(parse_syn_color);
        settings.line_highlight = colors
            .get("editor.lineHighlightBackground")
            .and_then(|v| v.as_str())
            .and_then(parse_syn_color);
        settings.selection = colors
            .get("editor.selectionBackground")
            .and_then(|v| v.as_str())
            .and_then(parse_syn_color);
        settings.selection_foreground = colors
            .get("editor.selectionForeground")
            .and_then(|v| v.as_str())
            .and_then(parse_syn_color);
        settings.gutter = colors
            .get("editorGutter.background")
            .and_then(|v| v.as_str())
            .and_then(parse_syn_color);
        settings.gutter_foreground = colors
            .get("editorLineNumber.foreground")
            .and_then(|v| v.as_str())
            .and_then(parse_syn_color);
        settings.find_highlight = colors
            .get("editor.findMatchHighlightBackground")
            .and_then(|v| v.as_str())
            .and_then(parse_syn_color);
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
                            settings.foreground = s
                                .get("foreground")
                                .and_then(|v| v.as_str())
                                .and_then(parse_syn_color);
                        }
                        if settings.background.is_none() {
                            settings.background = s
                                .get("background")
                                .and_then(|v| v.as_str())
                                .and_then(parse_syn_color);
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
                foreground: tc_settings
                    .get("foreground")
                    .and_then(|v| v.as_str())
                    .and_then(parse_syn_color),
                background: tc_settings
                    .get("background")
                    .and_then(|v| v.as_str())
                    .and_then(parse_syn_color),
                font_style: tc_settings
                    .get("fontStyle")
                    .and_then(|v| v.as_str())
                    .and_then(parse_font_style),
            };

            scopes.push(ThemeItem { scope, style });
        }
    }

    Some(Theme {
        name,
        author: None,
        settings,
        scopes,
    })
}

fn theme_set() -> &'static ThemeSet {
    THEME_SET.get_or_init(|| {
        let mut ts = ThemeSet::load_defaults();

        // Catppuccin themes (tmTheme format)
        let catppuccin_themes: &[(&str, &str)] = &[
            (
                "catppuccin-latte",
                include_str!("../../../../themes/Catppuccin Latte.tmTheme"),
            ),
            (
                "catppuccin-frappe",
                include_str!("../../../../themes/Catppuccin Frappe.tmTheme"),
            ),
            (
                "catppuccin-macchiato",
                include_str!("../../../../themes/Catppuccin Macchiato.tmTheme"),
            ),
            (
                "catppuccin-mocha",
                include_str!("../../../../themes/Catppuccin Mocha.tmTheme"),
            ),
        ];
        for (name, xml) in catppuccin_themes {
            if let Some(theme) = load_theme_from_str(xml) {
                ts.themes.insert(name.to_string(), theme);
            }
        }

        // VS Code / Shiki themes (JSON format)
        let vscode_themes: &[(&str, &str)] = &[
            (
                "andromeeda",
                include_str!("../../../../themes/vscode/andromeeda.json"),
            ),
            (
                "aurora-x",
                include_str!("../../../../themes/vscode/aurora-x.json"),
            ),
            (
                "ayu-dark",
                include_str!("../../../../themes/vscode/ayu-dark.json"),
            ),
            (
                "ayu-light",
                include_str!("../../../../themes/vscode/ayu-light.json"),
            ),
            (
                "ayu-mirage",
                include_str!("../../../../themes/vscode/ayu-mirage.json"),
            ),
            (
                "dark-plus",
                include_str!("../../../../themes/vscode/dark-plus.json"),
            ),
            (
                "dracula",
                include_str!("../../../../themes/vscode/dracula.json"),
            ),
            (
                "dracula-soft",
                include_str!("../../../../themes/vscode/dracula-soft.json"),
            ),
            (
                "everforest-dark",
                include_str!("../../../../themes/vscode/everforest-dark.json"),
            ),
            (
                "everforest-light",
                include_str!("../../../../themes/vscode/everforest-light.json"),
            ),
            (
                "github-dark",
                include_str!("../../../../themes/vscode/github-dark.json"),
            ),
            (
                "github-dark-default",
                include_str!("../../../../themes/vscode/github-dark-default.json"),
            ),
            (
                "github-dark-dimmed",
                include_str!("../../../../themes/vscode/github-dark-dimmed.json"),
            ),
            (
                "github-dark-high-contrast",
                include_str!("../../../../themes/vscode/github-dark-high-contrast.json"),
            ),
            (
                "github-light",
                include_str!("../../../../themes/vscode/github-light.json"),
            ),
            (
                "github-light-default",
                include_str!("../../../../themes/vscode/github-light-default.json"),
            ),
            (
                "github-light-high-contrast",
                include_str!("../../../../themes/vscode/github-light-high-contrast.json"),
            ),
            (
                "gruvbox-dark-hard",
                include_str!("../../../../themes/vscode/gruvbox-dark-hard.json"),
            ),
            (
                "gruvbox-dark-medium",
                include_str!("../../../../themes/vscode/gruvbox-dark-medium.json"),
            ),
            (
                "gruvbox-dark-soft",
                include_str!("../../../../themes/vscode/gruvbox-dark-soft.json"),
            ),
            (
                "gruvbox-light-hard",
                include_str!("../../../../themes/vscode/gruvbox-light-hard.json"),
            ),
            (
                "gruvbox-light-medium",
                include_str!("../../../../themes/vscode/gruvbox-light-medium.json"),
            ),
            (
                "gruvbox-light-soft",
                include_str!("../../../../themes/vscode/gruvbox-light-soft.json"),
            ),
            (
                "horizon",
                include_str!("../../../../themes/vscode/horizon.json"),
            ),
            (
                "horizon-bright",
                include_str!("../../../../themes/vscode/horizon-bright.json"),
            ),
            (
                "houston",
                include_str!("../../../../themes/vscode/houston.json"),
            ),
            (
                "kanagawa-dragon",
                include_str!("../../../../themes/vscode/kanagawa-dragon.json"),
            ),
            (
                "kanagawa-lotus",
                include_str!("../../../../themes/vscode/kanagawa-lotus.json"),
            ),
            (
                "kanagawa-wave",
                include_str!("../../../../themes/vscode/kanagawa-wave.json"),
            ),
            (
                "laserwave",
                include_str!("../../../../themes/vscode/laserwave.json"),
            ),
            (
                "light-plus",
                include_str!("../../../../themes/vscode/light-plus.json"),
            ),
            (
                "material-theme",
                include_str!("../../../../themes/vscode/material-theme.json"),
            ),
            (
                "material-theme-darker",
                include_str!("../../../../themes/vscode/material-theme-darker.json"),
            ),
            (
                "material-theme-lighter",
                include_str!("../../../../themes/vscode/material-theme-lighter.json"),
            ),
            (
                "material-theme-ocean",
                include_str!("../../../../themes/vscode/material-theme-ocean.json"),
            ),
            (
                "material-theme-palenight",
                include_str!("../../../../themes/vscode/material-theme-palenight.json"),
            ),
            (
                "min-dark",
                include_str!("../../../../themes/vscode/min-dark.json"),
            ),
            (
                "min-light",
                include_str!("../../../../themes/vscode/min-light.json"),
            ),
            (
                "monokai",
                include_str!("../../../../themes/vscode/monokai.json"),
            ),
            (
                "night-owl",
                include_str!("../../../../themes/vscode/night-owl.json"),
            ),
            (
                "night-owl-light",
                include_str!("../../../../themes/vscode/night-owl-light.json"),
            ),
            ("nord", include_str!("../../../../themes/vscode/nord.json")),
            (
                "one-dark-pro",
                include_str!("../../../../themes/vscode/one-dark-pro.json"),
            ),
            (
                "one-light",
                include_str!("../../../../themes/vscode/one-light.json"),
            ),
            (
                "plastic",
                include_str!("../../../../themes/vscode/plastic.json"),
            ),
            (
                "poimandres",
                include_str!("../../../../themes/vscode/poimandres.json"),
            ),
            ("red", include_str!("../../../../themes/vscode/red.json")),
            (
                "rose-pine",
                include_str!("../../../../themes/vscode/rose-pine.json"),
            ),
            (
                "rose-pine-dawn",
                include_str!("../../../../themes/vscode/rose-pine-dawn.json"),
            ),
            (
                "rose-pine-moon",
                include_str!("../../../../themes/vscode/rose-pine-moon.json"),
            ),
            (
                "slack-dark",
                include_str!("../../../../themes/vscode/slack-dark.json"),
            ),
            (
                "slack-ochin",
                include_str!("../../../../themes/vscode/slack-ochin.json"),
            ),
            (
                "snazzy-light",
                include_str!("../../../../themes/vscode/snazzy-light.json"),
            ),
            (
                "solarized-dark",
                include_str!("../../../../themes/vscode/solarized-dark.json"),
            ),
            (
                "solarized-light",
                include_str!("../../../../themes/vscode/solarized-light.json"),
            ),
            (
                "synthwave-84",
                include_str!("../../../../themes/vscode/synthwave-84.json"),
            ),
            (
                "tokyo-night",
                include_str!("../../../../themes/vscode/tokyo-night.json"),
            ),
            (
                "vesper",
                include_str!("../../../../themes/vscode/vesper.json"),
            ),
            (
                "vitesse-black",
                include_str!("../../../../themes/vscode/vitesse-black.json"),
            ),
            (
                "vitesse-dark",
                include_str!("../../../../themes/vscode/vitesse-dark.json"),
            ),
            (
                "vitesse-light",
                include_str!("../../../../themes/vscode/vitesse-light.json"),
            ),
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

pub(super) struct ColoredSpan {
    pub(super) text: String,
    pub(super) r: u8,
    pub(super) g: u8,
    pub(super) b: u8,
    pub(super) a: u8,
}

pub(super) struct HighlightedLine {
    pub(super) spans: Vec<ColoredSpan>,
}

// ─── Public API ──────────────────────────────────────────────────────────────

pub(super) fn get_theme(name: &str) -> &'static Theme {
    let ts = theme_set();
    ts.themes
        .get(name)
        .unwrap_or_else(|| ts.themes.values().next().unwrap())
}

pub(super) fn highlight_code(code: &str, language: &str, theme: &Theme) -> Vec<HighlightedLine> {
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

pub(super) fn resolve_monospace_font(family: &str, size: f32, weight: FontWeight) -> Font {
    let font_mgr = rustmotion_core::engine::renderer::font_mgr();
    let w: i32 = match weight {
        FontWeight::Normal => 400,
        FontWeight::Bold => 700,
        FontWeight::Weight(w) => w as i32,
    };
    let skia_weight = skia_safe::font_style::Weight::from(w);
    let style = FontStyle::new(
        skia_weight,
        skia_safe::font_style::Width::NORMAL,
        skia_safe::font_style::Slant::Upright,
    );
    let fallbacks = [
        family,
        "JetBrains Mono",
        "Fira Code",
        "Menlo",
        "Courier New",
        "monospace",
    ];
    let typeface = fallbacks
        .iter()
        .filter_map(|name| font_mgr.match_family_style(name, style))
        .next()
        .unwrap_or_else(|| {
            if font_mgr.count_families() > 0 {
                font_mgr
                    .match_family_style(font_mgr.family_name(0), style)
                    .unwrap()
            } else {
                panic!("No fonts available on this system");
            }
        });
    Font::from_typeface(typeface, size)
}
