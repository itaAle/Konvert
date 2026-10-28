//! Text recognition (OCR) of scanned pages and photos of documents, entirely
//! on this computer.
//!
//! The PaddleOCR PP-OCRv5 models (text detection, then recognition of text in
//! Latin script) are ONNX files bundled with the app (`scripts/setup-ocr.mjs`),
//! run by rten, a pure Rust inference engine.
//!
//! - Detection: the picture, at most 1600 pixels on its long side (in
//!   multiples of 32), gives a map of text probability. Pixels above 0.3 are
//!   grouped into connected regions; a region with a mean score of at least
//!   0.6 becomes a box along its own direction (the main axis of its pixels),
//!   grown like PaddleOCR's "unclip" (ratio 1.5).
//! - Recognition: each box is sampled along its direction into a straight
//!   line picture 48 pixels high, read by greedy CTC decoding with the
//!   model's dictionary. Lines read with less than 50% confidence are dropped.
//! - Pages scanned sideways or upside down: when most of the text runs
//!   vertically, the page is read again turned a quarter both ways; when the
//!   reading is doubtful, turned upside down. The most confident reading wins.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use image::GrayImage;
use rten::Model;
use rten_tensor::NdTensor;
use rten_tensor::prelude::*;

use crate::error::CoreError;

/// Files of the OCR models, in the folder given to [`Ocr::load`].
pub const MODEL_FILES: [&str; 3] = ["det.onnx", "rec.onnx", "dict.txt"];

/// Longest side of the picture given to the detection model.
const DETECTION_SIDE: f32 = 1600.0;
/// Height of the line pictures given to the recognition model.
const LINE_HEIGHT: u32 = 48;
const MAX_LINE_WIDTH: u32 = 3200;
/// Text probability of a pixel that belongs to text.
const PIXEL_THRESHOLD: f32 = 0.3;
/// Mean text probability of a region kept as a line.
const BOX_THRESHOLD: f32 = 0.6;
const UNCLIP_RATIO: f32 = 1.5;
/// Lines read with less confidence are dropped.
const MIN_CONFIDENCE: f32 = 0.5;
/// Mean confidence below which an upright page is also read upside down.
const DOUBTFUL: f32 = 0.8;

/// The OCR models, loaded once and used for any number of pictures.
pub struct Ocr {
    det: Model,
    rec: Model,
    /// Characters of the recognition model (class `i + 1`; the last class is
    /// a space, class 0 is "no character").
    dict: Vec<String>,
}

impl std::fmt::Debug for Ocr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Ocr({} characters)", self.dict.len())
    }
}

/// One line of text read in a picture. Positions are in pixels from the top
/// left of the picture, y downwards.
#[derive(Debug, Clone, PartialEq)]
pub struct TextLine {
    pub text: String,
    /// Mean probability of the characters read (0 to 1).
    pub confidence: f32,
    /// Center of the line.
    pub cx: f32,
    pub cy: f32,
    /// Direction of the text (unit vector): (1, 0) for an upright line.
    pub ux: f32,
    pub uy: f32,
    /// Half length along the text and half height across it.
    pub half_len: f32,
    pub half_h: f32,
}

impl TextLine {
    /// Corners: start of the text at the top and bottom, end at the bottom
    /// and top (for an upright line: top left, bottom left, bottom right,
    /// top right).
    pub fn corners(&self) -> [(f32, f32); 4] {
        let (ax, ay) = (self.ux * self.half_len, self.uy * self.half_len);
        // Across the text, towards its bottom.
        let (bx, by) = (-self.uy * self.half_h, self.ux * self.half_h);
        [
            (self.cx - ax - bx, self.cy - ay - by),
            (self.cx - ax + bx, self.cy - ay + by),
            (self.cx + ax + bx, self.cy + ay + by),
            (self.cx + ax - bx, self.cy + ay - by),
        ]
    }
}

/// A quarter turn of the picture, clockwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Turn {
    None,
    Quarter,
    Half,
    ThreeQuarters,
}

