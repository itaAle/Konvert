//! Test pictures FFmpeg cannot write by itself: EXIF data inside a JPEG and
//! HEIC files (an HEIF grid of HEVC tiles, as phones store their photos).
//! Everything is built in memory from FFmpeg output; no file is stored in the
//! repository.

/// EXIF data (big-endian TIFF) with a camera make and an orientation
/// (6 = the camera was turned: rotate 90° clockwise to display).
pub fn exif_tiff(orientation: u16) -> Vec<u8> {
    let make = b"KonvertCam\0";
    let mut t = Vec::new();
    t.extend(b"MM\0\x2a");
    t.extend(8u32.to_be_bytes());
    t.extend(2u16.to_be_bytes());
    // Make (ASCII), stored after the directory: 8 + 2 + 2 × 12 + 4 = 38.
    t.extend(0x010fu16.to_be_bytes());
    t.extend(2u16.to_be_bytes());
    t.extend((make.len() as u32).to_be_bytes());
    t.extend(38u32.to_be_bytes());
    // Orientation (SHORT), value stored in place.
    t.extend(0x0112u16.to_be_bytes());
    t.extend(3u16.to_be_bytes());
    t.extend(1u32.to_be_bytes());
    t.extend(orientation.to_be_bytes());
    t.extend([0, 0]);
    // No further directory.
    t.extend(0u32.to_be_bytes());
    t.extend(make);
    t
}

/// EXIF data (big-endian TIFF) with an orientation and a GPS position
/// (latitude 45° 30′ N, stored as three rationals).
pub fn exif_tiff_with_gps(orientation: u16) -> Vec<u8> {
    let mut t = Vec::new();
    t.extend(b"MM\0\x2a");
    t.extend(8u32.to_be_bytes());
    let entry = |t: &mut Vec<u8>, tag: u16, kind: u16, count: u32, value: [u8; 4]| {
        t.extend(tag.to_be_bytes());
        t.extend(kind.to_be_bytes());
        t.extend(count.to_be_bytes());
        t.extend(value);
    };
    // IFD0 at 8: orientation and the GPS link → 8 + 2 + 24 + 4 = 38.
    t.extend(2u16.to_be_bytes());
    entry(
        &mut t,
        0x0112,
        3,
        1,
        [(orientation >> 8) as u8, orientation as u8, 0, 0],
    );
    entry(&mut t, 0x8825, 4, 1, 38u32.to_be_bytes());
    t.extend(0u32.to_be_bytes());
    // GPS IFD at 38: latitude ref and latitude (rationals at 38 + 30 = 68).
    t.extend(2u16.to_be_bytes());
    entry(&mut t, 0x0001, 2, 2, *b"N\0\0\0");
    entry(&mut t, 0x0002, 5, 3, 68u32.to_be_bytes());
    t.extend(0u32.to_be_bytes());
    for (num, den) in [(45u32, 1u32), (30, 1), (0, 1)] {
        t.extend(num.to_be_bytes());
        t.extend(den.to_be_bytes());
    }
    t
}

/// A PDF with one page per size (points), each filled with a gray rectangle
/// and carrying personal metadata (author, date) that conversions must drop.
pub fn pdf(sizes: &[(f32, f32)]) -> Vec<u8> {
    use lopdf::{Dictionary, Document, Object, Stream, dictionary};
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let mut kids = Vec::new();
    for (w, h) in sizes {
        let content = format!("0.5 g 10 10 {} {} re f", w - 20.0, h - 20.0);
        let content_id = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
        kids.push(Object::from(doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), (*w).into(), (*h).into()],
            "Contents" => content_id,
        })));
    }
    let count = kids.len() as i64;
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    let info = doc.add_object(dictionary! {
        "Author" => Object::string_literal("Mario Rossi"),
        "CreationDate" => Object::string_literal("D:20260101120000"),
    });
    doc.trailer.set("Root", catalog);
    doc.trailer.set("Info", info);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}

