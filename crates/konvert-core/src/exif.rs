//! EXIF data (camera, date, location...) copied from a photo into the
//! converted picture. FFmpeg reads EXIF data but its encoders do not write
//! it, so Konvert moves the block itself:
//!
//! * [`extract`] finds the EXIF block (TIFF structure) in a JPEG, PNG, WebP or
//!   HEIF/AVIF file;
//! * [`prepare`] adapts it to the converted picture: the orientation becomes
//!   "normal" (FFmpeg already turned the pixels), the thumbnail goes (it would
//!   still be turned) and, on request, the location is erased;
//! * [`insert`] writes it into a JPEG, PNG, WebP or AVIF file.
//!
//! Pure functions on byte buffers, with bounds checks everywhere: a damaged
//! or unusual file gives `None`/an error, never a panic. No dependency.

use std::fmt;

/// Why the EXIF block could not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExifError(&'static str);

impl fmt::Display for ExifError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ExifError {}

type Result<T> = std::result::Result<T, ExifError>;

const TAG_ORIENTATION: u16 = 0x0112;
const TAG_EXIF_IFD: u16 = 0x8769;
const TAG_GPS_IFD: u16 = 0x8825;
const TAG_PIXEL_X: u16 = 0xa002;
const TAG_PIXEL_Y: u16 = 0xa003;
const TAG_THUMBNAIL_OFFSET: u16 = 0x0201;
const TAG_THUMBNAIL_LENGTH: u16 = 0x0202;
/// Largest EXIF block a JPEG APP1 segment can hold.
const JPEG_MAX_EXIF: usize = 65_533 - 6;

/// File formats the EXIF block can be read from or written to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    Jpeg,
    Png,
    Webp,
    /// HEIF and AVIF (ISO base media file with a `meta` box).
    Heif,
}

/// Recognizes a file from its first bytes.
pub fn container_of(data: &[u8]) -> Option<Container> {
    if data.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(Container::Jpeg)
    } else if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(Container::Png)
    } else if data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        Some(Container::Webp)
    } else if data.len() >= 12 && &data[4..8] == b"ftyp" {
        Some(Container::Heif)
    } else {
        None
    }
}

/// The EXIF block (starting with the TIFF header `II*\0` or `MM\0*`), if any.
pub fn extract(data: &[u8]) -> Option<Vec<u8>> {
    let block = match container_of(data)? {
        Container::Jpeg => jpeg_segments(data)
            .ok()?
            .into_iter()
            .find(|s| s.marker == 0xe1 && data[s.payload.clone()].starts_with(b"Exif\0\0"))
            .map(|s| data[s.payload.start + 6..s.payload.end].to_vec())?,
        Container::Png => png_chunks(data)
            .ok()?
            .into_iter()
            .find(|c| &c.kind == b"eXIf")
            .map(|c| data[c.data.clone()].to_vec())?,
        Container::Webp => webp_chunks(data)
            .ok()?
            .into_iter()
            .find(|c| &c.kind == b"EXIF")
            .map(|c| data[c.data.clone()].to_vec())?,
        Container::Heif => heif_exif(data)?,
    };
    // Some writers keep the JPEG "Exif\0\0" prefix in PNG and WebP too.
    let tiff = block.strip_prefix(b"Exif\0\0").unwrap_or(&block);
    Tiff::new(tiff).map(|_| tiff.to_vec())
}

/// The EXIF block records where the photo was taken.
pub fn has_location(tiff: &[u8]) -> bool {
    Tiff::new(tiff)
        .and_then(|t| {
            let ifd0 = t.u32(4)? as usize;
            let gps = t.find_entry(ifd0, TAG_GPS_IFD)?;
            let offset = t.u32(gps + 8)? as usize;
            Some(t.u16(offset)? > 0)
        })
        .unwrap_or(false)
}

/// Adapts an EXIF block to the converted picture: orientation "normal",
/// no thumbnail, the new size and, if asked, no location left anywhere in
/// the block (the GPS data is overwritten with zeros, not just unlinked).
pub fn prepare(tiff: &[u8], remove_location: bool, size: Option<(u32, u32)>) -> Option<Vec<u8>> {
    // Offsets are read from the original, edits go into the copy.
    let t = Tiff::new(tiff)?;
    let mut out = tiff.to_vec();
    let little = t.little;
    let ifd0 = t.u32(4)? as usize;
    let mut edits: Vec<(usize, Vec<u8>)> = Vec::new();
    let mut erase: Vec<std::ops::Range<usize>> = Vec::new();

    if let Some(entry) = t.find_entry(ifd0, TAG_ORIENTATION) {
        edits.push((entry + 8, t.encode_u16(1)));
    }
    if let (Some(exif_ifd), Some((width, height))) = (
        t.find_entry(ifd0, TAG_EXIF_IFD)
            .and_then(|e| t.u32(e + 8))
            .map(|o| o as usize),
        size,
    ) {
        for (tag, value) in [(TAG_PIXEL_X, width), (TAG_PIXEL_Y, height)] {
            if let Some(entry) = t.find_entry(exif_ifd, tag) {
                match t.u16(entry + 2)? {
                    3 if value <= u16::MAX as u32 => {
                        let mut v = t.encode_u16(value as u16);
                        v.extend([0, 0]);
                        edits.push((entry + 8, v));
                    }
                    4 => edits.push((entry + 8, t.encode_u32(value))),
                    _ => {}
                }
            }
        }
    }
    // The thumbnail (second directory) shows the photo as it was stored.
    let entries = t.u16(ifd0)? as usize;
    let link = ifd0 + 2 + entries * 12;
    let ifd1 = t.u32(link)? as usize;
    if ifd1 != 0 {
        if let Some(region) = t.ifd_region(ifd1) {
            erase.extend(region);
        }
        let start = t
            .find_entry(ifd1, TAG_THUMBNAIL_OFFSET)
            .and_then(|e| t.u32(e + 8));
        let length = t
            .find_entry(ifd1, TAG_THUMBNAIL_LENGTH)
            .and_then(|e| t.u32(e + 8));
        if let (Some(start), Some(length)) = (start, length) {
            erase.push(start as usize..start as usize + length as usize);
        }
        edits.push((link, vec![0; 4]));
    }
    let mut remove_gps_entry = None;
    if remove_location && let Some(entry) = t.find_entry(ifd0, TAG_GPS_IFD) {
        let gps = t.u32(entry + 8)? as usize;
        if let Some(region) = t.ifd_region(gps) {
            erase.extend(region);
        }
        remove_gps_entry = Some(entry);
    }

    for range in erase {
        let end = range.end.min(out.len());
        if range.start < end {
            out[range.start..end].fill(0);
        }
    }
    for (offset, bytes) in edits {
        out.get_mut(offset..offset + bytes.len())?
            .copy_from_slice(&bytes);
    }
    if let Some(entry) = remove_gps_entry {
        // Remove the directory entry: the following entries and the link move
        // up by 12 bytes, the freed space at the end is zeroed.
        let end = link + 4;
        out.copy_within(entry + 12..end, entry);
        out[end - 12..end].fill(0);
        let count = encode_u16(little, entries as u16 - 1);
        out[ifd0..ifd0 + 2].copy_from_slice(&count);
    }
    Some(out)
}