impl Turn {
    fn apply(self, picture: &GrayImage) -> GrayImage {
        match self {
            Turn::None => picture.clone(),
            Turn::Quarter => image::imageops::rotate90(picture),
            Turn::Half => image::imageops::rotate180(picture),
            Turn::ThreeQuarters => image::imageops::rotate270(picture),
        }
    }

    /// Takes a line read in the turned picture back to the picture of
    /// `width`×`height` pixels.
    fn undo(self, line: &mut TextLine, width: f32, height: f32) {
        let (x, y, ux, uy) = (line.cx, line.cy, line.ux, line.uy);
        (line.cx, line.cy, line.ux, line.uy) = match self {
            Turn::None => (x, y, ux, uy),
            Turn::Quarter => (y, height - x, uy, -ux),
            Turn::Half => (width - x, height - y, -ux, -uy),
            Turn::ThreeQuarters => (width - y, x, -uy, ux),
        };
    }
}

impl Ocr {
    /// Loads the models from `dir` (see [`MODEL_FILES`]).
    pub fn load(dir: &Path) -> Result<Self, CoreError> {
        if MODEL_FILES.iter().any(|f| !dir.join(f).is_file()) {
            return Err(CoreError::EngineMissing);
        }
        let model = |name: &str| {
            Model::load_file(dir.join(name)).map_err(|e| CoreError::EngineFailed {
                details: format!("OCR model {name}: {e}"),
            })
        };
        let dict: Vec<String> = std::fs::read_to_string(dir.join(MODEL_FILES[2]))?
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect();
        Ok(Self {
            det: model(MODEL_FILES[0])?,
            rec: model(MODEL_FILES[1])?,
            dict,
        })
    }

    /// Reads the lines of text of a picture, in no particular order.
    /// Blocking (about a second for a page at 300 dpi): run it on a worker
    /// thread.
    pub fn read(
        &self,
        picture: &GrayImage,
        cancel: &AtomicBool,
    ) -> Result<Vec<TextLine>, CoreError> {
        if picture.width() < 8 || picture.height() < 8 {
            return Ok(Vec::new());
        }
        let mut best = self.read_turned(picture, Turn::None, cancel)?;
        if best.is_empty() {
            return Ok(best);
        }
        let total: f32 = best.iter().map(|l| l.half_len).sum();
        let vertical: f32 = best
            .iter()
            .filter(|l| l.uy.abs() > l.ux.abs())
            .map(|l| l.half_len)
            .sum();
        let others: &[Turn] = if vertical > total / 2.0 {
            &[Turn::Quarter, Turn::ThreeQuarters]
        } else if score(&best) < DOUBTFUL {
            &[Turn::Half]
        } else {
            &[]
        };
        for &turn in others {
            let lines = self.read_turned(picture, turn, cancel)?;
            if score(&lines) > score(&best) {
                best = lines;
            }
        }
        Ok(best)
    }

    fn read_turned(
        &self,
        picture: &GrayImage,
        turn: Turn,
        cancel: &AtomicBool,
    ) -> Result<Vec<TextLine>, CoreError> {
        let turned = turn.apply(picture);
        let mut lines = Vec::new();
        for found in self.detect(&turned)? {
            if cancel.load(Ordering::Relaxed) {
                return Err(CoreError::Cancelled);
            }
            let Some((text, confidence)) = self.recognize(&straightened(&turned, &found))? else {
                continue;
            };
            let mut line = TextLine {
                text,
                confidence,
                ..found
            };
            turn.undo(&mut line, picture.width() as f32, picture.height() as f32);
            lines.push(line);
        }
        Ok(lines)
    }

