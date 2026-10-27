//! DOCX → PDF: the model of [`super::read`] written as a Typst document, laid
//! out by the Typst engine with the fonts installed on the computer.
//!
//! Text is always passed as Typst strings (never as markup), so nothing in a
//! document can run as code. The Typst "world" only knows the generated
//! source and the document's own pictures: it cannot read other files, and
//! Typst never downloads packages here.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::OnceLock;

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_kit::fonts::FontStore;
use typst_layout::PagedDocument;

use super::read::{
    Align, Baseline, Block, Border, Borders, Document, Inline, LineSpacing, Paragraph, RunProps,
    Section, Table, VAlign,
};
use crate::error::CoreError;

/// Fonts metrically compatible with the usual Office fonts (same widths, so
/// lines break at the same places), tried in order after the font itself.
const ALTERNATES: &[(&str, &[&str])] = &[
    (
        "calibri",
        &["Carlito", "Liberation Sans", "Arial", "Helvetica"],
    ),
    (
        "cambria",
        &["Caladea", "Liberation Serif", "Times New Roman", "Times"],
    ),
    (
        "aptos",
        &["Calibri", "Carlito", "Segoe UI", "Arial", "Helvetica"],
    ),
    ("arial", &["Liberation Sans", "Arimo", "Helvetica"]),
    ("helvetica", &["Arial", "Liberation Sans", "Arimo"]),
    ("times new roman", &["Liberation Serif", "Tinos", "Times"]),
    ("times", &["Times New Roman", "Liberation Serif", "Tinos"]),
    ("courier new", &["Liberation Mono", "Cousine", "Courier"]),
    (
        "georgia",
        &["Gelasio", "Times New Roman", "Liberation Serif"],
    ),
    ("verdana", &["DejaVu Sans", "Arial"]),
    ("segoe ui", &["Helvetica Neue", "Arial", "Liberation Sans"]),
];

/// Height of a line at single spacing, in ems (Word uses the font's own,
/// about this much for the usual fonts).
const LINE: f32 = 1.2;

/// Last resort for every run: common fonts of Windows, macOS and Linux.
const FALLBACK: &[&str] = &[
    "Arial",
    "Helvetica",
    "Liberation Sans",
    "DejaVu Sans",
    "Noto Sans",
];

/// The fonts of the computer, found once (scanning takes a fraction of a second).
fn fonts() -> &'static FontStore {
    static FONTS: OnceLock<FontStore> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut store = FontStore::new();
        store.extend(typst_kit::fonts::system());
        store
    })
}

