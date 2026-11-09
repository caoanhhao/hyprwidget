//! Parsing of `Notify` hints (Desktop Notifications spec 1.2).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use zbus::zvariant::{OwnedValue, Value};

use crate::dbus::{unwrap_variant, v_bool, v_bytes, v_i64, v_str};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Urgency {
    Low,
    #[default]
    Normal,
    Critical,
}

impl Urgency {
    fn from_byte(b: i64) -> Self {
        match b {
            0 => Self::Low,
            2 => Self::Critical,
            _ => Self::Normal,
        }
    }
}

/// Raw pixels of an `image-data` hint (`(iiibiiay)`).
#[derive(Debug, Clone, PartialEq)]
pub struct RawImage {
    pub width: i32,
    pub height: i32,
    pub rowstride: i32,
    pub has_alpha: bool,
    pub bits_per_sample: i32,
    pub channels: i32,
    pub data: Vec<u8>,
}

impl RawImage {
    /// Only 8-bit RGB(A) is defined by the spec.
    pub fn is_supported(&self) -> bool {
        self.bits_per_sample == 8
            && self.channels == if self.has_alpha { 4 } else { 3 }
            && self.width > 0
            && self.height > 0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImageHint {
    Data(RawImage),
    /// A file path (`file://` URIs are decoded) or an icon name.
    Path(String),
}

/// The hints we understand. Unknown hints are dropped.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Hints {
    pub urgency: Urgency,
    pub category: Option<String>,
    pub desktop_entry: Option<String>,
    pub image: Option<ImageHint>,
    pub action_icons: bool,
    pub resident: bool,
    pub transient: bool,
    pub sound_file: Option<String>,
    pub sound_name: Option<String>,
    pub suppress_sound: bool,
    pub x: Option<i32>,
    pub y: Option<i32>,
    /// Progress hint used by many apps (`value`, 0–100).
    pub value: Option<i32>,
}

pub fn parse_image_data(v: &Value) -> Option<RawImage> {
    let Value::Structure(s) = unwrap_variant(v) else {
        return None;
    };
    let f = s.fields();
    if f.len() != 7 {
        return None;
    }
    Some(RawImage {
        width: v_i64(&f[0])? as i32,
        height: v_i64(&f[1])? as i32,
        rowstride: v_i64(&f[2])? as i32,
        has_alpha: v_bool(&f[3])?,
        bits_per_sample: v_i64(&f[4])? as i32,
        channels: v_i64(&f[5])? as i32,
        data: v_bytes(&f[6])?,
    })
}

/// `file:///a%20b.png` → `/a b.png`; other strings unchanged.
pub fn uri_to_path(s: &str) -> String {
    if s.starts_with("file://")
        && let Ok((path, _)) = glib::filename_from_uri(s)
    {
        return path.to_string_lossy().into_owned();
    }
    s.to_owned()
}

pub fn parse_hints(hints: &HashMap<String, OwnedValue>) -> Hints {
    let get = |k: &str| hints.get(k).map(|v| &**v);
    let str_hint = |k: &str| get(k).and_then(v_str).filter(|s| !s.is_empty());
    let bool_hint = |k: &str| get(k).and_then(v_bool).unwrap_or(false);
    let int_hint = |k: &str| get(k).and_then(v_i64).map(|n| n as i32);

    // Priority (spec 1.2 "Icons and Images"): image-data > image-path >
    // (app_icon, handled by the caller) > icon_data. Older spellings too.
    let image = ["image-data", "image_data"]
        .iter()
        .find_map(|k| get(k).and_then(parse_image_data))
        .filter(RawImage::is_supported)
        .map(ImageHint::Data)
        .or_else(|| {
            ["image-path", "image_path"]
                .iter()
                .find_map(|k| str_hint(k))
                .map(|p| ImageHint::Path(uri_to_path(&p)))
        })
        .or_else(|| {
            get("icon_data")
                .and_then(parse_image_data)
                .filter(RawImage::is_supported)
                .map(ImageHint::Data)
        });

    Hints {
        urgency: get("urgency")
            .and_then(v_i64)
            .map(Urgency::from_byte)
            .unwrap_or_default(),
        category: str_hint("category"),
        desktop_entry: str_hint("desktop-entry"),
        image,
        action_icons: bool_hint("action-icons"),
        resident: bool_hint("resident"),
        transient: bool_hint("transient"),
        sound_file: str_hint("sound-file").map(|p| uri_to_path(&p)),
        sound_name: str_hint("sound-name"),
        suppress_sound: bool_hint("suppress-sound"),
        x: int_hint("x"),
        y: int_hint("y"),
        value: int_hint("value"),
    }
}

/// `["default", "Open", "x", "Close"]` → `[("default","Open"), ("x","Close")]`.
pub fn parse_actions(actions: &[String]) -> Vec<(String, String)> {
    actions
        .as_chunks::<2>()
        .0
        .iter()
        .map(|[k, v]| (k.clone(), v.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::{Structure, StructureBuilder};

    fn ov(v: Value<'_>) -> OwnedValue {
        v.try_to_owned().unwrap()
    }

    fn image(w: i32, h: i32, alpha: bool) -> Structure<'static> {
        let ch = if alpha { 4 } else { 3 };
        StructureBuilder::new()
            .add_field(w)
            .add_field(h)
            .add_field(w * ch)
            .add_field(alpha)
            .add_field(8i32)
            .add_field(ch)
            .add_field(vec![7u8; (w * h * ch) as usize])
            .build()
            .unwrap()
    }

    #[test]
    fn basic_hints() {
        let mut m = HashMap::new();
        m.insert("urgency".into(), ov(Value::U8(2)));
        m.insert("category".into(), ov(Value::from("im.received")));
        m.insert(
            "desktop-entry".into(),
            ov(Value::from("org.telegram.desktop")),
        );
        m.insert("resident".into(), ov(Value::Bool(true)));
        m.insert("transient".into(), ov(Value::Bool(false)));
        m.insert(
            "sound-file".into(),
            ov(Value::from("file:///tmp/a%20b.oga")),
        );
        m.insert("suppress-sound".into(), ov(Value::Bool(true)));
        m.insert("x".into(), ov(Value::I32(10)));
        m.insert("value".into(), ov(Value::I32(42)));
        m.insert("unknown".into(), ov(Value::from("ignored")));
        let h = parse_hints(&m);
        assert_eq!(h.urgency, Urgency::Critical);
        assert_eq!(h.category.as_deref(), Some("im.received"));
        assert_eq!(h.desktop_entry.as_deref(), Some("org.telegram.desktop"));
        assert!(h.resident && !h.transient && h.suppress_sound);
        assert_eq!(h.sound_file.as_deref(), Some("/tmp/a b.oga"));
        assert_eq!((h.x, h.y, h.value), (Some(10), None, Some(42)));
        assert_eq!(h.image, None);
    }

    #[test]
    fn defaults_and_variant_wrapping() {
        let h = parse_hints(&HashMap::new());
        assert_eq!(h, Hints::default());
        // Some senders wrap values in an extra variant or use other int types.
        let mut m = HashMap::new();
        m.insert("urgency".into(), ov(Value::Value(Box::new(Value::I32(0)))));
        m.insert("resident".into(), ov(Value::U32(1)));
        let h = parse_hints(&m);
        assert_eq!(h.urgency, Urgency::Low);
        assert!(h.resident);
    }

    #[test]
    fn image_data_beats_image_path() {
        let mut m = HashMap::new();
        m.insert("image-path".into(), ov(Value::from("/tmp/x.png")));
        m.insert("image-data".into(), ov(Value::Structure(image(2, 3, true))));
        let h = parse_hints(&m);
        let Some(ImageHint::Data(img)) = h.image else {
            panic!("expected image data, got {:?}", h.image);
        };
        assert_eq!((img.width, img.height, img.rowstride), (2, 3, 8));
        assert!(img.has_alpha && img.is_supported());
        assert_eq!(img.data.len(), 24);
    }

    #[test]
    fn image_path_uri_and_legacy_names() {
        let mut m = HashMap::new();
        m.insert(
            "image_path".into(),
            ov(Value::from("file:///home/u/pic.png")),
        );
        assert_eq!(
            parse_hints(&m).image,
            Some(ImageHint::Path("/home/u/pic.png".into()))
        );
        let mut m = HashMap::new();
        m.insert("image-path".into(), ov(Value::from("dialog-information")));
        assert_eq!(
            parse_hints(&m).image,
            Some(ImageHint::Path("dialog-information".into()))
        );
        let mut m = HashMap::new();
        m.insert("icon_data".into(), ov(Value::Structure(image(1, 1, false))));
        assert!(matches!(parse_hints(&m).image, Some(ImageHint::Data(_))));
    }

    #[test]
    fn unsupported_image_data_falls_back() {
        let mut bad = image(2, 2, true);
        // 16-bit samples aren't defined by the spec.
        bad = {
            let mut f = bad.into_fields();
            f[4] = Value::I32(16);
            let mut b = StructureBuilder::new();
            for v in f {
                b = b.append_field(v);
            }
            b.build().unwrap()
        };
        let mut m = HashMap::new();
        m.insert("image-data".into(), ov(Value::Structure(bad)));
        m.insert("image-path".into(), ov(Value::from("/a.png")));
        assert_eq!(
            parse_hints(&m).image,
            Some(ImageHint::Path("/a.png".into()))
        );
    }

    #[test]
    fn actions_pairs() {
        let a: Vec<String> = ["default", "Open", "reply", "Reply", "dangling"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            parse_actions(&a),
            vec![
                ("default".into(), "Open".into()),
                ("reply".into(), "Reply".into())
            ]
        );
    }
}
