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
