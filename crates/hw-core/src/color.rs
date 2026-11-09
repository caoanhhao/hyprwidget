//! Hyprland color syntax → CSS.
//!
//! Accepted forms (same as Hyprland's config):
//! `rgb(rrggbb)`, `rgba(rrggbbaa)`, `rgb(r, g, b)`, `rgba(r, g, b, a)`,
//! `0xAARRGGBB`, bare `AARRGGBB` (what `hyprctl getoption -j` prints), and
//! gradients: several colors followed by an optional `<angle>deg`.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    /// 0.0 ..= 1.0
    pub a: f64,
}

impl fmt::Display for Rgba {
    /// CSS `rgba(r,g,b,a)` with the alpha trimmed to 3 decimals.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "rgba({},{},{},{})",
            self.r,
            self.g,
            self.b,
            fmt_num(self.a)
        )
    }
}

/// One or more colors plus an angle in degrees (Hyprland convention).
#[derive(Debug, Clone, PartialEq)]
pub struct Gradient {
    pub colors: Vec<Rgba>,
    pub angle: f64,
}

impl Gradient {
    /// CSS value: a plain color for a single stop, otherwise
    /// `linear-gradient`. Hyprland's 0deg runs left → right and turns
    /// clockwise like CSS, whose 0deg runs bottom → top; hence +90.
    pub fn to_css(&self) -> String {
        match self.colors.as_slice() {
            [] => "transparent".into(),
            [c] => c.to_string(),
            cs => {
                let angle = (self.angle + 90.0).rem_euclid(360.0);
                let stops: Vec<String> = cs.iter().map(Rgba::to_string).collect();
                format!(
                    "linear-gradient({}deg, {})",
                    fmt_num(angle),
                    stops.join(", ")
                )
            }
        }
    }
}

fn fmt_num(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColorError(pub String);

impl fmt::Display for ColorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid color: {}", self.0)
    }
}

impl std::error::Error for ColorError {}

fn err(s: &str) -> ColorError {
    ColorError(s.to_string())
}

fn hex_byte(s: &str) -> Option<u8> {
    u8::from_str_radix(s, 16).ok()
}

/// `rrggbb` or `rrggbbaa`.
fn parse_hex_rgba(h: &str, src: &str) -> Result<Rgba, ColorError> {
    if !h.is_ascii() || !(h.len() == 6 || h.len() == 8) {
        return Err(err(src));
    }
    let b = |i: usize| hex_byte(&h[i..i + 2]).ok_or_else(|| err(src));
    let a = if h.len() == 8 {
        f64::from(b(6)?) / 255.0
    } else {
        1.0
    };
    Ok(Rgba {
        r: b(0)?,
        g: b(2)?,
        b: b(4)?,
        a,
    })
}

/// `aarrggbb`.
fn parse_hex_argb(h: &str, src: &str) -> Result<Rgba, ColorError> {
    if !h.is_ascii() || h.len() != 8 {
        return Err(err(src));
    }
    let b = |i: usize| hex_byte(&h[i..i + 2]).ok_or_else(|| err(src));
    Ok(Rgba {
        a: f64::from(b(0)?) / 255.0,
        r: b(2)?,
        g: b(4)?,
        b: b(6)?,
    })
}

/// Parses a single color.
pub fn parse_color(s: &str) -> Result<Rgba, ColorError> {
    let t = s.trim();
    let lower = t.to_ascii_lowercase();
    if let Some(hex) = lower.strip_prefix("0x") {
        return parse_hex_argb(hex, s);
    }
    let func = |name: &str| {
        lower
            .strip_prefix(name)
            .and_then(|r| r.trim_start().strip_prefix('('))
            .and_then(|r| r.strip_suffix(')'))
            .map(str::trim)
    };
    if let Some(body) = func("rgba").or_else(|| func("rgb")) {
        if !body.contains(',') {
            return parse_hex_rgba(body, s);
        }
        let parts: Vec<&str> = body.split(',').map(str::trim).collect();
        let chan = |p: &str| p.parse::<u8>().map_err(|_| err(s));
        return match parts.as_slice() {
            [red, green, blue] => Ok(Rgba {
                r: chan(red)?,
                g: chan(green)?,
                b: chan(blue)?,
                a: 1.0,
            }),
            [red, green, blue, alpha] => {
                let alpha: f64 = alpha.parse().map_err(|_| err(s))?;
                Ok(Rgba {
                    r: chan(red)?,
                    g: chan(green)?,
                    b: chan(blue)?,
                    a: alpha.clamp(0.0, 1.0),
                })
            }
            _ => Err(err(s)),
        };
    }
    if lower.len() == 8 && lower.chars().all(|c| c.is_ascii_hexdigit()) {
        return parse_hex_argb(&lower, s);
    }
    Err(err(s))
}

