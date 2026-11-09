//! `hw.service.theme` (also `hw.theme`): a Material You palette computed
//! from the wallpaper with `material-colors` (replaces AGS'
//! `get-image-color.sh` + cwal for the widgets). hw-lua writes the `scss`
//! prop to the generated partial `_material.scss` (`@import "material";`).
//!
//! ```lua
//! hw.theme.colors().primary            -- "#a4c8ff" (every Material role)
//! hw.theme:set_wallpaper(path)         -- mode "auto": light if the image is bright
//! hw.theme:set_wallpaper(path, "dark")
//! hw.theme:set_mode("light")           -- same source color
//! hw.theme:set_source("#6388bd")
//! hw.theme:from_image(path)            -- { source, dominant, brightness, mode }
//! ```
//!
//! SCSS: `$md-primary`, `$md-on-primary`, … (`_` → `-`), `$md-source`,
//! `$md-dominant`, `$md-mode`, and the same as `--md-*` on `:root`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use material_colors::color::Argb;
use material_colors::quantize::{Quantizer, QuantizerCelebi};
use material_colors::score::Score;
use material_colors::theme::ThemeBuilder;
use serde_json::{Map, Value, json};

use crate::num::{f64_to_u8, usize_to_f64};
use crate::service::{Pusher, Service, arg_str, cache_dir};

pub const PROPS: &[&str] = &[
    "colors",
    "mode",
    "source",
    "dominant",
    "wallpaper",
    "scss",
    "busy",
];
/// Google Blue, material-colors' own fallback.
const DEFAULT_SOURCE: Argb = Argb {
    alpha: 255,
    red: 66,
    green: 133,
    blue: 244,
};
/// Pixels sampled: the image is scaled to fit this square first.
const SAMPLE: i32 = 128;
const VIDEO: &[&str] = &["mp4", "webm", "mkv", "mov", "avi"];

/// What an image yields.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageColors {
    /// Best theme source color (Material's quantize + score).
    pub source: Argb,
    /// Average color (`magick img -scale 1x1!`).
    pub dominant: Argb,
    /// 0..255, `(r*299 + g*587 + b*114) / 1000` of the average, like
    /// `detect_variant_from_wallpaper` in hypr/theme.
    pub brightness: f64,
}

impl ImageColors {
    fn auto_mode(&self) -> &'static str {
        if self.brightness > 128.0 {
            "light"
        } else {
            "dark"
        }
    }
}

#[derive(Debug, Clone)]
struct State {
    image: ImageColors,
    /// "auto", "dark" or "light" as requested.
    mode: String,
    wallpaper: Option<String>,
}

pub fn start() -> Result<Rc<Service>> {
    let state = Arc::new(Mutex::new(State {
        image: ImageColors {
            source: DEFAULT_SOURCE,
            dominant: DEFAULT_SOURCE,
            brightness: 0.0,
        },
        mode: "auto".into(),
        wallpaper: None,
    }));
    let svc = Rc::new_cyclic(|weak: &std::rc::Weak<Service>| {
        let mut svc = Service::new("theme", PROPS);
        let (st, w) = (Arc::clone(&state), std::rc::Weak::clone(weak));
        svc.method("set_wallpaper", move |args| {
            let path = arg_str(&args, 0, "path")?;
            let mode = mode_arg(&args, 1)?;
            let Some(svc) = w.upgrade() else {
                return Ok(Value::Null);
            };
            svc.set("busy", json!(true));
            let (st, pusher) = (Arc::clone(&st), svc.pusher());
            std::thread::Builder::new()
                .name("hw-theme".into())
                .spawn(move || {
                    let res = from_image(Path::new(&path));
                    let s = {
                        let mut s = st.lock().expect("theme state lock poisoned");
                        match res {
                            Ok(img) => {
                                s.image = img;
                                s.wallpaper = Some(path.clone());
                            }
                            Err(e) => tracing::warn!("hw.theme: {path}: {e:#}"),
                        }
                        if let Some(m) = mode {
                            s.mode = m;
                        }
                        s.clone()
                    };
                    push(&pusher, &s);
                    pusher.set("busy", json!(false));
                })?;
            Ok(Value::Null)
        });
        let (st, w) = (Arc::clone(&state), std::rc::Weak::clone(weak));
        svc.method("set_mode", move |args| {
            let mode = mode_arg(&args, 0)?.unwrap_or_else(|| "auto".into());
            Ok(update(&st, &w, |s| s.mode = mode))
        });
        let (st, w) = (Arc::clone(&state), std::rc::Weak::clone(weak));
        svc.method("set_source", move |args| {
            let hex = arg_str(&args, 0, "color")?;
            let c = parse_hex(&hex).with_context(|| format!("invalid color {hex:?}"))?;
            let mode = mode_arg(&args, 1)?;
            Ok(update(&st, &w, |s| {
                s.image = ImageColors {
                    source: c,
                    dominant: c,
                    brightness: luma(c),
                };
                s.wallpaper = None;
                if let Some(m) = mode {
                    s.mode = m;
                }
            }))
        });
        svc.method("from_image", |args| {
            let img = from_image(Path::new(&arg_str(&args, 0, "path")?))?;
            Ok(json!({
                "source": img.source.to_hex_with_pound(),
                "dominant": img.dominant.to_hex_with_pound(),
                "brightness": img.brightness,
                "mode": img.auto_mode(),
            }))
        });
        svc
    });
    svc.set("busy", json!(false));
    let s = state.lock().expect("theme state lock poisoned").clone();
    for (p, v) in props(&s) {
        svc.set(p, v);
    }
    Ok(svc)
}

