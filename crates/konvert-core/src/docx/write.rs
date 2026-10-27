//! Writes a DOCX from the model of [`super::read`]: paragraphs with their
//! formatting, list labels as text, tab stops, line breaks and pictures, in
//! one section. Headings use Word's "Heading 1–3" styles, so they appear in
//! Word's navigation pane. The package holds no author, dates or paths.
//!
//! The XML namespaces and relationship types below are names defined by the
//! Office Open XML standard: they identify parts of the format and are never
//! opened.

use std::fmt::Write as _;
use std::io::Write;

use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

use super::read::{Align, Baseline, Block, Document, Inline, LineSpacing, Paragraph, RunProps};
use crate::error::CoreError;

const NS_MAIN: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const NS_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const NS_PIC: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
const NS_PACKAGE_RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const NS_TYPES: &str = "http://schemas.openxmlformats.org/package/2006/content-types";

/// Points → twips (1/20 point).
fn twips(points: f32) -> i64 {
    (points * 20.0).round() as i64
}

/// Points → EMU.
fn emu(points: f32) -> i64 {
    (points * 12_700.0).round() as i64
}

/// Text for XML content and attributes.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // Characters XML 1.0 does not allow.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            '\u{FFFE}' | '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

/// A DOCX file of `document` (its first section's page setup).
pub fn to_docx(document: &Document) -> Result<Vec<u8>, CoreError> {
    let mut pictures: Vec<(String, String, &[u8])> = Vec::new();
    let mut body = String::new();
    let mut ids = Ids::default();
    for block in &document.body {
        if let Block::Paragraph(p) = block {
            paragraph(&mut body, p, document, &mut pictures, &mut ids);
        }
    }
    let page = document
        .sections
        .first()
        .map(|s| s.page)
        .unwrap_or_default();
    let _ = write!(
        body,
        "<w:sectPr><w:pgSz w:w=\"{}\" w:h=\"{}\"{}/><w:pgMar w:top=\"{}\" w:right=\"{}\" w:bottom=\"{}\" w:left=\"{}\" w:header=\"{}\" w:footer=\"{}\" w:gutter=\"0\"/></w:sectPr>",
        twips(page.width),
        twips(page.height),
        if page.width > page.height {
            " w:orient=\"landscape\""
        } else {
            ""
        },
        twips(page.top),
        twips(page.right),
        twips(page.bottom),
        twips(page.left),
        twips(page.header),
        twips(page.footer),
    );
    let document_xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <w:document xmlns:w=\"{NS_MAIN}\" xmlns:r=\"{NS_REL}\" xmlns:wp=\"{NS_WP}\" xmlns:a=\"{NS_A}\" xmlns:pic=\"{NS_PIC}\"><w:body>{body}</w:body></w:document>"
    );

    let mut rels = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"{NS_PACKAGE_RELS}\">\
         <Relationship Id=\"rStyles\" Type=\"{NS_REL}/styles\" Target=\"styles.xml\"/>\
         <Relationship Id=\"rSettings\" Type=\"{NS_REL}/settings\" Target=\"settings.xml\"/>"
    );
    for (i, (_, name, _)) in pictures.iter().enumerate() {
        let _ = write!(
            rels,
            "<Relationship Id=\"rPicture{}\" Type=\"{NS_REL}/image\" Target=\"media/{name}\"/>",
            i + 1
        );
    }
    rels.push_str("</Relationships>");

    let body_font = most_used_font(document);
    let files: Vec<(String, Vec<u8>)> = vec![
        ("[Content_Types].xml".into(), content_types().into_bytes()),
        (
            "_rels/.rels".into(),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"{NS_PACKAGE_RELS}\">\
                 <Relationship Id=\"rDocument\" Type=\"{NS_REL}/officeDocument\" Target=\"word/document.xml\"/></Relationships>"
            )
            .into_bytes(),
        ),
        ("word/document.xml".into(), document_xml.into_bytes()),
        ("word/_rels/document.xml.rels".into(), rels.into_bytes()),
        ("word/styles.xml".into(), styles(&body_font).into_bytes()),
        (
            "word/settings.xml".into(),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:settings xmlns:w=\"{NS_MAIN}\">\
                 <w:defaultTabStop w:val=\"709\"/></w:settings>"
            )
            .into_bytes(),
        ),
    ];

    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(9));
    let io = |e: zip::result::ZipError| CoreError::Io(std::io::Error::other(e.to_string()));
    for (name, data) in &files {
        zip.start_file(name.as_str(), options).map_err(io)?;
        zip.write_all(data)?;
    }
    for (_, name, data) in &pictures {
        // Pictures are compressed already.
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        zip.start_file(format!("word/media/{name}"), stored)
            .map_err(io)?;
        zip.write_all(data)?;
    }
    Ok(zip.finish().map_err(io)?.into_inner())
}