/// The same PDF, protected: it needs `user_password` to be opened.
pub fn encrypted_pdf(user_password: &str) -> Vec<u8> {
    use lopdf::encryption::crypt_filters::{Aes128CryptFilter, CryptFilter};
    use lopdf::{Document, EncryptionState, EncryptionVersion, Object, Permissions};
    use std::collections::BTreeMap;
    use std::sync::Arc;
    let mut doc = Document::load_mem(&pdf(&[(595.0, 842.0)])).unwrap();
    doc.trailer.set(
        "ID",
        vec![
            Object::string_literal("0123456789abcdef"),
            Object::string_literal("0123456789abcdef"),
        ],
    );
    let filter: Arc<dyn CryptFilter> = Arc::new(Aes128CryptFilter);
    let version = EncryptionVersion::V4 {
        document: &doc,
        encrypt_metadata: true,
        crypt_filters: BTreeMap::from([(b"StdCF".to_vec(), filter)]),
        stream_filter: b"StdCF".to_vec(),
        string_filter: b"StdCF".to_vec(),
        owner_password: "proprietario",
        user_password,
        permissions: Permissions::all(),
    };
    doc.encrypt(&EncryptionState::try_from(version).unwrap())
        .unwrap();
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}

/// Adds an EXIF segment (APP1) right after the start of a JPEG file.
pub fn jpeg_with_exif(jpeg: &[u8], orientation: u16) -> Vec<u8> {
    jpeg_with_exif_block(jpeg, &exif_tiff(orientation))
}

/// Adds the given EXIF data (TIFF) as an APP1 segment of a JPEG file.
pub fn jpeg_with_exif_block(jpeg: &[u8], tiff: &[u8]) -> Vec<u8> {
    assert_eq!(&jpeg[..2], [0xff, 0xd8], "not a JPEG file");
    let mut payload = b"Exif\0\0".to_vec();
    payload.extend(tiff);
    let mut out = jpeg[..2].to_vec();
    out.extend([0xff, 0xe1]);
    out.extend(((payload.len() + 2) as u16).to_be_bytes());
    out.extend(payload);
    out.extend(&jpeg[2..]);
    out
}

/// A picture made of `columns` × `rows` tiles of `tile_width` × `tile_height`.
pub struct HeifGrid<'a> {
    /// HEVC tiles (Annex B streams with one picture each), row by row.
    pub tiles: &'a [Vec<u8>],
    pub columns: u8,
    pub rows: u8,
    pub tile_width: u32,
    pub tile_height: u32,
    /// Display rotation, counter-clockwise, in quarter turns (`irot`).
    pub quarter_turns: u8,
    /// EXIF data (TIFF) attached to the picture.
    pub exif: Option<Vec<u8>>,
}

