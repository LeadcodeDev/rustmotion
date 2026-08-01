//! CSS colour parsing.
//!
//! [`parse_css_color`] is the single entry point every colour-consuming path
//! in the engine should route through. It accepts every colour form a CSS
//! author would reasonably write:
//!
//! - hex: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa` (case-insensitive)
//! - `rgb()` / `rgba()`, comma- or space-separated, integers or percentages,
//!   alpha as a `0..1` float, a percentage, or after a `/`
//! - `hsl()` / `hsla()`, same separator/alpha flexibility
//! - the full CSS named-colour keyword set (147 names + `transparent`)
//!
//! It returns `None` for anything else so callers can detect the failure
//! instead of silently painting something wrong.
//!
//! [`parse_hex_color`] predates this module and keeps its historical,
//! infallible signature so every existing call site keeps compiling
//! untouched — it is now a thin wrapper around [`parse_css_color`]. Its name
//! is a bit of a misnomer at this point (it accepts any CSS colour form, not
//! just hex) but changing it would ripple across dozens of call sites in
//! sibling crates, which is out of scope here.
//!
//! Because `parse_hex_color` can't propagate a `None` (its signature is
//! frozen), unresolvable input logs a warning to stderr and returns
//! [`UNRESOLVED_COLOR`] — an unmistakable opaque magenta — instead of
//! quietly falling back to black. Black is a real, common colour choice; on
//! the dark backgrounds this tool targets, a black fallback for a failed
//! parse is *invisible*, which is exactly the silent-failure bug this module
//! exists to close. Magenta never blends in.

use skia_safe::{Color4f, Paint};

/// Sentinel colour returned by [`parse_hex_color`] (and used by callers of
/// [`parse_css_color`] that choose to mirror this convention) when the input
/// string cannot be resolved as any known CSS colour form. Deliberately
/// jarring so a bad colour string is obvious on screen rather than
/// disappearing into a dark background.
pub const UNRESOLVED_COLOR: (u8, u8, u8, u8) = (255, 0, 255, 255);

/// Parse any CSS colour string into `(r, g, b, a)` bytes.
///
/// Returns `None` when `input` doesn't match any recognised form. Callers
/// that need an infallible result should decide their own fallback and
/// **should not** default to black without a very good reason — see the
/// module docs.
pub fn parse_css_color(input: &str) -> Option<(u8, u8, u8, u8)> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }

    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex_digits(hex);
    }

    let lower = s.to_ascii_lowercase();
    if let Some(rgba) = parse_rgb_fn(&lower) {
        return Some(rgba);
    }
    if let Some(rgba) = parse_hsl_fn(&lower) {
        return Some(rgba);
    }
    named_color(&lower)
}

/// Parse a hex colour string (any CSS colour form, historically hex-only —
/// see module docs) into RGBA components. Infallible: unresolvable input is
/// reported to stderr and mapped to [`UNRESOLVED_COLOR`] rather than black.
pub fn parse_hex_color(hex: &str) -> (u8, u8, u8, u8) {
    let s = hex.trim();

    if let Some(rgba) = parse_css_color(s) {
        return rgba;
    }

    // Historical leniency: the original implementation stripped a leading
    // '#' unconditionally, so callers could (and do) pass hex digits with no
    // '#' at all. Retry with one prepended before giving up.
    if !s.starts_with('#') {
        if let Some(rgba) = parse_css_color(&format!("#{s}")) {
            return rgba;
        }
    }

    eprintln!(
        "Warning: unrecognized color '{hex}' — rendering as opaque magenta {UNRESOLVED_COLOR:?} \
         instead of silently falling back to black"
    );
    UNRESOLVED_COLOR
}

pub fn color4f_from_hex(hex: &str) -> Color4f {
    let (r, g, b, a) = parse_hex_color(hex);
    Color4f::new(
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    )
}

pub fn paint_from_hex(hex: &str) -> Paint {
    let mut paint = Paint::new(color4f_from_hex(hex), None);
    paint.set_anti_alias(true);
    paint
}

// ---- hex ----