#[derive(Default)]
struct Ids {
    drawing: u32,
}

fn content_types() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"{NS_TYPES}\">\
         <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
         <Default Extension=\"xml\" ContentType=\"application/xml\"/>\
         <Default Extension=\"png\" ContentType=\"image/png\"/>\
         <Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/>\
         <Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
         <Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/>\
         <Override PartName=\"/word/settings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml\"/>\
         </Types>"
    )
}

/// Normal text in the document's most used font; Word's heading styles.
fn styles(font: &str) -> String {
    let font = escape(font);
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:styles xmlns:w=\"{NS_MAIN}\">\
         <w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"{font}\" w:hAnsi=\"{font}\" w:eastAsia=\"{font}\" w:cs=\"{font}\"/>\
         <w:sz w:val=\"22\"/><w:szCs w:val=\"22\"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>\
         <w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/></w:style>"
    );
    for level in 1..=3 {
        let _ = write!(
            out,
            "<w:style w:type=\"paragraph\" w:styleId=\"Heading{level}\"><w:name w:val=\"heading {level}\"/><w:basedOn w:val=\"Normal\"/>\
             <w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:keepNext/><w:outlineLvl w:val=\"{}\"/></w:pPr><w:rPr><w:b/></w:rPr></w:style>",
            level - 1
        );
    }
    out.push_str("</w:styles>");
    out
}

