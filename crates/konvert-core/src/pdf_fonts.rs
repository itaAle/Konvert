//! Smaller embedded fonts, for Compress: a TrueType font keeps only the
//! glyphs the document shows.
//!
//! Glyph numbers never change: the glyphs nobody uses are emptied, so the
//! codes in the pages, the font's own character maps and the text a reader
//! selects or searches stay exactly as they were. Layout tables that PDF
//! readers never use (substitutions, positioning, kerning, metrics for
//! screen sizes, the font's signature) go as well.
//!
//! Which glyphs are shown comes from every content stream that can show text
//! (pages, form XObjects, tiling patterns, Type 3 glyphs): the codes drawn
//! with each font. A font stays as it is when anything is unclear: a stream
//! that cannot be read, a font used by fillable fields (new text may be
//! typed), a font never seen in use, an encoding other than Identity for a
//! Type 0 font, or a font program Konvert does not understand completely.
//! CFF and Type 1 font programs are not reduced.

use std::collections::{BTreeSet, HashMap, HashSet};

use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

/// What happened to one embedded font.
#[derive(Debug, Clone, PartialEq)]
pub struct FontReport {
    /// The font's name (`BaseFont`).
    pub name: String,
    /// The kind of font program.
    pub kind: &'static str,
    /// Size of its program, uncompressed.
    pub bytes: usize,
    /// Its new size, when it was made smaller.
    pub reduced: Option<usize>,
}

/// A reduced font is kept only if it is at least this much smaller.
const MIN_GAIN: f64 = 0.10;
/// Deepest nesting of form XObjects followed.
const MAX_FORM_DEPTH: u32 = 16;
/// Tables PDF readers never use: glyph substitutions and positioning,
/// kerning, metrics precomputed for screen sizes, the font's digital
/// signature (no longer valid once the font changes).
const LAYOUT_TABLES: [&[u8; 4]; 16] = [
    b"DSIG", b"GSUB", b"GPOS", b"GDEF", b"JSTF", b"BASE", b"MATH", b"kern", b"kerx", b"morx",
    b"mort", b"feat", b"trak", b"hdmx", b"LTSH", b"VDMX",
];

/// Makes the TrueType fonts of `doc` smaller; reports every embedded font.
pub fn subset_fonts(doc: &mut Document) -> Vec<FontReport> {
    let fonts = find_fonts(doc);
    let usage = Usage::scan(doc);
    // The glyphs to keep in each font program (`None`: keep it as it is).
    let mut wanted: HashMap<ObjectId, Option<BTreeSet<u16>>> = HashMap::new();
    for (id, font) in &fonts {
        let Some((file, access)) = &font.program else {
            continue;
        };
        let glyphs = match &usage {
            Some(usage) if !usage.fixed.contains(id) => usage
                .shown
                .get(id)
                .and_then(|shown| program(doc, *file).and_then(|data| access.glyphs(&data, shown))),
            _ => None,
        };
        let slot = wanted.entry(*file).or_insert_with(|| Some(BTreeSet::new()));
        match (slot.as_mut(), glyphs) {
            (Some(kept), Some(glyphs)) => kept.extend(glyphs),
            _ => *slot = None,
        }
    }
    let mut reduced: HashMap<ObjectId, usize> = HashMap::new();
    for (file, glyphs) in wanted {
        let Some(glyphs) = glyphs else { continue };
        let Some(data) = program(doc, file) else {
            continue;
        };
        let Some(smaller) = prune(&data, &glyphs) else {
            continue;
        };
        if (smaller.len() as f64) > data.len() as f64 * (1.0 - MIN_GAIN) {
            continue;
        }
        if let Ok(Object::Stream(stream)) = doc.get_object_mut(file) {
            let mut dict = stream.dict.clone();
            dict.remove(b"Filter");
            dict.remove(b"DecodeParms");
            dict.set("Length1", smaller.len() as i64);
            reduced.insert(file, smaller.len());
            let mut new = Stream::new(dict, smaller);
            let _ = new.compress();
            *stream = new;
        }
    }
    let mut reports: Vec<FontReport> = fonts
        .into_values()
        .map(|font| FontReport {
            reduced: font
                .program
                .as_ref()
                .and_then(|(file, _)| reduced.get(file).copied()),
            name: font.name,
            kind: font.kind,
            bytes: font.bytes,
        })
        .collect();
    reports.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
    reports
}

/// An embedded font.
struct Font {
    name: String,
    kind: &'static str,
    bytes: usize,
    /// Its TrueType program and how codes reach its glyphs, when Konvert can
    /// reduce it.
    program: Option<(ObjectId, Access)>,
}

/// How the codes of a font reach the glyphs of its program.
enum Access {
    /// A simple TrueType font: one byte per character, through the font's
    /// character maps and the PDF encoding.
    Simple(SimpleEncoding),
    /// A Type 0 font whose 2-byte codes are CIDs (Identity-H/V), with the map
    /// from CIDs to glyphs (`None`: the identity).
    Composite(Option<Vec<u8>>),
}

/// The encoding of a simple font.
struct SimpleEncoding {
    base: Base,
    /// Glyph names given to some codes.
    differences: HashMap<u8, Vec<u8>>,
}

#[derive(Clone, Copy, PartialEq)]
enum Base {
    WinAnsi,
    MacRoman,
    /// The standard encoding, or the font's own (no encoding given).
    Other,
}