fn parse_hex_digits(hex: &str) -> Option<(u8, u8, u8, u8)> {
    if hex.is_empty() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let nibble = |c: char| c.to_digit(16).unwrap() as u8;
    match hex.len() {
        3 => {
            let mut cs = hex.chars();
            let r = nibble(cs.next()?) * 17;
            let g = nibble(cs.next()?) * 17;
            let b = nibble(cs.next()?) * 17;
            Some((r, g, b, 255))
        }
        4 => {
            let mut cs = hex.chars();
            let r = nibble(cs.next()?) * 17;
            let g = nibble(cs.next()?) * 17;
            let b = nibble(cs.next()?) * 17;
            let a = nibble(cs.next()?) * 17;
            Some((r, g, b, a))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some((r, g, b, 255))
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some((r, g, b, a))
        }
        _ => None,
    }
}

// ---- functional notation: rgb()/rgba(), hsl()/hsla() ----

/// Split `name(inner)` into its lowercase function name and inner argument
/// string. `s` is expected to already be trimmed and lowercased.
fn split_function(s: &str) -> Option<(&str, &str)> {
    let s = s.trim();
    let open = s.find('(')?;
    if !s.ends_with(')') {
        return None;
    }
    let name = s[..open].trim();
    let inner = &s[open + 1..s.len() - 1];
    Some((name, inner))
}

/// Tokenize a functional-notation argument list. Accepts the classic
/// comma-separated form (`10, 20, 30, 0.5`) and the modern space-separated
/// form with an optional `/` before alpha (`10 20 30 / 50%`).
fn split_components(inner: &str) -> Vec<String> {
    let inner = inner.trim();
    if inner.contains(',') {
        inner
            .split(',')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect()
    } else {
        inner
            .split('/')
            .flat_map(|part| part.split_whitespace())
            .map(|p| p.to_string())
            .collect()
    }
}

fn parse_rgb_fn(s: &str) -> Option<(u8, u8, u8, u8)> {
    let (name, inner) = split_function(s)?;
    if name != "rgb" && name != "rgba" {
        return None;
    }
    let comps = split_components(inner);
    match comps.len() {
        3 => {
            let r = parse_channel_255(&comps[0])?;
            let g = parse_channel_255(&comps[1])?;
            let b = parse_channel_255(&comps[2])?;
            Some((r, g, b, 255))
        }
        4 => {
            let r = parse_channel_255(&comps[0])?;
            let g = parse_channel_255(&comps[1])?;
            let b = parse_channel_255(&comps[2])?;
            let a = parse_alpha(&comps[3])?;
            Some((r, g, b, a))
        }
        _ => None,
    }
}

fn parse_hsl_fn(s: &str) -> Option<(u8, u8, u8, u8)> {
    let (name, inner) = split_function(s)?;
    if name != "hsl" && name != "hsla" {
        return None;
    }
    let comps = split_components(inner);
    if comps.len() != 3 && comps.len() != 4 {
        return None;
    }
    let h = parse_hue(&comps[0])?;
    let sat = parse_percent_unit(&comps[1])?;
    let light = parse_percent_unit(&comps[2])?;
    let a = if comps.len() == 4 {
        parse_alpha(&comps[3])?
    } else {
        255
    };
    let (r, g, b) = hsl_to_rgb(h, sat, light);
    Some((r, g, b, a))
}

/// A single `rgb()`/`rgba()` colour channel: an integer/float 0-255, or a
/// percentage 0%-100%.
fn parse_channel_255(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        let v: f32 = pct.trim().parse().ok()?;
        return Some((v.clamp(0.0, 100.0) / 100.0 * 255.0).round() as u8);
    }
    let v: f32 = s.parse().ok()?;
    Some(v.clamp(0.0, 255.0).round() as u8)
}

