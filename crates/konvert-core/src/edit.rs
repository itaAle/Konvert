//! Pictures turned, mirrored, cropped and resized ([`PictureEdit`]): the size
//! of each step, worked out before FFmpeg runs, and the formats an edited
//! copy can keep.
//!
//! The steps always come in this order: the turn (clockwise), the mirroring,
//! the crop (fractions of the picture as it is shown after the turn), then
//! the new size, computed on the cropped picture. A picture is never made
//! larger unless asked (`enlarge`; drawings always may), never larger than
//! 16384 pixels on a side or 100 megapixels, and never larger than its format
//! allows.

use std::path::Path;

use crate::error::CoreError;
use crate::formats::{self, FormatKind, TargetFormat};
use crate::probe::{MediaInfo, MediaKind};
use crate::request::{CropRect, PictureEdit, Resize};

/// Longest side of an edited picture.
pub const MAX_SIDE: u32 = 16384;
/// Most pixels of an edited picture.
const MAX_PIXELS: f64 = 100_000_000.0;

/// The format an edited copy of this file keeps ("Modifica"): its own
/// picture, audio or video format, when Konvert can write it (not HEIC or
/// SVG).
pub fn edit_format(media: &MediaInfo, input: &Path) -> Option<TargetFormat> {
    let format = formats::source_format(media, input)?;
    let kind = formats::spec(format).kind;
    let fits = match media.kind {
        MediaKind::Image => kind == FormatKind::Image,
        MediaKind::Video | MediaKind::VideoWithAudio => kind == FormatKind::Video,
        MediaKind::Audio => kind == FormatKind::Audio,
        MediaKind::Document | MediaKind::Text => false,
    };
    fits.then_some(format)
}

/// Checks the values of an edit.
pub fn check(edit: &PictureEdit) -> Result<(), CoreError> {
    let wrong = |what: &str| CoreError::UnsupportedConversion {
        details: format!("invalid picture edit: {what}"),
    };
    if !edit.rotation.is_multiple_of(90) {
        return Err(wrong("the turn must be a quarter turn"));
    }
    if let Some(crop) = &edit.crop {
        let CropRect {
            x,
            y,
            width,
            height,
        } = *crop;
        let inside = |v: f32| v.is_finite() && (0.0..=1.0).contains(&v);
        if !(inside(x) && inside(y) && width > 0.0 && height > 0.0)
            || !(inside(x + width - 1e-4) && inside(y + height - 1e-4))
        {
            return Err(wrong("the crop is outside the picture"));
        }
    }
    match edit.resize {
        Some(Resize::Percent { percent }) if !(percent.is_finite() && percent > 0.0) => {
            Err(wrong("the percentage must be positive"))
        }
        Some(Resize::Pixels {
            width: None,
            height: None,
            ..
        }) => Err(wrong("a width or a height is needed")),
        Some(Resize::Pixels { width, height, .. }) if width == Some(0) || height == Some(0) => {
            Err(wrong("sizes must be positive"))
        }
        Some(Resize::LongSide { side: 0 }) => Err(wrong("sizes must be positive")),
        _ => Ok(()),
    }
}

/// Size of a `width`×`height` picture after a clockwise turn.
pub fn turned(width: f64, height: f64, rotation: u16) -> (f64, f64) {
    if rotation % 180 == 90 {
        (height, width)
    } else {
        (width, height)
    }
}

/// The part kept of a picture of `size` (as shown after its turn):
/// `(x, y, width, height)` in its units, at least one unit wide and high.
pub fn crop_area(size: (f64, f64), crop: Option<&CropRect>) -> (f64, f64, f64, f64) {
    let Some(crop) = crop else {
        return (0.0, 0.0, size.0, size.1);
    };
    let (w, h) = size;
    let x = (f64::from(crop.x) * w).clamp(0.0, (w - 1.0).max(0.0));
    let y = (f64::from(crop.y) * h).clamp(0.0, (h - 1.0).max(0.0));
    let width = (f64::from(crop.width) * w).clamp(1.0_f64.min(w), w - x);
    let height = (f64::from(crop.height) * h).clamp(1.0_f64.min(h), h - y);
    (x, y, width, height)
}

