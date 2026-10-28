//! Watermarks and page numbers drawn over the pages of new PDFs.
//!
//! Texts are laid out by Typst (with the fonts of the computer, so any
//! language and script works), one tight page per text; [`crate::pdf`] then
//! places each page over the document's pages as a form XObject, drawn after
//! the page's own content. Positions follow the page as it is shown: its
//! visible area (crop box) and its final turn. Each document of a split has
//! its own numbers.

use std::collections::HashMap;
use std::path::Path;

use lopdf::{Dictionary, Stream, dictionary};

use crate::docx::typeset;
use crate::error::CoreError;
use crate::request::{Anchor, NumberFormat, PageNumbers, Watermark};

/// Distance of page numbers from the edges, in points (about 1 cm).
pub(crate) const NUMBER_MARGIN: f32 = 28.0;
/// Distance of a watermark from the edges when it is not centered.
pub(crate) const WATERMARK_MARGIN: f32 = 36.0;
/// Space kept free around a stamp that has to be made smaller to fit.
const FIT_MARGIN: f32 = 18.0;
/// Longest side of a watermark picture, in pixels.
const MAX_PICTURE: u32 = 2048;

/// The pages of a document (`count` pages) chosen by `text` such as
/// `"1-3, 5, 8-"`: `None` means every page. Pages beyond the document are
/// ignored (each document of a split has its own pages).
pub(crate) fn chosen(text: Option<&str>, count: u32) -> Result<Vec<bool>, CoreError> {
    let text = text.map(str::trim).unwrap_or_default();
    if text.is_empty() {
        return Ok(vec![true; count as usize]);
    }
    let invalid = |part: &str| CoreError::InvalidPages {
        details: format!("invalid page range \"{part}\""),
    };
    let mut chosen = vec![false; count as usize];
    for part in text
        .split([',', ';'])
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        let number = |s: &str| s.trim().parse::<u32>().map_err(|_| invalid(part));
        let (first, last) = match part.split_once('-') {
            Some((a, b)) => (
                if a.trim().is_empty() { 1 } else { number(a)? },
                if b.trim().is_empty() {
                    u32::MAX
                } else {
                    number(b)?
                },
            ),
            None => (number(part)?, number(part)?),
        };
        if first == 0 || first > last {
            return Err(invalid(part));
        }
        for page in first..=last.min(count) {
            chosen[page as usize - 1] = true;
        }
    }
    Ok(chosen)
}

/// The number shown on each page of a document of `count` pages (`None`:
/// no number there). Pages before `first` and skipped pages show none but
/// still count.
pub(crate) fn labels(numbers: &PageNumbers, count: u32) -> Result<Vec<Option<String>>, CoreError> {
    let skipped = match numbers.skip.as_deref().map(str::trim) {
        Some(skip) if !skip.is_empty() => chosen(Some(skip), count)?,
        _ => vec![false; count as usize],
    };
    let first = numbers.first.max(1);
    let total = numbers.start as i64 + count as i64 - first as i64;
    Ok((1..=count)
        .map(|position| {
            if position < first || skipped[position as usize - 1] {
                return None;
            }
            let n = numbers.start as i64 + (position - first) as i64;
            Some(match numbers.format {
                NumberFormat::Plain => n.to_string(),
                NumberFormat::OfTotal => format!("{n} / {total}"),
                NumberFormat::Word => match numbers.word.as_deref().map(str::trim) {
                    Some(word) if !word.is_empty() => format!("{word} {n}"),
                    _ => n.to_string(),
                },
            })
        })
        .collect())
}

/// The page ranges of a watermark and of page numbers are well written.
pub fn check(
    watermark: Option<&Watermark>,
    numbers: Option<&PageNumbers>,
) -> Result<(), CoreError> {
    if let Some(pages) = watermark.and_then(|w| w.pages.as_deref()) {
        chosen(Some(pages), 1)?;
    }
    if let Some(skip) = numbers.and_then(|n| n.skip.as_deref()) {
        chosen(Some(skip), 1)?;
    }
    Ok(())
}

/// `#rrggbb` as red, green and blue (gray if it is not one).
pub(crate) fn rgb(color: &str) -> [u8; 3] {
    let hex = color.trim().trim_start_matches('#');
    let channel = |i: usize| {
        hex.get(i..i + 2)
            .and_then(|c| u8::from_str_radix(c, 16).ok())
    };
    match (hex.len(), channel(0), channel(2), channel(4)) {
        (6, Some(r), Some(g), Some(b)) => [r, g, b],
        _ => [128, 128, 128],
    }
}

