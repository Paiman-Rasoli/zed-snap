//! Frame layout and rasterization: background, shadow, window, controls, text.

use std::sync::OnceLock;

use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};
use anyhow::{anyhow, Result};
use image::RgbaImage;
use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Paint, Path, PathBuilder, Pixmap, PixmapPaint,
    Point, SpreadMode, Stroke, Transform,
};

use crate::highlight::Highlighted;
use crate::{Background, Options, Rgba};

const REGULAR: &[u8] = include_bytes!("../fonts/JetBrainsMono-Regular.ttf");
const BOLD: &[u8] = include_bytes!("../fonts/JetBrainsMono-Bold.ttf");
const DOTS: [Rgba; 3] = [
    Rgba(0xFF, 0x5F, 0x56, 255),
    Rgba(0xFF, 0xBD, 0x2E, 255),
    Rgba(0x27, 0xC9, 0x3F, 255),
];

fn fonts() -> &'static (FontRef<'static>, FontRef<'static>) {
    static FONTS: OnceLock<(FontRef<'static>, FontRef<'static>)> = OnceLock::new();
    FONTS.get_or_init(|| {
        (
            FontRef::try_from_slice(REGULAR).expect("bundled regular font"),
            FontRef::try_from_slice(BOLD).expect("bundled bold font"),
        )
    })
}

fn color(c: Rgba) -> Color {
    Color::from_rgba8(c.0, c.1, c.2, c.3)
}

fn with_alpha(c: Rgba, a: u8) -> Rgba {
    Rgba(c.0, c.1, c.2, a)
}

fn is_dark(c: Rgba) -> bool {
    (0.299 * c.0 as f32 + 0.587 * c.1 as f32 + 0.114 * c.2 as f32) < 128.0
}