/// New size of a picture of `size` (not rounded), and whether its
/// proportions change.
pub fn resized(size: (f64, f64), resize: Option<&Resize>, enlarge: bool) -> ((f64, f64), bool) {
    let (w, h) = size;
    let limit = |factor: f64| if enlarge { factor } else { factor.min(1.0) };
    let scaled = |factor: f64| ((w * factor, h * factor), false);
    match resize {
        None => (size, false),
        Some(Resize::Percent { percent }) => scaled(limit(f64::from(*percent) / 100.0)),
        Some(Resize::LongSide { side }) => scaled(limit(f64::from(*side) / w.max(h))),
        Some(Resize::Pixels {
            width,
            height,
            keep_ratio: true,
        }) => {
            let factors = [
                width.map(|v| f64::from(v) / w),
                height.map(|v| f64::from(v) / h),
            ];
            let factor = factors.into_iter().flatten().fold(f64::MAX, f64::min);
            scaled(limit(if factor == f64::MAX { 1.0 } else { factor }))
        }
        Some(Resize::Pixels {
            width,
            height,
            keep_ratio: false,
        }) => {
            let side = |wanted: Option<u32>, own: f64| {
                wanted.map_or(own, |v| {
                    let v = f64::from(v);
                    if enlarge { v } else { v.min(own) }
                })
            };
            let (nw, nh) = (side(*width, w), side(*height, h));
            let stretched = ((nw / nh) / (w / h) - 1.0).abs() > 0.01;
            ((nw, nh), stretched)
        }
    }
}

/// A size made smaller, keeping its proportions, to fit the limits: at most
/// `side` pixels on a side, and 100 megapixels for an `enlarged` picture.
pub fn within_limits(size: (f64, f64), side: u32, enlarged: bool) -> (f64, f64) {
    let (w, h) = size;
    let mut factor = (f64::from(side) / w.max(h)).min(1.0);
    let pixels = w * h * factor * factor;
    if enlarged && pixels > MAX_PIXELS {
        factor *= (MAX_PIXELS / pixels).sqrt();
    }
    (w * factor, h * factor)
}