/// A text to lay out: its words, size in points and color.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Text {
    pub text: String,
    pub size: f32,
    pub color: [u8; 3],
}

/// Lays out `texts` with Typst: a PDF with one tight page per text, on a
/// transparent background.
pub(crate) fn typeset(texts: &[Text]) -> Result<Vec<u8>, CoreError> {
    let fonts = typeset::FALLBACK
        .iter()
        .map(|f| typeset::quote(f))
        .collect::<Vec<_>>()
        .join(", ");
    let mut source = format!(
        "#set page(width: auto, height: auto, margin: 0pt, fill: none)\n\
         #set text(font: ({fonts},), top-edge: \"ascender\", bottom-edge: \"descender\")\n"
    );
    for (i, text) in texts.iter().enumerate() {
        if i > 0 {
            source.push_str("#pagebreak()\n");
        }
        let [r, g, b] = text.color;
        source.push_str(&format!(
            "#text(size: {:.2}pt, fill: rgb({r},{g},{b}), {})\n",
            text.size.clamp(4.0, 400.0),
            typeset::quote(&text.text)
        ));
    }
    typeset::compile(source, HashMap::new())
}

/// An affine map of the plane, as PDF writes it: `[a b c d e f]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Affine(pub [f32; 6]);

impl Affine {
    #[cfg(test)]
    pub(crate) const IDENTITY: Affine = Affine([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    pub(crate) fn translate(x: f32, y: f32) -> Self {
        Affine([1.0, 0.0, 0.0, 1.0, x, y])
    }

    pub(crate) fn scale(x: f32, y: f32) -> Self {
        Affine([x, 0.0, 0.0, y, 0.0, 0.0])
    }

    /// A counterclockwise turn.
    pub(crate) fn rotate(degrees: f32) -> Self {
        let (sin, cos) = degrees.to_radians().sin_cos();
        Affine([cos, sin, -sin, cos, 0.0, 0.0])
    }

    /// This map, then `next`.
    pub(crate) fn then(self, next: Affine) -> Affine {
        let [a, b, c, d, e, f] = self.0;
        let [na, nb, nc, nd, ne, nf] = next.0;
        Affine([
            na * a + nc * b,
            nb * a + nd * b,
            na * c + nc * d,
            nb * c + nd * d,
            na * e + nc * f + ne,
            nb * e + nd * f + nf,
        ])
    }

    #[cfg(test)]
    pub(crate) fn apply(self, x: f32, y: f32) -> (f32, f32) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + c * y + e, b * x + d * y + f)
    }

    /// The `cm` operator setting this map.
    pub(crate) fn cm(self) -> String {
        let [a, b, c, d, e, f] = self.0;
        format!("{a:.5} {b:.5} {c:.5} {d:.5} {e:.3} {f:.3} cm")
    }
}

/// How a page is shown: from the visible area `crop` (`[x0 y0 x1 y1]`, in
/// the page's own coordinates) turned clockwise by `rotate` degrees. Gives
/// the map from the shown page (origin at its bottom left) to the page's
/// coordinates, and the size of the shown page.
pub(crate) fn shown_page(crop: [f32; 4], rotate: i64) -> (Affine, (f32, f32)) {
    let (x0, y0) = (crop[0].min(crop[2]), crop[1].min(crop[3]));
    let (w, h) = ((crop[2] - crop[0]).abs(), (crop[3] - crop[1]).abs());
    match rotate.rem_euclid(360) {
        90 => (Affine([0.0, 1.0, -1.0, 0.0, x0 + w, y0]), (h, w)),
        180 => (Affine([-1.0, 0.0, 0.0, -1.0, x0 + w, y0 + h]), (w, h)),
        270 => (Affine([0.0, -1.0, 1.0, 0.0, x0, y0 + h]), (h, w)),
        _ => (Affine([1.0, 0.0, 0.0, 1.0, x0, y0]), (w, h)),
    }
}