/// Writes a HEIC file: `ftyp`, `meta` (items, properties, locations) and `mdat`.
pub fn heif_grid(grid: &HeifGrid) -> Vec<u8> {
    let nals: Vec<Vec<&[u8]>> = grid.tiles.iter().map(|t| split_nal_units(t)).collect();
    let parameter_sets: Vec<&[u8]> = nals[0]
        .iter()
        .copied()
        .filter(|n| matches!(nal_type(n), 32..=34))
        .collect();
    // Tile samples: the picture NAL units, each prefixed with its length.
    let samples: Vec<Vec<u8>> = nals
        .iter()
        .map(|units| {
            units
                .iter()
                .filter(|n| !matches!(nal_type(n), 32..=35))
                .flat_map(|n| [&(n.len() as u32).to_be_bytes()[..], n].concat())
                .collect()
        })
        .collect();

    let width = grid.columns as u32 * grid.tile_width;
    let height = grid.rows as u32 * grid.tile_height;
    let grid_id = 1u16;
    let tile_ids: Vec<u16> = (2..2 + samples.len() as u16).collect();
    let exif_id = 2 + samples.len() as u16;

    let mut grid_data = vec![0, 0, grid.rows - 1, grid.columns - 1];
    grid_data.extend((width as u16).to_be_bytes());
    grid_data.extend((height as u16).to_be_bytes());
    let mut items: Vec<(u16, &[u8], &[u8; 4])> = vec![(grid_id, &grid_data, b"grid")];
    items.extend(
        tile_ids
            .iter()
            .zip(&samples)
            .map(|(id, s)| (*id, &s[..], b"hvc1")),
    );
    let exif_data = grid
        .exif
        .as_ref()
        .map(|tiff| [&[0u8; 4][..], tiff].concat());
    if let Some(data) = &exif_data {
        items.push((exif_id, data, b"Exif"));
    }

    // Properties (1-based): hvcC, tile size, picture size, rotation.
    let mut properties = vec![
        hvcc(&parameter_sets),
        full_box(
            b"ispe",
            0,
            &[
                &grid.tile_width.to_be_bytes(),
                &grid.tile_height.to_be_bytes(),
            ],
        ),
        full_box(b"ispe", 0, &[&width.to_be_bytes(), &height.to_be_bytes()]),
    ];
    let mut grid_properties = vec![0x03u8];
    if !grid.quarter_turns.is_multiple_of(4) {
        properties.push(make_box(b"irot", &[&[grid.quarter_turns % 4]]));
        grid_properties.push(0x84);
    }
    let mut associations = Vec::new();
    associations.extend((1 + tile_ids.len() as u32).to_be_bytes());
    associations.extend(grid_id.to_be_bytes());
    associations.push(grid_properties.len() as u8);
    associations.extend(&grid_properties);
    for id in &tile_ids {
        associations.extend(id.to_be_bytes());
        associations.extend([2, 0x81, 0x02]);
    }

    let item_infos: Vec<Vec<u8>> = items
        .iter()
        .map(|(id, _, kind)| full_box(b"infe", 2 << 24, &[&id.to_be_bytes(), &[0, 0], *kind, &[0]]))
        .collect();
    let mut item_info = (items.len() as u16).to_be_bytes().to_vec();
    item_info.extend(item_infos.concat());

    let mut dimg = grid_id.to_be_bytes().to_vec();
    dimg.extend((tile_ids.len() as u16).to_be_bytes());
    tile_ids.iter().for_each(|id| dimg.extend(id.to_be_bytes()));
    let mut references = make_box(b"dimg", &[&dimg]);
    if exif_data.is_some() {
        let cdsc = [
            exif_id.to_be_bytes(),
            1u16.to_be_bytes(),
            grid_id.to_be_bytes(),
        ]
        .concat();
        references.extend(make_box(b"cdsc", &[&cdsc]));
    }

    let ftyp = make_box(b"ftyp", &[b"heic", &0u32.to_be_bytes(), b"mif1heic"]);
    let meta = |data_start: u32| {
        // Item locations: absolute offsets inside `mdat`, 4-byte fields.
        let mut locations = vec![0x44, 0x00];
        locations.extend((items.len() as u16).to_be_bytes());
        let mut offset = data_start;
        for (id, data, _) in &items {
            locations.extend(id.to_be_bytes());
            locations.extend([0, 0, 0, 1]);
            locations.extend(offset.to_be_bytes());
            locations.extend((data.len() as u32).to_be_bytes());
            offset += data.len() as u32;
        }
        full_box(
            b"meta",
            0,
            &[
                &full_box(b"hdlr", 0, &[&[0; 4], b"pict", &[0; 13]]),
                &full_box(b"pitm", 0, &[&grid_id.to_be_bytes()]),
                &full_box(b"iinf", 0, &[&item_info]),
                &full_box(b"iref", 0, &[&references]),
                &make_box(
                    b"iprp",
                    &[
                        &make_box(b"ipco", &[&properties.concat()]),
                        &full_box(b"ipma", 0, &[&associations]),
                    ],
                ),
                &full_box(b"iloc", 0, &[&locations]),
            ],
        )
    };
    // The size of `meta` does not depend on the offsets it stores.
    let data_start = (ftyp.len() + meta(0).len() + 8) as u32;
    let data: Vec<u8> = items
        .iter()
        .flat_map(|(_, d, _)| d.iter().copied())
        .collect();
    [ftyp, meta(data_start), make_box(b"mdat", &[&data])].concat()
}