/// A size in whole pixels, at least one.
pub fn whole(size: (f64, f64)) -> (u32, u32) {
    (
        (size.0.round() as u32).max(1),
        (size.1.round() as u32).max(1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixels(width: Option<u32>, height: Option<u32>, keep_ratio: bool) -> Resize {
        Resize::Pixels {
            width,
            height,
            keep_ratio,
        }
    }

    #[test]
    fn sizes_follow_the_mode() {
        let photo = (4032.0, 3024.0);
        let size = |resize: Resize, enlarge| {
            let (s, stretched) = resized(photo, Some(&resize), enlarge);
            (whole(s), stretched)
        };
        assert_eq!(
            size(Resize::Percent { percent: 50.0 }, false),
            ((2016, 1512), false)
        );
        assert_eq!(
            size(Resize::Percent { percent: 33.3 }, false),
            ((1343, 1007), false)
        );
        // Not larger unless asked.
        assert_eq!(
            size(Resize::Percent { percent: 200.0 }, false),
            ((4032, 3024), false)
        );
        assert_eq!(
            size(Resize::Percent { percent: 200.0 }, true),
            ((8064, 6048), false)
        );
        assert_eq!(
            size(Resize::LongSide { side: 1920 }, false),
            ((1920, 1440), false)
        );
        // One side given: the other follows.
        assert_eq!(
            size(pixels(Some(1000), None, true), false),
            ((1000, 750), false)
        );
        assert_eq!(
            size(pixels(None, Some(600), true), false),
            ((800, 600), false)
        );
        // Both sides with the proportions kept: a box to fit in.
        assert_eq!(
            size(pixels(Some(1000), Some(1000), true), false),
            ((1000, 750), false)
        );
        // Exact sizes: the proportions change.
        assert_eq!(
            size(pixels(Some(1000), Some(1000), false), false),
            ((1000, 1000), true)
        );
        assert_eq!(
            size(pixels(Some(1000), None, false), false),
            ((1000, 3024), true)
        );
        assert_eq!(
            size(pixels(Some(400), Some(300), false), false),
            ((400, 300), false)
        );
        assert_eq!(
            size(pixels(Some(9000), Some(100), false), false),
            ((4032, 100), true)
        );
        assert_eq!(resized(photo, None, false), (photo, false));
    }

    #[test]
    fn crops_are_taken_from_the_turned_picture() {
        // A portrait photo turned a quarter: landscape.
        let (w, h) = turned(3000.0, 4000.0, 90);
        assert_eq!((w, h), (4000.0, 3000.0));
        assert_eq!(turned(3000.0, 4000.0, 180), (3000.0, 4000.0));
        let crop = CropRect {
            x: 0.25,
            y: 0.5,
            width: 0.5,
            height: 0.5,
        };
        assert_eq!(
            crop_area((w, h), Some(&crop)),
            (1000.0, 1500.0, 2000.0, 1500.0)
        );
        assert_eq!(crop_area((w, h), None), (0.0, 0.0, 4000.0, 3000.0));
        // A sliver is still one pixel.
        let thin = CropRect {
            x: 0.5,
            y: 0.0,
            width: 0.0001,
            height: 1.0,
        };
        assert_eq!(crop_area((w, h), Some(&thin)).2, 1.0);
    }

    #[test]
    fn huge_pictures_are_kept_within_limits() {
        assert_eq!(
            whole(within_limits((40000.0, 10000.0), MAX_SIDE, true)),
            (16384, 4096)
        );
        // 16384 × 8192 would be 134 megapixels.
        assert_eq!(
            whole(within_limits((40000.0, 20000.0), MAX_SIDE, true)),
            (14142, 7071)
        );
        let (w, h) = whole(within_limits((15000.0, 15000.0), MAX_SIDE, true));
        assert!(
            (w * h) as f64 <= MAX_PIXELS && w == h && w > 9990,
            "{w}×{h}"
        );
        // A large photo kept as it is stays large.
        assert_eq!(
            whole(within_limits((15000.0, 15000.0), u32::MAX, false)),
            (15000, 15000)
        );
        assert_eq!(
            whole(within_limits((800.0, 600.0), MAX_SIDE, true)),
            (800, 600)
        );
        assert_eq!(whole(within_limits((800.0, 600.0), 400, false)), (400, 300));
    }

    #[test]
    fn edits_are_checked() {
        let edit = |f: &dyn Fn(&mut PictureEdit)| {
            let mut e = PictureEdit::default();
            f(&mut e);
            check(&e)
        };
        assert!(edit(&|_| {}).is_ok());
        assert!(edit(&|e| e.rotation = 270).is_ok());
        assert!(edit(&|e| e.rotation = 45).is_err());
        assert!(edit(&|e| e.resize = Some(Resize::Percent { percent: 0.0 })).is_err());
        assert!(edit(&|e| e.resize = Some(pixels(None, None, true))).is_err());
        assert!(edit(&|e| e.resize = Some(pixels(Some(0), None, true))).is_err());
        assert!(edit(&|e| e.resize = Some(Resize::LongSide { side: 0 })).is_err());
        let crop = |x, y, width, height| {
            edit(&move |e| {
                e.crop = Some(CropRect {
                    x,
                    y,
                    width,
                    height,
                })
            })
        };
        assert!(crop(0.0, 0.0, 1.0, 1.0).is_ok());
        assert!(crop(0.5, 0.5, 0.6, 0.2).is_err());
        assert!(crop(0.1, 0.1, 0.0, 0.2).is_err());
        assert!(crop(f32::NAN, 0.1, 0.5, 0.5).is_err());
    }

    #[test]
    fn empty_edits_change_nothing() {
        let mut edit = PictureEdit {
            rotation: 360,
            copy: true,
            ..PictureEdit::default()
        };
        assert!(edit.is_empty());
        edit.flip_vertical = true;
        assert!(!edit.is_empty());
    }
}