    /// Boxes of text (with no text yet).
    fn detect(&self, picture: &GrayImage) -> Result<Vec<TextLine>, CoreError> {
        let (width, height) = (picture.width() as f32, picture.height() as f32);
        let scale = (DETECTION_SIDE / width.max(height)).min(1.0);
        let dw = ((width * scale / 32.0).round() as usize).max(1) * 32;
        let dh = ((height * scale / 32.0).round() as usize).max(1) * 32;
        let small = image::imageops::resize(
            picture,
            dw as u32,
            dh as u32,
            image::imageops::FilterType::Triangle,
        );
        // Gray in the three channels, normalized like ImageNet.
        let mean = [0.485f32, 0.456, 0.406];
        let std = [0.229f32, 0.224, 0.225];
        let mut data = Vec::with_capacity(3 * dh * dw);
        for c in 0..3 {
            data.extend(
                small
                    .pixels()
                    .map(|p| (f32::from(p.0[0]) / 255.0 - mean[c]) / std[c]),
            );
        }
        let input = NdTensor::from_data([1, 3, dh, dw], data);
        let map: NdTensor<f32, 4> = self
            .det
            .run_one(input.view().into(), None)
            .map_err(failed)?
            .try_into()
            .map_err(failed)?;
        let [_, _, mh, mw] = map.shape();
        if (mh, mw) != (dh, dw) {
            return Err(failed("unexpected size of the detection map"));
        }
        Ok(find_boxes(
            &map.to_vec(),
            dw,
            dh,
            width / dw as f32,
            height / dh as f32,
        ))
    }

    /// The text of a straight line picture and its confidence, if any.
    fn recognize(&self, line: &GrayImage) -> Result<Option<(String, f32)>, CoreError> {
        let width = ((LINE_HEIGHT as f32 * line.width() as f32 / line.height().max(1) as f32)
            .round() as u32)
            .clamp(8, MAX_LINE_WIDTH);
        let line = image::imageops::resize(
            line,
            width,
            LINE_HEIGHT,
            image::imageops::FilterType::Triangle,
        );
        let mut data = Vec::with_capacity(3 * (LINE_HEIGHT * width) as usize);
        for _ in 0..3 {
            data.extend(
                line.pixels()
                    .map(|p| (f32::from(p.0[0]) / 255.0 - 0.5) / 0.5),
            );
        }
        let input = NdTensor::from_data([1, 3, LINE_HEIGHT as usize, width as usize], data);
        let probs: NdTensor<f32, 3> = self
            .rec
            .run_one(input.view().into(), None)
            .map_err(failed)?
            .try_into()
            .map_err(failed)?;
        let [_, steps, classes] = probs.shape();
        let (text, confidence) = decode(&probs.to_vec(), steps, classes, &self.dict);
        let text = text.trim().to_string();
        Ok((!text.is_empty() && confidence >= MIN_CONFIDENCE).then_some((text, confidence)))
    }
}

fn failed(error: impl std::fmt::Display) -> CoreError {
    CoreError::EngineFailed {
        details: format!("OCR: {error}"),
    }
}

/// Confidence of a reading, weighted by the length of the lines.
fn score(lines: &[TextLine]) -> f32 {
    let total: f32 = lines.iter().map(|l| l.half_len).sum();
    if total <= 0.0 {
        return 0.0;
    }
    lines.iter().map(|l| l.confidence * l.half_len).sum::<f32>() / total
}

/// Greedy CTC decoding of `steps`×`classes` probabilities: the most likely
/// class of each step, repeats merged, class 0 ("no character") skipped.
/// Gives the text and the mean probability of its characters.
fn decode(probs: &[f32], steps: usize, classes: usize, dict: &[String]) -> (String, f32) {
    let mut text = String::new();
    let (mut sum, mut count) = (0.0f32, 0usize);
    let mut last = 0usize;
    for step in probs.chunks_exact(classes).take(steps) {
        let (best, p) = step
            .iter()
            .copied()
            .enumerate()
            .fold((0, f32::MIN), |a, (c, p)| if p > a.1 { (c, p) } else { a });
        if best != 0 && best != last {
            match dict.get(best - 1) {
                Some(c) => text.push_str(c),
                None => text.push(' '),
            }
            sum += p;
            count += 1;
        }
        last = best;
    }
    (text, if count == 0 { 0.0 } else { sum / count as f32 })
}

