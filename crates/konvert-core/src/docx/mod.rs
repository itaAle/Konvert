//! Word documents (DOCX), handled without FFmpeg and without other programs:
//!
//! * [`read`]: the document's XML parts → a simple model (paragraphs with
//!   their resolved formatting, lists, tables, pictures, sections with their
//!   page setup, headers and footers, footnotes);
//! * [`typeset`]: the model → a Typst document → a PDF, laid out by the Typst
//!   engine with the fonts installed on the computer;
//! * [`from_pdf`]: the text and pictures of a PDF → paragraphs, written as a
//!   new DOCX by [`write`];
//! * [`compress`]: a smaller DOCX (its pictures stored again);
//! * [`text`]: the plain text of the model (TXT).
//!
//! Nothing is fetched: pictures linked from outside the file, fields and
//! macros are never followed or run. The documents Konvert writes carry no
//! author, dates or paths.

pub mod compress;
pub mod from_pdf;
pub mod read;
pub mod text;
pub mod typeset;
pub mod write;

use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::CoreError;
use crate::probe::{MediaInfo, MediaKind};

/// Largest part of a DOCX Konvert reads (a guard against archive bombs).
const MAX_PART: u64 = 512 * 1024 * 1024;

/// What Konvert knows about a Word document before converting it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TextInfo {
    /// Pictures inside the document.
    pub pictures: u32,
    /// Page size of the first section, in points.
    pub width_pt: f32,
    pub height_pt: f32,
    /// Text boxes, charts, equations or objects that Konvert leaves out.
    pub simplified: bool,
}

/// The file is a ZIP archive with a Word main document.
pub fn is_docx(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    if !matches!(ext.as_deref(), Some("docx" | "docm" | "dotx")) {
        return false;
    }
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    zip::ZipArchive::new(file).is_ok_and(|mut zip| zip.by_name("word/document.xml").is_ok())
}

/// Analyses a Word document.
pub fn probe(path: &Path) -> Result<MediaInfo, CoreError> {
    let data = std::fs::read(path)?;
    let size = data.len() as u64;
    let package = Package::open(&data)?;
    let document = read::read(&package)?;
    let page = document
        .sections
        .first()
        .map(|s| s.page)
        .unwrap_or_default();
    Ok(MediaInfo {
        container: "docx".into(),
        kind: MediaKind::Text,
        duration_secs: None,
        size_bytes: Some(size),
        bit_rate: None,
        audio: Vec::new(),
        video: Vec::new(),
        cover_art: None,
        subtitle_streams: 0,
        picture: None,
        has_tags: false,
        has_location: false,
        document: None,
        text: Some(TextInfo {
            pictures: package.media_count() as u32,
            width_pt: page.width,
            height_pt: page.height,
            simplified: document.simplified,
        }),
    })
}

fn damaged(details: impl std::fmt::Display) -> CoreError {
    CoreError::CorruptInput {
        details: format!("DOCX: {details}"),
    }
}

/// The parts of a DOCX archive, read into memory.
pub struct Package {
    parts: Vec<(String, Vec<u8>)>,
}

impl Package {
    pub fn open(data: &[u8]) -> Result<Self, CoreError> {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(damaged)?;
        let mut parts = Vec::with_capacity(zip.len());
        let mut total = 0u64;
        for i in 0..zip.len() {
            let mut file = zip.by_index(i).map_err(damaged)?;
            if file.is_dir() {
                continue;
            }
            if file.encrypted() {
                return Err(damaged("the document is protected by a password"));
            }
            let name = file.name().map_err(damaged)?.replace('\\', "/");
            let mut bytes = Vec::new();
            (&mut file)
                .take(MAX_PART + 1)
                .read_to_end(&mut bytes)
                .map_err(damaged)?;
            total += bytes.len() as u64;
            if bytes.len() as u64 > MAX_PART || total > 4 * MAX_PART {
                return Err(damaged("a part of the document is too large"));
            }
            parts.push((name, bytes));
        }
        if !parts.iter().any(|(n, _)| n == "word/document.xml") {
            return Err(damaged("no main document"));
        }
        Ok(Self { parts })
    }

    pub fn get(&self, name: &str) -> Option<&[u8]> {
        let name = name.trim_start_matches('/');
        self.parts
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, b)| b.as_slice())
    }

    pub fn text(&self, name: &str) -> Option<String> {
        self.get(name)
            .map(|b| String::from_utf8_lossy(b).into_owned())
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.parts.iter().map(|(n, _)| n.as_str())
    }

    pub fn into_parts(self) -> Vec<(String, Vec<u8>)> {
        self.parts
    }

    fn media_count(&self) -> usize {
        self.parts
            .iter()
            .filter(|(n, _)| n.contains("/media/"))
            .count()
    }
}

/// Resolves `target` (a relationship target) against the folder of `part`:
/// `word/document.xml` + `media/a.png` → `word/media/a.png`.
pub(crate) fn resolve_target(part: &str, target: &str) -> String {
    if let Some(absolute) = target.strip_prefix('/') {
        return absolute.to_owned();
    }
    let mut folder: Vec<&str> = part.split('/').collect();
    folder.pop();
    for piece in target.split('/') {
        match piece {
            "" | "." => {}
            ".." => {
                folder.pop();
            }
            other => folder.push(other),
        }
    }
    folder.join("/")
}

/// The relationships file of a part: `word/document.xml` →
/// `word/_rels/document.xml.rels`.
pub(crate) fn rels_name(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((folder, file)) => format!("{folder}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_are_resolved_against_their_part() {
        assert_eq!(
            resolve_target("word/document.xml", "media/a.png"),
            "word/media/a.png"
        );
        assert_eq!(
            resolve_target("word/header1.xml", "../customXml/x.xml"),
            "customXml/x.xml"
        );
        assert_eq!(
            resolve_target("word/document.xml", "/word/media/b.jpeg"),
            "word/media/b.jpeg"
        );
        assert_eq!(
            rels_name("word/document.xml"),
            "word/_rels/document.xml.rels"
        );
        assert_eq!(
            rels_name("[Content_Types].xml"),
            "_rels/[Content_Types].xml.rels"
        );
    }
}
