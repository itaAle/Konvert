//! Plain text (TXT) of a document: each paragraph on one line (list numbers
//! and bullets kept), table rows with their cells separated by tabs, a blank
//! line before each new page of a PDF, and the footnotes at the end, numbered
//! like their marks. Pictures, headers and footers are left out.

use super::read::{Block, Document, Inline};

/// The text of `doc`, with `\n` line ends.
pub fn plain_text(doc: &Document) -> String {
    let mut writer = Writer::default();
    writer.blocks(&doc.body);
    let mut out = std::mem::take(&mut writer.out);
    let mut written = 0;
    while written < writer.notes.len() {
        let id = writer.notes[written].clone();
        written += 1;
        if let Some(blocks) = doc.footnotes.get(&id) {
            writer.out = format!("[{written}] ");
            writer.blocks(blocks);
            if written == 1 {
                out.push('\n');
            }
            out.push_str(&writer.out);
        }
    }
    out
}

#[derive(Default)]
struct Writer {
    out: String,
    /// Footnotes, in the order of their marks.
    notes: Vec<String>,
}

impl Writer {
    fn blocks(&mut self, blocks: &[Block]) {
        for block in blocks {
            match block {
                Block::Paragraph(paragraph) => {
                    if paragraph.page_break_before && !self.out.is_empty() {
                        self.out.push('\n');
                    }
                    if let Some((label, _)) = &paragraph.label {
                        self.out.push_str(label);
                        self.out.push(' ');
                    }
                    self.inlines(&paragraph.inlines);
                    self.end_line();
                }
                Block::Table(table) => {
                    for row in &table.rows {
                        let mut first = true;
                        for cell in row.cells.iter().filter(|c| !c.covered) {
                            if !first {
                                self.out.push('\t');
                            }
                            first = false;
                            // The paragraphs of a cell, on one line.
                            let mut inner = Writer {
                                notes: std::mem::take(&mut self.notes),
                                ..Writer::default()
                            };
                            inner.blocks(&cell.blocks);
                            self.notes = inner.notes;
                            let text: Vec<&str> = inner
                                .out
                                .split('\n')
                                .map(str::trim)
                                .filter(|t| !t.is_empty())
                                .collect();
                            self.out.push_str(&text.join(" "));
                        }
                        self.end_line();
                    }
                }
            }
        }
    }

    fn inlines(&mut self, inlines: &[Inline]) {
        for inline in inlines {
            match inline {
                Inline::Text(text, _) => self.out.push_str(text),
                Inline::Tab(_) => self.out.push('\t'),
                Inline::LineBreak | Inline::ColumnBreak => self.out.push('\n'),
                Inline::PageBreak => self.out.push_str("\n\n"),
                Inline::Footnote(id) => {
                    self.notes.push(id.clone());
                    self.out.push_str(&format!("[{}]", self.notes.len()));
                }
                Inline::Link { inlines, .. } => self.inlines(inlines),
                Inline::Picture { .. } | Inline::PageNumber(_) | Inline::PageCount(_) => {}
            }
        }
    }

    /// Ends the current line, without spaces left at its end.
    fn end_line(&mut self) {
        let kept = self.out.trim_end_matches([' ', '\t']).len();
        self.out.truncate(kept);
        self.out.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::super::read::{Align, Cell, LineSpacing, Paragraph, Row, RunProps, Table};
    use super::*;

    fn paragraph(inlines: Vec<Inline>) -> Paragraph {
        Paragraph {
            align: Align::Left,
            before: 0.0,
            after: 0.0,
            line: LineSpacing::Multiple(1.0),
            left: 0.0,
            right: 0.0,
            first_line: 0.0,
            outline: None,
            page_break_before: false,
            keep_lines: false,
            keep_next: false,
            shading: None,
            mark: RunProps::default(),
            label: None,
            tabs: Vec::new(),
            inlines,
        }
    }

    fn text(t: &str) -> Inline {
        Inline::Text(t.into(), RunProps::default())
    }

    fn cell(t: &str, covered: bool) -> Cell {
        Cell {
            column: 0,
            span: 1,
            rows: 1,
            covered,
            fill: None,
            borders: Default::default(),
            valign: Default::default(),
            blocks: vec![Block::Paragraph(paragraph(vec![text(t)]))],
        }
    }

    #[test]
    fn paragraphs_lists_tables_and_notes_become_lines() {
        let mut item = paragraph(vec![text("Primo punto")]);
        item.label = Some(("1.".into(), RunProps::default()));
        let mut next_page = paragraph(vec![text("Seconda pagina  ")]);
        next_page.page_break_before = true;
        let mut doc = Document {
            body: vec![
                Block::Paragraph(paragraph(vec![
                    text("Testo con "),
                    text("grassetto"),
                    Inline::Footnote("7".into()),
                    text("."),
                    Inline::Picture {
                        part: "word/media/image1.png".into(),
                        width: 10.0,
                        height: 10.0,
                    },
                ])),
                Block::Paragraph(item),
                Block::Table(Table {
                    columns: vec![100.0, 100.0],
                    rows: vec![
                        Row {
                            cells: vec![cell("Alta", false), cell("Unita", false)],
                            header: false,
                        },
                        Row {
                            cells: vec![cell("", true), cell("B", false), cell("C", false)],
                            header: false,
                        },
                    ],
                    align: Default::default(),
                    indent: 0.0,
                    borders: Default::default(),
                    margins: [0.0; 4],
                }),
                Block::Paragraph(next_page),
            ],
            ..Document::default()
        };
        doc.footnotes.insert(
            "7".into(),
            vec![Block::Paragraph(paragraph(vec![text("Testo della nota.")]))],
        );
        assert_eq!(
            plain_text(&doc),
            "Testo con grassetto[1].\n1. Primo punto\nAlta\tUnita\nB\tC\n\nSeconda pagina\n\n[1] Testo della nota.\n"
        );
    }
}