/// Lays out a Word document as PDF.
pub fn to_pdf(document: &Document) -> Result<Vec<u8>, CoreError> {
    let (source, files) = to_typst(document);
    let world = KonvertWorld::new(source, files);
    let result = typst::compile::<PagedDocument>(&world);
    let laid_out = result.output.map_err(|errors| CoreError::EngineFailed {
        details: errors
            .iter()
            .map(|e| e.message.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    })?;
    let options = typst_pdf::PdfOptions {
        // No date: the PDF says nothing about when it was made.
        timestamp: None,
        ..typst_pdf::PdfOptions::default()
    };
    let pdf = typst_pdf::pdf(&laid_out, &options).map_err(|errors| CoreError::EngineFailed {
        details: errors
            .iter()
            .map(|e| e.message.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    })?;
    only_konvert(&pdf)
}

/// Like every PDF Konvert writes, the only metadata is `Producer: Konvert`
/// (Typst names itself as the creator and adds XMP metadata).
fn only_konvert(pdf: &[u8]) -> Result<Vec<u8>, CoreError> {
    let mut doc = lopdf::Document::load_mem(pdf).map_err(|e| CoreError::EngineFailed {
        details: e.to_string(),
    })?;
    let info = doc
        .add_object(lopdf::dictionary! { "Producer" => lopdf::Object::string_literal("Konvert") });
    doc.trailer.set("Info", info);
    if let Ok(catalog) = doc.catalog_mut() {
        catalog.remove(b"Metadata");
    }
    crate::pdf_compress::keep_reachable(&mut doc);
    crate::pdf_compress::save(&mut doc)
}

/// The Typst source of the document and the pictures it uses (by path).
pub fn to_typst(document: &Document) -> (String, HashMap<String, Vec<u8>>) {
    let mut writer = Writer {
        out: String::new(),
        document,
        files: HashMap::new(),
    };
    writer.document();
    (writer.out, writer.files)
}

struct Writer<'d> {
    out: String,
    document: &'d Document,
    files: HashMap<String, Vec<u8>>,
}

/// A Typst string literal.
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' | '\r' => out.push(' '),
            c if c.is_control() && c != '\t' => {}
            '\t' => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn pt(value: f32) -> String {
    format!("{:.2}pt", value)
}

fn color(rgb: [u8; 3]) -> String {
    format!("rgb({},{},{})", rgb[0], rgb[1], rgb[2])
}

fn font_list(font: &str) -> String {
    let mut fonts: Vec<&str> = vec![font];
    let lower = font.to_ascii_lowercase();
    if let Some((_, alternates)) = ALTERNATES.iter().find(|(name, _)| *name == lower) {
        fonts.extend(alternates.iter());
    }
    fonts.extend(FALLBACK.iter());
    let mut seen = Vec::new();
    for f in fonts {
        if !seen.iter().any(|s: &&str| s.eq_ignore_ascii_case(f)) {
            seen.push(f);
        }
    }
    format!(
        "({},)",
        seen.iter().map(|f| quote(f)).collect::<Vec<_>>().join(", ")
    )
}

fn stroke(border: Option<Border>) -> String {
    match border {
        Some(b) => format!("{} + {}", pt(b.width), color(b.color)),
        None => "none".into(),
    }
}

impl Writer<'_> {
    fn document(&mut self) {
        let doc = self.document;
        self.out.push_str(
            "#set text(top-edge: 0.8em, bottom-edge: -0.2em, hyphenate: false)\n\
             #set par(spacing: 0pt, leading: 0pt, justify: false)\n\
             #set block(spacing: 0pt)\n\
             #show heading: it => it.body\n",
        );
        let mut start = 0;
        let sections: Vec<Section> = if doc.sections.is_empty() {
            vec![Section {
                end: doc.body.len(),
                columns: 1,
                ..Section::default()
            }]
        } else {
            doc.sections.clone()
        };
        for (i, section) in sections.iter().enumerate() {
            let end = section.end.min(doc.body.len()).max(start);
            let blocks = &doc.body[start..end];
            start = end;
            let previous = i.checked_sub(1).map(|p| &sections[p]);
            self.section(i, section, previous, blocks);
        }
        if start < doc.body.len() {
            let rest: Vec<Block> = doc.body[start..].to_vec();
            self.blocks(&rest, text_width(sections.last().expect("one section")));
        }
    }

    fn section(
        &mut self,
        index: usize,
        section: &Section,
        previous: Option<&Section>,
        blocks: &[Block],
    ) {
        let page = section.page;
        let same_page = previous.is_some_and(|p| p.page == page);
        let width = text_width(section);
        // A section that continues the page keeps its page setup; columns
        // change inside the page.
        let continuous = !section.new_page && same_page;
        let label = format!("konvert-section-{index}");
        if !continuous {
            let header = self.margin_content(section, true, &label);
            let footer = self.margin_content(section, false, &label);
            let _ = writeln!(
                self.out,
                "#set page(width: {}, height: {}, margin: (top: {}, bottom: {}, left: {}, right: {}), \
                 header: {header}, footer: {footer}, header-ascent: {}, footer-descent: {}, columns: {}, \
                 numbering: none)",
                pt(page.width),
                pt(page.height),
                pt(page.top),
                pt(page.bottom),
                pt(page.left),
                pt(page.right),
                pt((page.top - page.header - 14.0).max(4.0)),
                pt((page.bottom - page.footer - 14.0).max(4.0)),
                section.columns.max(1),
            );
            if section.columns > 1 {
                let _ = writeln!(self.out, "#set columns(gutter: {})", pt(section.column_gap));
            }
        }
        let _ = writeln!(self.out, "#metadata(none) <{label}>");
        if let Some(start) = section.page_start {
            let _ = writeln!(self.out, "#counter(page).update({start})");
        }
        if continuous && section.columns > 1 {
            let _ = writeln!(
                self.out,
                "#columns({}, gutter: {})[",
                section.columns,
                pt(section.column_gap)
            );
            self.blocks(blocks, width / section.columns as f32);
            self.out.push_str("]\n");
        } else {
            self.blocks(blocks, width / section.columns.max(1) as f32);
        }
    }

    /// The header (or footer) of a section: a different one on its first
    /// page or on even pages when the document says so.
    fn margin_content(&mut self, section: &Section, header: bool, label: &str) -> String {
        let (default, first, even) = if header {
            (&section.header, &section.first_header, &section.even_header)
        } else {
            (&section.footer, &section.first_footer, &section.even_footer)
        };
        let width = text_width(section);
        let even_and_odd = self.document.even_and_odd;
        let mut render = |blocks: &Option<Vec<super::read::Block>>| -> String {
            match blocks {
                Some(blocks) if !blocks.is_empty() => {
                    let saved = std::mem::take(&mut self.out);
                    self.blocks(blocks, width);
                    let body = std::mem::replace(&mut self.out, saved);
                    format!("[{body}]")
                }
                _ => "[]".into(),
            }
        };
        let default_content = render(default);
        let first_content = if section.title_page {
            render(first)
        } else {
            default_content.clone()
        };
        let even_content = if even_and_odd {
            render(even)
        } else {
            default_content.clone()
        };
        if first_content == default_content && even_content == default_content {
            return default_content;
        }
        format!(
            "context {{ if here().page() == locate(<{label}>).page() {{ {first_content} }} \
             else if calc.even(here().page()) {{ {even_content} }} else {{ {default_content} }} }}"
        )
    }

    fn blocks(&mut self, blocks: &[Block], width: f32) {
        for block in blocks {
            match block {
                Block::Paragraph(p) => self.paragraph(p),
                Block::Table(t) => self.table(t, width),
            }
        }
    }

    fn paragraph(&mut self, p: &Paragraph) {
        if p.page_break_before {
            self.out.push_str("#pagebreak(weak: true)\n");
        }
        let size = p
            .inlines
            .iter()
            .find_map(|i| match i {
                Inline::Text(_, props) => Some(props.size),
                _ => None,
            })
            .unwrap_or(p.mark.size);
        // Every line box is one em (0.8 above the baseline, 0.2 below); Word's
        // single spacing is about 1.2 em, so the rest is leading.
        let leading = match p.line {
            LineSpacing::Multiple(m) => size * (LINE * m - 1.0),
            LineSpacing::Exact(l) | LineSpacing::AtLeast(l) => l - size,
        }
        .max(0.0);
        let (indent, hanging) = if p.first_line < 0.0 {
            (0.0, -p.first_line)
        } else {
            (p.first_line, 0.0)
        };
        let align = match p.align {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
            Align::Justify => "left",
        };
        let left = (p.left - hanging).max(0.0);
        let _ = write!(
            self.out,
            "#block(above: {}, below: {}, inset: (left: {}, right: {}), width: 100%, breakable: {}, sticky: {}{})[",
            pt(p.before),
            pt(p.after),
            pt(left),
            pt(p.right.max(0.0)),
            !p.keep_lines,
            p.keep_next,
            p.shading
                .map(|c| format!(", fill: {}", color(c)))
                .unwrap_or_default(),
        );
        let _ = write!(
            self.out,
            "#set par(justify: {}, leading: {}, first-line-indent: (amount: {}, all: true), hanging-indent: {})\n#set align({align})\n",
            p.align == Align::Justify,
            pt(leading),
            pt(indent),
            pt(hanging),
        );
        let empty = p.label.is_none()
            && p.inlines.iter().all(|i| match i {
                Inline::Text(t, _) => t.trim().is_empty(),
                _ => false,
            });
        if empty {
            // An empty paragraph is one empty line.
            let _ = write!(
                self.out,
                "#v({})",
                pt(size * LINE * line_multiple(p.line, size))
            );
            self.out.push_str("]\n");
            return;
        }
        let heading = p.outline.map(|level| level + 1).filter(|l| *l <= 6);
        if let Some(level) = heading {
            let _ = write!(
                self.out,
                "#heading(level: {level}, outlined: true, bookmarked: true)["
            );
        }
        if let Some((label, props)) = &p.label {
            // The label sits in the hanging indent, the text starts at the indent.
            let gap = if hanging > 0.0 {
                hanging
            } else {
                props.size * 1.5
            };
            let _ = write!(
                self.out,
                "#box(width: {})[{}]",
                pt(gap),
                self.text(label, props)
            );
        }
        let inlines = match self.tabbed(p) {
            Some(grid) => grid,
            None => self.inlines(&p.inlines),
        };
        self.out.push_str(&inlines);
        if heading.is_some() {
            self.out.push(']');
        }
        self.out.push_str("]\n");
    }

    /// A line with tab stops as a grid whose columns start at the stops
    /// (Typst has no tab stops). `None` when the paragraph does not fit
    /// that shape (more tabs than stops, several lines, an indent).
    fn tabbed(&mut self, p: &Paragraph) -> Option<String> {
        let tabs = p
            .inlines
            .iter()
            .filter(|i| matches!(i, Inline::Tab(_)))
            .count();
        let breaks = p.inlines.iter().any(|i| matches!(i, Inline::LineBreak));
        if p.tabs.is_empty()
            || tabs == 0
            || tabs > p.tabs.len()
            || breaks
            || p.first_line != 0.0
            || p.label.is_some()
        {
            return None;
        }
        let mut edges = vec![p.left];
        for stop in p.tabs.iter().filter(|s| **s > p.left + 1.0).take(tabs) {
            edges.push(*stop);
        }
        if edges.len() != tabs + 1 {
            return None;
        }
        let mut columns: Vec<String> = edges.windows(2).map(|w| pt(w[1] - w[0])).collect();
        columns.push("1fr".into());
        let mut cells = Vec::new();
        let mut current = Vec::new();
        for inline in &p.inlines {
            if matches!(inline, Inline::Tab(_)) {
                cells.push(std::mem::take(&mut current));
            } else {
                current.push(inline.clone());
            }
        }
        cells.push(current);
        let cells: Vec<String> = cells
            .iter()
            .map(|c| format!("[{}]", self.inlines(c)))
            .collect();
        Some(format!(
            "#grid(columns: ({},), {})",
            columns.join(", "),
            cells.join(", ")
        ))
    }

    fn inlines(&mut self, inlines: &[Inline]) -> String {
        let mut out = String::new();
        for inline in inlines {
            match inline {
                Inline::Text(text, props) => out.push_str(&self.text(text, props)),
                Inline::Tab(props) => {
                    let _ = write!(out, "#h({})", pt(props.size * 2.5));
                }
                Inline::LineBreak => out.push_str("#linebreak()"),
                Inline::ColumnBreak => out.push_str("#colbreak()"),
                Inline::PageBreak => {}
                Inline::Picture {
                    part,
                    width,
                    height,
                } => {
                    let Some(data) = self.document.media.get(part) else {
                        continue;
                    };
                    let path = format!("/{}", part.trim_start_matches('/'));
                    self.files.insert(path.clone(), data.clone());
                    let _ = write!(
                        out,
                        "#box(image({}, width: {}, height: {}, fit: \"stretch\"))",
                        quote(&path),
                        pt(width.max(1.0)),
                        pt(height.max(1.0))
                    );
                }
                Inline::Footnote(id) => {
                    let blocks = self.document.footnotes.get(id).cloned().unwrap_or_default();
                    let mut body = String::new();
                    for (i, block) in blocks.iter().enumerate() {
                        if let Block::Paragraph(p) = block {
                            if i > 0 {
                                body.push_str("#linebreak()");
                            }
                            body.push_str(&self.inlines(&p.inlines));
                        }
                    }
                    let _ = write!(out, "#footnote[{body}]");
                }
                Inline::PageNumber(props) => {
                    let _ = write!(
                        out,
                        "#context {}",
                        styled("counter(page).display()", props, false)
                    );
                }
                Inline::PageCount(props) => {
                    let _ = write!(
                        out,
                        "#context {}",
                        styled("str(counter(page).final().first())", props, false)
                    );
                }
                Inline::Link { url, inlines } => {
                    let inner = self.inlines(inlines);
                    let _ = write!(out, "#link({})[{inner}]", quote(url));
                }
            }
        }
        out
    }

    fn text(&self, text: &str, props: &RunProps) -> String {
        let text = if props.caps {
            text.to_uppercase()
        } else {
            text.to_owned()
        };
        format!("#{}", styled(&quote(&text), props, true))
    }

    fn table(&mut self, table: &Table, width: f32) {
        // Columns wider than the text are narrowed in proportion.
        let total: f32 = table.columns.iter().sum();
        let scale = if total > width && total > 0.0 {
            width / total
        } else {
            1.0
        };
        let columns: Vec<String> = table.columns.iter().map(|c| pt(c * scale)).collect();
        let [top, right, bottom, left] = table.margins;
        let wrap = match table.align {
            Align::Center => "#align(center)[",
            Align::Right => "#align(right)[",
            _ => "#block[",
        };
        self.out.push_str(wrap);
        if table.indent > 0.0 && table.align == Align::Left {
            let _ = write!(self.out, "#pad(left: {})[", pt(table.indent));
        }
        let _ = writeln!(
            self.out,
            "#table(columns: ({},), inset: (top: {}, right: {}, bottom: {}, left: {}), stroke: none, align: left + top,",
            columns.join(", "),
            pt(top),
            pt(right),
            pt(bottom.max(1.0)),
            pt(left),
        );
        let headers = table.rows.iter().take_while(|r| r.header).count();
        let rows = table.rows.len();
        for (r, row) in table.rows.iter().enumerate() {
            if r == 0 && headers > 0 {
                self.out.push_str("table.header(");
            }
            for cell in row.cells.iter().filter(|c| !c.covered) {
                let first = cell.column == 0;
                let last = cell.column + cell.span >= table.columns.len();
                let top_row = r == 0;
                let bottom_row = r + cell.rows >= rows;
                let b = &table.borders;
                let pick = |own: Option<Border>,
                            outer: bool,
                            outer_b: Option<Border>,
                            inner_b: Option<Border>| {
                    own.or(if outer { outer_b } else { inner_b })
                };
                let edges = Borders {
                    top: pick(cell.borders.top, top_row, b.top, b.inside_h),
                    bottom: pick(cell.borders.bottom, bottom_row, b.bottom, b.inside_h),
                    left: pick(cell.borders.left, first, b.left, b.inside_v),
                    right: pick(cell.borders.right, last, b.right, b.inside_v),
                    ..Borders::default()
                };
                let valign = match cell.valign {
                    VAlign::Top => "top",
                    VAlign::Center => "horizon",
                    VAlign::Bottom => "bottom",
                };
                let cell_width: f32 = table
                    .columns
                    .iter()
                    .skip(cell.column)
                    .take(cell.span)
                    .sum::<f32>()
                    * scale;
                let saved = std::mem::take(&mut self.out);
                self.blocks(&cell.blocks, (cell_width - left - right).max(10.0));
                let body = std::mem::replace(&mut self.out, saved);
                let _ = writeln!(
                    self.out,
                    "table.cell(x: {}, y: {}, colspan: {}, rowspan: {}, align: {valign}, stroke: (top: {}, bottom: {}, left: {}, right: {}){})[{body}],",
                    cell.column,
                    r,
                    cell.span,
                    cell.rows,
                    stroke(edges.top),
                    stroke(edges.bottom),
                    stroke(edges.left),
                    stroke(edges.right),
                    cell.fill
                        .map(|c| format!(", fill: {}", color(c)))
                        .unwrap_or_default(),
                );
            }
            if headers > 0 && r + 1 == headers {
                self.out.push_str("),\n");
            }
        }
        self.out.push(')');
        if table.indent > 0.0 && table.align == Align::Left {
            self.out.push(']');
        }
        self.out.push_str("]\n");
    }
}