/// `data` without any EXIF block (JPEG, PNG and WebP; other files are
/// returned unchanged, they never get EXIF data from FFmpeg).
pub fn strip(data: &[u8]) -> Option<Vec<u8>> {
    match container_of(data)? {
        Container::Jpeg => {
            let mut out = data[..2].to_vec();
            let mut at = 2;
            for s in jpeg_segments(data).ok()? {
                if s.marker == 0xe1 && data[s.payload.clone()].starts_with(b"Exif\0\0") {
                    out.extend(&data[at..s.span.start]);
                    at = s.span.end;
                }
            }
            out.extend(&data[at.max(2)..]);
            Some(out)
        }
        Container::Png => {
            let chunks = png_chunks(data).ok()?;
            let mut out = data[..8].to_vec();
            for c in chunks.iter().filter(|c| &c.kind != b"eXIf") {
                out.extend(&data[c.span.clone()]);
            }
            Some(out)
        }
        Container::Webp => {
            let chunks = webp_chunks(data).ok()?;
            let mut body: Vec<u8> = Vec::with_capacity(data.len());
            for c in chunks.iter().filter(|c| &c.kind != b"EXIF") {
                let start = body.len();
                body.extend(&data[c.span.clone()]);
                if &c.kind == b"VP8X" {
                    *body.get_mut(start + 8)? &= !0x08;
                }
            }
            let mut out = b"RIFF".to_vec();
            out.extend(((body.len() + 4) as u32).to_le_bytes());
            out.extend(b"WEBP");
            out.extend(body);
            Some(out)
        }
        Container::Heif => Some(data.to_vec()),
    }
}

/// Writes `tiff` as the EXIF block of `data` (replacing any existing one).
pub fn insert(data: &[u8], tiff: &[u8]) -> Result<Vec<u8>> {
    Tiff::new(tiff).ok_or(ExifError("not an EXIF block"))?;
    match container_of(data).ok_or(ExifError("unknown file type"))? {
        Container::Jpeg => insert_jpeg(data, tiff),
        Container::Png => insert_png(data, tiff),
        Container::Webp => insert_webp(data, tiff),
        Container::Heif => insert_heif(data, tiff),
    }
}

// --- TIFF structure ------------------------------------------------------------

struct Tiff<'a> {
    data: &'a [u8],
    little: bool,
}

impl<'a> Tiff<'a> {
    fn new(data: &'a [u8]) -> Option<Self> {
        let little = match data.get(..4)? {
            b"II*\0" => true,
            b"MM\0*" => false,
            _ => return None,
        };
        let t = Self { data, little };
        let ifd0 = t.u32(4)? as usize;
        t.u16(ifd0)?;
        Some(t)
    }