/// Where a stamp of `size` (its box from the origin) goes on a shown page of
/// `page` size: at `anchor`, `margin` away from the edges, turned
/// counterclockwise by `angle` degrees around its center (the turned box
/// keeps its distance from the edges). A stamp larger than the page is made
/// smaller to fit it.
pub(crate) fn place(
    anchor: Anchor,
    page: (f32, f32),
    size: (f32, f32),
    margin: f32,
    angle: f32,
) -> Affine {
    let (sin, cos) = angle.to_radians().sin_cos();
    let (w, h) = size;
    let turned = (w * cos.abs() + h * sin.abs(), w * sin.abs() + h * cos.abs());
    let room = margin.max(FIT_MARGIN);
    let fit = ((page.0 - 2.0 * room) / turned.0)
        .min((page.1 - 2.0 * room) / turned.1)
        .clamp(0.01, 1.0);
    let turned = (turned.0 * fit, turned.1 * fit);
    let x = match anchor {
        Anchor::TopLeft | Anchor::Left | Anchor::BottomLeft => margin + turned.0 / 2.0,
        Anchor::Top | Anchor::Center | Anchor::Bottom => page.0 / 2.0,
        Anchor::TopRight | Anchor::Right | Anchor::BottomRight => page.0 - margin - turned.0 / 2.0,
    };
    let y = match anchor {
        Anchor::TopLeft | Anchor::Top | Anchor::TopRight => page.1 - margin - turned.1 / 2.0,
        Anchor::Left | Anchor::Center | Anchor::Right => page.1 / 2.0,
        Anchor::BottomLeft | Anchor::Bottom | Anchor::BottomRight => margin + turned.1 / 2.0,
    };
    Affine::translate(-w / 2.0, -h / 2.0)
        .then(Affine::scale(fit, fit))
        .then(Affine::rotate(angle))
        .then(Affine::translate(x, y))
}

/// A watermark picture: the image (and its transparency, if any) as PDF
/// streams, with its size in pixels.
pub(crate) struct Picture {
    pub image: Stream,
    pub mask: Option<Stream>,
    pub width: u32,
    pub height: u32,
}

/// Reads a PNG or JPEG picture for a watermark (at most 2048 pixels a side).
pub(crate) fn picture(path: &Path) -> Result<Picture, CoreError> {
    let data = std::fs::read(path)?;
    let decoded = image::load_from_memory(&data).map_err(|e| CoreError::CorruptInput {
        details: format!("watermark picture: {e}"),
    })?;
    let decoded = if decoded.width().max(decoded.height()) > MAX_PICTURE {
        decoded.resize(
            MAX_PICTURE,
            MAX_PICTURE,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        decoded
    };
    let rgba = decoded.to_rgba8();
    let (width, height) = rgba.dimensions();
    let rgb: Vec<u8> = rgba
        .pixels()
        .flat_map(|p| [p.0[0], p.0[1], p.0[2]])
        .collect();
    let alpha: Vec<u8> = rgba.pixels().map(|p| p.0[3]).collect();
    let stream = |dict: Dictionary, data: Vec<u8>| {
        let mut stream = Stream::new(dict, data);
        let _ = stream.compress();
        stream
    };
    let image_dict = |space: &str| {
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => width as i64,
            "Height" => height as i64, "ColorSpace" => space, "BitsPerComponent" => 8,
        }
    };
    Ok(Picture {
        image: stream(image_dict("DeviceRGB"), rgb),
        mask: alpha
            .iter()
            .any(|&a| a < 255)
            .then(|| stream(image_dict("DeviceGray"), alpha)),
        width,
        height,
    })
}

