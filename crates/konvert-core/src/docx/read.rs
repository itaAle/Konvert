//! Reads a DOCX into a simple model: every paragraph and run carries its
//! formatting resolved from the document defaults, the style chain and its
//! own settings; list numbers are computed; pictures are referenced by their
//! part name. What the model cannot hold (text boxes, charts, equations,
//! embedded objects) is left out and noted in [`Document::simplified`].

use std::collections::HashMap;

use roxmltree::{Document as Xml, Node};

use super::{Package, damaged, rels_name, resolve_target};
use crate::error::CoreError;

/// Points per twip (1/20 point), the unit of most Word lengths.
const TWIP: f32 = 1.0 / 20.0;
/// Points per EMU (English Metric Unit), the unit of drawings.
pub const EMU: f32 = 1.0 / 12_700.0;

/// A color as red, green, blue.
pub type Rgb = [u8; 3];

#[derive(Debug, Clone, Default)]
pub struct Document {
    pub body: Vec<Block>,
    pub sections: Vec<Section>,
    pub footnotes: HashMap<String, Vec<Block>>,
    /// Pictures by part name (`word/media/image1.png`).
    pub media: HashMap<String, Vec<u8>>,
    /// Headers differ on even pages.
    pub even_and_odd: bool,
    /// Text boxes, charts, equations or objects were left out.
    pub simplified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSetup {
    pub width: f32,
    pub height: f32,
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
    pub header: f32,
    pub footer: f32,
}

impl Default for PageSetup {
    /// A4 with Word's default margins.
    fn default() -> Self {
        Self {
            width: 595.3,
            height: 841.9,
            top: 70.85,
            bottom: 56.7,
            left: 56.7,
            right: 56.7,
            header: 35.4,
            footer: 35.4,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Section {
    /// The section ends before this block of the body.
    pub end: usize,
    pub page: PageSetup,
    pub columns: u32,
    pub column_gap: f32,
    /// The section starts on a new page (otherwise it continues the page).
    pub new_page: bool,
    /// Page numbering starts again at this number.
    pub page_start: Option<u32>,
    pub title_page: bool,
    pub header: Option<Vec<Block>>,
    pub first_header: Option<Vec<Block>>,
    pub even_header: Option<Vec<Block>>,
    pub footer: Option<Vec<Block>>,
    pub first_footer: Option<Vec<Block>>,
    pub even_footer: Option<Vec<Block>>,
}

#[derive(Debug, Clone)]
pub enum Block {
    Paragraph(Paragraph),
    Table(Table),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineSpacing {
    /// Times the font's natural line height.
    Multiple(f32),
    Exact(f32),
    AtLeast(f32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Paragraph {
    pub align: Align,
    pub before: f32,
    pub after: f32,
    pub line: LineSpacing,
    pub left: f32,
    pub right: f32,
    /// First line indent (negative: hanging).
    pub first_line: f32,
    /// Heading level (0 = title level 1).
    pub outline: Option<u8>,
    pub page_break_before: bool,
    pub keep_lines: bool,
    pub keep_next: bool,
    pub shading: Option<Rgb>,
    /// Formatting of the paragraph mark: the height of an empty paragraph.
    pub mark: RunProps,
    /// List number or bullet, with its own formatting.
    pub label: Option<(String, RunProps)>,
    /// Left tab stops, from the left margin, in points.
    pub tabs: Vec<f32>,
    pub inlines: Vec<Inline>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    Text(String, RunProps),
    Tab(RunProps),
    LineBreak,
    PageBreak,
    ColumnBreak,
    Picture {
        part: String,
        width: f32,
        height: f32,
    },
    Footnote(String),
    PageNumber(RunProps),
    PageCount(RunProps),
    Link {
        url: String,
        inlines: Vec<Inline>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Baseline {
    #[default]
    Normal,
    Super,
    Sub,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunProps {
    pub font: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: Option<Rgb>,
    pub highlight: Option<Rgb>,
    pub baseline: Baseline,
    pub caps: bool,
    pub small_caps: bool,
    /// Extra space between letters, in points.
    pub spacing: f32,
}

impl Default for RunProps {
    fn default() -> Self {
        Self {
            font: "Calibri".into(),
            size: 11.0,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            color: None,
            highlight: None,
            baseline: Baseline::Normal,
            caps: false,
            small_caps: false,
            spacing: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Border {
    pub width: f32,
    pub color: Rgb,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Borders {
    pub top: Option<Border>,
    pub bottom: Option<Border>,
    pub left: Option<Border>,
    pub right: Option<Border>,
    pub inside_h: Option<Border>,
    pub inside_v: Option<Border>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VAlign {
    #[default]
    Top,
    Center,
    Bottom,
}

#[derive(Debug, Clone)]
pub struct Table {
    pub columns: Vec<f32>,
    pub rows: Vec<Row>,
    pub align: Align,
    pub indent: f32,
    pub borders: Borders,
    /// Space inside the cells: top, right, bottom, left.
    pub margins: [f32; 4],
}

#[derive(Debug, Clone)]
pub struct Row {
    pub cells: Vec<Cell>,
    pub header: bool,
}

#[derive(Debug, Clone)]
pub struct Cell {
    /// First grid column of the cell.
    pub column: usize,
    pub span: usize,
    /// Rows the cell covers (merged vertically).
    pub rows: usize,
    /// Covered by the cell above (merged vertically): not drawn.
    pub covered: bool,
    pub fill: Option<Rgb>,
    /// The cell's own borders (they win over the table's).
    pub borders: Borders,
    pub valign: VAlign,
    pub blocks: Vec<Block>,
}

// --- Formatting as written (every setting optional) -------------------------------

#[derive(Debug, Clone, Default)]
struct RunStyle {
    font: Option<String>,
    size: Option<f32>,
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    color: Option<Option<Rgb>>,
    highlight: Option<Option<Rgb>>,
    baseline: Option<Baseline>,
    caps: Option<bool>,
    small_caps: Option<bool>,
    hidden: Option<bool>,
    spacing: Option<f32>,
    style: Option<String>,
}

impl RunStyle {
    /// `other` on top of `self`.
    fn merge(&mut self, other: &RunStyle) {
        macro_rules! take {
            ($($f:ident),*) => { $( if other.$f.is_some() { self.$f = other.$f.clone(); } )* };
        }
        take!(
            font, size, bold, italic, underline, strike, color, highlight, baseline, caps,
            small_caps, hidden, spacing
        );
    }

    fn resolve(&self) -> RunProps {
        let d = RunProps::default();
        RunProps {
            font: self.font.clone().unwrap_or(d.font),
            size: self.size.unwrap_or(d.size),
            bold: self.bold.unwrap_or(false),
            italic: self.italic.unwrap_or(false),
            underline: self.underline.unwrap_or(false),
            strike: self.strike.unwrap_or(false),
            color: self.color.flatten(),
            highlight: self.highlight.flatten(),
            baseline: self.baseline.unwrap_or_default(),
            caps: self.caps.unwrap_or(false),
            small_caps: self.small_caps.unwrap_or(false),
            spacing: self.spacing.unwrap_or(0.0),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct ParaStyle {
    align: Option<Align>,
    before: Option<f32>,
    after: Option<f32>,
    line: Option<LineSpacing>,
    left: Option<f32>,
    right: Option<f32>,
    first_line: Option<f32>,
    outline: Option<Option<u8>>,
    page_break_before: Option<bool>,
    keep_lines: Option<bool>,
    keep_next: Option<bool>,
    shading: Option<Option<Rgb>>,
    numbering: Option<(String, u8)>,
    level: Option<u8>,
    contextual: Option<bool>,
    tabs: Option<Vec<f32>>,
}

impl ParaStyle {
    fn merge(&mut self, other: &ParaStyle) {
        macro_rules! take {
            ($($f:ident),*) => { $( if other.$f.is_some() { self.$f = other.$f.clone(); } )* };
        }
        take!(
            align,
            before,
            after,
            line,
            left,
            right,
            first_line,
            outline,
            page_break_before,
            keep_lines,
            keep_next,
            shading,
            numbering,
            contextual,
            tabs
        );
        if other.level.is_some() {
            self.level = other.level;
        }
    }
}

#[derive(Debug, Clone, Default)]
struct Style {
    based_on: Option<String>,
    para: ParaStyle,
    run: RunStyle,
    table_borders: Option<Borders>,
    table_margins: Option<[Option<f32>; 4]>,
}

/// One level of a list definition.
#[derive(Debug, Clone, Default)]
struct Level {
    start: u32,
    format: String,
    text: String,
    para: ParaStyle,
    run: RunStyle,
}

// --- Reading -------------------------------------------------------------------

/// Context shared while reading the parts of one document.
struct Reader<'p> {
    package: &'p Package,
    styles: HashMap<String, Style>,
    default_run: RunStyle,
    default_para: ParaStyle,
    default_paragraph_style: Option<String>,
    major_font: Option<String>,
    minor_font: Option<String>,
    /// List definitions: num id → levels.
    lists: HashMap<String, Vec<Level>>,
    /// Abstract definition of each num id, to share counters.
    list_family: HashMap<String, String>,
    counters: HashMap<String, Vec<Option<u32>>>,
    media: HashMap<String, Vec<u8>>,
    simplified: bool,
}

/// Relationships of one part: id → (type suffix, resolved target, external).
type Rels = HashMap<String, (String, String, bool)>;

fn local<'a>(node: &Node<'a, '_>) -> &'a str {
    node.tag_name().name()
}

fn attr<'a>(node: &Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.attributes()
        .find(|a| a.name() == name)
        .map(|a| a.value())
}

fn child<'a, 'i>(node: &Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|c| c.is_element() && local(c) == name)
}

fn children<'a, 'i>(node: &Node<'a, 'i>, name: &'static str) -> impl Iterator<Item = Node<'a, 'i>> {
    node.children()
        .filter(move |c| c.is_element() && local(c) == name)
}

fn twips(value: Option<&str>) -> Option<f32> {
    value
        .and_then(|v| v.trim().parse::<f32>().ok())
        .map(|v| v * TWIP)
}

/// On/off properties: present means on, unless `val` says otherwise.
fn toggle(node: &Node) -> bool {
    !matches!(attr(node, "val"), Some("0" | "false" | "off" | "none"))
}

fn hex_color(value: Option<&str>) -> Option<Rgb> {
    let v = value?.trim();
    if v.len() != 6 || v.eq_ignore_ascii_case("auto") {
        return None;
    }
    let n = u32::from_str_radix(v, 16).ok()?;
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

/// Word's named highlight colors.
fn highlight_color(name: &str) -> Option<Rgb> {
    Some(match name {
        "yellow" => [255, 255, 0],
        "green" => [0, 255, 0],
        "cyan" => [0, 255, 255],
        "magenta" => [255, 0, 255],
        "blue" => [0, 0, 255],
        "red" => [255, 0, 0],
        "darkBlue" => [0, 0, 139],
        "darkCyan" => [0, 139, 139],
        "darkGreen" => [0, 100, 0],
        "darkMagenta" => [128, 0, 128],
        "darkRed" => [139, 0, 0],
        "darkYellow" => [128, 128, 0],
        "darkGray" => [169, 169, 169],
        "lightGray" => [211, 211, 211],
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        _ => return None,
    })
}

fn parse_xml(text: &str) -> Result<Xml<'_>, CoreError> {
    let options = roxmltree::ParsingOptions {
        allow_dtd: false,
        ..Default::default()
    };
    Xml::parse_with_options(text, options).map_err(damaged)
}

/// Reads the whole document.
pub fn read(package: &Package) -> Result<Document, CoreError> {
    let mut reader = Reader {
        package,
        styles: HashMap::new(),
        default_run: RunStyle::default(),
        default_para: ParaStyle::default(),
        default_paragraph_style: None,
        major_font: None,
        minor_font: None,
        lists: HashMap::new(),
        list_family: HashMap::new(),
        counters: HashMap::new(),
        media: HashMap::new(),
        simplified: false,
    };
    reader.read_theme();
    reader.read_styles()?;
    reader.read_numbering()?;
    let even_and_odd = package.text("word/settings.xml").is_some_and(|s| {
        s.contains("evenAndOddHeaders") && !s.contains("evenAndOddHeaders w:val=\"0\"")
    });

    let main = "word/document.xml";
    let text = package
        .text(main)
        .ok_or_else(|| damaged("no main document"))?;
    let xml = parse_xml(&text)?;
    let rels = reader.rels(main);
    let body = xml
        .root_element()
        .children()
        .find(|c| c.is_element() && local(c) == "body")
        .ok_or_else(|| damaged("no document body"))?;

    let mut blocks = Vec::new();
    let mut ends: Vec<usize> = Vec::new();
    reader.read_blocks(&body, &rels, main, &mut blocks, &mut |len| ends.push(len))?;
    ends.push(blocks.len());
    let mut section_nodes: Vec<Node> = Vec::new();
    collect_section_nodes(&body, &mut section_nodes);
    let mut sections: Vec<Section> = Vec::new();
    for (i, end) in ends.iter().enumerate() {
        let node = section_nodes.get(i).copied();
        let section = reader.read_section(node, *end, sections.last(), &rels)?;
        sections.push(section);
    }

    let footnotes = reader.read_notes("word/footnotes.xml")?;
    Ok(Document {
        body: blocks,
        sections,
        footnotes,
        media: std::mem::take(&mut reader.media),
        even_and_odd,
        simplified: reader.simplified,
    })
}

/// Section properties in document order: those inside paragraphs, then the
/// body's own (the last section).
fn collect_section_nodes<'a, 'i>(body: &Node<'a, 'i>, out: &mut Vec<Node<'a, 'i>>) {
    for node in body.descendants() {
        if node.is_element()
            && local(&node) == "sectPr"
            && node
                .parent()
                .is_some_and(|p| local(&p) == "pPr" || local(&p) == "body")
        {
            out.push(node);
        }
    }
    // The body's own section comes last in the file already; keep paragraph
    // ones first in case a writer put it elsewhere.
    out.sort_by_key(|n| n.parent().is_some_and(|p| local(&p) == "body"));
}

impl<'p> Reader<'p> {
    fn rels(&self, part: &str) -> Rels {
        let mut rels = Rels::new();
        let Some(text) = self.package.text(&rels_name(part)) else {
            return rels;
        };
        let Ok(xml) = parse_xml(&text) else {
            return rels;
        };
        for rel in xml.root_element().children().filter(|c| c.is_element()) {
            let (Some(id), Some(target)) = (attr(&rel, "Id"), attr(&rel, "Target")) else {
                continue;
            };
            let kind = attr(&rel, "Type")
                .and_then(|t| t.rsplit('/').next())
                .unwrap_or_default()
                .to_owned();
            let external = attr(&rel, "TargetMode") == Some("External");
            let target = if external {
                target.to_owned()
            } else {
                resolve_target(part, target)
            };
            rels.insert(id.to_owned(), (kind, target, external));
        }
        rels
    }

    fn read_theme(&mut self) {
        let theme = self
            .package
            .names()
            .find(|n| n.starts_with("word/theme/") && n.ends_with(".xml"))
            .map(str::to_owned);
        let Some(text) = theme.and_then(|t| self.package.text(&t)) else {
            return;
        };
        let Ok(xml) = parse_xml(&text) else { return };
        for node in xml.descendants().filter(|n| n.is_element()) {
            let name = local(&node);
            if name == "majorFont" || name == "minorFont" {
                let latin = child(&node, "latin")
                    .and_then(|l| attr(&l, "typeface"))
                    .filter(|t| !t.is_empty())
                    .map(str::to_owned);
                if name == "majorFont" {
                    self.major_font = latin;
                } else {
                    self.minor_font = latin;
                }
            }
        }
    }

    fn read_styles(&mut self) -> Result<(), CoreError> {
        let Some(text) = self.package.text("word/styles.xml") else {
            return Ok(());
        };
        let xml = parse_xml(&text)?;
        let root = xml.root_element();
        if let Some(defaults) = child(&root, "docDefaults") {
            if let Some(rpr) = child(&defaults, "rPrDefault").and_then(|n| child(&n, "rPr")) {
                self.default_run = self.run_style(&rpr);
            }
            if let Some(ppr) = child(&defaults, "pPrDefault").and_then(|n| child(&n, "pPr")) {
                self.default_para = self.para_style(&ppr);
            }
        }
        for node in children(&root, "style") {
            let Some(id) = attr(&node, "styleId") else {
                continue;
            };
            let mut style = Style {
                based_on: child(&node, "basedOn")
                    .and_then(|b| attr(&b, "val"))
                    .map(str::to_owned),
                ..Style::default()
            };
            if let Some(ppr) = child(&node, "pPr") {
                style.para = self.para_style(&ppr);
            }
            if let Some(rpr) = child(&node, "rPr") {
                style.run = self.run_style(&rpr);
            }
            if let Some(tpr) = child(&node, "tblPr") {
                style.table_borders = child(&tpr, "tblBorders").map(|b| borders(&b));
                style.table_margins = child(&tpr, "tblCellMar").map(|m| cell_margins(&m));
            }
            if attr(&node, "type") == Some("paragraph") && attr(&node, "default") == Some("1") {
                self.default_paragraph_style = Some(id.to_owned());
            }
            self.styles.insert(id.to_owned(), style);
        }
        Ok(())
    }

    /// A style with everything it inherits (`basedOn` chain, at most 32 deep).
    fn style_chain(&self, id: &str) -> (ParaStyle, RunStyle) {
        let mut chain = Vec::new();
        let mut current = Some(id.to_owned());
        while let Some(name) = current {
            if chain.len() > 32 {
                break;
            }
            let Some(style) = self.styles.get(&name) else {
                break;
            };
            chain.push(style);
            current = style.based_on.clone();
        }
        let (mut para, mut run) = (ParaStyle::default(), RunStyle::default());
        for style in chain.iter().rev() {
            para.merge(&style.para);
            run.merge(&style.run);
        }
        (para, run)
    }

    fn read_numbering(&mut self) -> Result<(), CoreError> {
        let Some(text) = self.package.text("word/numbering.xml") else {
            return Ok(());
        };
        let xml = parse_xml(&text)?;
        let root = xml.root_element();
        let mut abstracts: HashMap<String, Vec<Level>> = HashMap::new();
        for node in children(&root, "abstractNum") {
            let Some(id) = attr(&node, "abstractNumId") else {
                continue;
            };
            let mut levels = vec![Level::default(); 9];
            for lvl in children(&node, "lvl") {
                let Some(index) = attr(&lvl, "ilvl").and_then(|v| v.parse::<usize>().ok()) else {
                    continue;
                };
                if index >= levels.len() {
                    continue;
                }
                levels[index] = self.level(&lvl);
            }
            abstracts.insert(id.to_owned(), levels);
        }
        for node in children(&root, "num") {
            let (Some(id), Some(abstract_id)) = (
                attr(&node, "numId"),
                child(&node, "abstractNumId").and_then(|a| attr(&a, "val")),
            ) else {
                continue;
            };
            let Some(mut levels) = abstracts.get(abstract_id).cloned() else {
                continue;
            };
            let mut restarted = false;
            for over in children(&node, "lvlOverride") {
                let Some(index) = attr(&over, "ilvl").and_then(|v| v.parse::<usize>().ok()) else {
                    continue;
                };
                if index >= levels.len() {
                    continue;
                }
                if let Some(start) = child(&over, "startOverride")
                    .and_then(|s| attr(&s, "val"))
                    .and_then(|v| v.parse().ok())
                {
                    levels[index].start = start;
                    restarted = true;
                }
                if let Some(lvl) = child(&over, "lvl") {
                    levels[index] = self.level(&lvl);
                    restarted = true;
                }
            }
            // Lists sharing a definition continue each other's numbers,
            // unless one starts again.
            let family = if restarted {
                format!("num:{id}")
            } else {
                format!("abstract:{abstract_id}")
            };
            self.list_family.insert(id.to_owned(), family);
            self.lists.insert(id.to_owned(), levels);
        }
        Ok(())
    }

    fn level(&self, lvl: &Node) -> Level {
        let value = |name: &str| child(lvl, name).and_then(|n| attr(&n, "val"));
        Level {
            start: value("start").and_then(|v| v.parse().ok()).unwrap_or(1),
            format: value("numFmt").unwrap_or("decimal").to_owned(),
            text: value("lvlText").unwrap_or("").to_owned(),
            para: child(lvl, "pPr")
                .map(|p| self.para_style(&p))
                .unwrap_or_default(),
            run: child(lvl, "rPr")
                .map(|r| self.run_style(&r))
                .unwrap_or_default(),
        }
    }

    /// The label of the next item of list `id` at `level` (an empty label
    /// when the level shows nothing), with the level's formatting.
    fn next_label(&mut self, id: &str, level: u8) -> Option<(String, RunStyle, ParaStyle)> {
        let levels = self.lists.get(id)?.clone();
        if levels.is_empty() {
            return None;
        }
        let level = (level as usize).min(levels.len() - 1);
        let family = self
            .list_family
            .get(id)
            .cloned()
            .unwrap_or_else(|| id.to_owned());
        let counters = self
            .counters
            .entry(family)
            .or_insert_with(|| vec![None; levels.len()]);
        counters[level] = Some(counters[level].map_or(levels[level].start, |n| n + 1));
        // Deeper levels start again under a new item.
        for deeper in counters.iter_mut().skip(level + 1) {
            *deeper = None;
        }
        let values = counters.clone();
        let def = &levels[level];
        if def.format == "none" {
            return Some((String::new(), def.run.clone(), def.para.clone()));
        }
        let text = if def.format == "bullet" {
            bullet(&def.text, def.run.font.as_deref())
        } else {
            let mut text = def.text.clone();
            for (i, l) in levels.iter().enumerate() {
                let placeholder = format!("%{}", i + 1);
                if text.contains(&placeholder) {
                    let n = values[i].unwrap_or(l.start);
                    text = text.replace(&placeholder, &number(n, &l.format));
                }
            }
            text
        };
        Some((text, def.run.clone(), def.para.clone()))
    }

    fn run_style(&self, rpr: &Node) -> RunStyle {
        let mut style = RunStyle::default();
        for node in rpr.children().filter(|c| c.is_element()) {
            match local(&node) {
                "rStyle" => style.style = attr(&node, "val").map(str::to_owned),
                "rFonts" => {
                    let font = attr(&node, "ascii")
                        .or_else(|| attr(&node, "hAnsi"))
                        .map(str::to_owned)
                        .or_else(|| {
                            match attr(&node, "asciiTheme").or_else(|| attr(&node, "hAnsiTheme")) {
                                Some(t) if t.starts_with("major") => self.major_font.clone(),
                                Some(_) => self.minor_font.clone(),
                                None => None,
                            }
                        })
                        .or_else(|| attr(&node, "eastAsia").map(str::to_owned));
                    if font.is_some() {
                        style.font = font;
                    }
                }
                "sz" => {
                    style.size = attr(&node, "val")
                        .and_then(|v| v.parse::<f32>().ok())
                        .map(|v| v / 2.0)
                }
                "b" => style.bold = Some(toggle(&node)),
                "i" => style.italic = Some(toggle(&node)),
                "u" => style.underline = Some(toggle(&node)),
                "strike" | "dstrike" => style.strike = Some(toggle(&node)),
                "color" => style.color = Some(hex_color(attr(&node, "val"))),
                "highlight" => style.highlight = Some(attr(&node, "val").and_then(highlight_color)),
                "shd" => {
                    if let Some(fill) = hex_color(attr(&node, "fill")) {
                        style.highlight = Some(Some(fill));
                    }
                }
                "vertAlign" => {
                    style.baseline = Some(match attr(&node, "val") {
                        Some("superscript") => Baseline::Super,
                        Some("subscript") => Baseline::Sub,
                        _ => Baseline::Normal,
                    })
                }
                "caps" => style.caps = Some(toggle(&node)),
                "smallCaps" => style.small_caps = Some(toggle(&node)),
                "vanish" | "specVanish" => style.hidden = Some(toggle(&node)),
                "spacing" => style.spacing = twips(attr(&node, "val")),
                _ => {}
            }
        }
        style
    }

    fn para_style(&self, ppr: &Node) -> ParaStyle {
        let mut style = ParaStyle::default();
        for node in ppr.children().filter(|c| c.is_element()) {
            match local(&node) {
                "jc" => {
                    style.align = Some(match attr(&node, "val") {
                        Some("center") => Align::Center,
                        Some("right" | "end") => Align::Right,
                        Some(
                            "both" | "distribute" | "lowKashida" | "mediumKashida" | "highKashida"
                            | "thaiDistribute",
                        ) => Align::Justify,
                        _ => Align::Left,
                    })
                }
                "spacing" => {
                    if attr(&node, "beforeAutospacing").is_some_and(|v| v == "1" || v == "true") {
                        style.before = Some(14.0);
                    } else if let Some(v) = twips(attr(&node, "before")) {
                        style.before = Some(v);
                    }
                    if attr(&node, "afterAutospacing").is_some_and(|v| v == "1" || v == "true") {
                        style.after = Some(14.0);
                    } else if let Some(v) = twips(attr(&node, "after")) {
                        style.after = Some(v);
                    }
                    if let Some(line) = attr(&node, "line").and_then(|v| v.parse::<f32>().ok()) {
                        style.line = Some(match attr(&node, "lineRule") {
                            Some("exact") => LineSpacing::Exact(line * TWIP),
                            Some("atLeast") => LineSpacing::AtLeast(line * TWIP),
                            _ => LineSpacing::Multiple(line / 240.0),
                        });
                    }
                }
                "ind" => {
                    if let Some(v) = twips(attr(&node, "left").or_else(|| attr(&node, "start"))) {
                        style.left = Some(v);
                    }
                    if let Some(v) = twips(attr(&node, "right").or_else(|| attr(&node, "end"))) {
                        style.right = Some(v);
                    }
                    if let Some(v) = twips(attr(&node, "hanging")) {
                        style.first_line = Some(-v);
                    } else if let Some(v) = twips(attr(&node, "firstLine")) {
                        style.first_line = Some(v);
                    }
                }
                "numPr" => {
                    let id = child(&node, "numId").and_then(|n| attr(&n, "val"));
                    let level = child(&node, "ilvl")
                        .and_then(|n| attr(&n, "val"))
                        .and_then(|v| v.parse().ok());
                    if let Some(id) = id {
                        style.numbering = Some((id.to_owned(), level.unwrap_or(0)));
                    }
                    style.level = level;
                }
                "outlineLvl" => {
                    style.outline = Some(
                        attr(&node, "val")
                            .and_then(|v| v.parse::<u8>().ok())
                            .filter(|l| *l < 9),
                    )
                }
                "pageBreakBefore" => style.page_break_before = Some(toggle(&node)),
                "keepLines" => style.keep_lines = Some(toggle(&node)),
                "keepNext" => style.keep_next = Some(toggle(&node)),
                "contextualSpacing" => style.contextual = Some(toggle(&node)),
                "shd" => style.shading = Some(hex_color(attr(&node, "fill"))),
                "tabs" => {
                    let mut stops: Vec<f32> = children(&node, "tab")
                        .filter(|t| attr(t, "val") != Some("clear"))
                        .filter_map(|t| twips(attr(&t, "pos")))
                        .collect();
                    stops.sort_by(f32::total_cmp);
                    style.tabs = Some(stops);
                }
                _ => {}
            }
        }
        style
    }

    /// Body content: paragraphs and tables, in order. `on_section` is told
    /// whenever a paragraph ends a section.
    fn read_blocks(
        &mut self,
        parent: &Node,
        rels: &Rels,
        part: &str,
        out: &mut Vec<Block>,
        on_section: &mut dyn FnMut(usize),
    ) -> Result<(), CoreError> {
        for node in parent.children().filter(|c| c.is_element()) {
            match local(&node) {
                "p" => {
                    let section = child(&node, "pPr").and_then(|p| child(&p, "sectPr"));
                    for paragraph in self.read_paragraph(&node, rels) {
                        out.push(Block::Paragraph(paragraph));
                    }
                    if section.is_some() {
                        on_section(out.len());
                    }
                }
                "tbl" => {
                    if let Some(table) = self.read_table(&node, rels, part)? {
                        out.push(Block::Table(table));
                    }
                }
                "sdt" => {
                    if let Some(content) = child(&node, "sdtContent") {
                        self.read_blocks(&content, rels, part, out, on_section)?;
                    }
                }
                "customXml" | "ins" | "moveTo" => {
                    self.read_blocks(&node, rels, part, out, on_section)?
                }
                "altChunk" => self.simplified = true,
                _ => {}
            }
        }
        Ok(())
    }

    /// A paragraph, split where it has a page break (Typst breaks pages
    /// between blocks only).
    fn read_paragraph(&mut self, node: &Node, rels: &Rels) -> Vec<Paragraph> {
        let ppr = child(node, "pPr");
        let own = ppr.map(|p| self.para_style(&p)).unwrap_or_default();
        let style_id = ppr
            .and_then(|p| child(&p, "pStyle"))
            .and_then(|s| attr(&s, "val"))
            .map(str::to_owned)
            .or_else(|| self.default_paragraph_style.clone());
        let (style_para, style_run) = style_id
            .as_deref()
            .map(|id| self.style_chain(id))
            .unwrap_or_default();
        let mut para = self.default_para.clone();
        para.merge(&style_para);
        // A list from the style or the paragraph puts its indents first.
        let numbering = own
            .numbering
            .clone()
            .or(style_para.numbering.clone())
            .map(|(id, level)| (id, own.level.or(style_para.level).unwrap_or(level)));
        let mut label = None;
        if let Some((id, level)) = numbering.filter(|(id, _)| id != "0")
            && let Some((text, run, list_para)) = self.next_label(&id, level)
        {
            para.merge(&list_para);
            let mut label_run = self.default_run.clone();
            label_run.merge(&style_run);
            label_run.merge(&run);
            if !text.is_empty() {
                label = Some((text, label_run.resolve()));
            }
        }
        para.merge(&own);

        let mut base_run = self.default_run.clone();
        if base_run.font.is_none() {
            base_run.font = self.minor_font.clone();
        }
        base_run.merge(&style_run);
        let mark = {
            let mut mark = base_run.clone();
            if let Some(rpr) = ppr.and_then(|p| child(&p, "rPr")) {
                mark.merge(&self.run_style(&rpr));
            }
            mark.resolve()
        };

        let mut inlines = Vec::new();
        let mut field = FieldState::default();
        self.read_inlines(node, rels, &base_run, &mut inlines, &mut field);

        let line = para.line.unwrap_or(LineSpacing::Multiple(1.0));
        let make =
            |inlines: Vec<Inline>, first: bool, label: Option<(String, RunProps)>| Paragraph {
                align: para.align.unwrap_or_default(),
                before: if first {
                    para.before.unwrap_or(0.0)
                } else {
                    0.0
                },
                after: para.after.unwrap_or(0.0),
                line,
                left: para.left.unwrap_or(0.0),
                right: para.right.unwrap_or(0.0),
                first_line: para.first_line.unwrap_or(0.0),
                outline: para.outline.flatten(),
                page_break_before: first && para.page_break_before.unwrap_or(false),
                keep_lines: para.keep_lines.unwrap_or(false),
                keep_next: para.keep_next.unwrap_or(false),
                shading: para.shading.flatten(),
                mark: mark.clone(),
                label,
                tabs: para.tabs.clone().unwrap_or_default(),
                inlines,
            };
        // Split at page breaks.
        let mut paragraphs = Vec::new();
        let mut current = Vec::new();
        let mut first = true;
        for inline in inlines {
            if inline == Inline::PageBreak {
                paragraphs.push(make(std::mem::take(&mut current), first, label.take()));
                first = false;
                let mut next = make(Vec::new(), false, None);
                next.page_break_before = true;
                paragraphs.push(next);
                continue;
            }
            current.push(inline);
        }
        if paragraphs.is_empty() {
            return vec![make(current, true, label)];
        }
        // The text after the last break goes into the paragraph that starts the page.
        if let Some(last) = paragraphs.last_mut() {
            last.inlines = current;
        }
        // An empty part before the first break is not a line of its own.
        if paragraphs
            .first()
            .is_some_and(|p| p.inlines.is_empty() && p.label.is_none())
        {
            let before = paragraphs[0].before;
            paragraphs.remove(0);
            if let Some(next) = paragraphs.first_mut() {
                next.before = before;
            }
        }
        paragraphs
    }

    fn read_inlines(
        &mut self,
        parent: &Node,
        rels: &Rels,
        base: &RunStyle,
        out: &mut Vec<Inline>,
        field: &mut FieldState,
    ) {
        for node in parent.children().filter(|c| c.is_element()) {
            match local(&node) {
                "r" => self.read_run(&node, rels, base, out, field),
                "hyperlink" => {
                    let url = attr(&node, "id")
                        .and_then(|id| rels.get(id))
                        .filter(|(_, _, external)| *external)
                        .map(|(_, target, _)| target.clone());
                    let mut inner = Vec::new();
                    self.read_inlines(&node, rels, base, &mut inner, field);
                    match url {
                        // Only web and mail addresses become links.
                        Some(url)
                            if url.starts_with("https:")
                                || url.starts_with("http:")
                                || url.starts_with("mailto:") =>
                        {
                            out.push(Inline::Link {
                                url,
                                inlines: inner,
                            })
                        }
                        _ => out.extend(inner),
                    }
                }
                "fldSimple" => {
                    let instr = attr(&node, "instr")
                        .unwrap_or_default()
                        .trim()
                        .to_uppercase();
                    let props = base.resolve();
                    if instr.starts_with("PAGE") {
                        out.push(Inline::PageNumber(props));
                    } else if instr.starts_with("NUMPAGES") || instr.starts_with("SECTIONPAGES") {
                        out.push(Inline::PageCount(props));
                    } else {
                        self.read_inlines(&node, rels, base, out, field);
                    }
                }
                "ins" | "moveTo" | "smartTag" | "customXml" | "bdo" | "dir" => {
                    self.read_inlines(&node, rels, base, out, field)
                }
                "sdt" => {
                    if let Some(content) = child(&node, "sdtContent") {
                        self.read_inlines(&content, rels, base, out, field);
                    }
                }
                "oMath" | "oMathPara" => self.simplified = true,
                _ => {}
            }
        }
    }

    fn read_run(
        &mut self,
        run: &Node,
        rels: &Rels,
        base: &RunStyle,
        out: &mut Vec<Inline>,
        field: &mut FieldState,
    ) {
        let mut style = base.clone();
        if let Some(rpr) = child(run, "rPr") {
            let own = self.run_style(&rpr);
            if let Some(id) = own.style.as_deref() {
                let (_, character) = self.style_chain(id);
                style.merge(&character);
            }
            style.merge(&own);
        }
        if style.hidden == Some(true) {
            return;
        }
        let props = style.resolve();
        for node in run.children().filter(|c| c.is_element()) {
            match local(&node) {
                "fldChar" => match attr(&node, "fldCharType") {
                    Some("begin") => {
                        field.depth += 1;
                        if field.depth == 1 {
                            field.instr.clear();
                            field.collecting = true;
                            field.skipping = false;
                        }
                    }
                    Some("separate") if field.depth == 1 => {
                        let instr = field.instr.trim().to_uppercase();
                        if instr.starts_with("PAGE") && !instr.starts_with("PAGEREF") {
                            out.push(Inline::PageNumber(props.clone()));
                            field.skipping = true;
                        } else if instr.starts_with("NUMPAGES") || instr.starts_with("SECTIONPAGES")
                        {
                            out.push(Inline::PageCount(props.clone()));
                            field.skipping = true;
                        }
                        field.collecting = false;
                    }
                    Some("end") => {
                        field.depth = field.depth.saturating_sub(1);
                        if field.depth == 0 {
                            field.collecting = false;
                            field.skipping = false;
                        }
                    }
                    _ => {}
                },
                "instrText" if field.depth >= 1 => {
                    field.instr.push_str(node.text().unwrap_or_default());
                }
                _ if field.skipping => {}
                "t" => {
                    // Text of a field's instruction is not shown.
                    if field.collecting {
                        continue;
                    }
                    let text = node.text().unwrap_or_default();
                    if !text.is_empty() {
                        push_text(out, text, &props);
                    }
                }
                "tab" | "ptab" => out.push(Inline::Tab(props.clone())),
                "br" => out.push(match attr(&node, "type") {
                    Some("page") => Inline::PageBreak,
                    Some("column") => Inline::ColumnBreak,
                    _ => Inline::LineBreak,
                }),
                "cr" => out.push(Inline::LineBreak),
                "noBreakHyphen" => push_text(out, "\u{2011}", &props),
                "softHyphen" => push_text(out, "\u{ad}", &props),
                "sym" => {
                    let code = attr(&node, "char")
                        .and_then(|c| u32::from_str_radix(c, 16).ok())
                        .unwrap_or(0x2022);
                    let text = bullet(
                        &char::from_u32(code).map(String::from).unwrap_or_default(),
                        attr(&node, "font"),
                    );
                    push_text(out, &text, &props);
                }
                "footnoteReference" => {
                    if let Some(id) = attr(&node, "id") {
                        out.push(Inline::Footnote(id.to_owned()));
                    }
                }
                "drawing" => self.read_drawing(&node, rels, out),
                "pict" | "object" => self.read_vml(&node, rels, out),
                "AlternateContent" => {
                    // The fallback is what older programs show; the choice
                    // usually holds a drawing Konvert reads too.
                    let choice = child(&node, "Choice").or_else(|| child(&node, "Fallback"));
                    if let Some(choice) = choice {
                        for inner in choice.children().filter(|c| c.is_element()) {
                            match local(&inner) {
                                "drawing" => self.read_drawing(&inner, rels, out),
                                "pict" => self.read_vml(&inner, rels, out),
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// A DrawingML picture (inline or anchored, which is laid out inline).
    fn read_drawing(&mut self, drawing: &Node, rels: &Rels, out: &mut Vec<Inline>) {
        for frame in drawing.children().filter(|c| c.is_element()) {
            let Some(extent) = child(&frame, "extent") else {
                continue;
            };
            let size = |name: &str| {
                attr(&extent, name)
                    .and_then(|v| v.parse::<f32>().ok())
                    .map(|v| v * EMU)
                    .unwrap_or(0.0)
            };
            let (width, height) = (size("cx"), size("cy"));
            let blip = frame
                .descendants()
                .find(|n| n.is_element() && local(n) == "blip");
            match blip.and_then(|b| attr(&b, "embed")) {
                Some(id) => {
                    if let Some(part) = self.picture(rels, id) {
                        out.push(Inline::Picture {
                            part,
                            width,
                            height,
                        });
                    }
                }
                // Charts, shapes, text boxes and diagrams.
                None => self.simplified = true,
            }
            if local(&frame) == "anchor"
                && frame
                    .descendants()
                    .any(|n| n.is_element() && local(&n) == "txbx")
            {
                self.simplified = true;
            }
        }
    }

    /// A legacy (VML) picture: `<v:imagedata r:id>` sized by the shape style.
    fn read_vml(&mut self, pict: &Node, rels: &Rels, out: &mut Vec<Inline>) {
        let shape = pict
            .descendants()
            .find(|n| n.is_element() && local(n) == "shape");
        let image = pict
            .descendants()
            .find(|n| n.is_element() && local(n) == "imagedata")
            .and_then(|n| attr(&n, "id"));
        let (Some(shape), Some(id)) = (shape, image) else {
            self.simplified = true;
            return;
        };
        let style = attr(&shape, "style").unwrap_or_default();
        let length = |name: &str| {
            style.split(';').find_map(|pair| {
                let (key, value) = pair.split_once(':')?;
                (key.trim() == name).then(|| css_length(value.trim()))?
            })
        };
        if let (Some(part), Some(width), Some(height)) =
            (self.picture(rels, id), length("width"), length("height"))
        {
            out.push(Inline::Picture {
                part,
                width,
                height,
            });
        }
    }

    /// Loads the picture of relationship `id` (once), if it is a format the
    /// typesetter can draw.
    fn picture(&mut self, rels: &Rels, id: &str) -> Option<String> {
        let (_, target, external) = rels.get(id)?;
        if *external {
            // Pictures linked from outside the file are never fetched.
            self.simplified = true;
            return None;
        }
        if !self.media.contains_key(target) {
            let data = self.package.get(target)?;
            if !drawable(data) {
                self.simplified = true;
                return None;
            }
            self.media.insert(target.clone(), data.to_vec());
        }
        Some(target.clone())
    }

    fn read_table(
        &mut self,
        node: &Node,
        rels: &Rels,
        part: &str,
    ) -> Result<Option<Table>, CoreError> {
        let tpr = child(node, "tblPr");
        let style = tpr
            .and_then(|t| child(&t, "tblStyle"))
            .and_then(|s| attr(&s, "val"))
            .and_then(|id| self.styles.get(id))
            .cloned()
            .unwrap_or_default();
        let mut borders = style.table_borders.unwrap_or_default();
        if let Some(own) = tpr.and_then(|t| child(&t, "tblBorders")) {
            merge_borders(&mut borders, &self::borders(&own));
        }
        let mut margins = [0.0, 5.4, 0.0, 5.4];
        for source in [
            style.table_margins,
            tpr.and_then(|t| child(&t, "tblCellMar"))
                .map(|m| cell_margins(&m)),
        ]
        .into_iter()
        .flatten()
        {
            for (i, m) in source.iter().enumerate() {
                if let Some(m) = m {
                    margins[i] = *m;
                }
            }
        }
        let align = match tpr
            .and_then(|t| child(&t, "jc"))
            .and_then(|j| attr(&j, "val"))
        {
            Some("center") => Align::Center,
            Some("right" | "end") => Align::Right,
            _ => Align::Left,
        };
        let indent = tpr
            .and_then(|t| child(&t, "tblInd"))
            .and_then(|i| twips(attr(&i, "w")))
            .unwrap_or(0.0);
        let mut columns: Vec<f32> = child(node, "tblGrid")
            .map(|g| {
                children(&g, "gridCol")
                    .map(|c| twips(attr(&c, "w")).unwrap_or(0.0))
                    .collect()
            })
            .unwrap_or_default();

        let mut rows: Vec<Row> = Vec::new();
        for tr in children(node, "tr") {
            let trpr = child(&tr, "trPr");
            let header = trpr.is_some_and(|t| child(&t, "tblHeader").is_some_and(|h| toggle(&h)));
            let mut column = trpr
                .and_then(|t| child(&t, "gridBefore"))
                .and_then(|g| attr(&g, "val"))
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0);
            let mut cells = Vec::new();
            for tc in tr.children().filter(|c| c.is_element()) {
                let tc = match local(&tc) {
                    "tc" => tc,
                    "sdt" => match child(&tc, "sdtContent").and_then(|c| child(&c, "tc")) {
                        Some(inner) => inner,
                        None => continue,
                    },
                    _ => continue,
                };
                let tcpr = child(&tc, "tcPr");
                let span = tcpr
                    .and_then(|t| child(&t, "gridSpan"))
                    .and_then(|g| attr(&g, "val"))
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(1)
                    .max(1);
                let vmerge = tcpr.and_then(|t| child(&t, "vMerge"));
                let covered = vmerge.is_some_and(|v| attr(&v, "val") != Some("restart"));
                let mut blocks = Vec::new();
                self.read_blocks(&tc, rels, part, &mut blocks, &mut |_| {})?;
                cells.push(Cell {
                    column,
                    span,
                    rows: 1,
                    covered,
                    fill: tcpr
                        .and_then(|t| child(&t, "shd"))
                        .and_then(|s| hex_color(attr(&s, "fill"))),
                    borders: tcpr
                        .and_then(|t| child(&t, "tcBorders"))
                        .map(|b| self::borders(&b))
                        .unwrap_or_default(),
                    valign: match tcpr
                        .and_then(|t| child(&t, "vAlign"))
                        .and_then(|v| attr(&v, "val"))
                    {
                        Some("center") => VAlign::Center,
                        Some("bottom") => VAlign::Bottom,
                        _ => VAlign::Top,
                    },
                    blocks,
                });
                column += span;
            }
            if columns.len() < column {
                columns.resize(column, 72.0);
            }
            rows.push(Row { cells, header });
        }
        if rows.is_empty() {
            return Ok(None);
        }
        // Vertically merged cells: the first one spans the rows below it.
        for r in 0..rows.len() {
            for c in 0..rows[r].cells.len() {
                if rows[r].cells[c].covered {
                    continue;
                }
                let column = rows[r].cells[c].column;
                let mut span = 1;
                for below in rows.iter().skip(r + 1) {
                    match below.cells.iter().find(|x| x.column == column) {
                        Some(next) if next.covered => span += 1,
                        _ => break,
                    }
                }
                rows[r].cells[c].rows = span;
            }
        }
        for width in &mut columns {
            if *width <= 0.0 {
                *width = 36.0;
            }
        }
        Ok(Some(Table {
            columns,
            rows,
            align,
            indent,
            borders,
            margins,
        }))
    }

    fn read_section(
        &mut self,
        node: Option<Node>,
        end: usize,
        previous: Option<&Section>,
        rels: &Rels,
    ) -> Result<Section, CoreError> {
        let mut section = Section {
            end,
            columns: 1,
            column_gap: 36.0,
            new_page: true,
            ..previous.cloned().unwrap_or_default()
        };
        section.end = end;
        section.page_start = None;
        section.title_page = false;
        if previous.is_none() {
            section.page = PageSetup::default();
        }
        let Some(node) = node else {
            return Ok(section);
        };
        if let Some(size) = child(&node, "pgSz")
            && let (Some(w), Some(h)) = (twips(attr(&size, "w")), twips(attr(&size, "h")))
        {
            section.page.width = w;
            section.page.height = h;
        }
        if let Some(margin) = child(&node, "pgMar") {
            let set = |value: &mut f32, name: &str| {
                if let Some(v) = twips(attr(&margin, name)) {
                    *value = v.abs();
                }
            };
            set(&mut section.page.top, "top");
            set(&mut section.page.bottom, "bottom");
            set(&mut section.page.left, "left");
            set(&mut section.page.right, "right");
            set(&mut section.page.header, "header");
            set(&mut section.page.footer, "footer");
        }
        if let Some(cols) = child(&node, "cols") {
            section.columns = attr(&cols, "num")
                .and_then(|v| v.parse().ok())
                .unwrap_or(1)
                .clamp(1, 6);
            section.column_gap = twips(attr(&cols, "space")).unwrap_or(36.0);
        } else {
            section.columns = 1;
        }
        section.new_page = !matches!(
            child(&node, "type").and_then(|t| attr(&t, "val")),
            Some("continuous")
        );
        section.page_start = child(&node, "pgNumType")
            .and_then(|p| attr(&p, "start"))
            .and_then(|v| v.parse().ok());
        section.title_page = child(&node, "titlePg").is_some_and(|t| toggle(&t));
        for reference in node.children().filter(|c| c.is_element()) {
            let kind = local(&reference);
            if kind != "headerReference" && kind != "footerReference" {
                continue;
            }
            let Some((_, target, _)) = attr(&reference, "id").and_then(|id| rels.get(id)) else {
                continue;
            };
            let blocks = self.read_part_blocks(target)?;
            let header = kind == "headerReference";
            let slot = match (attr(&reference, "type"), header) {
                (Some("first"), true) => &mut section.first_header,
                (Some("even"), true) => &mut section.even_header,
                (_, true) => &mut section.header,
                (Some("first"), false) => &mut section.first_footer,
                (Some("even"), false) => &mut section.even_footer,
                (_, false) => &mut section.footer,
            };
            *slot = Some(blocks);
        }
        Ok(section)
    }

    /// The blocks of a header, footer or other part with its own relationships.
    fn read_part_blocks(&mut self, part: &str) -> Result<Vec<Block>, CoreError> {
        let Some(text) = self.package.text(part) else {
            return Ok(Vec::new());
        };
        let xml = parse_xml(&text)?;
        let rels = self.rels(part);
        let mut blocks = Vec::new();
        self.read_blocks(&xml.root_element(), &rels, part, &mut blocks, &mut |_| {})?;
        Ok(blocks)
    }

    fn read_notes(&mut self, part: &str) -> Result<HashMap<String, Vec<Block>>, CoreError> {
        let mut notes = HashMap::new();
        let Some(text) = self.package.text(part) else {
            return Ok(notes);
        };
        let xml = parse_xml(&text)?;
        let rels = self.rels(part);
        for note in xml.root_element().children().filter(|c| c.is_element()) {
            // Separators are not notes.
            if attr(&note, "type").is_some_and(|t| t != "normal") {
                continue;
            }
            let Some(id) = attr(&note, "id") else {
                continue;
            };
            let mut blocks = Vec::new();
            self.read_blocks(&note, &rels, part, &mut blocks, &mut |_| {})?;
            notes.insert(id.to_owned(), blocks);
        }
        Ok(notes)
    }
}

#[derive(Default)]
struct FieldState {
    depth: u32,
    instr: String,
    /// Between `begin` and `separate`: instruction text, not shown.
    collecting: bool,
    /// The field's shown result is replaced (page numbers).
    skipping: bool,
}

/// Adds text to the last run when its formatting is the same.
fn push_text(out: &mut Vec<Inline>, text: &str, props: &RunProps) {
    if let Some(Inline::Text(last, last_props)) = out.last_mut()
        && last_props == props
    {
        last.push_str(text);
        return;
    }
    out.push(Inline::Text(text.to_owned(), props.clone()));
}

/// Pictures the typesetter draws: PNG, JPEG, GIF, WebP.
pub fn drawable(data: &[u8]) -> bool {
    data.starts_with(&[0x89, b'P', b'N', b'G'])
        || data.starts_with(&[0xFF, 0xD8, 0xFF])
        || data.starts_with(b"GIF8")
        || (data.len() > 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP")
}

/// `12pt`, `1.5in`, `2cm`, `40px` → points.
fn css_length(value: &str) -> Option<f32> {
    let split = value
        .find(|c: char| c.is_ascii_alphabetic())
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(split);
    let n: f32 = number.trim().parse().ok()?;
    Some(match unit.trim() {
        "pt" | "" => n,
        "in" => n * 72.0,
        "cm" => n * 72.0 / 2.54,
        "mm" => n * 72.0 / 25.4,
        "px" => n * 0.75,
        "pc" => n * 12.0,
        _ => return None,
    })
}

fn border(node: &Node) -> Option<Border> {
    match attr(node, "val") {
        None | Some("nil" | "none") => None,
        Some(_) => Some(Border {
            // Eighths of a point; Word draws at least a quarter point.
            width: attr(node, "sz")
                .and_then(|v| v.parse::<f32>().ok())
                .map(|v| (v / 8.0).max(0.25))
                .unwrap_or(0.5),
            color: hex_color(attr(node, "color")).unwrap_or([0, 0, 0]),
        }),
    }
}

fn borders(node: &Node) -> Borders {
    let get = |names: &[&str]| -> Option<Option<Border>> {
        names
            .iter()
            .find_map(|n| child(node, n))
            .map(|b| border(&b))
    };
    let mut result = Borders::default();
    let set = |slot: &mut Option<Border>, names: &[&str]| {
        if let Some(value) = get(names) {
            *slot = value;
        }
    };
    set(&mut result.top, &["top"]);
    set(&mut result.bottom, &["bottom"]);
    set(&mut result.left, &["left", "start"]);
    set(&mut result.right, &["right", "end"]);
    set(&mut result.inside_h, &["insideH"]);
    set(&mut result.inside_v, &["insideV"]);
    result
}

/// Borders written in `own` replace those of `base`; a border written as
/// "none" removes it.
fn merge_borders(base: &mut Borders, own: &Borders) {
    // `borders` cannot tell "not written" from "none" once parsed: a table's
    // own borders replace the style's as a whole when any is written.
    *base = Borders {
        top: own.top.or(base.top),
        bottom: own.bottom.or(base.bottom),
        left: own.left.or(base.left),
        right: own.right.or(base.right),
        inside_h: own.inside_h.or(base.inside_h),
        inside_v: own.inside_v.or(base.inside_v),
    };
}

fn cell_margins(node: &Node) -> [Option<f32>; 4] {
    let get = |names: &[&str]| {
        names
            .iter()
            .find_map(|n| child(node, n))
            .and_then(|m| twips(attr(&m, "w")))
    };
    [
        get(&["top"]),
        get(&["right", "end"]),
        get(&["bottom"]),
        get(&["left", "start"]),
    ]
}

/// List numbers in Word's formats.
fn number(n: u32, format: &str) -> String {
    match format {
        "lowerLetter" => letters(n, b'a'),
        "upperLetter" => letters(n, b'A'),
        "lowerRoman" => roman(n).to_lowercase(),
        "upperRoman" => roman(n),
        "decimalZero" => format!("{n:02}"),
        "ordinal" => format!("{n}."),
        _ => n.to_string(),
    }
}

/// a, b, … z, aa, bb, … as Word counts.
fn letters(n: u32, base: u8) -> String {
    if n == 0 {
        return String::new();
    }
    let letter = (base + ((n - 1) % 26) as u8) as char;
    letter.to_string().repeat(((n - 1) / 26 + 1) as usize)
}

fn roman(mut n: u32) -> String {
    const TABLE: [(u32, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for (value, text) in TABLE {
        while n >= value {
            out.push_str(text);
            n -= value;
        }
    }
    out
}

/// Bullets of symbol fonts (private use characters) as ordinary characters.
fn bullet(text: &str, font: Option<&str>) -> String {
    let symbol = font.is_some_and(|f| {
        let f = f.to_ascii_lowercase();
        f.contains("symbol") || f.contains("wingdings") || f.contains("webdings")
    });
    text.chars()
        .map(|c| {
            let code = c as u32;
            let low = if (0xF000..=0xF0FF).contains(&code) {
                code - 0xF000
            } else {
                code
            };
            match (low, symbol || (0xF000..=0xF0FF).contains(&code)) {
                (0xB7, true) => '•',
                (0xA7, true) => '▪',
                (0xA8, true) => '◆',
                (0xD8, true) => '➢',
                (0xFC, true) => '✓',
                (0x6F, true) => '◦',
                (0x76, true) => '❖',
                (0x71, true) => '◆',
                (0x6E, true) => '■',
                (_, true) => '•',
                _ if c == 'o' && font.is_some_and(|f| f.contains("Courier")) => '◦',
                _ => c,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_numbers_follow_word() {
        assert_eq!(number(3, "decimal"), "3");
        assert_eq!(number(28, "lowerLetter"), "bb");
        assert_eq!(number(14, "upperRoman"), "XIV");
        assert_eq!(number(4, "lowerRoman"), "iv");
        assert_eq!(number(7, "decimalZero"), "07");
        assert_eq!(bullet("\u{F0B7}", Some("Symbol")), "•");
        assert_eq!(bullet("o", Some("Courier New")), "◦");
        assert_eq!(bullet("–", None), "–");
    }

    #[test]
    fn lengths_and_colors() {
        assert_eq!(css_length("72pt"), Some(72.0));
        assert_eq!(css_length("1in"), Some(72.0));
        assert!((css_length("2.54cm").unwrap() - 72.0).abs() < 0.01);
        assert_eq!(hex_color(Some("1F3864")), Some([0x1F, 0x38, 0x64]));
        assert_eq!(hex_color(Some("auto")), None);
        assert_eq!(twips(Some("240")), Some(12.0));
        assert!(drawable(b"\x89PNG\r\n") && !drawable(b"\x01\x00\x00\x00 EMF"));
    }
}