fn make_box(kind: &[u8; 4], parts: &[&[u8]]) -> Vec<u8> {
    let body = parts.concat();
    [&((body.len() + 8) as u32).to_be_bytes()[..], kind, &body].concat()
}

/// Box with a version (high byte of `version_flags`) and 24-bit flags.
fn full_box(kind: &[u8; 4], version_flags: u32, parts: &[&[u8]]) -> Vec<u8> {
    let header = version_flags.to_be_bytes();
    let all: Vec<&[u8]> = std::iter::once(&header[..])
        .chain(parts.iter().copied())
        .collect();
    make_box(kind, &all)
}

/// HEVC decoder configuration (8-bit 4:2:0 Main) with the parameter sets.
fn hvcc(parameter_sets: &[&[u8]]) -> Vec<u8> {
    let mut c = vec![1, 0x01];
    c.extend(0x6000_0000u32.to_be_bytes());
    c.extend([0; 6]);
    c.push(90);
    c.extend(0xf000u16.to_be_bytes());
    c.extend([0xfc, 0xfd, 0xf8, 0xf8]);
    c.extend(0u16.to_be_bytes());
    // One temporal layer, nested; 4-byte NAL unit lengths.
    c.push(0x0f);
    c.push(parameter_sets.len() as u8);
    for unit in parameter_sets {
        c.push(0x80 | nal_type(unit));
        c.extend(1u16.to_be_bytes());
        c.extend((unit.len() as u16).to_be_bytes());
        c.extend(*unit);
    }
    make_box(b"hvcC", &[&c])
}

fn nal_type(unit: &[u8]) -> u8 {
    (unit[0] >> 1) & 0x3f
}

/// NAL units of an Annex B stream (start codes removed).
fn split_nal_units(stream: &[u8]) -> Vec<&[u8]> {
    let starts: Vec<usize> = (0..stream.len().saturating_sub(2))
        .filter(|&i| stream[i..i + 3] == [0, 0, 1])
        .map(|i| i + 3)
        .collect();
    starts
        .iter()
        .enumerate()
        .map(|(n, &start)| {
            let end = starts.get(n + 1).map_or(stream.len(), |next| next - 3);
            let unit = &stream[start..end];
            // A 4-byte start code leaves a zero byte at the end of the previous unit.
            let trimmed = unit.iter().rposition(|b| *b != 0).map_or(0, |p| p + 1);
            &unit[..trimmed]
        })
        .filter(|unit| !unit.is_empty())
        .collect()
}

/// An A4 PDF with a line of text and a 1500×1000 photo-like picture
/// (gradients and grain, stored without loss) drawn 3 inches wide, plus
/// personal metadata.
pub fn pdf_with_photo() -> Vec<u8> {
    use lopdf::{Document, Object, Stream, dictionary};
    let (w, h) = (1500u32, 1000u32);
    let mut seed = 12345u32;
    let mut pixels = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let grain = (seed >> 16) as i32 % 17 - 8;
            let channel = |v: u32| (v as i32 + grain).clamp(0, 255) as u8;
            pixels.extend([
                channel(x * 255 / w),
                channel(y * 255 / h),
                channel((x + y) % 256),
            ]);
        }
    }
    let mut doc = Document::with_version("1.7");
    let mut photo = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => w as i64,
            "Height" => h as i64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
        },
        pixels,
    );
    photo.compress().unwrap();
    let photo_id = doc.add_object(photo);
    let font = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let content = "BT /F1 18 Tf 72 760 Td (Relazione) Tj ET q 216 0 0 144 72 500 cm /Im0 Do Q";
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
    let pages_id = doc.new_object_id();
    let page = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Contents" => content_id,
        "Resources" => dictionary! {
            "Font" => dictionary! { "F1" => font },
            "XObject" => dictionary! { "Im0" => photo_id },
        },
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(
            dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 },
        ),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    let info = doc.add_object(dictionary! { "Author" => Object::string_literal("Mario Rossi") });
    doc.trailer.set("Root", catalog);
    doc.trailer.set("Info", info);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}