pub fn draw(hl: &Highlighted, opts: &Options) -> Result<RgbaImage> {
    let s = opts.scale.clamp(0.5, 4.0);
    let px = opts.font_size.clamp(6.0, 72.0) * s;
    let (regular, bold) = fonts();
    let font = regular.as_scaled(PxScale::from(px));
    let advance = font.h_advance(regular.glyph_id('M'));
    let line_h = (px * 1.5).round();

    let has_bar = opts.window_controls || opts.title.is_some();
    let pad = opts.padding as f32 * s;
    let bar_h = if has_bar { 44.0 * s } else { 0.0 };
    let inset_x = 24.0 * s;
    let code_top = if has_bar { bar_h + 4.0 * s } else { 24.0 * s };
    let code_bottom = 24.0 * s;

    let line_count = hl.lines.len();
    let last_number = opts.start_line.max(1) + line_count - 1;
    let gutter_w = if opts.line_numbers {
        (last_number.to_string().len() as f32 + 2.0) * advance
    } else {
        0.0
    };
    let max_cols = hl
        .lines
        .iter()
        .map(|l| l.iter().map(|sp| sp.text.chars().count()).sum::<usize>())
        .max()
        .unwrap_or(0);
    let code_w = max_cols as f32 * advance;

    let title_px = px * 0.85;
    let title_w = opts
        .title
        .as_deref()
        .map_or(0.0, |t| t.chars().count() as f32 * advance * 0.85);
    let min_w = (title_w + 2.0 * 84.0 * s).max(240.0 * s);

    let win_w = (inset_x * 2.0 + gutter_w + code_w).max(min_w).ceil();
    let win_h = (code_top + line_count as f32 * line_h + code_bottom).ceil();
    let canvas_w = (win_w + pad * 2.0).ceil() as u32;
    let canvas_h = (win_h + pad * 2.0).ceil() as u32;
    let (wx, wy) = (pad, pad);
    let radius = 12.0 * s;

    let mut pm = Pixmap::new(canvas_w, canvas_h)
        .ok_or_else(|| anyhow!("image too large: {canvas_w}x{canvas_h}"))?;

    // Background.
    match opts.background {
        Background::Transparent => {}
        Background::Solid(c) => pm.fill(color(c)),
        Background::Gradient(a, b) => {
            let shader = LinearGradient::new(
                Point::from_xy(0.0, 0.0),
                Point::from_xy(canvas_w as f32, canvas_h as f32),
                vec![
                    GradientStop::new(0.0, color(a)),
                    GradientStop::new(1.0, color(b)),
                ],
                SpreadMode::Pad,
                Transform::identity(),
            )
            .ok_or_else(|| anyhow!("invalid gradient"))?;
            let paint = Paint {
                shader,
                ..Default::default()
            };
            let rect =
                tiny_skia::Rect::from_xywh(0.0, 0.0, canvas_w as f32, canvas_h as f32).unwrap();
            pm.fill_rect(rect, &paint, Transform::identity(), None);
        }
    }

    let window =
        rounded_rect(wx, wy, win_w, win_h, radius).ok_or_else(|| anyhow!("bad window geometry"))?;

    // Drop shadow: blurred copy of the window shape, offset downwards.
    if opts.shadow && pad > 0.0 {
        let mut shadow = Pixmap::new(canvas_w, canvas_h).unwrap();
        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba8(0, 0, 0, 100));
        paint.anti_alias = true;
        shadow.fill_path(
            &window,
            &paint,
            FillRule::Winding,
            Transform::from_translate(0.0, 16.0 * s),
            None,
        );
        blur_alpha(&mut shadow, (22.0 * s).round() as usize);
        pm.draw_pixmap(
            0,
            0,
            shadow.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    }

    // Window body + hairline border.
    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };
    paint.set_color(color(hl.background));
    pm.fill_path(
        &window,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
    let border = if is_dark(hl.background) {
        Rgba(255, 255, 255, 30)
    } else {
        Rgba(0, 0, 0, 30)
    };
    paint.set_color(color(border));
    let stroke = Stroke {
        width: s.max(1.0),
        ..Default::default()
    };
    if let Some(inner) = rounded_rect(wx + 0.5 * s, wy + 0.5 * s, win_w - s, win_h - s, radius) {
        pm.stroke_path(&inner, &paint, &stroke, Transform::identity(), None);
    }

    // Traffic lights.
    if opts.window_controls {
        for (i, c) in DOTS.iter().enumerate() {
            let cx = wx + 22.0 * s + i as f32 * 20.0 * s;
            if let Some(circle) = PathBuilder::from_circle(cx, wy + bar_h / 2.0, 6.0 * s) {
                paint.set_color(color(*c));
                pm.fill_path(
                    &circle,
                    &paint,
                    FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
        }
    }

    // Title.
    if let Some(title) = opts.title.as_deref() {
        let tfont = regular.as_scaled(PxScale::from(title_px));
        let tw: f32 = title
            .chars()
            .map(|c| tfont.h_advance(regular.glyph_id(c)))
            .sum();
        let baseline = wy + bar_h / 2.0 + (tfont.ascent() + tfont.descent()) / 2.0;
        let fg = with_alpha(hl.foreground, 150);
        draw_text(
            &mut pm,
            regular,
            title_px,
            wx + (win_w - tw) / 2.0,
            baseline,
            title,
            fg,
        );
    }

    // Code.
    let text_ascent = font.ascent();
    let text_descent = font.descent();
    let number_color = with_alpha(hl.foreground, 90);
    for (i, line) in hl.lines.iter().enumerate() {
        let top = wy + code_top + i as f32 * line_h;
        let baseline = (top + (line_h + text_ascent + text_descent) / 2.0).round();
        if opts.line_numbers {
            let n = (opts.start_line.max(1) + i).to_string();
            let x = wx + inset_x + gutter_w - (n.len() as f32 + 2.0) * advance;
            draw_text(&mut pm, regular, px, x, baseline, &n, number_color);
        }
        let mut x = wx + inset_x + gutter_w;
        for span in line {
            let f = if span.bold { bold } else { regular };
            x = draw_text(&mut pm, f, px, x, baseline, &span.text, span.color);
        }
    }

    let mut img = RgbaImage::new(canvas_w, canvas_h);
    for (dst, src) in img.pixels_mut().zip(pm.pixels()) {
        let c = src.demultiply();
        *dst = image::Rgba([c.red(), c.green(), c.blue(), c.alpha()]);
    }
    Ok(img)
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w / 2.0).min(h / 2.0);
    let k = 0.552_284_8 * r;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

/// Draws `text` with its left edge at `x`; returns the x after the last glyph.
fn draw_text(
    pm: &mut Pixmap,
    font: &FontRef<'_>,
    px: f32,
    mut x: f32,
    baseline: f32,
    text: &str,
    c: Rgba,
) -> f32 {
    let scaled = font.as_scaled(PxScale::from(px));
    let (w, h) = (pm.width() as i32, pm.height() as i32);
    let data = pm.data_mut();
    for ch in text.chars() {
        let id = font.glyph_id(ch);
        let glyph = id.with_scale_and_position(px, point(x, baseline));
        x += scaled.h_advance(id);
        let Some(outline) = font.outline_glyph(glyph) else {
            continue;
        };
        let bounds = outline.px_bounds();
        outline.draw(|gx, gy, cov| {
            let px_x = bounds.min.x as i32 + gx as i32;
            let px_y = bounds.min.y as i32 + gy as i32;
            if px_x < 0 || px_y < 0 || px_x >= w || px_y >= h {
                return;
            }
            let a = cov.clamp(0.0, 1.0) * c.3 as f32 / 255.0;
            if a <= 0.0 {
                return;
            }
            let i = ((px_y * w + px_x) * 4) as usize;
            let inv = 1.0 - a;
            // Premultiplied source-over.
            data[i] = (c.0 as f32 * a + data[i] as f32 * inv).round() as u8;
            data[i + 1] = (c.1 as f32 * a + data[i + 1] as f32 * inv).round() as u8;
            data[i + 2] = (c.2 as f32 * a + data[i + 2] as f32 * inv).round() as u8;
            data[i + 3] = (255.0 * a + data[i + 3] as f32 * inv).round() as u8;
        });
    }
    x
}

/// Approximate gaussian blur of a black-only pixmap: three box-blur passes on alpha.
fn blur_alpha(pm: &mut Pixmap, radius: usize) {
    if radius == 0 {
        return;
    }
    let (w, h) = (pm.width() as usize, pm.height() as usize);
    let mut alpha: Vec<f32> = pm
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| p[3] as f32)
        .collect();
    let mut tmp = vec![0.0f32; alpha.len()];
    let r = (radius / 2).max(1);
    for _ in 0..3 {
        box_pass(&alpha, &mut tmp, w, h, r, true);
        box_pass(&tmp, &mut alpha, w, h, r, false);
    }
    for (p, a) in pm.data_mut().as_chunks_mut::<4>().0.iter_mut().zip(alpha) {
        p[0] = 0;
        p[1] = 0;
        p[2] = 0;
        p[3] = a.round().clamp(0.0, 255.0) as u8;
    }
}

fn box_pass(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize, horizontal: bool) {
    let (outer, inner) = if horizontal { (h, w) } else { (w, h) };
    let idx = |o: usize, i: usize| if horizontal { o * w + i } else { i * w + o };
    let norm = 1.0 / (2 * r + 1) as f32;
    for o in 0..outer {
        let mut sum = 0.0;
        for i in 0..=r.min(inner - 1) {
            sum += src[idx(o, i)];
        }
        for i in 0..inner {
            dst[idx(o, i)] = sum * norm;
            if i + r + 1 < inner {
                sum += src[idx(o, i + r + 1)];
            }
            if i >= r {
                sum -= src[idx(o, i - r)];
            }
        }
    }
}