/// The font of most text: the default of the document.
fn most_used_font(document: &Document) -> String {
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for block in &document.body {
        if let Block::Paragraph(p) = block {
            for inline in &p.inlines {
                if let Inline::Text(text, props) = inline {
                    *counts.entry(props.font.as_str()).or_default() += text.len();
                }
            }
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(f, _)| f.to_owned())
        .unwrap_or_else(|| "Calibri".into())
}

fn paragraph<'d>(
    out: &mut String,
    p: &'d Paragraph,
    document: &'d Document,
    pictures: &mut Vec<(String, String, &'d [u8])>,
    ids: &mut Ids,
) {
    out.push_str("<w:p><w:pPr>");
    if let Some(level) = p.outline.filter(|l| *l < 3) {
        let _ = write!(out, "<w:pStyle w:val=\"Heading{}\"/>", level + 1);
    }
    if p.keep_next {
        out.push_str("<w:keepNext/>");
    }
    if p.page_break_before {
        out.push_str("<w:pageBreakBefore/>");
    }
    if let Some([r, g, b]) = p.shading {
        let _ = write!(
            out,
            "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{r:02X}{g:02X}{b:02X}\"/>"
        );
    }
    if !p.tabs.is_empty() {
        out.push_str("<w:tabs>");
        for tab in &p.tabs {
            let _ = write!(out, "<w:tab w:val=\"left\" w:pos=\"{}\"/>", twips(*tab));
        }
        out.push_str("</w:tabs>");
    }
    let (line, rule) = match p.line {
        LineSpacing::Multiple(m) => ((m * 240.0).round() as i64, "auto"),
        LineSpacing::Exact(l) => (twips(l), "exact"),
        LineSpacing::AtLeast(l) => (twips(l), "atLeast"),
    };
    let _ = write!(
        out,
        "<w:spacing w:before=\"{}\" w:after=\"{}\" w:line=\"{line}\" w:lineRule=\"{rule}\"/>",
        twips(p.before.max(0.0)),
        twips(p.after.max(0.0))
    );
    let first = if p.first_line < 0.0 {
        format!(" w:hanging=\"{}\"", twips(-p.first_line))
    } else if p.first_line > 0.0 {
        format!(" w:firstLine=\"{}\"", twips(p.first_line))
    } else {
        String::new()
    };
    let _ = write!(
        out,
        "<w:ind w:left=\"{}\" w:right=\"{}\"{first}/>",
        twips(p.left.max(0.0)),
        twips(p.right.max(0.0))
    );
    let jc = match p.align {
        Align::Left => "left",
        Align::Center => "center",
        Align::Right => "right",
        Align::Justify => "both",
    };
    let _ = write!(out, "<w:jc w:val=\"{jc}\"/>");
    let _ = write!(out, "<w:rPr>{}</w:rPr></w:pPr>", run_props(&p.mark));
    if let Some((label, props)) = &p.label {
        run(out, label, props);
        out.push_str("<w:r><w:tab/></w:r>");
    }
    for inline in &p.inlines {
        match inline {
            Inline::Text(text, props) => run(out, text, props),
            Inline::Tab(props) => {
                let _ = write!(
                    out,
                    "<w:r><w:rPr>{}</w:rPr><w:tab/></w:r>",
                    run_props(props)
                );
            }
            Inline::LineBreak => out.push_str("<w:r><w:br/></w:r>"),
            Inline::PageBreak => out.push_str("<w:r><w:br w:type=\"page\"/></w:r>"),
            Inline::ColumnBreak => out.push_str("<w:r><w:br w:type=\"column\"/></w:r>"),
            Inline::Picture {
                part,
                width,
                height,
            } => {
                let Some(data) = document.media.get(part) else {
                    continue;
                };
                let id = format!("rPicture{}", picture_index(pictures, part, data));
                ids.drawing += 1;
                let n = ids.drawing;
                let (cx, cy) = (emu(*width), emu(*height));
                let _ = write!(
                    out,
                    "<w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{cx}\" cy=\"{cy}\"/>\
                     <wp:docPr id=\"{n}\" name=\"Picture {n}\"/><a:graphic><a:graphicData uri=\"{NS_PIC}\"><pic:pic>\
                     <pic:nvPicPr><pic:cNvPr id=\"{n}\" name=\"Picture {n}\"/><pic:cNvPicPr/></pic:nvPicPr>\
                     <pic:blipFill><a:blip r:embed=\"{id}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
                     <pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr>\
                     </pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"
                );
            }
            // Fields, notes and links do not come out of PDFs.
            Inline::Footnote(_) | Inline::PageNumber(_) | Inline::PageCount(_) => {}
            Inline::Link { inlines, .. } => {
                for inner in inlines {
                    if let Inline::Text(text, props) = inner {
                        run(out, text, props);
                    }
                }
            }
        }
    }
    out.push_str("</w:p>");
}

/// The number of the picture of `part` in the package (added the first
/// time, named `imageN.png` or `imageN.jpeg` after its content).
fn picture_index<'d>(
    pictures: &mut Vec<(String, String, &'d [u8])>,
    part: &str,
    data: &'d [u8],
) -> usize {
    if let Some(i) = pictures.iter().position(|(source, _, _)| source == part) {
        return i + 1;
    }
    let n = pictures.len() + 1;
    let ext = if data.starts_with(&[0xFF, 0xD8]) {
        "jpeg"
    } else {
        "png"
    };
    pictures.push((part.to_owned(), format!("image{n}.{ext}"), data));
    n
}

fn run(out: &mut String, text: &str, props: &RunProps) {
    let space = if text.starts_with(' ') || text.ends_with(' ') {
        " xml:space=\"preserve\""
    } else {
        ""
    };
    let _ = write!(
        out,
        "<w:r><w:rPr>{}</w:rPr><w:t{space}>{}</w:t></w:r>",
        run_props(props),
        escape(text)
    );
}

fn run_props(props: &RunProps) -> String {
    let font = escape(&props.font);
    let size = (props.size * 2.0).round().max(2.0) as i64;
    let mut out = format!(
        "<w:rFonts w:ascii=\"{font}\" w:hAnsi=\"{font}\" w:eastAsia=\"{font}\" w:cs=\"{font}\"/>"
    );
    if props.bold {
        out.push_str("<w:b/>");
    }
    if props.italic {
        out.push_str("<w:i/>");
    }
    if props.caps {
        out.push_str("<w:caps/>");
    }
    if props.small_caps {
        out.push_str("<w:smallCaps/>");
    }
    if props.strike {
        out.push_str("<w:strike/>");
    }
    if let Some([r, g, b]) = props.color {
        let _ = write!(out, "<w:color w:val=\"{r:02X}{g:02X}{b:02X}\"/>");
    }
    if props.spacing != 0.0 {
        let _ = write!(out, "<w:spacing w:val=\"{}\"/>", twips(props.spacing));
    }
    let _ = write!(out, "<w:sz w:val=\"{size}\"/><w:szCs w:val=\"{size}\"/>");
    // Elements in the order the standard's schema gives (Word checks it).
    if props.underline {
        out.push_str("<w:u w:val=\"single\"/>");
    }
    if let Some([r, g, b]) = props.highlight {
        let _ = write!(
            out,
            "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{r:02X}{g:02X}{b:02X}\"/>"
        );
    }
    match props.baseline {
        Baseline::Super => out.push_str("<w:vertAlign w:val=\"superscript\"/>"),
        Baseline::Sub => out.push_str("<w:vertAlign w:val=\"subscript\"/>"),
        Baseline::Normal => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docx::Package;
    use crate::docx::read::{Section, read};

    fn paragraph_of(text: &str, props: RunProps) -> Paragraph {
        Paragraph {
            align: Align::Justify,
            before: 6.0,
            after: 3.0,
            line: LineSpacing::Multiple(1.15),
            left: 0.0,
            right: 0.0,
            first_line: 14.0,
            outline: None,
            page_break_before: false,
            keep_lines: false,
            keep_next: false,
            shading: None,
            mark: props.clone(),
            label: None,
            tabs: Vec::new(),
            inlines: vec![Inline::Text(text.into(), props)],
        }
    }

    #[test]
    fn written_documents_read_back_the_same() {
        let bold = RunProps {
            font: "Times New Roman".into(),
            size: 12.0,
            bold: true,
            color: Some([0x1F, 0x38, 0x64]),
            ..RunProps::default()
        };
        let mut heading = paragraph_of("Titolo <&>", bold.clone());
        heading.outline = Some(0);
        heading.align = Align::Center;
        let mut body = paragraph_of(" testo  normale ", RunProps::default());
        body.page_break_before = true;
        let png = b"\x89PNG\r\n\x1a\nfake".to_vec();
        body.inlines.push(Inline::Picture {
            part: "image1.png".into(),
            width: 100.0,
            height: 50.0,
        });
        let document = Document {
            body: vec![Block::Paragraph(heading), Block::Paragraph(body)],
            sections: vec![Section {
                end: 2,
                columns: 1,
                ..Section::default()
            }],
            media: [("image1.png".to_owned(), png.clone())].into(),
            ..Document::default()
        };
        let bytes = to_docx(&document).unwrap();
        let package = Package::open(&bytes).unwrap();
        assert_eq!(package.get("word/media/image1.png"), Some(png.as_slice()));
        let back = read(&package).unwrap();
        assert_eq!(back.body.len(), 2);
        let Block::Paragraph(h) = &back.body[0] else {
            panic!()
        };
        assert_eq!((h.outline, h.align), (Some(0), Align::Center));
        assert_eq!(h.inlines, [Inline::Text("Titolo <&>".into(), bold)]);
        let Block::Paragraph(b) = &back.body[1] else {
            panic!()
        };
        assert!(b.page_break_before);
        assert_eq!((b.before, b.after, b.first_line), (6.0, 3.0, 14.0));
        assert_eq!(b.line, LineSpacing::Multiple(1.15));
        assert!(matches!(&b.inlines[0], Inline::Text(t, _) if t == " testo  normale "));
        assert!(
            matches!(&b.inlines[1], Inline::Picture { width, .. } if (*width - 100.0).abs() < 0.01)
        );
        assert!(package.names().all(|n| !n.contains("docProps")));
        let (a, b) = (back.sections[0].page, document.sections[0].page);
        assert!((a.width - b.width).abs() < 0.1 && (a.top - b.top).abs() < 0.1);
    }
}