/// Lines of text in a `w`×`h` probability map: regions of connected pixels
/// above the threshold, as boxes along their main axis grown like
/// PaddleOCR's unclip, in picture pixels (`sx`, `sy` pixels per map pixel).
fn find_boxes(map: &[f32], w: usize, h: usize, sx: f32, sy: f32) -> Vec<TextLine> {
    let mut seen = vec![false; w * h];
    let mut boxes = Vec::new();
    let mut stack = Vec::new();
    let mut pixels: Vec<(f32, f32)> = Vec::new();
    for start in 0..w * h {
        if seen[start] || map[start] <= PIXEL_THRESHOLD {
            continue;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
        let mut sum = 0.0f32;
        pixels.clear();
        stack.push(start);
        seen[start] = true;
        while let Some(i) = stack.pop() {
            let (y, x) = (i / w, i % w);
            sum += map[i];
            pixels.push(((x as f32 + 0.5) * sx, (y as f32 + 0.5) * sy));
            (x0, x1, y0, y1) = (x0.min(x), x1.max(x), y0.min(y), y1.max(y));
            let neighbours = [
                (x > 0).then(|| i - 1),
                (x + 1 < w).then(|| i + 1),
                (y > 0).then(|| i - w),
                (y + 1 < h).then(|| i + w),
            ];
            for j in neighbours.into_iter().flatten() {
                if !seen[j] && map[j] > PIXEL_THRESHOLD {
                    seen[j] = true;
                    stack.push(j);
                }
            }
        }
        if sum / (pixels.len() as f32) < BOX_THRESHOLD || x1 - x0 < 2 || y1 - y0 < 2 {
            continue;
        }
        // Direction of the text: main axis of the region's pixels.
        let n = pixels.len() as f32;
        let (mx, my) = pixels
            .iter()
            .fold((0.0, 0.0), |a, p| (a.0 + p.0 / n, a.1 + p.1 / n));
        let (mut cxx, mut cyy, mut cxy) = (0.0f32, 0.0f32, 0.0f32);
        for p in &pixels {
            cxx += (p.0 - mx) * (p.0 - mx);
            cyy += (p.1 - my) * (p.1 - my);
            cxy += (p.0 - mx) * (p.1 - my);
        }
        let angle = 0.5 * (2.0 * cxy).atan2(cxx - cyy);
        let (mut ux, mut uy) = (angle.cos(), angle.sin());
        let (vx, vy) = (-uy, ux);
        let (mut umin, mut umax, mut vmin, mut vmax) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for p in &pixels {
            let (dx, dy) = (p.0 - mx, p.1 - my);
            let (u, v) = (dx * ux + dy * uy, dx * vx + dy * vy);
            (umin, umax) = (umin.min(u), umax.max(u));
            (vmin, vmax) = (vmin.min(v), vmax.max(v));
        }
        let (cu, cv) = ((umin + umax) / 2.0, (vmin + vmax) / 2.0);
        let (cx, cy) = (mx + cu * ux + cv * vx, my + cu * uy + cv * vy);
        // Each pixel stands for a whole map pixel.
        let (mut len, mut height) = (umax - umin + sx, vmax - vmin + sy);
        // A box hardly longer than high (a letter or two) is read across
        // the page, not down it.
        if len < 1.5 * height && uy.abs() > ux.abs() {
            (ux, uy) = (vx.abs(), vy * vx.signum());
            (len, height) = (height, len);
        }
        let grow = len * height * UNCLIP_RATIO / (2.0 * (len + height));
        boxes.push(TextLine {
            text: String::new(),
            confidence: 0.0,
            cx,
            cy,
            ux,
            uy,
            half_len: len / 2.0 + grow,
            half_h: height / 2.0 + grow,
        });
    }
    boxes
}

/// The line's box sampled along its direction (bilinear) into a straight
/// picture; outside the picture is white.
fn straightened(picture: &GrayImage, line: &TextLine) -> GrayImage {
    let width = (2.0 * line.half_len).max(1.0) as u32;
    let height = (2.0 * line.half_h).max(1.0) as u32;
    let (pw, ph) = (picture.width() as i64, picture.height() as i64);
    let at = |x: i64, y: i64| -> f32 {
        if x < 0 || y < 0 || x >= pw || y >= ph {
            255.0
        } else {
            f32::from(picture.get_pixel(x as u32, y as u32).0[0])
        }
    };
    let sample = |x: f32, y: f32| -> u8 {
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let v = at(x0, y0) * (1.0 - fx) * (1.0 - fy)
            + at(x0 + 1, y0) * fx * (1.0 - fy)
            + at(x0, y0 + 1) * (1.0 - fx) * fy
            + at(x0 + 1, y0 + 1) * fx * fy;
        v.round().clamp(0.0, 255.0) as u8
    };
    let (vx, vy) = (-line.uy, line.ux);
    GrayImage::from_fn(width, height, |i, j| {
        // Pixel centers along and across the line.
        let a = i as f32 + 0.5 - width as f32 / 2.0;
        let c = j as f32 + 0.5 - height as f32 / 2.0;
        image::Luma([sample(
            line.cx + a * line.ux + c * vx - 0.5,
            line.cy + a * line.uy + c * vy - 0.5,
        )])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(cx: f32, cy: f32, ux: f32, uy: f32) -> TextLine {
        TextLine {
            text: String::new(),
            confidence: 1.0,
            cx,
            cy,
            ux,
            uy,
            half_len: 40.0,
            half_h: 6.0,
        }
    }

    #[test]
    fn ctc_decoding_merges_repeats_and_skips_blanks() {
        let dict: Vec<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
        // Classes: blank, a, b, space.
        let steps = [
            [0.1, 0.8, 0.05, 0.05],
            [0.1, 0.7, 0.1, 0.1],
            [0.9, 0.05, 0.03, 0.02],
            [0.1, 0.6, 0.2, 0.1],
            [0.0, 0.0, 0.1, 0.9],
            [0.1, 0.1, 0.8, 0.0],
            [0.95, 0.0, 0.05, 0.0],
        ];
        let probs: Vec<f32> = steps.iter().flatten().copied().collect();
        let (text, confidence) = decode(&probs, steps.len(), 4, &dict);
        assert_eq!(text, "aa b");
        assert!((confidence - (0.8 + 0.6 + 0.9 + 0.8) / 4.0).abs() < 1e-6);
        assert_eq!(
            decode(&[1.0, 0.0, 0.0, 0.0], 1, 4, &dict),
            (String::new(), 0.0)
        );
    }

    #[test]
    fn regions_become_boxes_along_the_text() {
        // A 200×100 map with a bar 120 long and 6 high, turned by 10°.
        let (w, h) = (200usize, 100usize);
        let angle = 10f32.to_radians();
        let (ux, uy) = (angle.cos(), angle.sin());
        let mut map = vec![0.0f32; w * h];
        for y in 0..h {
            for x in 0..w {
                let (dx, dy) = (x as f32 + 0.5 - 100.0, y as f32 + 0.5 - 50.0);
                let (u, v) = (dx * ux + dy * uy, -dx * uy + dy * ux);
                if u.abs() <= 60.0 && v.abs() <= 3.0 {
                    map[y * w + x] = 0.9;
                }
            }
        }
        // A faint region is not text.
        map[5 * w + 5..5 * w + 15].fill(0.4);
        map[6 * w + 5..6 * w + 15].fill(0.4);
        map[7 * w + 5..7 * w + 15].fill(0.4);
        let boxes = find_boxes(&map, w, h, 2.0, 2.0);
        assert_eq!(boxes.len(), 1);
        let b = &boxes[0];
        assert!(
            (b.cx - 200.0).abs() < 2.0 && (b.cy - 100.0).abs() < 2.0,
            "{b:?}"
        );
        assert!((b.uy.atan2(b.ux) - angle).abs() < 0.02, "{b:?}");
        // 240 × 14 picture pixels, grown on every side.
        let grow = 242.0 * 14.0 * UNCLIP_RATIO / (2.0 * 256.0);
        assert!((b.half_len - (121.0 + grow)).abs() < 3.0, "{b:?}");
        assert!((b.half_h - (7.0 + grow)).abs() < 3.0, "{b:?}");
    }

    #[test]
    fn single_letters_are_read_across_the_page() {
        // A letter 8 wide and 11 high.
        let (w, h) = (40usize, 40usize);
        let mut map = vec![0.0f32; w * h];
        for y in 10..21 {
            map[y * w + 16..y * w + 24].fill(0.9);
        }
        let boxes = find_boxes(&map, w, h, 1.0, 1.0);
        assert_eq!(boxes.len(), 1);
        let b = &boxes[0];
        assert!((b.ux - 1.0).abs() < 1e-3 && b.uy.abs() < 1e-3, "{b:?}");
        assert!(b.half_len < b.half_h, "{b:?}");
        assert!(
            (b.cx - 20.0).abs() < 0.5 && (b.cy - 15.5).abs() < 0.5,
            "{b:?}"
        );
    }

    #[test]
    fn turned_lines_are_straightened() {
        // A dark bar 100 long and 10 high turned by 30°, on white.
        let angle = 30f32.to_radians();
        let (ux, uy) = (angle.cos(), angle.sin());
        let picture = GrayImage::from_fn(200, 200, |x, y| {
            let (dx, dy) = (x as f32 + 0.5 - 100.0, y as f32 + 0.5 - 100.0);
            let (u, v) = (dx * ux + dy * uy, -dx * uy + dy * ux);
            image::Luma([if u.abs() <= 50.0 && v.abs() <= 5.0 {
                0
            } else {
                255
            }])
        });
        let mut found = line(100.0, 100.0, ux, uy);
        found.half_len = 60.0;
        found.half_h = 10.0;
        let straight = straightened(&picture, &found);
        assert_eq!(straight.dimensions(), (120, 20));
        // Dark in the middle, white around it.
        let dark = |x: u32, y: u32| straight.get_pixel(x, y).0[0] < 64;
        assert!((15..105).all(|x| (7..13).all(|y| dark(x, y))));
        assert!((0..120).all(|x| !dark(x, 1) && !dark(x, 18)));
        assert!((0..20).all(|y| !dark(3, y) && !dark(116, y)));
    }

    #[test]
    fn lines_read_in_a_turned_picture_come_back_to_the_picture() {
        // A picture 300×200 with one dark pixel; each turn moves it, and the
        // line found there comes back to it.
        let mut picture = GrayImage::from_pixel(300, 200, image::Luma([255]));
        picture.put_pixel(40, 30, image::Luma([0]));
        for turn in [Turn::None, Turn::Quarter, Turn::Half, Turn::ThreeQuarters] {
            let turned = turn.apply(&picture);
            let (x, y) = turned
                .enumerate_pixels()
                .find(|p| p.2.0[0] == 0)
                .map(|p| (p.0 as f32 + 0.5, p.1 as f32 + 0.5))
                .unwrap();
            let mut found = line(x, y, 1.0, 0.0);
            turn.undo(&mut found, 300.0, 200.0);
            assert!(
                (found.cx - 40.5).abs() < 1e-3 && (found.cy - 30.5).abs() < 1e-3,
                "{turn:?} {found:?}"
            );
        }
        // Text read left to right in a picture turned a quarter clockwise
        // runs upwards in the picture.
        let mut found = line(10.0, 10.0, 1.0, 0.0);
        Turn::Quarter.undo(&mut found, 300.0, 200.0);
        assert_eq!((found.ux, found.uy), (0.0, -1.0));
        let mut found = line(10.0, 10.0, 1.0, 0.0);
        Turn::Half.undo(&mut found, 300.0, 200.0);
        assert_eq!((found.ux, found.uy), (-1.0, 0.0));
    }

    #[test]
    fn corners_go_around_the_line() {
        let corners = line(100.0, 50.0, 1.0, 0.0).corners();
        assert_eq!(
            corners,
            [(60.0, 44.0), (60.0, 56.0), (140.0, 56.0), (140.0, 44.0)]
        );
    }

    #[test]
    fn missing_models_are_reported() {
        let dir = std::env::temp_dir().join("konvert-test-no-ocr-models-here");
        assert!(matches!(Ocr::load(&dir), Err(CoreError::EngineMissing)));
    }
}