/// Alpha channel: a float 0-1, or a percentage 0%-100%.
///
/// Both branches round, so `0.5` and `50%` resolve to the same byte (128).
/// The old hand-rolled `rgba()` parser truncated (`as u8`), mapping `0.5` to
/// `127`; that was an artefact of the formula rather than a deliberate
/// choice, and it made the two notations for one value disagree. Rounding
/// matches the CSS spec and browsers.
fn parse_alpha(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Some(pct) = s.strip_suffix('%') {
        let v: f32 = pct.trim().parse().ok()?;
        return Some((v.clamp(0.0, 100.0) / 100.0 * 255.0).round() as u8);
    }
    let v: f32 = s.parse().ok()?;
    Some((v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// Hue: a bare number or a number with a `deg` suffix, normalised into
/// `0..360`.
fn parse_hue(s: &str) -> Option<f32> {
    let s = s.trim();
    let s = s.strip_suffix("deg").unwrap_or(s).trim();
    let v: f32 = s.parse().ok()?;
    let mut h = v % 360.0;
    if h < 0.0 {
        h += 360.0;
    }
    Some(h)
}

/// A required percentage (saturation/lightness), normalised to `0..1`.
fn parse_percent_unit(s: &str) -> Option<f32> {
    let s = s.trim().strip_suffix('%')?;
    let v: f32 = s.trim().parse().ok()?;
    Some(v.clamp(0.0, 100.0) / 100.0)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    if s <= 0.0 {
        let v = (l.clamp(0.0, 1.0) * 255.0).round() as u8;
        return (v, v, v);
    }
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match hp as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let to_u8 = |v: f32| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    (to_u8(r1), to_u8(g1), to_u8(b1))
}

// ---- named colours ----

/// The full CSS named-colour keyword set (CSS Color Module Level 4 extended
/// keywords, 147 names) plus `transparent`. `name` must already be
/// lowercased.
fn named_color(name: &str) -> Option<(u8, u8, u8, u8)> {
    if name == "transparent" {
        return Some((0, 0, 0, 0));
    }
    let hex: u32 = match name {
        "aliceblue" => 0xF0F8FF,
        "antiquewhite" => 0xFAEBD7,
        "aqua" => 0x00FFFF,
        "aquamarine" => 0x7FFFD4,
        "azure" => 0xF0FFFF,
        "beige" => 0xF5F5DC,
        "bisque" => 0xFFE4C4,
        "black" => 0x000000,
        "blanchedalmond" => 0xFFEBCD,
        "blue" => 0x0000FF,
        "blueviolet" => 0x8A2BE2,
        "brown" => 0xA52A2A,
        "burlywood" => 0xDEB887,
        "cadetblue" => 0x5F9EA0,
        "chartreuse" => 0x7FFF00,
        "chocolate" => 0xD2691E,
        "coral" => 0xFF7F50,
        "cornflowerblue" => 0x6495ED,
        "cornsilk" => 0xFFF8DC,
        "crimson" => 0xDC143C,
        "cyan" => 0x00FFFF,
        "darkblue" => 0x00008B,
        "darkcyan" => 0x008B8B,
        "darkgoldenrod" => 0xB8860B,
        "darkgray" => 0xA9A9A9,
        "darkgreen" => 0x006400,
        "darkgrey" => 0xA9A9A9,
        "darkkhaki" => 0xBDB76B,
        "darkmagenta" => 0x8B008B,
        "darkolivegreen" => 0x556B2F,
        "darkorange" => 0xFF8C00,
        "darkorchid" => 0x9932CC,
        "darkred" => 0x8B0000,
        "darksalmon" => 0xE9967A,
        "darkseagreen" => 0x8FBC8F,
        "darkslateblue" => 0x483D8B,
        "darkslategray" => 0x2F4F4F,
        "darkslategrey" => 0x2F4F4F,
        "darkturquoise" => 0x00CED1,
        "darkviolet" => 0x9400D3,
        "deeppink" => 0xFF1493,
        "deepskyblue" => 0x00BFFF,
        "dimgray" => 0x696969,
        "dimgrey" => 0x696969,
        "dodgerblue" => 0x1E90FF,
        "firebrick" => 0xB22222,
        "floralwhite" => 0xFFFAF0,
        "forestgreen" => 0x228B22,
        "fuchsia" => 0xFF00FF,
        "gainsboro" => 0xDCDCDC,
        "ghostwhite" => 0xF8F8FF,
        "gold" => 0xFFD700,
        "goldenrod" => 0xDAA520,
        "gray" => 0x808080,
        "green" => 0x008000,
        "greenyellow" => 0xADFF2F,
        "grey" => 0x808080,
        "honeydew" => 0xF0FFF0,
        "hotpink" => 0xFF69B4,
        "indianred" => 0xCD5C5C,
        "indigo" => 0x4B0082,
        "ivory" => 0xFFFFF0,
        "khaki" => 0xF0E68C,
        "lavender" => 0xE6E6FA,
        "lavenderblush" => 0xFFF0F5,
        "lawngreen" => 0x7CFC00,
        "lemonchiffon" => 0xFFFACD,
        "lightblue" => 0xADD8E6,
        "lightcoral" => 0xF08080,
        "lightcyan" => 0xE0FFFF,
        "lightgoldenrodyellow" => 0xFAFAD2,
        "lightgray" => 0xD3D3D3,
        "lightgreen" => 0x90EE90,
        "lightgrey" => 0xD3D3D3,
        "lightpink" => 0xFFB6C1,
        "lightsalmon" => 0xFFA07A,
        "lightseagreen" => 0x20B2AA,
        "lightskyblue" => 0x87CEFA,
        "lightslategray" => 0x778899,
        "lightslategrey" => 0x778899,
        "lightsteelblue" => 0xB0C4DE,
        "lightyellow" => 0xFFFFE0,
        "lime" => 0x00FF00,
        "limegreen" => 0x32CD32,
        "linen" => 0xFAF0E6,
        "magenta" => 0xFF00FF,
        "maroon" => 0x800000,
        "mediumaquamarine" => 0x66CDAA,
        "mediumblue" => 0x0000CD,
        "mediumorchid" => 0xBA55D3,
        "mediumpurple" => 0x9370DB,
        "mediumseagreen" => 0x3CB371,
        "mediumslateblue" => 0x7B68EE,
        "mediumspringgreen" => 0x00FA9A,
        "mediumturquoise" => 0x48D1CC,
        "mediumvioletred" => 0xC71585,
        "midnightblue" => 0x191970,
        "mintcream" => 0xF5FFFA,
        "mistyrose" => 0xFFE4E1,
        "moccasin" => 0xFFE4B5,
        "navajowhite" => 0xFFDEAD,
        "navy" => 0x000080,
        "oldlace" => 0xFDF5E6,
        "olive" => 0x808000,
        "olivedrab" => 0x6B8E23,
        "orange" => 0xFFA500,
        "orangered" => 0xFF4500,
        "orchid" => 0xDA70D6,
        "palegoldenrod" => 0xEEE8AA,
        "palegreen" => 0x98FB98,
        "paleturquoise" => 0xAFEEEE,
        "palevioletred" => 0xDB7093,
        "papayawhip" => 0xFFEFD5,
        "peachpuff" => 0xFFDAB9,
        "peru" => 0xCD853F,
        "pink" => 0xFFC0CB,
        "plum" => 0xDDA0DD,
        "powderblue" => 0xB0E0E6,
        "purple" => 0x800080,
        "rebeccapurple" => 0x663399,
        "red" => 0xFF0000,
        "rosybrown" => 0xBC8F8F,
        "royalblue" => 0x4169E1,
        "saddlebrown" => 0x8B4513,
        "salmon" => 0xFA8072,
        "sandybrown" => 0xF4A460,
        "seagreen" => 0x2E8B57,
        "seashell" => 0xFFF5EE,
        "sienna" => 0xA0522D,
        "silver" => 0xC0C0C0,
        "skyblue" => 0x87CEEB,
        "slateblue" => 0x6A5ACD,
        "slategray" => 0x708090,
        "slategrey" => 0x708090,
        "snow" => 0xFFFAFA,
        "springgreen" => 0x00FF7F,
        "steelblue" => 0x4682B4,
        "tan" => 0xD2B48C,
        "teal" => 0x008080,
        "thistle" => 0xD8BFD8,
        "tomato" => 0xFF6347,
        "turquoise" => 0x40E0D0,
        "violet" => 0xEE82EE,
        "wheat" => 0xF5DEB3,
        "white" => 0xFFFFFF,
        "whitesmoke" => 0xF5F5F5,
        "yellow" => 0xFFFF00,
        "yellowgreen" => 0x9ACD32,
        _ => return None,
    };
    Some((
        ((hex >> 16) & 0xFF) as u8,
        ((hex >> 8) & 0xFF) as u8,
        (hex & 0xFF) as u8,
        255,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- hex ----

    #[test]
    fn hex_3_digit_shorthand() {
        assert_eq!(parse_css_color("#fff"), Some((255, 255, 255, 255)));
        assert_eq!(parse_css_color("#FFF"), Some((255, 255, 255, 255)));
        assert_eq!(parse_css_color("#f00"), Some((255, 0, 0, 255)));
    }

    #[test]
    fn hex_4_digit_shorthand_with_alpha() {
        assert_eq!(parse_css_color("#ffff"), Some((255, 255, 255, 255)));
        assert_eq!(parse_css_color("#f008"), Some((255, 0, 0, 136)));
    }

    #[test]
    fn hex_6_digit() {
        assert_eq!(parse_css_color("#ffffff"), Some((255, 255, 255, 255)));
        assert_eq!(parse_css_color("#102030"), Some((0x10, 0x20, 0x30, 255)));
    }

    #[test]
    fn hex_8_digit_with_alpha() {
        assert_eq!(parse_css_color("#ffffffff"), Some((255, 255, 255, 255)));
        assert_eq!(parse_css_color("#80ffffff"), Some((0x80, 255, 255, 255)));
    }

    #[test]
    fn hex_case_insensitive() {
        assert_eq!(parse_css_color("#AbCdEf"), parse_css_color("#abcdef"));
    }

    // ---- rgb / rgba ----

    #[test]
    fn rgb_function_white() {
        assert_eq!(parse_css_color("rgb(255,255,255)"), Some((255, 255, 255, 255)));
        assert_eq!(
            parse_css_color("rgb(255, 255, 255)"),
            Some((255, 255, 255, 255))
        );
    }

    #[test]
    fn rgb_function_space_syntax() {
        assert_eq!(parse_css_color("rgb(255 255 255)"), Some((255, 255, 255, 255)));
    }

    #[test]
    fn rgba_function_with_float_alpha() {
        // 0.5 * 255 = 127.5, rounded like the CSS spec and like browsers.
        // Percentage and bare-float alpha resolve identically.
        let (r, g, b, a) = parse_css_color("rgba(255,255,255,0.5)").unwrap();
        assert_eq!((r, g, b), (255, 255, 255));
        assert_eq!(a, 128);
        assert_eq!(parse_css_color("rgba(255,255,255,50%)").unwrap().3, a);
    }

    #[test]
    fn rgba_function_space_slash_alpha() {
        let (r, g, b, a) = parse_css_color("rgb(255 255 255 / 50%)").unwrap();
        assert_eq!((r, g, b), (255, 255, 255));
        assert_eq!(a, 128);
    }

    #[test]
    fn rgb_function_percent_channels() {
        assert_eq!(
            parse_css_color("rgb(100%, 100%, 100%)"),
            Some((255, 255, 255, 255))
        );
    }

    // ---- hsl / hsla ----

    #[test]
    fn hsl_function_primary_colors() {
        assert_eq!(parse_css_color("hsl(0, 100%, 50%)"), Some((255, 0, 0, 255)));
        assert_eq!(
            parse_css_color("hsl(120, 100%, 50%)"),
            Some((0, 255, 0, 255))
        );
        assert_eq!(
            parse_css_color("hsl(240, 100%, 50%)"),
            Some((0, 0, 255, 255))
        );
    }

    #[test]
    fn hsl_function_white_and_black() {
        assert_eq!(
            parse_css_color("hsl(0, 0%, 100%)"),
            Some((255, 255, 255, 255))
        );
        assert_eq!(parse_css_color("hsl(0, 0%, 0%)"), Some((0, 0, 0, 255)));
    }

    #[test]
    fn hsla_function_with_alpha() {
        let (r, g, b, a) = parse_css_color("hsla(0, 100%, 50%, 0.5)").unwrap();
        assert_eq!((r, g, b), (255, 0, 0));
        assert_eq!(a, 128);
    }

    #[test]
    fn hsl_function_space_syntax() {
        assert_eq!(
            parse_css_color("hsl(0 100% 50%)"),
            Some((255, 0, 0, 255))
        );
    }

    // ---- named colors ----

    #[test]
    fn named_color_white_black() {
        assert_eq!(parse_css_color("white"), Some((255, 255, 255, 255)));
        assert_eq!(parse_css_color("WHITE"), Some((255, 255, 255, 255)));
        assert_eq!(parse_css_color("black"), Some((0, 0, 0, 255)));
    }

    #[test]
    fn named_color_transparent() {
        assert_eq!(parse_css_color("transparent"), Some((0, 0, 0, 0)));
    }

    #[test]
    fn named_color_extended_set_sample() {
        // Spot-check a handful outside the old 11-name table.
        assert_eq!(parse_css_color("rebeccapurple"), Some((0x66, 0x33, 0x99, 255)));
        assert_eq!(parse_css_color("cornflowerblue"), Some((0x64, 0x95, 0xED, 255)));
        assert_eq!(parse_css_color("tomato"), Some((0xFF, 0x63, 0x47, 255)));
        assert_eq!(parse_css_color("dodgerblue"), Some((0x1E, 0x90, 0xFF, 255)));
    }

    #[test]
    fn named_color_gray_grey_spelling() {
        assert_eq!(parse_css_color("gray"), parse_css_color("grey"));
        assert_eq!(parse_css_color("darkgray"), parse_css_color("darkgrey"));
    }

    // ---- whitespace tolerance ----

    #[test]
    fn tolerates_surrounding_and_internal_whitespace() {
        assert_eq!(parse_css_color("  #fff  "), Some((255, 255, 255, 255)));
        assert_eq!(
            parse_css_color("  white  "),
            Some((255, 255, 255, 255))
        );
        assert_eq!(
            parse_css_color("rgb( 10 , 20 , 30 )"),
            Some((10, 20, 30, 255))
        );
    }

    // ---- rejected forms ----

    #[test]
    fn rejects_garbage_word() {
        assert_eq!(parse_css_color("notacolor"), None);
    }

    #[test]
    fn rejects_malformed_hex_length() {
        assert_eq!(parse_css_color("#ff"), None);
        assert_eq!(parse_css_color("#fffff"), None);
    }

    #[test]
    fn rejects_malformed_hex_digits() {
        assert_eq!(parse_css_color("#gggggg"), None);
    }

    #[test]
    fn rejects_incomplete_rgb() {
        assert_eq!(parse_css_color("rgb(255, 255)"), None);
    }

    #[test]
    fn rejects_unclosed_function() {
        assert_eq!(parse_css_color("rgb(255,255,255"), None);
    }

    #[test]
    fn rejects_empty_string() {
        assert_eq!(parse_css_color(""), None);
        assert_eq!(parse_css_color("   "), None);
    }

    // ---- parse_hex_color (infallible wrapper) ----

    #[test]
    fn parse_hex_color_still_infallible_for_6_digit() {
        assert_eq!(parse_hex_color("#ffffff"), (255, 255, 255, 255));
    }

    #[test]
    fn parse_hex_color_now_handles_3_digit_shorthand() {
        // Regression test for the exact silent-black bug: previously any hex
        // string shorter than 6 chars (post `#`-strip) returned black.
        assert_eq!(parse_hex_color("#fff"), (255, 255, 255, 255));
        assert_eq!(parse_hex_color("#FFF"), (255, 255, 255, 255));
    }

    #[test]
    fn parse_hex_color_now_handles_named_and_functional_forms() {
        // Callers across the codebase pass arbitrary CSS colour strings
        // (not just hex) through this function via `paint_from_hex`.
        assert_eq!(parse_hex_color("white"), (255, 255, 255, 255));
        assert_eq!(parse_hex_color("rgb(255,255,255)"), (255, 255, 255, 255));
    }

    #[test]
    fn parse_hex_color_without_leading_hash_still_works() {
        assert_eq!(parse_hex_color("ffffff"), (255, 255, 255, 255));
        assert_eq!(parse_hex_color("fff"), (255, 255, 255, 255));
    }

    #[test]
    fn parse_hex_color_unresolvable_input_is_loud_not_black() {
        assert_eq!(parse_hex_color("not-a-color"), UNRESOLVED_COLOR);
        assert_ne!(UNRESOLVED_COLOR, (0, 0, 0, 255));
    }

    #[test]
    fn color4f_from_hex_white_is_actually_white() {
        let c = color4f_from_hex("#fff");
        assert_eq!((c.r, c.g, c.b, c.a), (1.0, 1.0, 1.0, 1.0));
    }
}