/// An A4 PDF whose page is a heavy drawing: 30 000 thin lines, stored without
/// compression.
pub fn pdf_with_drawing() -> Vec<u8> {
    use lopdf::{Document, Object, Stream, dictionary};
    let mut seed = 777u32;
    let mut next = |max: u32| {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        (seed >> 8) % max
    };
    let mut content = String::from("0.2 w\n");
    for _ in 0..30_000 {
        let (x, y) = (next(595), next(842));
        let (dx, dy) = (next(20), next(20));
        content.push_str(&format!("{x} {y} m {} {} l S\n", x + dx, y + dy));
    }
    let mut doc = Document::with_version("1.7");
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
    let pages_id = doc.new_object_id();
    let page = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Contents" => content_id,
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(
            dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 },
        ),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}

/// A Word document as Word writes them: theme fonts, styles with
/// inheritance, a numbered list, a table with merged cells, a header with
/// the page number, a footnote, a page break, and `picture` (PNG) shown
/// `shown_pt` points wide and high.
pub fn docx(picture: &[u8], shown_pt: (f32, f32)) -> Vec<u8> {
    use std::io::Write;
    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    const WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
    const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
    const PIC: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
    const PR: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
    let (cx, cy) = ((shown_pt.0 * 12700.0) as i64, (shown_pt.1 * 12700.0) as i64);
    let ns = format!(
        "xmlns:w=\"{W}\" xmlns:r=\"{R}\" xmlns:wp=\"{WP}\" xmlns:a=\"{A}\" xmlns:pic=\"{PIC}\""
    );
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document {ns}><w:body>
<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Relazione annuale</w:t></w:r></w:p>
<w:p><w:r><w:t xml:space="preserve">Testo con </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>grassetto</w:t></w:r><w:r><w:t xml:space="preserve"> e una nota</w:t></w:r><w:r><w:rPr><w:vertAlign w:val="superscript"/></w:rPr><w:footnoteReference w:id="1"/></w:r><w:r><w:t>.</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Primo punto</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Secondo punto</w:t></w:r></w:p>
<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="0" w:type="auto"/></w:tblPr>
<w:tblGrid><w:gridCol w:w="3000"/><w:gridCol w:w="3000"/><w:gridCol w:w="3000"/></w:tblGrid>
<w:tr><w:tc><w:tcPr><w:vMerge w:val="restart"/><w:shd w:val="clear" w:fill="D9E2F3"/></w:tcPr><w:p><w:r><w:t>Alta</w:t></w:r></w:p></w:tc>
<w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>Cella unita</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc><w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>C</w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>
<w:p><w:r><w:br w:type="page"/></w:r><w:r><w:t>Seconda pagina</w:t></w:r></w:p>
<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:drawing><wp:inline><wp:extent cx="{cx}" cy="{cy}"/><wp:docPr id="1" name="Foto"/><a:graphic><a:graphicData uri="{PIC}"><pic:pic><pic:blipFill><a:blip r:embed="rIdPicture"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>
<w:sectPr><w:headerReference w:type="default" r:id="rIdHeader"/><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1134" w:bottom="1134" w:left="1134" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr>
</w:body></w:document>"#
    );
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="{W}"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:asciiTheme="minorHAnsi" w:hAnsiTheme="minorHAnsi"/><w:sz w:val="22"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>
<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>
<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:pPr><w:keepNext/><w:spacing w:before="240" w:after="0"/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:rFonts w:asciiTheme="majorHAnsi" w:hAnsiTheme="majorHAnsi"/><w:color w:val="2F5496"/><w:sz w:val="32"/></w:rPr></w:style>
<w:style w:type="table" w:styleId="TableGrid"><w:name w:val="Table Grid"/><w:tblPr><w:tblBorders><w:top w:val="single" w:sz="4" w:color="auto"/><w:left w:val="single" w:sz="4" w:color="auto"/><w:bottom w:val="single" w:sz="4" w:color="auto"/><w:right w:val="single" w:sz="4" w:color="auto"/><w:insideH w:val="single" w:sz="4" w:color="auto"/><w:insideV w:val="single" w:sz="4" w:color="auto"/></w:tblBorders></w:tblPr></w:style>
</w:styles>"#
    );
    let numbering = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:numbering xmlns:w="{W}"><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>"#
    );
    let header = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:hdr xmlns:w="{W}"><w:p><w:pPr><w:jc w:val="right"/></w:pPr><w:r><w:t xml:space="preserve">Pagina </w:t></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> PAGE </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>1</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p></w:hdr>"#
    );
    let footnotes = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:footnotes xmlns:w="{W}"><w:footnote w:type="separator" w:id="-1"><w:p/></w:footnote><w:footnote w:id="1"><w:p><w:r><w:t>Testo della nota.</w:t></w:r></w:p></w:footnote></w:footnotes>"#
    );
    let theme = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="{A}" name="Office"><a:themeElements><a:fontScheme name="Office"><a:majorFont><a:latin typeface="Calibri Light"/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/></a:minorFont></a:fontScheme></a:themeElements></a:theme>"#
    );
    let rels = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{PR}"><Relationship Id="rIdStyles" Type="{R}/styles" Target="styles.xml"/><Relationship Id="rIdNumbering" Type="{R}/numbering" Target="numbering.xml"/><Relationship Id="rIdHeader" Type="{R}/header" Target="header1.xml"/><Relationship Id="rIdFootnotes" Type="{R}/footnotes" Target="footnotes.xml"/><Relationship Id="rIdTheme" Type="{R}/theme" Target="theme/theme1.xml"/><Relationship Id="rIdPicture" Type="{R}/image" Target="media/image1.png"/></Relationships>"#
    );
    let package_rels = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{PR}"><Relationship Id="rId1" Type="{R}/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="{R}/metadata/thumbnail" Target="docProps/thumbnail.jpeg"/></Relationships>"#
    );
    let types = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Default Extension="jpeg" ContentType="image/jpeg"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let parts: [(&str, &[u8]); 11] = [
        ("[Content_Types].xml", types.as_bytes()),
        ("_rels/.rels", package_rels.as_bytes()),
        ("docProps/thumbnail.jpeg", &[0xFF, 0xD8, 0xFF, 0xD9]),
        ("word/document.xml", document.as_bytes()),
        ("word/_rels/document.xml.rels", rels.as_bytes()),
        ("word/styles.xml", styles.as_bytes()),
        ("word/numbering.xml", numbering.as_bytes()),
        ("word/header1.xml", header.as_bytes()),
        ("word/footnotes.xml", footnotes.as_bytes()),
        ("word/theme/theme1.xml", theme.as_bytes()),
        ("word/media/image1.png", picture),
    ];
    for (name, data) in parts {
        zip.start_file(name, options).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// A photo-like PNG (gradients and grain): large when stored without loss.
pub fn noisy_png(width: u32, height: u32) -> Vec<u8> {
    let mut seed = 99u32;
    let pixels: Vec<u8> = (0..width * height)
        .flat_map(|i| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let grain = ((seed >> 16) % 25) as i32 - 12;
            let (x, y) = (i % width, i / width);
            [x * 255 / width, y * 255 / height, (x + y) % 256]
                .map(|v| (v as i32 + grain).clamp(0, 255) as u8)
        })
        .collect();
    let mut png = Vec::new();
    image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new(&mut png),
        &pixels,
        width,
        height,
        image::ExtendedColorType::Rgb8,
    )
    .unwrap();
    png
}