fn line_multiple(line: LineSpacing, size: f32) -> f32 {
    match line {
        LineSpacing::Multiple(m) => m.max(0.5),
        LineSpacing::Exact(l) | LineSpacing::AtLeast(l) => (l / (size * LINE)).max(0.5),
    }
}

/// `body` (Typst code giving text or content) with the run's formatting.
fn styled(body: &str, props: &RunProps, is_string: bool) -> String {
    let mut args = vec![
        format!("font: {}", font_list(&props.font)),
        format!("size: {}", pt(props.size)),
    ];
    if props.bold {
        args.push("weight: \"bold\"".into());
    }
    if props.italic {
        args.push("style: \"italic\"".into());
    }
    if let Some(c) = props.color {
        args.push(format!("fill: {}", color(c)));
    }
    if props.spacing != 0.0 {
        args.push(format!("tracking: {}", pt(props.spacing)));
    }
    let _ = is_string;
    let mut out = format!("text({}, {body})", args.join(", "));
    if props.small_caps {
        out = format!("smallcaps({out})");
    }
    match props.baseline {
        Baseline::Super => out = format!("super({out})"),
        Baseline::Sub => out = format!("sub({out})"),
        Baseline::Normal => {}
    }
    if props.underline {
        out = format!("underline({out})");
    }
    if props.strike {
        out = format!("strike({out})");
    }
    if let Some(c) = props.highlight {
        out = format!("highlight(fill: {}, {out})", color(c));
    }
    out
}