/// Splits on whitespace that is not inside parentheses.
fn tokens(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0i32, None);
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            c if c.is_whitespace() && depth == 0 => {
                if let Some(st) = start.take() {
                    out.push(&s[st..i]);
                }
                continue;
            }
            _ => {}
        }
        start.get_or_insert(i);
    }
    if let Some(st) = start {
        out.push(&s[st..]);
    }
    out
}

/// Parses a color or a gradient (`c1 c2 … [Ndeg]`).
pub fn parse_gradient(s: &str) -> Result<Gradient, ColorError> {
    let mut angle = 0.0;
    let mut colors = Vec::new();
    for tok in tokens(s) {
        if let Some(deg) = tok.strip_suffix("deg") {
            angle = deg.parse().map_err(|_| err(s))?;
        } else {
            colors.push(parse_color(tok)?);
        }
    }
    if colors.is_empty() {
        return Err(err(s));
    }
    Ok(Gradient { colors, angle })
}

/// Converts any Hyprland color/gradient string to CSS.
pub fn to_css(s: &str) -> Result<String, ColorError> {
    parse_gradient(s).map(|g| g.to_css())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_hex() {
        assert_eq!(to_css("rgb(808080)").unwrap(), "rgba(128,128,128,1)");
        assert_eq!(to_css("RGB(FF0000)").unwrap(), "rgba(255,0,0,1)");
    }

    #[test]
    fn rgba_hex() {
        assert_eq!(to_css("rgba(33ccffee)").unwrap(), "rgba(51,204,255,0.933)");
    }

    #[test]
    fn rgb_decimal() {
        assert_eq!(to_css("rgb(1, 2, 3)").unwrap(), "rgba(1,2,3,1)");
        assert_eq!(to_css("rgba(1,2,3,0.5)").unwrap(), "rgba(1,2,3,0.5)");
    }

    #[test]
    fn argb_legacy_and_getoption() {
        assert_eq!(to_css("0xee1a1a1a").unwrap(), "rgba(26,26,26,0.933)");
        // `hyprctl getoption -j` gradient output
        assert_eq!(to_css("99808080 0deg").unwrap(), "rgba(128,128,128,0.6)");
    }

    #[test]
    fn gradient_with_angle() {
        let g = parse_gradient("rgba(33ccffee) rgba(00ff99ee) 45deg").unwrap();
        assert_eq!(g.colors.len(), 2);
        assert_eq!(g.angle, 45.0);
        assert_eq!(
            g.to_css(),
            "linear-gradient(135deg, rgba(51,204,255,0.933), rgba(0,255,153,0.933))"
        );
    }

    #[test]
    fn gradient_decimal_stops_keep_spaces_inside_parens() {
        let g = parse_gradient("rgb(1, 2, 3) rgb(4, 5, 6) 300deg").unwrap();
        assert_eq!(
            g.colors[1],
            Rgba {
                r: 4,
                g: 5,
                b: 6,
                a: 1.0
            }
        );
        assert!(g.to_css().starts_with("linear-gradient(30deg"));
    }

    #[test]
    fn invalid() {
        assert!(to_css("rgb(12345)").is_err());
        assert!(to_css("blue").is_err());
        assert!(to_css("45deg").is_err());
        assert!(to_css("rgb(300,0,0)").is_err());
    }
}
