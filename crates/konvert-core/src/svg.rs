//! SVG pictures, rendered by Konvert with resvg (FFmpeg reads SVG only when
//! built with librsvg, which the macOS build is not).
//!
//! An SVG file is analysed here instead of by FFprobe; to convert it, it is
//! rendered at the planned size into a PNG file in the private workspace,
//! which then goes through the normal picture pipeline.
//!
//! Privacy: resvg has no network code, and the SVG may not read other files
//! either: `<image>` elements pointing to files are ignored, only pictures
//! embedded in the SVG itself (`data:` URLs) are drawn.

use std::path::Path;
use std::sync::{Arc, OnceLock};

use resvg::tiny_skia;
use resvg::usvg;

use crate::error::CoreError;
use crate::probe::{MediaInfo, MediaKind, PictureInfo, VideoStream};

/// Largest side Konvert renders (16384 px, like the largest WebP).
pub const MAX_SIDE: u32 = 16_384;
/// Largest picture Konvert renders, in pixels (a 256 MP canvas is 1 GB).
const MAX_PIXELS: u64 = 100_000_000;

/// The file looks like an SVG picture (`.svg`/`.svgz`, or XML with `<svg`).
pub fn is_svg(path: &Path) -> bool {
    let by_extension = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg") || e.eq_ignore_ascii_case("svgz"));
    by_extension || sniff(path)
}

fn sniff(path: &Path) -> bool {
    use std::io::Read;
    let mut head = Vec::with_capacity(1024);
    let read = std::fs::File::open(path).and_then(|f| f.take(1024).read_to_end(&mut head));
    if read.is_err() {
        return false;
    }
    let text = String::from_utf8_lossy(&head);
    let text = text.trim_start_matches('\u{feff}').trim_start();
    (text.starts_with("<?xml") || text.starts_with("<svg") || text.starts_with("<!--"))
        && text.contains("<svg")
}

/// Fonts of the system, loaded once (reading font files only).
fn fonts() -> Arc<usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = usvg::fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        })
        .clone()
}

fn options() -> usvg::Options<'static> {
    usvg::Options {
        resources_dir: None,
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: usvg::ImageHrefResolver::default_data_resolver(),
            // Never open a file named by the SVG.
            resolve_string: Box::new(|_, _| None),
        },
        fontdb: fonts(),
        ..usvg::Options::default()
    }
}

fn parse(data: &[u8]) -> Result<usvg::Tree, CoreError> {
    // A parser bug must not take the app down with it.
    std::panic::catch_unwind(|| usvg::Tree::from_data(data, &options()))
        .map_err(|_| damaged("the SVG parser failed"))?
        .map_err(|e| damaged(&e.to_string()))
}

fn damaged(details: &str) -> CoreError {
    CoreError::CorruptInput {
        details: format!("SVG: {details}"),
    }
}

/// Size of an SVG picture in pixels (at least 1×1).
fn intrinsic_size(tree: &usvg::Tree) -> (u32, u32) {
    let size = tree.size();
    let px = |v: f32| (v.ceil() as u32).clamp(1, u32::MAX / 2);
    (px(size.width()), px(size.height()))
}

/// Analyses an SVG file: a transparent picture of its intrinsic size.
pub fn probe(path: &Path) -> Result<MediaInfo, CoreError> {
    let data = std::fs::read(path)?;
    let tree = parse(&data)?;
    let (width, height) = intrinsic_size(&tree);
    Ok(MediaInfo {
        container: "svg".into(),
        kind: MediaKind::Image,
        duration_secs: None,
        size_bytes: Some(data.len() as u64),
        bit_rate: None,
        audio: Vec::new(),
        video: vec![VideoStream {
            index: 0,
            codec: "svg".into(),
            width: Some(width),
            height: Some(height),
            fps: None,
            bit_rate: None,
            pix_fmt: Some("rgba".into()),
        }],
        cover_art: None,
        subtitle_streams: 0,
        picture: Some(PictureInfo {
            alpha: true,
            ..PictureInfo::default()
        }),
        has_tags: false,
        has_location: false,
        document: None,
        text: None,
    })
}