    fn u16(&self, at: usize) -> Option<u16> {
        let b: [u8; 2] = self.data.get(at..at + 2)?.try_into().ok()?;
        Some(if self.little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    }

    fn u32(&self, at: usize) -> Option<u32> {
        let b: [u8; 4] = self.data.get(at..at + 4)?.try_into().ok()?;
        Some(if self.little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    }

    fn encode_u16(&self, v: u16) -> Vec<u8> {
        encode_u16(self.little, v).to_vec()
    }

    fn encode_u32(&self, v: u32) -> Vec<u8> {
        if self.little {
            v.to_le_bytes().to_vec()
        } else {
            v.to_be_bytes().to_vec()
        }
    }

    /// Offset of the 12-byte entry for `tag` in the directory at `ifd`.
    fn find_entry(&self, ifd: usize, tag: u16) -> Option<usize> {
        let count = self.u16(ifd)? as usize;
        (0..count)
            .map(|i| ifd + 2 + i * 12)
            .find(|&entry| self.u16(entry) == Some(tag))
    }

    /// Bytes of a directory: the entries, the values stored outside them and
    /// the link to the next directory.
    fn ifd_region(&self, ifd: usize) -> Option<Vec<std::ops::Range<usize>>> {
        let count = self.u16(ifd)? as usize;
        let mut regions = Vec::with_capacity(count + 1);
        regions.push(ifd..ifd + 2 + count * 12 + 4);
        for i in 0..count {
            let entry = ifd + 2 + i * 12;
            let size = type_size(self.u16(entry + 2)?) * self.u32(entry + 4)? as usize;
            if size > 4 {
                let at = self.u32(entry + 8)? as usize;
                regions.push(at..at.saturating_add(size));
            }
        }
        Some(regions)
    }
}

fn encode_u16(little: bool, v: u16) -> [u8; 2] {
    if little {
        v.to_le_bytes()
    } else {
        v.to_be_bytes()
    }
}

/// Size in bytes of one value of a TIFF field type.
fn type_size(field_type: u16) -> usize {
    match field_type {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 => 4,
        5 | 10 | 12 => 8,
        _ => 1,
    }
}

// --- JPEG ----------------------------------------------------------------------

struct Segment {
    marker: u8,
    /// Whole segment, marker included.
    span: std::ops::Range<usize>,
    payload: std::ops::Range<usize>,
}

/// Segments before the image data (start of scan).
fn jpeg_segments(data: &[u8]) -> Result<Vec<Segment>> {
    let mut segments = Vec::new();
    let mut at = 2;
    loop {
        let header = data.get(at..at + 4).ok_or(ExifError("truncated JPEG"))?;
        if header[0] != 0xff {
            return Err(ExifError("damaged JPEG"));
        }
        let marker = header[1];
        if marker == 0xda {
            return Ok(segments);
        }
        let length = u16::from_be_bytes([header[2], header[3]]) as usize;
        let end = at + 2 + length;
        if length < 2 || end > data.len() {
            return Err(ExifError("damaged JPEG"));
        }
        segments.push(Segment {
            marker,
            span: at..end,
            payload: at + 4..end,
        });
        at = end;
    }
}

fn insert_jpeg(data: &[u8], tiff: &[u8]) -> Result<Vec<u8>> {
    if tiff.len() > JPEG_MAX_EXIF {
        return Err(ExifError("EXIF block too large for a JPEG file"));
    }
    let segments = jpeg_segments(data)?;
    // After the JFIF segment, which must come first.
    let position = segments
        .first()
        .filter(|s| s.marker == 0xe0)
        .map_or(2, |s| s.span.end);
    let mut segment = vec![0xff, 0xe1];
    segment.extend(((tiff.len() + 8) as u16).to_be_bytes());
    segment.extend(b"Exif\0\0");
    segment.extend(tiff);

    let mut out = Vec::with_capacity(data.len() + segment.len());
    out.extend(&data[..position]);
    out.extend(&segment);
    let mut at = position;
    for s in segments.iter().filter(|s| s.span.start >= position) {
        let is_exif = s.marker == 0xe1 && data[s.payload.clone()].starts_with(b"Exif\0\0");
        if is_exif {
            out.extend(&data[at..s.span.start]);
            at = s.span.end;
        }
    }
    out.extend(&data[at..]);
    Ok(out)
}

// --- PNG -----------------------------------------------------------------------

struct Chunk {
    kind: [u8; 4],
    span: std::ops::Range<usize>,
    data: std::ops::Range<usize>,
}

fn png_chunks(data: &[u8]) -> Result<Vec<Chunk>> {
    let mut chunks = Vec::new();
    let mut at = 8;
    while at < data.len() {
        let header = data.get(at..at + 8).ok_or(ExifError("truncated PNG"))?;
        let length = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        let end = at
            .checked_add(12 + length)
            .filter(|e| *e <= data.len())
            .ok_or(ExifError("damaged PNG"))?;
        let kind: [u8; 4] = header[4..8].try_into().unwrap();
        chunks.push(Chunk {
            kind,
            span: at..end,
            data: at + 8..at + 8 + length,
        });
        at = end;
        if &kind == b"IEND" {
            break;
        }
    }
    Ok(chunks)
}

fn insert_png(data: &[u8], tiff: &[u8]) -> Result<Vec<u8>> {
    let chunks = png_chunks(data)?;
    let header = chunks
        .first()
        .filter(|c| &c.kind == b"IHDR")
        .ok_or(ExifError("damaged PNG"))?;
    let mut chunk = (tiff.len() as u32).to_be_bytes().to_vec();
    chunk.extend(b"eXIf");
    chunk.extend(tiff);
    chunk.extend(crc32(&chunk[4..]).to_be_bytes());

    let mut out = Vec::with_capacity(data.len() + chunk.len());
    out.extend(&data[..header.span.end]);
    out.extend(&chunk);
    for c in chunks.iter().skip(1).filter(|c| &c.kind != b"eXIf") {
        out.extend(&data[c.span.clone()]);
    }
    Ok(out)
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

// --- WebP ----------------------------------------------------------------------

fn webp_chunks(data: &[u8]) -> Result<Vec<Chunk>> {
    let mut chunks = Vec::new();
    let mut at = 12;
    while at + 8 <= data.len() {
        let kind: [u8; 4] = data[at..at + 4].try_into().unwrap();
        let length = u32::from_le_bytes(data[at + 4..at + 8].try_into().unwrap()) as usize;
        let data_end = at
            .checked_add(8 + length)
            .filter(|e| *e <= data.len())
            .ok_or(ExifError("damaged WebP"))?;
        // Chunks are padded to an even size.
        let end = (data_end + (length & 1)).min(data.len());
        chunks.push(Chunk {
            kind,
            span: at..end,
            data: at + 8..data_end,
        });
        at = end;
    }
    Ok(chunks)
}

/// Canvas size and transparency of a simple (single-image) WebP file.
fn webp_image_info(data: &[u8], chunk: &Chunk) -> Option<(u32, u32, bool)> {
    let body = &data[chunk.data.clone()];
    match &chunk.kind {
        b"VP8 " => {
            // Frame tag (3 bytes), start code, then 14-bit width and height.
            if body.get(3..6)? != [0x9d, 0x01, 0x2a] {
                return None;
            }
            let w = u16::from_le_bytes(body.get(6..8)?.try_into().ok()?) & 0x3fff;
            let h = u16::from_le_bytes(body.get(8..10)?.try_into().ok()?) & 0x3fff;
            Some((w as u32, h as u32, false))
        }
        b"VP8L" => {
            if *body.first()? != 0x2f {
                return None;
            }
            let bits = u32::from_le_bytes(body.get(1..5)?.try_into().ok()?);
            Some((
                (bits & 0x3fff) + 1,
                ((bits >> 14) & 0x3fff) + 1,
                (bits >> 28) & 1 == 1,
            ))
        }
        _ => None,
    }
}

fn insert_webp(data: &[u8], tiff: &[u8]) -> Result<Vec<u8>> {
    const EXIF_FLAG: u8 = 0x08;
    const ALPHA_FLAG: u8 = 0x10;
    let chunks = webp_chunks(data)?;
    let first = chunks.first().ok_or(ExifError("empty WebP"))?;

    let mut body = Vec::with_capacity(data.len() + tiff.len() + 32);
    if &first.kind == b"VP8X" {
        let mut vp8x = data[first.span.clone()].to_vec();
        *vp8x.get_mut(8).ok_or(ExifError("damaged WebP"))? |= EXIF_FLAG;
        body.extend(vp8x);
    } else {
        // Simple format: the extended header announces the EXIF chunk.
        let (width, height, alpha) =
            webp_image_info(data, first).ok_or(ExifError("unsupported WebP"))?;
        body.extend(b"VP8X");
        body.extend(10u32.to_le_bytes());
        body.push(EXIF_FLAG | if alpha { ALPHA_FLAG } else { 0 });
        body.extend([0, 0, 0]);
        body.extend(&(width - 1).to_le_bytes()[..3]);
        body.extend(&(height - 1).to_le_bytes()[..3]);
        body.extend(&data[first.span.clone()]);
    }
    for c in chunks.iter().skip(1).filter(|c| &c.kind != b"EXIF") {
        body.extend(&data[c.span.clone()]);
    }
    // EXIF comes after the image data.
    body.extend(b"EXIF");
    body.extend((tiff.len() as u32).to_le_bytes());
    body.extend(tiff);
    if tiff.len() % 2 == 1 {
        body.push(0);
    }

    let mut out = Vec::with_capacity(body.len() + 12);
    out.extend(b"RIFF");
    out.extend(((body.len() + 4) as u32).to_le_bytes());
    out.extend(b"WEBP");
    out.extend(body);
    Ok(out)
}

// --- HEIF / AVIF ---------------------------------------------------------------

/// An ISO box: `start..end` is the whole box, `body` follows the header.
#[derive(Clone)]
struct IsoBox {
    kind: [u8; 4],
    start: usize,
    body: usize,
    end: usize,
}

fn iso_boxes(data: &[u8], from: usize, to: usize) -> Option<Vec<IsoBox>> {
    let mut boxes = Vec::new();
    let mut at = from;
    while at + 8 <= to {
        let size = u32::from_be_bytes(data[at..at + 4].try_into().ok()?) as usize;
        let kind: [u8; 4] = data[at + 4..at + 8].try_into().ok()?;
        let (size, header) = match size {
            0 => (to - at, 8),
            1 => (
                u64::from_be_bytes(data.get(at + 8..at + 16)?.try_into().ok()?) as usize,
                16,
            ),
            n => (n, 8),
        };
        if size < header || at + size > to {
            return None;
        }
        boxes.push(IsoBox {
            kind,
            start: at,
            body: at + header,
            end: at + size,
        });
        at += size;
    }
    Some(boxes)
}

/// Item locations of an `iloc` box.
struct Iloc {
    version: u8,
    offset_size: usize,
    length_size: usize,
    base_offset_size: usize,
    index_size: usize,
    items: Vec<IlocItem>,
}

struct IlocItem {
    id: u32,
    construction_method: u8,
    base_offset: u64,
    /// (index, offset, length)
    extents: Vec<(u64, u64, u64)>,
}

fn read_uint(data: &[u8], at: usize, size: usize) -> Option<u64> {
    let bytes = data.get(at..at + size)?;
    Some(bytes.iter().fold(0u64, |v, b| v << 8 | *b as u64))
}

fn write_uint(out: &mut Vec<u8>, value: u64, size: usize) -> Option<()> {
    if size < 8 && value >> (size * 8) != 0 {
        return None;
    }
    for i in (0..size).rev() {
        out.push((value >> (i * 8)) as u8);
    }
    Some(())
}

fn parse_iloc(data: &[u8], b: &IsoBox) -> Option<Iloc> {
    let version = *data.get(b.body)?;
    let sizes = *data.get(b.body + 4)?;
    let more = *data.get(b.body + 5)?;
    let mut iloc = Iloc {
        version,
        offset_size: (sizes >> 4) as usize,
        length_size: (sizes & 15) as usize,
        base_offset_size: (more >> 4) as usize,
        index_size: if version > 0 { (more & 15) as usize } else { 0 },
        items: Vec::new(),
    };
    let mut at = b.body + 6;
    let count = if version < 2 {
        let c = read_uint(data, at, 2)?;
        at += 2;
        c
    } else {
        let c = read_uint(data, at, 4)?;
        at += 4;
        c
    };
    for _ in 0..count {
        let id_size = if version < 2 { 2 } else { 4 };
        let id = read_uint(data, at, id_size)? as u32;
        at += id_size;
        let mut construction_method = 0;
        if version > 0 {
            construction_method = (read_uint(data, at, 2)? & 15) as u8;
            at += 2;
        }
        at += 2; // data reference index
        let base_offset = read_uint(data, at, iloc.base_offset_size)?;
        at += iloc.base_offset_size;
        let extent_count = read_uint(data, at, 2)?;
        at += 2;
        let mut extents = Vec::new();
        for _ in 0..extent_count {
            let index = read_uint(data, at, iloc.index_size)?;
            at += iloc.index_size;
            let offset = read_uint(data, at, iloc.offset_size)?;
            at += iloc.offset_size;
            let length = read_uint(data, at, iloc.length_size)?;
            at += iloc.length_size;
            extents.push((index, offset, length));
        }
        iloc.items.push(IlocItem {
            id,
            construction_method,
            base_offset,
            extents,
        });
    }
    (at <= b.end).then_some(iloc)
}

fn encode_iloc(iloc: &Iloc) -> Option<Vec<u8>> {
    let mut body = vec![iloc.version, 0, 0, 0];
    body.push((iloc.offset_size << 4 | iloc.length_size) as u8);
    body.push((iloc.base_offset_size << 4 | iloc.index_size) as u8);
    let wide = iloc.version >= 2;
    write_uint(&mut body, iloc.items.len() as u64, if wide { 4 } else { 2 })?;
    for item in &iloc.items {
        write_uint(&mut body, item.id as u64, if wide { 4 } else { 2 })?;
        if iloc.version > 0 {
            write_uint(&mut body, item.construction_method as u64, 2)?;
        }
        write_uint(&mut body, 0, 2)?;
        write_uint(&mut body, item.base_offset, iloc.base_offset_size)?;
        write_uint(&mut body, item.extents.len() as u64, 2)?;
        for &(index, offset, length) in &item.extents {
            write_uint(&mut body, index, iloc.index_size)?;
            write_uint(&mut body, offset, iloc.offset_size)?;
            write_uint(&mut body, length, iloc.length_size)?;
        }
    }
    Some(make_box(b"iloc", &body))
}

fn make_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut b = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    b.extend(kind);
    b.extend(body);
    b
}

/// Item types of an `iinf` box: (item id, type).
fn parse_iinf(data: &[u8], b: &IsoBox) -> Option<Vec<(u32, [u8; 4])>> {
    let version = *data.get(b.body)?;
    let first = b.body + 4 + if version == 0 { 2 } else { 4 };
    iso_boxes(data, first, b.end)?
        .into_iter()
        .filter(|e| &e.kind == b"infe")
        .map(|e| {
            let v = *data.get(e.body)?;
            let (id, rest) = match v {
                2 => (read_uint(data, e.body + 4, 2)?, e.body + 6),
                3 => (read_uint(data, e.body + 4, 4)?, e.body + 8),
                _ => return Some((0, *b"\0\0\0\0")),
            };
            let kind: [u8; 4] = data.get(rest + 2..rest + 6)?.try_into().ok()?;
            Some((id as u32, kind))
        })
        .collect()
}

/// Top-level `meta` box and its children.
fn heif_meta(data: &[u8]) -> Option<(IsoBox, Vec<IsoBox>)> {
    let meta = iso_boxes(data, 0, data.len())?
        .into_iter()
        .find(|b| &b.kind == b"meta")?;
    let children = iso_boxes(data, meta.body + 4, meta.end)?;
    Some((meta, children))
}

fn heif_exif(data: &[u8]) -> Option<Vec<u8>> {
    let (_, children) = heif_meta(data)?;
    let find = |kind: &[u8; 4]| children.iter().find(|b| &b.kind == kind);
    let id = parse_iinf(data, find(b"iinf")?)?
        .into_iter()
        .find(|(_, kind)| kind == b"Exif")?
        .0;
    let iloc = parse_iloc(data, find(b"iloc")?)?;
    let item = iloc.items.iter().find(|i| i.id == id)?;
    let base = match item.construction_method {
        0 => 0,
        1 => find(b"idat")?.body as u64,
        _ => return None,
    };
    let mut block = Vec::new();
    for &(_, offset, length) in &item.extents {
        let start = (base + item.base_offset + offset) as usize;
        block.extend(data.get(start..start.checked_add(length as usize)?)?);
    }
    // ExifDataBlock: offset of the TIFF header, then the EXIF data.
    let skip = u32::from_be_bytes(block.get(..4)?.try_into().ok()?) as usize;
    Some(block.get(4 + skip..)?.to_vec())
}

fn insert_heif(data: &[u8], tiff: &[u8]) -> Result<Vec<u8>> {
    let err = || ExifError("unsupported HEIF/AVIF layout");
    let (meta, children) = heif_meta(data).ok_or_else(err)?;
    let find = |kind: &[u8; 4]| children.iter().find(|b| &b.kind == kind);
    let pitm = find(b"pitm").ok_or_else(err)?;
    let primary = match data[pitm.body] {
        0 => read_uint(data, pitm.body + 4, 2),
        _ => read_uint(data, pitm.body + 4, 4),
    }
    .ok_or_else(err)? as u32;
    let iinf = find(b"iinf").ok_or_else(err)?;
    let items = parse_iinf(data, iinf).ok_or_else(err)?;
    if items.iter().any(|(_, kind)| kind == b"Exif") {
        return Err(ExifError("the file already has EXIF data"));
    }
    let exif_id = items.iter().map(|(id, _)| *id).max().unwrap_or(0) + 1;
    if exif_id > u16::MAX as u32 {
        return Err(err());
    }
    let mut iloc = parse_iloc(data, find(b"iloc").ok_or_else(err)?).ok_or_else(err)?;
    if iloc.offset_size < 4 || iloc.length_size < 4 {
        return Err(err());
    }

    // New iinf: one more item entry.
    let iinf_version = data[iinf.body];
    let mut iinf_body = data[iinf.body..iinf.body + 4].to_vec();
    let count_size = if iinf_version == 0 { 2 } else { 4 };
    let count = read_uint(data, iinf.body + 4, count_size).ok_or_else(err)?;
    write_uint(&mut iinf_body, count + 1, count_size).ok_or_else(err)?;
    iinf_body.extend(&data[iinf.body + 4 + count_size..iinf.end]);
    let mut infe = vec![2, 0, 0, 0];
    infe.extend((exif_id as u16).to_be_bytes());
    infe.extend([0, 0]);
    infe.extend(b"Exif");
    infe.push(0);
    iinf_body.extend(make_box(b"infe", &infe));
    let new_iinf = make_box(b"iinf", &iinf_body);

    // New iref: the EXIF item describes the primary picture.
    let mut cdsc = (exif_id as u16).to_be_bytes().to_vec();
    cdsc.extend(1u16.to_be_bytes());
    cdsc.extend((primary as u16).to_be_bytes());
    let cdsc = make_box(b"cdsc", &cdsc);
    let new_iref = match find(b"iref") {
        Some(iref) if data[iref.body] == 0 => {
            let mut body = data[iref.body..iref.end].to_vec();
            body.extend(&cdsc);
            make_box(b"iref", &body)
        }
        Some(_) => return Err(err()),
        None => {
            let mut body = vec![0, 0, 0, 0];
            body.extend(&cdsc);
            make_box(b"iref", &body)
        }
    };

    // Rebuild the meta box with a placeholder iloc of the final size.
    iloc.items.push(IlocItem {
        id: exif_id,
        construction_method: 0,
        base_offset: 0,
        extents: vec![(0, 0, 0)],
    });
    let iloc_len = encode_iloc(&iloc).ok_or_else(err)?.len();
    let assemble = |iloc_box: &[u8]| -> Vec<u8> {
        let mut body = data[meta.body..meta.body + 4].to_vec();
        let mut iref_done = false;
        for child in &children {
            match &child.kind {
                b"iinf" => {
                    body.extend(&new_iinf);
                    if find(b"iref").is_none() {
                        body.extend(&new_iref);
                        iref_done = true;
                    }
                }
                b"iref" => {
                    body.extend(&new_iref);
                    iref_done = true;
                }
                b"iloc" => body.extend(iloc_box),
                _ => body.extend(&data[child.start..child.end]),
            }
        }
        debug_assert!(iref_done);
        make_box(b"meta", &body)
    };
    let new_meta_len = assemble(&vec![0; iloc_len]).len();
    let delta = new_meta_len as i64 - (meta.end - meta.start) as i64;

    // Data after the meta box moves by `delta`; the EXIF data goes at the end.
    let shift = |offset: u64| -> Option<u64> {
        if offset as usize >= meta.end {
            u64::try_from(offset as i64 + delta).ok()
        } else {
            Some(offset)
        }
    };
    let last = iloc.items.len() - 1;
    for item in iloc.items[..last]
        .iter_mut()
        .filter(|i| i.construction_method == 0)
    {
        if iloc.base_offset_size > 0 && item.base_offset > 0 {
            item.base_offset = shift(item.base_offset).ok_or_else(err)?;
        } else {
            for extent in &mut item.extents {
                extent.1 = shift(extent.1).ok_or_else(err)?;
            }
        }
    }
    let mut payload = 6u32.to_be_bytes().to_vec();
    payload.extend(b"Exif\0\0");
    payload.extend(tiff);
    let exif_box_start = (data.len() as i64 + delta) as u64;
    iloc.items[last].extents = vec![(0, exif_box_start + 8, payload.len() as u64)];
    let new_iloc = encode_iloc(&iloc).ok_or_else(err)?;
    let new_meta = assemble(&new_iloc);
    if new_meta.len() != new_meta_len {
        return Err(err());
    }

    let mut out = Vec::with_capacity(data.len() + new_meta.len() + payload.len());
    out.extend(&data[..meta.start]);
    out.extend(&new_meta);
    out.extend(&data[meta.end..]);
    out.extend(make_box(b"mdat", &payload));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(t: &mut Vec<u8>, tag: u16, kind: u16, count: u32, value: [u8; 4]) {
        t.extend(tag.to_le_bytes());
        t.extend(kind.to_le_bytes());
        t.extend(count.to_le_bytes());
        t.extend(value);
    }

    /// Little-endian EXIF block laid out like a camera's:
    /// * 8: IFD0 (make, orientation 6, EXIF and GPS links, link to IFD1 at 58);
    /// * 62: EXIF directory with the pixel size;
    /// * 92: GPS directory (latitude ref, latitude with 3 rationals at 122);
    /// * 146: IFD1 describing a 4-byte thumbnail stored at 176.
    fn tiff_with_thumbnail() -> Vec<u8> {
        let mut t = b"II*\0".to_vec();
        t.extend(8u32.to_le_bytes());
        t.extend(4u16.to_le_bytes());
        entry(&mut t, 0x010f, 2, 4, *b"Cam\0");
        entry(&mut t, TAG_ORIENTATION, 3, 1, [6, 0, 0, 0]);
        entry(&mut t, TAG_EXIF_IFD, 4, 1, 62u32.to_le_bytes());
        entry(&mut t, TAG_GPS_IFD, 4, 1, 92u32.to_le_bytes());
        t.extend(146u32.to_le_bytes());
        assert_eq!(t.len(), 62);
        t.extend(2u16.to_le_bytes());
        entry(&mut t, TAG_PIXEL_X, 4, 1, 4032u32.to_le_bytes());
        entry(&mut t, TAG_PIXEL_Y, 4, 1, 3024u32.to_le_bytes());
        t.extend(0u32.to_le_bytes());
        assert_eq!(t.len(), 92);
        t.extend(2u16.to_le_bytes());
        entry(&mut t, 0x0001, 2, 2, *b"N\0\0\0");
        entry(&mut t, 0x0002, 5, 3, 122u32.to_le_bytes());
        t.extend(0u32.to_le_bytes());
        for (num, den) in [(45u32, 1u32), (30, 1), (7, 1)] {
            t.extend(num.to_le_bytes());
            t.extend(den.to_le_bytes());
        }
        assert_eq!(t.len(), 146);
        t.extend(2u16.to_le_bytes());
        entry(&mut t, TAG_THUMBNAIL_OFFSET, 4, 1, 176u32.to_le_bytes());
        entry(&mut t, TAG_THUMBNAIL_LENGTH, 4, 1, 4u32.to_le_bytes());
        t.extend(0u32.to_le_bytes());
        t.extend(b"THMB");
        t
    }

    #[test]
    fn prepare_resets_orientation_and_drops_the_thumbnail() {
        let tiff = tiff_with_thumbnail();
        assert!(has_location(&tiff));
        let out = prepare(&tiff, false, Some((3024, 4032))).unwrap();
        let t = Tiff::new(&out).unwrap();
        let orientation = t.find_entry(8, TAG_ORIENTATION).unwrap();
        assert_eq!(t.u16(orientation + 8), Some(1));
        // No second directory, no thumbnail bytes left.
        assert_eq!(t.u32(58), Some(0));
        assert!(!out.windows(4).any(|w| w == b"THMB"));
        // Pixel size of the converted picture.
        assert_eq!(
            t.u32(t.find_entry(62, TAG_PIXEL_X).unwrap() + 8),
            Some(3024)
        );
        assert_eq!(
            t.u32(t.find_entry(62, TAG_PIXEL_Y).unwrap() + 8),
            Some(4032)
        );
        // The location stays unless asked.
        assert!(has_location(&out));
        assert_eq!(out.len(), tiff.len());
    }

    #[test]
    fn prepare_can_erase_the_location() {
        let tiff = tiff_with_thumbnail();
        let out = prepare(&tiff, true, None).unwrap();
        assert!(!has_location(&out));
        let t = Tiff::new(&out).unwrap();
        assert_eq!(t.u16(8), Some(3));
        assert!(t.find_entry(8, TAG_GPS_IFD).is_none());
        // Make, orientation and the EXIF directory survive.
        assert!(t.find_entry(8, 0x010f).is_some() && t.find_entry(8, TAG_EXIF_IFD).is_some());
        // The coordinates are gone from the bytes, not just unlinked.
        assert!(out[92..146].iter().all(|b| *b == 0));
        assert!(!out.windows(2).any(|w| w == b"N\0"));
    }

    fn jpeg() -> Vec<u8> {
        // SOI, APP0 (JFIF), an old EXIF segment, SOS and some data, EOI.
        let mut j = vec![0xff, 0xd8, 0xff, 0xe0, 0, 7, b'J', b'F', b'I', b'F', 0];
        j.extend([0xff, 0xe1, 0, 10]);
        j.extend(b"Exif\0\0OLD");
        j.truncate(j.len() - 1);
        j.extend([0xff, 0xda, 0, 2, 1, 2, 3, 0xff, 0xd9]);
        j
    }

    #[test]
    fn jpeg_gets_one_exif_segment_after_jfif() {
        let tiff = tiff_with_thumbnail();
        let out = insert(&jpeg(), &tiff).unwrap();
        assert_eq!(&out[2..4], [0xff, 0xe0]);
        assert_eq!(&out[11..13], [0xff, 0xe1]);
        assert_eq!(extract(&out).unwrap(), tiff);
        assert_eq!(out.windows(6).filter(|w| *w == b"Exif\0\0").count(), 1);
        assert!(out.ends_with(&[0xff, 0xda, 0, 2, 1, 2, 3, 0xff, 0xd9]));
        assert!(insert(&jpeg(), &vec![0; JPEG_MAX_EXIF + 1]).is_err());
    }

    #[test]
    fn png_and_webp_round_trip() {
        let tiff = tiff_with_thumbnail();
        // PNG: signature, IHDR, IDAT, IEND (CRCs are not checked here).
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        for (kind, body) in [
            (&b"IHDR"[..], &[0u8; 13][..]),
            (b"IDAT", b"xyz"),
            (b"IEND", b""),
        ] {
            png.extend((body.len() as u32).to_be_bytes());
            png.extend(kind);
            png.extend(body);
            png.extend([0; 4]);
        }
        let out = insert(&png, &tiff).unwrap();
        assert_eq!(extract(&out).unwrap(), tiff);
        let again = insert(&out, &tiff).unwrap();
        assert_eq!(again.len(), out.len(), "the old eXIf chunk is replaced");
        assert_eq!(crc32(b"IEND"), 0xae42_6082);

        // Simple lossy WebP 100×50: becomes extended, with the EXIF flag.
        let mut vp8 = vec![0, 0, 0, 0x9d, 0x01, 0x2a];
        vp8.extend(100u16.to_le_bytes());
        vp8.extend(50u16.to_le_bytes());
        vp8.push(0xaa);
        let mut webp = b"RIFF\0\0\0\0WEBPVP8 ".to_vec();
        webp.extend((vp8.len() as u32).to_le_bytes());
        webp.extend(&vp8);
        webp.push(0);
        let size = (webp.len() - 8) as u32;
        webp[4..8].copy_from_slice(&size.to_le_bytes());
        let out = insert(&webp, &tiff).unwrap();
        assert_eq!(&out[12..16], b"VP8X");
        assert_eq!(out[20], 0x08);
        assert_eq!(&out[24..27], &99u32.to_le_bytes()[..3]);
        assert_eq!(&out[27..30], &49u32.to_le_bytes()[..3]);
        assert_eq!(
            u32::from_le_bytes(out[4..8].try_into().unwrap()) as usize,
            out.len() - 8
        );
        assert_eq!(extract(&out).unwrap(), tiff);
        let chunks = webp_chunks(&out).unwrap();
        let kinds: Vec<&[u8; 4]> = chunks.iter().map(|c| &c.kind).collect();
        assert_eq!(kinds, [b"VP8X", b"VP8 ", b"EXIF"]);
    }

    /// Minimal AVIF-like file: ftyp, meta (pitm, iinf, iloc), mdat.
    fn heif() -> Vec<u8> {
        let mut ftyp = make_box(b"ftyp", b"avif\0\0\0\0avifmif1");
        let pitm = make_box(b"pitm", &[0, 0, 0, 0, 0, 1]);
        let mut infe = vec![2, 0, 0, 0, 0, 1, 0, 0];
        infe.extend(b"av01\0");
        let mut iinf_body = vec![0, 0, 0, 0, 0, 1];
        iinf_body.extend(make_box(b"infe", &infe));
        let iinf = make_box(b"iinf", &iinf_body);
        let picture = b"PICTURE-DATA";
        let iloc_for = |offset: u32| {
            let mut body = vec![0, 0, 0, 0, 0x44, 0x00, 0, 1, 0, 1, 0, 0, 0, 1];
            body.extend(offset.to_be_bytes());
            body.extend((picture.len() as u32).to_be_bytes());
            make_box(b"iloc", &body)
        };
        let meta_len = 8 + 4 + pitm.len() + iinf.len() + iloc_for(0).len();
        let offset = (ftyp.len() + meta_len + 8) as u32;
        let mut meta_body = vec![0, 0, 0, 0];
        meta_body.extend(&pitm);
        meta_body.extend(&iinf);
        meta_body.extend(iloc_for(offset));
        ftyp.extend(make_box(b"meta", &meta_body));
        ftyp.extend(make_box(b"mdat", picture));
        ftyp
    }

    #[test]
    fn heif_gets_an_exif_item_and_keeps_its_picture() {
        let tiff = tiff_with_thumbnail();
        let file = heif();
        assert_eq!(extract(&file), None);
        let out = insert(&file, &tiff).unwrap();
        assert_eq!(extract(&out).unwrap(), tiff);

        // The picture item still points at its (moved) data.
        let (_, children) = heif_meta(&out).unwrap();
        let iloc_box = children.iter().find(|b| &b.kind == b"iloc").unwrap();
        let iloc = parse_iloc(&out, iloc_box).unwrap();
        let picture = iloc.items.iter().find(|i| i.id == 1).unwrap();
        let (_, offset, length) = picture.extents[0];
        assert_eq!(
            &out[offset as usize..(offset + length) as usize],
            b"PICTURE-DATA"
        );
        let iinf = parse_iinf(&out, children.iter().find(|b| &b.kind == b"iinf").unwrap()).unwrap();
        assert_eq!(iinf, [(1, *b"av01"), (2, *b"Exif")]);
        assert!(children.iter().any(|b| &b.kind == b"iref"));
        assert!(insert(&out, &tiff).is_err(), "never two EXIF items");
    }

    #[test]
    fn strip_removes_every_exif_block() {
        let tiff = tiff_with_thumbnail();
        let with = insert(&jpeg(), &tiff).unwrap();
        let without = strip(&with).unwrap();
        assert_eq!(extract(&without), None);
        assert!(without.ends_with(&[0xff, 0xd9]));
        assert_eq!(&without[2..4], [0xff, 0xe0], "JFIF stays");

        let mut vp8l = vec![0x2f];
        vp8l.extend((9u32 | 9 << 14 | 1 << 28).to_le_bytes());
        let mut webp = b"RIFF\0\0\0\0WEBPVP8L".to_vec();
        webp.extend((vp8l.len() as u32).to_le_bytes());
        webp.extend(&vp8l);
        webp.push(0);
        let with = insert(&webp, &tiff).unwrap();
        // VP8L with transparency: the extended header says so.
        assert_eq!(with[20], 0x08 | 0x10);
        let without = strip(&with).unwrap();
        assert_eq!(extract(&without), None);
        assert_eq!(without[20], 0x10);
        assert_eq!(
            u32::from_le_bytes(without[4..8].try_into().unwrap()) as usize,
            without.len() - 8
        );
    }

    #[test]
    fn garbage_never_panics() {
        for data in [
            &b""[..],
            b"\xff\xd8\xff",
            b"RIFF\0\0\0\0WEBP",
            b"\0\0\0\x0cftypheic",
            b"II*\0\xff\xff\xff\xff",
        ] {
            assert_eq!(extract(data), None);
            assert!(insert(data, &tiff_with_thumbnail()).is_err());
            assert_eq!(prepare(data, true, None), None);
            assert!(!has_location(data));
        }
    }
}