fn text_width(section: &Section) -> f32 {
    (section.page.width - section.page.left - section.page.right).max(72.0)
}

/// The Typst "world" of one document: its source, its pictures, the fonts.
struct KonvertWorld {
    library: LazyHash<Library>,
    main: Source,
    files: HashMap<FileId, Bytes>,
}

fn file_id(path: &str) -> Option<FileId> {
    Some(FileId::new(RootedPath::new(
        VirtualRoot::Project,
        VirtualPath::new(path).ok()?,
    )))
}

impl KonvertWorld {
    fn new(source: String, files: HashMap<String, Vec<u8>>) -> Self {
        let main = file_id("/main.typ").expect("a valid path");
        Self {
            library: LazyHash::new(Library::builder().build()),
            main: Source::new(main, source),
            files: files
                .into_iter()
                .filter_map(|(path, data)| Some((file_id(&path)?, Bytes::new(data))))
                .collect(),
        }
    }
}

impl World for KonvertWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        fonts().book()
    }

    fn main(&self) -> FileId {
        self.main.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main.id() {
            Ok(self.main.clone())
        } else {
            Err(FileError::AccessDenied)
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.files.get(&id).cloned().ok_or(FileError::AccessDenied)
    }

    fn font(&self, index: usize) -> Option<Font> {
        fonts().font(index)
    }

    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_always_a_string() {
        assert_eq!(quote("a \"b\" \\ #c"), "\"a \\\"b\\\" \\\\ #c\"");
        assert_eq!(quote("x\ny\u{7}"), "\"x y\"");
        let props = RunProps {
            bold: true,
            underline: true,
            color: Some([255, 0, 0]),
            ..RunProps::default()
        };
        let text = styled(&quote("#import \"x\""), &props, true);
        assert!(
            text.starts_with("underline(text(font: (\"Calibri\", \"Carlito\""),
            "{text}"
        );
        assert!(text.contains("weight: \"bold\"") && text.contains("fill: rgb(255,0,0)"));
        assert!(text.contains("\"#import \\\"x\\\"\""));
    }

    #[test]
    fn office_fonts_have_compatible_alternates() {
        let list = font_list("Times New Roman");
        assert!(
            list.starts_with("(\"Times New Roman\", \"Liberation Serif\""),
            "{list}"
        );
        assert_eq!(font_list("Arial").matches("Arial").count(), 1);
    }
}