/// Renders an SVG file at `width`×`height` (stretched to fit exactly; the
/// plan keeps the aspect ratio) and returns it as a PNG file.
pub fn render_png(path: &Path, width: u32, height: u32) -> Result<Vec<u8>, CoreError> {
    if width == 0
        || height == 0
        || width.max(height) > MAX_SIDE
        || width as u64 * height as u64 > MAX_PIXELS
    {
        return Err(CoreError::UnsupportedConversion {
            details: format!("SVG picture too large to render: {width}×{height}"),
        });
    }
    let tree = parse(&std::fs::read(path)?)?;
    let size = tree.size();
    let mut pixmap = tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| damaged("cannot allocate the picture"))?;
    let transform = tiny_skia::Transform::from_scale(
        width as f32 / size.width(),
        height as f32 / size.height(),
    );
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        resvg::render(&tree, transform, &mut pixmap.as_mut())
    }))
    .map_err(|_| damaged("the SVG renderer failed"))?;
    pixmap
        .encode_png()
        .map_err(|e| CoreError::Io(std::io::Error::other(e.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(name: &str, content: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("konvert-svg-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    const LOGO: &str = r##"<?xml version="1.0"?>
<svg width="40" height="20" viewBox="0 0 40 20">
  <rect x="20" width="20" height="20" fill="#ff0000"/>
</svg>"##;

    #[test]
    fn svg_files_are_recognized_by_name_or_content() {
        assert!(is_svg(Path::new("logo.SVG")));
        assert!(is_svg(Path::new("logo.svgz")));
        let path = write("senza-estensione", LOGO);
        assert!(is_svg(&path));
        let path = write("testo.txt", "hello <svg");
        assert!(!is_svg(&path));
    }

    #[test]
    fn probe_and_render_keep_transparency_and_size() {
        let path = write("logo.svg", LOGO);
        let media = probe(&path).unwrap();
        assert_eq!(media.kind, MediaKind::Image);
        let v = media.primary_video().unwrap();
        assert_eq!(
            (v.codec.as_str(), v.width, v.height),
            ("svg", Some(40), Some(20))
        );
        assert!(media.picture.as_ref().unwrap().alpha);

        // Rendered larger than its intrinsic size: vectors scale without loss.
        let png = render_png(&path, 400, 200).unwrap();
        let pixmap = tiny_skia::Pixmap::decode_png(&png).unwrap();
        assert_eq!((pixmap.width(), pixmap.height()), (400, 200));
        let at = |x: u32, y: u32| pixmap.pixel(x, y).unwrap();
        assert_eq!(at(50, 100).alpha(), 0, "left half is transparent");
        let red = at(350, 100);
        assert_eq!((red.red(), red.green(), red.alpha()), (255, 0, 255));
        assert!(render_png(&path, MAX_SIDE + 1, 10).is_err());
    }

    #[test]
    fn files_named_by_the_svg_are_never_read() {
        // A real picture on disk, referenced by its absolute path.
        let picture = write("segreto.png", "");
        let png = tiny_skia::Pixmap::new(2, 2).unwrap().encode_png().unwrap();
        std::fs::write(&picture, png).unwrap();
        let svg = format!(
            r#"<svg width="10" height="10"><image href="{}" width="10" height="10"/></svg>"#,
            picture.display()
        );
        let data = svg.into_bytes();
        // With resvg's defaults the file would be read and drawn...
        let default = usvg::Tree::from_data(
            &data,
            &usvg::Options {
                fontdb: fonts(),
                ..usvg::Options::default()
            },
        )
        .unwrap();
        assert!(default.root().has_children());
        // ...Konvert's options leave it out.
        assert!(!parse(&data).unwrap().root().has_children());
    }

    #[test]
    fn damaged_svg_is_a_clear_error() {
        let path = write("rotto.svg", "<svg><rect");
        assert_eq!(probe(&path).unwrap_err().code(), "corrupt_input");
    }
}