/// The embedded fonts of a document, by object.
fn find_fonts(doc: &Document) -> HashMap<ObjectId, Font> {
    let mut fonts = HashMap::new();
    for (&id, object) in &doc.objects {
        let Ok(dict) = object.as_dict() else { continue };
        if !dict.has_type(b"Font") {
            continue;
        }
        let name = |d: &Dictionary| {
            d.get(b"BaseFont")
                .and_then(Object::as_name)
                .map(|n| String::from_utf8_lossy(n).into_owned())
                .unwrap_or_default()
        };
        let subtype = dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .unwrap_or(b"");
        let font = match subtype {
            b"TrueType" | b"Type1" | b"MMType1" => {
                let Some((file, key)) = font_file(doc, dict) else {
                    continue;
                };
                Font {
                    name: name(dict),
                    kind: kind(key, doc, file),
                    bytes: program(doc, file).map_or(0, |d| d.len()),
                    program: (subtype == b"TrueType" && key == b"FontFile2")
                        .then(|| (file, Access::Simple(simple_encoding(doc, dict)))),
                }
            }
            b"Type0" => {
                let Some(descendant) = dict
                    .get(b"DescendantFonts")
                    .map(|d| resolve(doc, d))
                    .and_then(Object::as_array)
                    .ok()
                    .and_then(|a| a.first())
                    .and_then(|d| resolve(doc, d).as_dict().ok())
                else {
                    continue;
                };
                let Some((file, key)) = font_file(doc, descendant) else {
                    continue;
                };
                let identity = dict
                    .get(b"Encoding")
                    .and_then(Object::as_name)
                    .is_ok_and(|e| e == b"Identity-H" || e == b"Identity-V");
                let truetype = descendant
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .is_ok_and(|s| s == b"CIDFontType2")
                    && key == b"FontFile2";
                let map = match descendant.get(b"CIDToGIDMap").map(|m| resolve(doc, m)) {
                    Ok(Object::Stream(map)) => map.decompressed_content().ok().map(Some),
                    Ok(Object::Name(n)) if n == b"Identity" => Some(None),
                    Err(_) => Some(None),
                    _ => None,
                };
                Font {
                    name: name(dict),
                    kind: kind(key, doc, file),
                    bytes: program(doc, file).map_or(0, |d| d.len()),
                    program: match map {
                        Some(map) if identity && truetype => Some((file, Access::Composite(map))),
                        _ => None,
                    },
                }
            }
            _ => continue,
        };
        fonts.insert(id, font);
    }
    fonts
}

/// The embedded program of a font (or CID font): its stream and which key holds it.
fn font_file(doc: &Document, dict: &Dictionary) -> Option<(ObjectId, &'static [u8])> {
    let descriptor = resolve(doc, dict.get(b"FontDescriptor").ok()?)
        .as_dict()
        .ok()?;
    [&b"FontFile2"[..], b"FontFile3", b"FontFile"]
        .into_iter()
        .find_map(|key| Some((descriptor.get(key).ok()?.as_reference().ok()?, key)))
}

fn kind(key: &[u8], doc: &Document, file: ObjectId) -> &'static str {
    match key {
        b"FontFile2" => "TrueType",
        b"FontFile" => "Type 1",
        _ => match doc
            .get_object(file)
            .ok()
            .and_then(|o| o.as_stream().ok())
            .and_then(|s| s.dict.get(b"Subtype").and_then(Object::as_name).ok())
        {
            Some(b"OpenType") => "OpenType",
            _ => "CFF",
        },
    }
}

/// The decompressed bytes of a font program.
fn program(doc: &Document, file: ObjectId) -> Option<Vec<u8>> {
    let stream = doc.get_object(file).ok()?.as_stream().ok()?;
    if stream.dict.has(b"Filter") {
        stream.decompressed_content().ok()
    } else {
        Some(stream.content.clone())
    }
}

