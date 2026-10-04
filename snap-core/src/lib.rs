//! Code screenshot renderer: highlighted code inside a window frame on a background.

mod highlight;
mod layout;
mod text;

use anyhow::Result;
use image::RgbaImage;
use serde::Deserialize;

pub use highlight::{available_themes, resolve_theme_name};

/// RGBA color, straight (non-premultiplied) alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

impl Rgba {
    /// Parses `#RGB`, `#RRGGBB` or `#RRGGBBAA`.
    pub fn parse(s: &str) -> Option<Self> {
        let hex = s.trim().strip_prefix('#')?;
        let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
        match hex.len() {
            3 => {
                let nib = |i: usize| {
                    u8::from_str_radix(hex.get(i..i + 1)?, 16)
                        .ok()
                        .map(|v| v * 17)
                };
                Some(Rgba(nib(0)?, nib(1)?, nib(2)?, 255))
            }
            6 => Some(Rgba(byte(0)?, byte(2)?, byte(4)?, 255)),
            8 => Some(Rgba(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Background {
    Solid(Rgba),
    /// Diagonal gradient, top-left to bottom-right.
    Gradient(Rgba, Rgba),
    Transparent,
}

impl<'de> Deserialize<'de> for Background {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        use serde::de::Error;
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            One(String),
            Many(Vec<String>),
        }
        let parse = |s: &str| {
            Rgba::parse(s).ok_or_else(|| D::Error::custom(format!("invalid color `{s}`")))
        };
        match Raw::deserialize(d)? {
            Raw::One(s)
                if s.eq_ignore_ascii_case("transparent") || s.eq_ignore_ascii_case("none") =>
            {
                Ok(Background::Transparent)
            }
            Raw::One(s) => Ok(Background::Solid(parse(&s)?)),
            Raw::Many(v) => match v.as_slice() {
                [a] => Ok(Background::Solid(parse(a)?)),
                [a, b, ..] => Ok(Background::Gradient(parse(a)?, parse(b)?)),
                [] => Err(D::Error::custom("empty background list")),
            },
        }
    }
}

/// Render options. All fields have defaults; deserializes from user settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Options {
    pub theme: String,
    pub background: Background,
    pub padding: u32,
    pub window_controls: bool,
    pub line_numbers: bool,
    /// Number shown on the first line when `line_numbers` is on.
    #[serde(skip)]
    pub start_line: usize,
    pub font_size: f32,
    pub scale: f32,
    pub shadow: bool,
    /// Title shown in the window bar (usually the file name).
    #[serde(skip)]
    pub title: Option<String>,
    pub tab_width: usize,
    pub max_lines: usize,
    pub max_columns: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            theme: "Dracula".into(),
            background: Background::Gradient(
                Rgba(0x8B, 0x5C, 0xF6, 255),
                Rgba(0xEC, 0x48, 0x99, 255),
            ),
            padding: 48,
            window_controls: true,
            line_numbers: false,
            start_line: 1,
            font_size: 16.0,
            scale: 2.0,
            shadow: true,
            title: None,
            tab_width: 4,
            max_lines: 300,
            max_columns: 200,
        }
    }
}

pub struct Rendered {
    pub image: RgbaImage,
    /// True when input exceeded `max_lines` / `max_columns` and was cut.
    pub truncated: bool,
    /// Theme name actually used (after fallback).
    pub theme: String,
    pub unknown_theme: bool,
}

/// Renders `code` to an image. `lang` is a language id, name or file extension;
/// `file_name` is used as a fallback for syntax detection.
pub fn render(
    code: &str,
    lang: Option<&str>,
    file_name: Option<&str>,
    opts: &Options,
) -> Result<Rendered> {
    let (lines, truncated) = text::prepare(code, opts.tab_width, opts.max_lines, opts.max_columns);
    let theme_name = resolve_theme_name(&opts.theme);
    let unknown_theme = theme_name.is_none();
    let theme_name = theme_name.unwrap_or("Dracula");
    let highlighted = highlight::highlight(&lines, lang, file_name, theme_name)?;
    let image = layout::draw(&highlighted, opts)?;
    Ok(Rendered {
        image,
        truncated,
        theme: theme_name.to_string(),
        unknown_theme,
    })
}

pub fn encode_png(img: &RgbaImage) -> Result<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png)?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_colors() {
        assert_eq!(Rgba::parse("#fff"), Some(Rgba(255, 255, 255, 255)));
        assert_eq!(Rgba::parse("#8B5CF6"), Some(Rgba(0x8B, 0x5C, 0xF6, 255)));
        assert_eq!(Rgba::parse("#00000080"), Some(Rgba(0, 0, 0, 0x80)));
        assert_eq!(Rgba::parse("red"), None);
    }

    #[test]
    fn parses_options() {
        let o: Options =
            serde_json::from_str(r##"{"background":["#000","#fff"],"padding":10}"##).unwrap();
        assert_eq!(
            o.background,
            Background::Gradient(Rgba(0, 0, 0, 255), Rgba(255, 255, 255, 255))
        );
        assert_eq!(o.padding, 10);
        assert_eq!(o.font_size, 16.0);
    }

    #[test]
    fn renders_rust() {
        let opts = Options {
            scale: 1.0,
            title: Some("main.rs".into()),
            line_numbers: true,
            ..Default::default()
        };
        let r = render(
            "fn main() {\n\tprintln!(\"hi\");\n}\n",
            Some("rust"),
            None,
            &opts,
        )
        .unwrap();
        assert!(!r.truncated);
        assert!(r.image.width() > 200 && r.image.height() > 100);
    }

    #[test]
    fn every_theme_renders() {
        let opts = Options {
            scale: 1.0,
            ..Default::default()
        };
        for t in available_themes() {
            let o = Options {
                theme: t.to_string(),
                ..opts.clone()
            };
            let r = render("let x = 1;", Some("js"), None, &o).unwrap();
            assert!(!r.unknown_theme, "{t}");
        }
    }

    #[test]
    fn caps_size() {
        let code = "x\n".repeat(1000);
        let opts = Options {
            scale: 1.0,
            max_lines: 10,
            ..Default::default()
        };
        let r = render(&code, None, None, &opts).unwrap();
        assert!(r.truncated);
    }
}