fn mode_arg(args: &[Value], i: usize) -> Result<Option<String>> {
    match args.get(i) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(m)) if ["auto", "dark", "light"].contains(&m.as_str()) => {
            Ok(Some(m.clone()))
        }
        Some(other) => bail!("mode must be \"auto\", \"dark\" or \"light\", got {other}"),
    }
}

fn update(st: &Arc<Mutex<State>>, w: &std::rc::Weak<Service>, f: impl FnOnce(&mut State)) -> Value {
    let s = {
        let mut s = st.lock().expect("theme state lock poisoned");
        f(&mut s);
        s.clone()
    };
    if let Some(svc) = w.upgrade() {
        for (p, v) in props(&s) {
            svc.set(p, v);
        }
    }
    Value::Null
}

fn push(pusher: &Pusher, s: &State) {
    for (p, v) in props(s) {
        pusher.set(p, v);
    }
}

fn props(s: &State) -> Vec<(&'static str, Value)> {
    let mode = match s.mode.as_str() {
        "auto" => s.image.auto_mode(),
        "light" => "light",
        _ => "dark",
    };
    let scheme = if mode == "dark" {
        Scheme::Dark
    } else {
        Scheme::Light
    };
    let colors = palette(s.image.source, scheme);
    vec![
        ("scss", json!(to_scss(&colors, &s.image, mode))),
        ("colors", Value::Object(colors)),
        ("mode", json!(mode)),
        ("source", json!(s.image.source.to_hex_with_pound())),
        ("dominant", json!(s.image.dominant.to_hex_with_pound())),
        ("wallpaper", json!(s.wallpaper)),
    ]
}

/// Light or dark variant of a Material scheme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    Light,
    Dark,
}

/// Every Material role of the scheme as `"#rrggbb"`.
pub fn palette(source: Argb, scheme: Scheme) -> Map<String, Value> {
    let theme = ThemeBuilder::with_source(source).build();
    let scheme = match scheme {
        Scheme::Dark => theme.schemes.dark,
        Scheme::Light => theme.schemes.light,
    };
    scheme
        .into_iter()
        .map(|(k, c)| (k, json!(c.to_hex_with_pound())))
        .collect()
}

/// Contents of `_material.scss`.
pub fn to_scss(colors: &Map<String, Value>, img: &ImageColors, mode: &str) -> String {
    let mut vars: Vec<(String, String)> = vec![
        ("md-source".into(), img.source.to_hex_with_pound()),
        ("md-dominant".into(), img.dominant.to_hex_with_pound()),
    ];
    vars.extend(colors.iter().map(|(k, v)| {
        (
            format!("md-{}", k.replace('_', "-")),
            v.as_str().unwrap_or_default().to_owned(),
        )
    }));
    let mut s = String::from(
        "// Generated by hyprwidget from the wallpaper (material-colors). Do not edit.\n",
    );
    let _ = writeln!(s, "$md-mode: {mode};");
    for (k, v) in &vars {
        let _ = writeln!(s, "${k}: {v};");
    }
    s.push_str(":root {\n");
    for (k, v) in &vars {
        let _ = writeln!(s, "  --{k}: {v};");
    }
    s.push_str("}\n");
    s
}

fn parse_hex(s: &str) -> Option<Argb> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let [_, red, green, blue] = u32::from_str_radix(h, 16).ok()?.to_be_bytes();
    Some(Argb::new(255, red, green, blue))
}