/// A watermark or page numbers to draw: the texts to lay out.
pub(crate) fn texts(
    watermark: Option<&Watermark>,
    labels: &[String],
    numbers: Option<&PageNumbers>,
) -> Vec<Text> {
    let mut texts = Vec::new();
    if let Some(w) = watermark.filter(|w| w.picture.is_none() && !w.text.trim().is_empty()) {
        texts.push(Text {
            text: w.text.trim().to_string(),
            size: w.size,
            color: rgb(&w.color),
        });
    }
    if let Some(numbers) = numbers {
        texts.extend(labels.iter().map(|label| Text {
            text: label.clone(),
            size: numbers.size,
            color: [0, 0, 0],
        }));
    }
    texts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: (f32, f32), b: (f32, f32)) -> bool {
        (a.0 - b.0).abs() < 0.01 && (a.1 - b.1).abs() < 0.01
    }

    #[test]
    fn pages_are_chosen_within_each_document() {
        assert_eq!(chosen(None, 3).unwrap(), [true; 3]);
        assert_eq!(chosen(Some(" "), 2).unwrap(), [true; 2]);
        assert_eq!(chosen(Some("1, 3-"), 4).unwrap(), [true, false, true, true]);
        // A page beyond this document is not an error: another one may have it.
        assert_eq!(chosen(Some("2, 9"), 3).unwrap(), [false, true, false]);
        for wrong in ["0", "3-2", "a", "1-x"] {
            assert_eq!(
                chosen(Some(wrong), 5).unwrap_err().code(),
                "invalid_pages",
                "{wrong}"
            );
        }
    }

    #[test]
    fn numbers_start_where_asked_and_skipped_pages_still_count() {
        let numbers = PageNumbers {
            first: 2,
            start: 1,
            skip: Some("4".into()),
            format: NumberFormat::OfTotal,
            ..PageNumbers::default()
        };
        assert_eq!(
            labels(&numbers, 5).unwrap(),
            [
                None,
                Some("1 / 4".into()),
                Some("2 / 4".into()),
                None,
                Some("4 / 4".into())
            ]
        );
        let word = PageNumbers {
            format: NumberFormat::Word,
            word: Some("Pagina".into()),
            start: 10,
            ..PageNumbers::default()
        };
        assert_eq!(
            labels(&word, 2).unwrap(),
            [Some("Pagina 10".into()), Some("Pagina 11".into())]
        );
        assert_eq!(
            labels(&PageNumbers::default(), 2).unwrap(),
            [Some("1".into()), Some("2".into())]
        );
        assert_eq!(rgb("#FF8000"), [255, 128, 0]);
        assert_eq!(rgb("rosso"), [128, 128, 128]);
    }

    #[test]
    fn shown_pages_map_back_to_the_page() {
        let crop = [10.0, 20.0, 210.0, 120.0];
        // Not turned: the shown page is the crop box.
        let (map, size) = shown_page(crop, 0);
        assert_eq!(size, (200.0, 100.0));
        assert!(close(map.apply(0.0, 0.0), (10.0, 20.0)));
        // Turned clockwise a quarter: shown taller than wide; its bottom
        // left corner is the page's bottom right one.
        let (map, size) = shown_page(crop, 90);
        assert_eq!(size, (100.0, 200.0));
        assert!(close(map.apply(0.0, 0.0), (210.0, 20.0)));
        assert!(close(map.apply(0.0, 200.0), (10.0, 20.0)));
        // Half a turn and three quarters.
        assert!(close(
            shown_page(crop, 180).0.apply(0.0, 0.0),
            (210.0, 120.0)
        ));
        let (map, _) = shown_page(crop, 270);
        assert!(close(map.apply(0.0, 0.0), (10.0, 120.0)));
        assert!(close(map.apply(100.0, 0.0), (10.0, 20.0)));
    }

    #[test]
    fn stamps_are_placed_at_their_anchor() {
        let page = (600.0, 800.0);
        // A number at the bottom center: 28 points from the bottom edge.
        let map = place(Anchor::Bottom, page, (20.0, 10.0), NUMBER_MARGIN, 0.0);
        assert!(close(map.apply(0.0, 0.0), (290.0, 28.0)));
        let map = place(Anchor::TopRight, page, (20.0, 10.0), NUMBER_MARGIN, 0.0);
        assert!(close(map.apply(20.0, 10.0), (572.0, 772.0)));
        // A centered text turned 90 degrees: its center stays at the center.
        let map = place(Anchor::Center, page, (200.0, 50.0), 0.0, 90.0);
        assert!(close(map.apply(100.0, 25.0), (300.0, 400.0)));
        assert!(close(map.apply(0.0, 0.0), (325.0, 300.0)));
        // Too long for the page: made smaller, still centered.
        let map = place(Anchor::Center, page, (1200.0, 60.0), 0.0, 0.0);
        assert!(close(map.apply(0.0, 30.0), (18.0, 400.0)));
        assert!(close(map.apply(1200.0, 30.0), (582.0, 400.0)));
        let both = Affine::scale(2.0, 3.0).then(Affine::translate(1.0, 1.0));
        assert!(close(both.apply(1.0, 1.0), (3.0, 4.0)));
        assert_eq!(
            Affine::IDENTITY.cm(),
            "1.00000 0.00000 0.00000 1.00000 0.000 0.000 cm"
        );
    }
}