fn simple_encoding(doc: &Document, font: &Dictionary) -> SimpleEncoding {
    let base_of = |name: &[u8]| match name {
        b"WinAnsiEncoding" => Base::WinAnsi,
        b"MacRomanEncoding" => Base::MacRoman,
        _ => Base::Other,
    };
    let mut encoding = SimpleEncoding {
        base: Base::Other,
        differences: HashMap::new(),
    };
    match font.get(b"Encoding").map(|e| resolve(doc, e)) {
        Ok(Object::Name(name)) => encoding.base = base_of(name),
        Ok(Object::Dictionary(dict)) => {
            if let Ok(name) = dict.get(b"BaseEncoding").and_then(Object::as_name) {
                encoding.base = base_of(name);
            }
            if let Ok(differences) = dict
                .get(b"Differences")
                .map(|d| resolve(doc, d))
                .and_then(Object::as_array)
            {
                let mut code: i64 = 0;
                for item in differences {
                    match item {
                        Object::Integer(n) => code = *n,
                        Object::Name(name) => {
                            if let Ok(byte) = u8::try_from(code) {
                                encoding.differences.insert(byte, name.clone());
                            }
                            code += 1;
                        }
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }
    encoding
}

fn resolve<'a>(doc: &'a Document, mut object: &'a Object) -> &'a Object {
    for _ in 0..32 {
        match object {
            Object::Reference(id) => match doc.get_object(*id) {
                Ok(next) => object = next,
                Err(_) => return &Object::Null,
            },
            _ => return object,
        }
    }
    &Object::Null
}

// --- Which codes each font shows ------------------------------------------------------

/// The codes shown with each font.
#[derive(Default)]
struct Shown {
    bytes: BTreeSet<u8>,
    /// Two-byte codes, when every string had an even length.
    pairs: BTreeSet<u16>,
    odd: bool,
}

struct Usage {
    shown: HashMap<ObjectId, Shown>,
    /// Fonts that must stay as they are.
    fixed: HashSet<ObjectId>,
}

impl Usage {
    /// The codes each font shows in the whole document; `None` when text is
    /// shown with a font that cannot be told (nothing is reduced then).
    fn scan(doc: &Document) -> Option<Self> {
        let mut scanner = Scanner {
            doc,
            usage: Usage {
                shown: HashMap::new(),
                fixed: HashSet::new(),
            },
            reached: HashSet::new(),
            lost: false,
        };
        for page in doc.get_pages().into_values() {
            let resources = page_resources(doc, page);
            let mut content = Vec::new();
            let mut readable = true;
            for id in doc.get_page_contents(page) {
                match doc
                    .get_object(id)
                    .and_then(Object::as_stream)
                    .map(Stream::decompressed_content_or_raw)
                {
                    Ok(Some(data)) => {
                        content.extend(data);
                        content.push(b'\n');
                    }
                    _ => readable = false,
                }
            }
            if readable {
                scanner.content(&content, &resources, 0);
            } else {
                scanner.fix_fonts(&resources);
            }
        }
        // Streams with resources of their own: every form XObject (also those
        // of annotations), tiling pattern and Type 3 glyph.
        let mut alone: Vec<(ObjectId, Option<&Dictionary>)> = Vec::new();
        for (&id, object) in &doc.objects {
            match object {
                Object::Stream(stream) => {
                    let dict = &stream.dict;
                    let form = dict
                        .get(b"Subtype")
                        .and_then(Object::as_name)
                        .is_ok_and(|s| s == b"Form");
                    let pattern = dict
                        .get(b"PatternType")
                        .and_then(Object::as_i64)
                        .is_ok_and(|t| t == 1);
                    if form || pattern {
                        let own = dict
                            .get(b"Resources")
                            .ok()
                            .and_then(|r| resolve(doc, r).as_dict().ok());
                        alone.push((id, own));
                    }
                }
                Object::Dictionary(dict) if dict.has_type(b"Font") => {
                    let own = dict
                        .get(b"Resources")
                        .ok()
                        .and_then(|r| resolve(doc, r).as_dict().ok());
                    if let Ok(procs) = dict
                        .get(b"CharProcs")
                        .map(|p| resolve(doc, p))
                        .and_then(Object::as_dict)
                    {
                        alone.extend(
                            procs
                                .iter()
                                .filter_map(|(_, p)| Some((p.as_reference().ok()?, own))),
                        );
                    }
                }
                _ => {}
            }
        }
        for (id, own) in alone {
            // A form without resources reached from a page used the page's.
            if own.is_none() && scanner.reached.contains(&id) {
                continue;
            }
            let resources: Vec<&Dictionary> = own.into_iter().collect();
            match doc
                .get_object(id)
                .and_then(Object::as_stream)
                .map(Stream::decompressed_content_or_raw)
            {
                Ok(Some(data)) => scanner.content(&data, &resources, 0),
                _ => scanner.fix_fonts(&resources),
            }
        }
        // Fields of a form show new text with their default fonts.
        if let Some(dr) = doc
            .catalog()
            .ok()
            .and_then(|c| c.get(b"AcroForm").ok())
            .and_then(|f| resolve(doc, f).as_dict().ok())
            .and_then(|f| f.get(b"DR").ok())
            .and_then(|r| resolve(doc, r).as_dict().ok())
        {
            scanner.fix_fonts(&[dr]);
        }
        (!scanner.lost).then_some(scanner.usage)
    }
}

/// The resources of a page: its own, then those it inherits.
fn page_resources(doc: &Document, page: ObjectId) -> Vec<&Dictionary> {
    let Ok((own, inherited)) = doc.get_page_resources(page) else {
        return Vec::new();
    };
    own.into_iter()
        .chain(
            inherited
                .into_iter()
                .filter_map(|id| doc.get_dictionary(id).ok()),
        )
        .collect()
}

trait DecompressedOrRaw {
    fn decompressed_content_or_raw(&self) -> Option<Vec<u8>>;
}

impl DecompressedOrRaw for Stream {
    /// The decoded content of a stream (`None` if a filter cannot be undone).
    fn decompressed_content_or_raw(&self) -> Option<Vec<u8>> {
        if self.dict.has(b"Filter") {
            self.decompressed_content().ok()
        } else {
            Some(self.content.clone())
        }
    }
}

struct Scanner<'a> {
    doc: &'a Document,
    usage: Usage,
    /// Form XObjects drawn from a content stream.
    reached: HashSet<ObjectId>,
    /// Text was shown with a font that could not be told.
    lost: bool,
}

impl<'a> Scanner<'a> {
    /// An entry of a resource category (`Font`, `XObject`, `ExtGState`).
    fn resource(
        &self,
        resources: &[&'a Dictionary],
        category: &[u8],
        name: &[u8],
    ) -> Option<&'a Object> {
        resources.iter().find_map(|r| {
            let entries = resolve(self.doc, r.get(category).ok()?).as_dict().ok()?;
            entries.get(name).ok()
        })
    }

    /// Fonts of resources whose content cannot be read stay as they are.
    fn fix_fonts(&mut self, resources: &[&Dictionary]) {
        for r in resources {
            if let Some(fonts) = r
                .get(b"Font")
                .ok()
                .and_then(|f| resolve(self.doc, f).as_dict().ok())
            {
                self.usage
                    .fixed
                    .extend(fonts.iter().filter_map(|(_, f)| f.as_reference().ok()));
            }
        }
    }

    fn content(&mut self, data: &[u8], resources: &[&'a Dictionary], depth: u32) {
        let Ok(content) = Content::decode(data) else {
            self.fix_fonts(resources);
            return;
        };
        let mut font: Option<ObjectId> = None;
        let mut saved = Vec::new();
        for op in &content.operations {
            let operands = op.operands.as_slice();
            match (op.operator.as_str(), operands) {
                ("q", _) => saved.push(font),
                ("Q", _) => font = saved.pop().unwrap_or(font),
                ("Tf", [Object::Name(name), ..]) => {
                    font = self
                        .resource(resources, b"Font", name)
                        .and_then(|f| f.as_reference().ok());
                    if font.is_none() {
                        self.lost = true;
                    }
                }
                ("gs", [Object::Name(name)]) => {
                    if let Some(Object::Array(set)) = self
                        .resource(resources, b"ExtGState", name)
                        .and_then(|g| resolve(self.doc, g).as_dict().ok())
                        .and_then(|g| g.get(b"Font").ok())
                        .map(|f| resolve(self.doc, f))
                    {
                        font = set.first().and_then(|f| f.as_reference().ok());
                    }
                }
                ("Tj" | "'", [.., Object::String(text, _)])
                | ("\"", [_, _, Object::String(text, _)]) => self.show(font, text),
                ("TJ", [Object::Array(items)]) => {
                    for item in items {
                        if let Object::String(text, _) = item {
                            self.show(font, text);
                        }
                    }
                }
                ("Do", [Object::Name(name)]) => {
                    let Some(id) = self
                        .resource(resources, b"XObject", name)
                        .and_then(|x| x.as_reference().ok())
                    else {
                        continue;
                    };
                    let Ok(Object::Stream(form)) = self.doc.get_object(id) else {
                        continue;
                    };
                    let is_form = form
                        .dict
                        .get(b"Subtype")
                        .and_then(Object::as_name)
                        .is_ok_and(|s| s == b"Form");
                    // Forms with resources of their own are read on their own.
                    if !is_form || form.dict.has(b"Resources") || !self.reached.insert(id) {
                        continue;
                    }
                    match form.decompressed_content_or_raw() {
                        Some(inner) if depth < MAX_FORM_DEPTH => {
                            self.content(&inner, resources, depth + 1)
                        }
                        _ => self.fix_fonts(resources),
                    }
                }
                _ => {}
            }
        }
    }

    fn show(&mut self, font: Option<ObjectId>, text: &[u8]) {
        let Some(font) = font else {
            // Text without a font: the stream is not valid, nothing is sure.
            self.lost = true;
            return;
        };
        let shown = self.usage.shown.entry(font).or_default();
        shown.bytes.extend(text.iter().copied());
        if text.len().is_multiple_of(2) {
            shown
                .pairs
                .extend(text.chunks(2).map(|p| u16::from_be_bytes([p[0], p[1]])));
        } else {
            shown.odd = true;
        }
    }
}

// --- From codes to glyphs ---------------------------------------------------------------

impl Access {
    /// Glyphs the shown codes may reach (`None`: not sure, keep the font).
    fn glyphs(&self, program: &[u8], shown: &Shown) -> Option<BTreeSet<u16>> {
        let face = ttf_parser::Face::parse(program, 0).ok()?;
        let count = face.number_of_glyphs();
        let mut glyphs = BTreeSet::from([0]);
        match self {
            Access::Composite(map) => {
                if shown.odd {
                    return None;
                }
                for &cid in &shown.pairs {
                    let glyph = match map {
                        None => cid,
                        Some(map) => {
                            let at = usize::from(cid) * 2;
                            u16::from_be_bytes([*map.get(at)?, *map.get(at + 1)?])
                        }
                    };
                    glyphs.insert(glyph);
                }
            }
            Access::Simple(encoding) => {
                let cmap = face.tables().cmap?;
                let mut broad = false;
                for &code in &shown.bytes {
                    // Readers differ: every way a code may reach a glyph counts.
                    glyphs.insert(u16::from(code));
                    let mut unicode = vec![u32::from(code), win_ansi(code), mac_roman(code)];
                    if let Some(name) = encoding.differences.get(&code) {
                        let name = std::str::from_utf8(name).ok()?;
                        match (face.glyph_index_by_name(name), uni_name(name)) {
                            (Some(glyph), _) => {
                                glyphs.insert(glyph.0);
                            }
                            (None, Some(c)) => unicode.push(c),
                            (None, None) => broad = true,
                        }
                    } else if encoding.base == Base::Other && code >= 0x80 {
                        broad = true;
                    }
                    for table in cmap.subtables {
                        let id = (table.platform_id, table.encoding_id);
                        use ttf_parser::PlatformId::{Macintosh, Unicode, Windows};
                        let codes: Vec<u32> = match id {
                            (Windows, 0) => {
                                let c = u32::from(code);
                                vec![c, 0xF000 | c, 0xF100 | c, 0xF200 | c]
                            }
                            (Macintosh, 0) => vec![u32::from(code)],
                            (Windows, 1 | 10) | (Unicode, _) => unicode.clone(),
                            _ => Vec::new(),
                        };
                        glyphs.extend(
                            codes
                                .into_iter()
                                .filter_map(|c| table.glyph_index(c))
                                .map(|g| g.0),
                        );
                    }
                }
                if broad {
                    // A glyph name only the reader knows: keep every glyph a
                    // one-byte encoding of Latin, Greek or Cyrillic text may name.
                    for table in cmap.subtables {
                        if table.is_unicode() {
                            table.codepoints(|c| {
                                if single_byte_range(c)
                                    && let Some(glyph) = table.glyph_index(c)
                                {
                                    glyphs.insert(glyph.0);
                                }
                            });
                        }
                    }
                }
            }
        }
        glyphs.retain(|&g| g < count);
        Some(glyphs)
    }
}

/// A character a one-byte encoding may name: Latin, Greek, Cyrillic,
/// punctuation, currency, letter-like and math symbols, ligatures, the
/// private characters of symbol fonts.
fn single_byte_range(c: u32) -> bool {
    matches!(c, 0x0000..=0x052F | 0x2000..=0x22FF | 0x2500..=0x25FF | 0xF000..=0xF0FF | 0xFB00..=0xFB06)
}

/// The character of a glyph named `uniXXXX` or `uXXXX[XX]`.
fn uni_name(name: &str) -> Option<u32> {
    let hex = name
        .strip_prefix("uni")
        .or_else(|| name.strip_prefix('u'))?;
    let hex = hex.get(..hex.len().min(6))?;
    (hex.len() >= 4)
        .then(|| u32::from_str_radix(hex, 16).ok())
        .flatten()
}

/// Windows-1252 (WinAnsiEncoding): the code's character.
fn win_ansi(code: u8) -> u32 {
    const HIGH: [u32; 32] = [
        0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160,
        0x2039, 0x0152, 0x8D, 0x017D, 0x8F, 0x90, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013,
        0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
    ];
    match code {
        0x80..=0x9F => HIGH[usize::from(code - 0x80)],
        _ => u32::from(code),
    }
}

/// Mac OS Roman (MacRomanEncoding): the code's character.
fn mac_roman(code: u8) -> u32 {
    const HIGH: [u32; 128] = [
        0xC4, 0xC5, 0xC7, 0xC9, 0xD1, 0xD6, 0xDC, 0xE1, 0xE0, 0xE2, 0xE4, 0xE3, 0xE5, 0xE7, 0xE9,
        0xE8, 0xEA, 0xEB, 0xED, 0xEC, 0xEE, 0xEF, 0xF1, 0xF3, 0xF2, 0xF4, 0xF6, 0xF5, 0xFA, 0xF9,
        0xFB, 0xFC, 0x2020, 0xB0, 0xA2, 0xA3, 0xA7, 0x2022, 0xB6, 0xDF, 0xAE, 0xA9, 0x2122, 0xB4,
        0xA8, 0x2260, 0xC6, 0xD8, 0x221E, 0xB1, 0x2264, 0x2265, 0xA5, 0xB5, 0x2202, 0x2211, 0x220F,
        0x3C0, 0x222B, 0xAA, 0xBA, 0x3A9, 0xE6, 0xF8, 0xBF, 0xA1, 0xAC, 0x221A, 0x192, 0x2248,
        0x2206, 0xAB, 0xBB, 0x2026, 0xA0, 0xC0, 0xC3, 0xD5, 0x152, 0x153, 0x2013, 0x2014, 0x201C,
        0x201D, 0x2018, 0x2019, 0xF7, 0x25CA, 0xFF, 0x178, 0x2044, 0x20AC, 0x2039, 0x203A, 0xFB01,
        0xFB02, 0x2021, 0xB7, 0x201A, 0x201E, 0x2030, 0xC2, 0xCA, 0xC1, 0xCB, 0xC8, 0xCD, 0xCE,
        0xCF, 0xCC, 0xD3, 0xD4, 0xF8FF, 0xD2, 0xDA, 0xDB, 0xD9, 0x131, 0x2C6, 0x2DC, 0xAF, 0x2D8,
        0x2D9, 0x2DA, 0xB8, 0x2DD, 0x2DB, 0x2C7,
    ];
    match code {
        0x80..=0xFF => HIGH[usize::from(code - 0x80)],
        _ => u32::from(code),
    }
}

// --- TrueType programs -------------------------------------------------------------------

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

/// The tables of a TrueType font: tag, offset, length.
fn tables(font: &[u8]) -> Option<Vec<([u8; 4], usize, usize)>> {
    let version = u32_at(font, 0)?;
    if version != 0x0001_0000 && version != u32::from_be_bytes(*b"true") {
        return None;
    }
    let count = usize::from(u16_at(font, 4)?);
    (0..count)
        .map(|i| {
            let at = 12 + 16 * i;
            let tag: [u8; 4] = font.get(at..at + 4)?.try_into().ok()?;
            let offset = u32_at(font, at + 8)? as usize;
            let length = u32_at(font, at + 12)? as usize;
            font.get(offset..offset.checked_add(length)?)?;
            Some((tag, offset, length))
        })
        .collect()
}

/// The glyphs a composite glyph is made of.
fn components(glyph: &[u8]) -> Option<Vec<u16>> {
    let mut found = Vec::new();
    let mut at = 10;
    loop {
        let flags = u16_at(glyph, at)?;
        found.push(u16_at(glyph, at + 2)?);
        at += 4 + if flags & 0x0001 != 0 { 4 } else { 2 };
        at += if flags & 0x0008 != 0 {
            2
        } else if flags & 0x0040 != 0 {
            4
        } else if flags & 0x0080 != 0 {
            8
        } else {
            0
        };
        if flags & 0x0020 == 0 || found.len() > 1024 {
            return Some(found);
        }
    }
}

/// A TrueType font program keeping only the glyphs of `keep` and those they
/// are made of; the other glyphs become empty, glyph numbers stay. `None` if
/// the font is not a TrueType font Konvert understands.
pub(crate) fn prune(font: &[u8], keep: &BTreeSet<u16>) -> Option<Vec<u8>> {
    let tables = tables(font)?;
    let table = |tag: &[u8; 4]| {
        tables
            .iter()
            .find(|t| &t.0 == tag)
            .map(|&(_, o, l)| &font[o..o + l])
    };
    let (head, maxp, loca, glyf) = (
        table(b"head")?,
        table(b"maxp")?,
        table(b"loca")?,
        table(b"glyf")?,
    );
    let count = usize::from(u16_at(maxp, 4)?);
    let long = match u16_at(head, 50)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let offsets: Vec<usize> = (0..=count)
        .map(|i| {
            if long {
                u32_at(loca, 4 * i).map(|o| o as usize)
            } else {
                u16_at(loca, 2 * i).map(|o| usize::from(o) * 2)
            }
        })
        .collect::<Option<_>>()?;
    if offsets.windows(2).any(|w| w[0] > w[1]) || offsets[count] > glyf.len() {
        return None;
    }
    let data = |g: usize| &glyf[offsets[g]..offsets[g + 1]];

    // The glyphs kept, with the glyphs composite ones are made of.
    let mut kept = vec![false; count];
    let mut todo: Vec<usize> = keep.iter().map(|&g| usize::from(g)).chain([0]).collect();
    while let Some(g) = todo.pop() {
        if g >= count || std::mem::replace(&mut kept[g], true) {
            continue;
        }
        let glyph = data(g);
        if glyph.len() >= 10 && (u16_at(glyph, 0)? as i16) < 0 {
            todo.extend(components(glyph)?.into_iter().map(usize::from));
        }
    }

    let mut new_glyf = Vec::new();
    let mut new_offsets = Vec::with_capacity(count + 1);
    for (g, &keep) in kept.iter().enumerate() {
        new_offsets.push(new_glyf.len());
        if keep {
            new_glyf.extend_from_slice(data(g));
            while new_glyf.len() % 4 != 0 {
                new_glyf.push(0);
            }
        }
    }
    new_offsets.push(new_glyf.len());
    let short = new_glyf.len() / 2 <= usize::from(u16::MAX);
    let new_loca: Vec<u8> = if short {
        new_offsets
            .iter()
            .flat_map(|&o| ((o / 2) as u16).to_be_bytes())
            .collect()
    } else {
        new_offsets
            .iter()
            .flat_map(|&o| (o as u32).to_be_bytes())
            .collect()
    };
    let mut new_head = head.to_vec();
    new_head[50..52].copy_from_slice(&u16::from(!short).to_be_bytes());
    // The whole-font checksum is computed again below.
    new_head.get_mut(8..12)?.copy_from_slice(&[0; 4]);

    let mut out_tables: Vec<([u8; 4], Vec<u8>)> = tables
        .iter()
        .filter(|(tag, ..)| !LAYOUT_TABLES.contains(&tag))
        .map(|&(tag, offset, length)| {
            let bytes = match &tag {
                b"glyf" => new_glyf.clone(),
                b"loca" => new_loca.clone(),
                b"head" => new_head.clone(),
                _ => font[offset..offset + length].to_vec(),
            };
            (tag, bytes)
        })
        .collect();
    out_tables.sort_by_key(|(tag, _)| *tag);
    let mut font = write_tables(&out_tables);
    // head.checkSumAdjustment makes the whole font add up to 0xB1B0AFBA.
    let adjustment = 0xB1B0_AFBAu32.wrapping_sub(checksum(&font));
    let head_at = 12 + 16 * out_tables.iter().position(|(tag, _)| tag == b"head")?;
    let head_offset = u32_at(&font, head_at + 8)? as usize;
    font.get_mut(head_offset + 8..head_offset + 12)?
        .copy_from_slice(&adjustment.to_be_bytes());
    Some(font)
}

fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

/// A TrueType font file made of `tables` (sorted by tag).
fn write_tables(tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let count = tables.len() as u16;
    let power = 1u16 << (15 - count.max(1).leading_zeros() as u16);
    let search_range = power * 16;
    let mut font = Vec::new();
    font.extend(0x0001_0000u32.to_be_bytes());
    font.extend(count.to_be_bytes());
    font.extend(search_range.to_be_bytes());
    font.extend((power.trailing_zeros() as u16).to_be_bytes());
    font.extend((count * 16 - search_range).to_be_bytes());
    let mut offset = 12 + 16 * tables.len();
    for (tag, bytes) in tables {
        font.extend(tag);
        font.extend(checksum(bytes).to_be_bytes());
        font.extend((offset as u32).to_be_bytes());
        font.extend((bytes.len() as u32).to_be_bytes());
        offset += bytes.len().div_ceil(4) * 4;
    }
    for (_, bytes) in tables {
        font.extend(bytes);
        font.resize(font.len().div_ceil(4) * 4, 0);
    }
    font
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use lopdf::dictionary;
    use std::sync::atomic::AtomicBool;

    /// A TrueType font with `count` glyphs, drawn as boxes of different
    /// widths: glyph 0 is empty, the last one is made of glyphs 1 and 2.
    /// Characters from `A` on reach glyphs from 1 on (cmap 3,1).
    pub(crate) fn sample_font(count: u16) -> Vec<u8> {
        let be16 = |v: i32| (v as u16).to_be_bytes();
        let mut glyf = Vec::new();
        let mut loca = Vec::new();
        for g in 0..count {
            loca.extend((glyf.len() as u32).to_be_bytes());
            let width = 100 + 5 * i32::from(g % 160);
            if g == 0 {
                continue;
            }
            if g == count - 1 {
                glyf.extend(be16(-1));
                for v in [0, 0, width + 300, 700] {
                    glyf.extend(be16(v));
                }
                // ARG_1_AND_2_ARE_WORDS | ARGS_ARE_XY_VALUES (| MORE_COMPONENTS).
                glyf.extend(be16(0x0023));
                glyf.extend(be16(1));
                glyf.extend(be16(0));
                glyf.extend(be16(0));
                glyf.extend(be16(0x0003));
                glyf.extend(be16(2));
                glyf.extend(be16(300));
                glyf.extend(be16(0));
            } else {
                glyf.extend(be16(1));
                for v in [0, 0, width, 700] {
                    glyf.extend(be16(v));
                }
                glyf.extend(be16(3));
                glyf.extend(be16(0));
                glyf.extend([1, 1, 1, 1]);
                for v in [0, width, 0, -width, 0, 0, 700, 0] {
                    glyf.extend(be16(v));
                }
            }
            while glyf.len() % 4 != 0 {
                glyf.push(0);
            }
        }
        loca.extend((glyf.len() as u32).to_be_bytes());

        let mut head = Vec::new();
        head.extend(0x0001_0000u32.to_be_bytes());
        head.extend(0x0001_0000u32.to_be_bytes());
        head.extend(0u32.to_be_bytes());
        head.extend(0x5F0F_3CF5u32.to_be_bytes());
        head.extend(be16(0));
        head.extend(be16(1000));
        head.extend([0; 16]);
        for v in [0, 0, 1000, 700, 0, 8, 2, 1, 0] {
            head.extend(be16(v));
        }
        let mut hhea = Vec::new();
        hhea.extend(0x0001_0000u32.to_be_bytes());
        for v in [800, -200, 0, 1000, 0, 0, 1000, 1, 0, 0, 0, 0, 0, 0, 0] {
            hhea.extend(be16(v));
        }
        hhea.extend(be16(i32::from(count)));
        let mut maxp = Vec::new();
        maxp.extend(0x0001_0000u32.to_be_bytes());
        for v in [i32::from(count), 4, 1, 8, 2, 2, 0, 0, 0, 0, 0, 0, 2, 1] {
            maxp.extend(be16(v));
        }
        let hmtx: Vec<u8> = (0..count)
            .flat_map(|_| [be16(1000), be16(0)].concat())
            .collect();
        // Format 4: one segment from 'A', then the final one.
        let mapped = i32::from(count) - 1;
        let mut format4 = Vec::new();
        for v in [
            4,
            32,
            0,
            4,
            4,
            1,
            0,
            0x40 + mapped,
            0xFFFF,
            0,
            0x41,
            0xFFFF,
            1 - 0x41,
            1,
            0,
            0,
        ] {
            format4.extend(be16(v));
        }
        let mut cmap = Vec::new();
        for v in [0, 1, 3, 1] {
            cmap.extend(be16(v));
        }
        cmap.extend(12u32.to_be_bytes());
        cmap.extend(format4);
        let mut post = Vec::new();
        post.extend(0x0003_0000u32.to_be_bytes());
        post.extend([0; 28]);
        let mut tables = vec![
            (*b"cmap", cmap),
            (*b"glyf", glyf),
            (*b"head", head),
            (*b"hhea", hhea),
            (*b"hmtx", hmtx),
            (*b"loca", loca),
            (*b"maxp", maxp),
            (*b"post", post),
            (*b"GSUB", vec![0; 4000]),
        ];
        tables.sort_by_key(|(tag, _)| *tag);
        write_tables(&tables)
    }

    /// Glyphs of a font program that have an outline.
    fn drawn(font: &[u8]) -> Vec<u16> {
        struct Count(usize);
        impl ttf_parser::OutlineBuilder for Count {
            fn move_to(&mut self, _: f32, _: f32) {
                self.0 += 1;
            }
            fn line_to(&mut self, _: f32, _: f32) {}
            fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {}
            fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {}
            fn close(&mut self) {}
        }
        let face = ttf_parser::Face::parse(font, 0).unwrap();
        (0..face.number_of_glyphs())
            .filter(|&g| {
                let mut count = Count(0);
                face.outline_glyph(ttf_parser::GlyphId(g), &mut count);
                count.0 > 0
            })
            .collect()
    }

    #[test]
    fn a_font_keeps_its_glyph_numbers_and_only_the_glyphs_asked_for() {
        let font = sample_font(300);
        assert_eq!(drawn(&font).len(), 299);
        // Glyph 299 is made of glyphs 1 and 2: they come along.
        let pruned = prune(&font, &BTreeSet::from([5, 299])).unwrap();
        assert_eq!(drawn(&pruned), [1, 2, 5, 299]);
        assert!(
            pruned.len() < font.len() / 3,
            "{} of {}",
            pruned.len(),
            font.len()
        );
        let face = ttf_parser::Face::parse(&pruned, 0).unwrap();
        assert_eq!(face.number_of_glyphs(), 300);
        assert_eq!(face.glyph_index('E').map(|g| g.0), Some(5));
        // Layout tables go; the checksums add up.
        assert!(
            tables(&pruned)
                .unwrap()
                .iter()
                .all(|(tag, ..)| tag != b"GSUB")
        );
        assert_eq!(checksum(&pruned), 0xB1B0_AFBA);
        // Not a TrueType font.
        assert!(prune(b"OTTO and more", &BTreeSet::new()).is_none());
    }

    /// A PDF with two pages: a simple TrueType font (WinAnsi) showing "ABC"
    /// and a Type 0 font (Identity-H) showing glyphs 5, 6 and 299, each
    /// embedding its own 300-glyph program.
    fn fonts_pdf() -> Vec<u8> {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let program = |doc: &mut Document| {
            let font = sample_font(300);
            let length = font.len() as i64;
            let mut stream = Stream::new(dictionary! { "Length1" => length }, font);
            let _ = stream.compress();
            doc.add_object(stream)
        };
        let descriptor = |doc: &mut Document, file: ObjectId| {
            doc.add_object(dictionary! {
                "Type" => "FontDescriptor", "FontName" => "Boxes", "Flags" => 32,
                "FontBBox" => vec![0.into(), 0.into(), 1000.into(), 700.into()],
                "ItalicAngle" => 0, "Ascent" => 800, "Descent" => -200, "CapHeight" => 700,
                "StemV" => 80, "FontFile2" => file,
            })
        };
        let file = program(&mut doc);
        let simple_descriptor = descriptor(&mut doc, file);
        let widths: Vec<Object> = (0..26).map(|_| 1000.into()).collect();
        let simple = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "TrueType", "BaseFont" => "Boxes",
            "FirstChar" => 65, "LastChar" => 90, "Widths" => widths,
            "Encoding" => "WinAnsiEncoding", "FontDescriptor" => simple_descriptor,
        });
        let file = program(&mut doc);
        let cid_descriptor = descriptor(&mut doc, file);
        let cid_font = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "CIDFontType2", "BaseFont" => "Boxes",
            "CIDSystemInfo" => dictionary! {
                "Registry" => Object::string_literal("Adobe"),
                "Ordering" => Object::string_literal("Identity"), "Supplement" => 0,
            },
            "FontDescriptor" => cid_descriptor, "DW" => 1000, "CIDToGIDMap" => "Identity",
        });
        let composite = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "Boxes",
            "Encoding" => "Identity-H", "DescendantFonts" => vec![cid_font.into()],
        });
        let mut kids = Vec::new();
        for text in [
            "BT /F1 40 Tf 20 100 Td (ABC) Tj ET".to_string(),
            "BT /F2 40 Tf 20 100 Td <00050006> Tj [<012B>] TJ ET".to_string(),
        ] {
            let content = doc.add_object(Stream::new(Dictionary::new(), text.into_bytes()));
            kids.push(Object::from(doc.add_object(dictionary! {
                "Type" => "Page", "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 300.into(), 200.into()],
                "Contents" => content,
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => simple, "F2" => composite } },
            })));
        }
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => 2 }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    fn render(data: &[u8]) -> Vec<Vec<u8>> {
        let mut pages = Vec::new();
        crate::pdf::render_pages(data.to_vec(), &[1, 2], 72, &AtomicBool::new(false), |p| {
            pages.push(p.png);
            true
        })
        .unwrap();
        pages
    }

    /// The programs of the fonts of a document, by `BaseFont` and subtype.
    fn programs(doc: &Document) -> Vec<Vec<u8>> {
        let mut found: Vec<Vec<u8>> = doc
            .objects
            .values()
            .filter_map(|o| {
                let stream = o.as_stream().ok()?;
                stream.dict.has(b"Length1").then(|| program_of(stream))
            })
            .collect();
        found.sort_by_key(Vec::len);
        found
    }

    fn program_of(stream: &Stream) -> Vec<u8> {
        if stream.dict.has(b"Filter") {
            stream.decompressed_content().unwrap()
        } else {
            stream.content.clone()
        }
    }

    #[test]
    fn embedded_fonts_keep_the_glyphs_the_pages_show() {
        let data = fonts_pdf();
        let before = render(&data);
        // The boxes are drawn: each page has dark pixels.
        for png in &before {
            let page = image::load_from_memory(png).unwrap().to_luma8();
            let dark = page.pixels().filter(|p| p.0[0] < 128).count();
            assert!(dark > 200, "{dark}");
        }
        let mut doc = Document::load_mem(&data).unwrap();
        let reports = subset_fonts(&mut doc);
        assert_eq!(reports.len(), 2);
        assert!(
            reports
                .iter()
                .all(|r| r.kind == "TrueType" && r.reduced.is_some_and(|n| n < r.bytes / 3)),
            "{reports:?}"
        );
        // "ABC" reaches glyphs 1-3 (and the 256 one-byte codes as glyph
        // numbers, for readers that use them so); CIDs 5, 6 and 299, the
        // last one made of glyphs 1 and 2.
        let kept: Vec<Vec<u16>> = programs(&doc).iter().map(|p| drawn(p)).collect();
        assert_eq!(kept[0], [1, 2, 5, 6, 299]);
        assert_eq!(&kept[1][..3], [1, 2, 3]);
        assert!(kept[1].len() <= 256);
        // The pages look exactly the same.
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        assert_eq!(render(&bytes), before);
    }

    #[test]
    fn compressed_pdfs_get_smaller_fonts_and_look_the_same() {
        use crate::request::{CompressLevel, CompressOptions};
        let data = fonts_pdf();
        let options = CompressOptions {
            level: CompressLevel::Light,
            ..CompressOptions::default()
        };
        let smaller =
            crate::pdf_compress::compress(&data, &options, &AtomicBool::new(false), |_| {})
                .unwrap();
        assert!(
            smaller.len() < data.len() / 2,
            "{} of {}",
            smaller.len(),
            data.len()
        );
        assert_eq!(render(&smaller), render(&data));
    }

    #[test]
    fn fonts_stay_as_they_are_when_anything_is_unclear() {
        let data = fonts_pdf();
        let original = programs(&Document::load_mem(&data).unwrap());
        let unchanged = |doc: &mut Document| {
            let reports = subset_fonts(doc);
            assert_eq!(programs(doc), original);
            reports
        };
        // A page that cannot be read: its fonts stay.
        let mut doc = Document::load_mem(&data).unwrap();
        for page in doc.get_pages().into_values().collect::<Vec<_>>() {
            let content = doc.get_page_contents(page)[0];
            let stream = doc
                .get_object_mut(content)
                .unwrap()
                .as_stream_mut()
                .unwrap();
            stream.dict.set("Filter", "FlateDecode");
        }
        assert!(unchanged(&mut doc).iter().all(|r| r.reduced.is_none()));
        // Fonts of fillable fields: new text may need any glyph.
        let mut doc = Document::load_mem(&data).unwrap();
        let fonts = doc
            .get_page_resources(doc.page_iter().next().unwrap())
            .unwrap()
            .0
            .unwrap()
            .get(b"Font")
            .unwrap()
            .clone();
        let form = doc.add_object(dictionary! { "Fields" => Vec::<Object>::new(), "DR" => dictionary! { "Font" => fonts } });
        doc.catalog_mut().unwrap().set("AcroForm", form);
        unchanged(&mut doc);
        // Text shown with a font the stream does not name: nothing is sure.
        let mut doc = Document::load_mem(&data).unwrap();
        let page = doc.page_iter().next().unwrap();
        let content = doc.get_page_contents(page)[0];
        doc.get_object_mut(content)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_plain_content(b"BT /F9 40 Tf (A) Tj ET".to_vec());
        unchanged(&mut doc);
    }
}