fn luma(c: Argb) -> f64 {
    (f64::from(c.red) * 299.0 + f64::from(c.green) * 587.0 + f64::from(c.blue) * 114.0) / 1000.0
}

/// Source color, average and brightness of an image (first frame of a
/// video, through `ffmpeg`).
pub fn from_image(path: &Path) -> Result<ImageColors> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let frame;
    let path = if VIDEO.contains(&ext.as_str()) {
        frame = video_frame(path)?;
        frame.as_path()
    } else {
        path
    };
    let pb = gdk_pixbuf::Pixbuf::from_file_at_scale(path, SAMPLE, SAMPLE, true)
        .with_context(|| format!("loading {}", path.display()))?;
    Ok(from_pixels(&pixels(&pb)))
}

pub fn from_pixels(px: &[Argb]) -> ImageColors {
    if px.is_empty() {
        return ImageColors {
            source: DEFAULT_SOURCE,
            dominant: DEFAULT_SOURCE,
            brightness: 0.0,
        };
    }
    let n = usize_to_f64(px.len());
    let avg = |f: fn(&Argb) -> u8| {
        f64_to_u8((px.iter().map(|p| f64::from(f(p))).sum::<f64>() / n).round())
    };
    let dominant = Argb::new(255, avg(|p| p.red), avg(|p| p.green), avg(|p| p.blue));
    let q = QuantizerCelebi::quantize(px, 128);
    let source = Score::score(&q.color_to_count, None, None, None)
        .first()
        .copied()
        .unwrap_or(DEFAULT_SOURCE);
    ImageColors {
        source,
        dominant,
        brightness: luma(dominant),
    }
}

fn pixels(pb: &gdk_pixbuf::Pixbuf) -> Vec<Argb> {
    let bytes = pb.read_pixel_bytes();
    let dim = |v: i32| usize::try_from(v).unwrap_or(0);
    let (width, height) = (dim(pb.width()), dim(pb.height()));
    let (stride, channels) = (dim(pb.rowstride()), dim(pb.n_channels()));
    let mut out = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let i = y * stride + x * channels;
            let Some(p) = bytes.get(i..i + channels.min(4)) else {
                continue;
            };
            if channels == 4 && p[3] < 255 {
                continue;
            }
            out.push(Argb::new(255, p[0], p[1], p[2]));
        }
    }
    out
}

fn video_frame(path: &Path) -> Result<PathBuf> {
    let out = cache_dir().join("theme-frame.jpg");
    std::fs::create_dir_all(cache_dir())?;
    let ok = Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error", "-ss", "00:00:01", "-i"])
        .arg(path)
        .args(["-frames:v", "1", "-q:v", "2"])
        .arg(&out)
        .stdin(Stdio::null())
        .status()
        .context("running ffmpeg")?
        .success();
    if !ok {
        bail!("ffmpeg could not extract a frame");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_has_material_roles() {
        let c = palette(DEFAULT_SOURCE, Scheme::Dark);
        assert!(c.len() >= 40, "{}", c.len());
        assert!(c["primary"].as_str().unwrap().starts_with('#'));
        assert_ne!(
            c["surface"],
            palette(DEFAULT_SOURCE, Scheme::Light)["surface"]
        );
    }

    #[test]
    fn image_colors() {
        // Mostly dark red with some blue: dark, red-ish source.
        let mut px = vec![Argb::new(255, 120, 20, 20); 900];
        px.extend(vec![Argb::new(255, 20, 20, 160); 100]);
        let img = from_pixels(&px);
        assert_eq!(img.auto_mode(), "dark");
        assert!(img.source.red > img.source.blue, "{:?}", img.source);
        assert_eq!(img.dominant, Argb::new(255, 110, 20, 34));
        let bright = from_pixels(&[Argb::new(255, 240, 240, 230); 10]);
        assert_eq!(bright.auto_mode(), "light");
    }

    #[test]
    fn scss_vars() {
        let img = from_pixels(&[]);
        let colors = palette(img.source, Scheme::Dark);
        let s = to_scss(&colors, &img, "dark");
        assert!(s.contains("$md-mode: dark;"));
        assert!(s.contains("$md-on-primary: #"));
        assert!(s.contains("--md-primary: #"));
        assert_eq!(parse_hex("#4285f4"), Some(DEFAULT_SOURCE));
        assert_eq!(parse_hex("nope"), None);
    }
}
