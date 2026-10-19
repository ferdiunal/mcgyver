//! Office documents and PDFs with the engines of the "craft" apps: WordCraft, GridCraft,
//! DeckCraft and PdfCraft. Pure Rust (MIT OR Apache-2.0), so nothing needs to be installed.
//! They are alpha software: a file one can't read fails with its message.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pdfcraft_cos::Document;

use crate::archive::Progress;

/// Every engine reads the whole file into memory; larger files are refused up front.
const MAX_INPUT: u64 = 512 << 20;

/// Which engine reads a document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Word,
    Sheet,
    Deck,
}

impl Family {
    pub fn of(ext: &str) -> Option<Family> {
        match ext {
            "docx" | "doc" | "odt" | "rtf" | "html" | "md" | "txt" => Some(Family::Word),
            "xlsx" | "xls" | "ods" | "csv" | "tsv" => Some(Family::Sheet),
            "pptx" => Some(Family::Deck),
            _ => None,
        }
    }

    /// What its engine saves to, most wanted first. The extension is the label in lower case.
    pub fn targets(self) -> &'static [&'static str] {
        match self {
            Family::Word => &["PDF", "DOCX", "ODT", "RTF", "HTML", "TXT"],
            Family::Sheet => &["PDF", "XLSX", "CSV", "TSV", "HTML"],
            // TXT is the outline: titles and bullet text.
            Family::Deck => &["PDF", "PPTX", "TXT"],
        }
    }
}

/// `input` saved as `label`, one of its family's targets. A spreadsheet's PDF prints every
/// visible sheet with something on it; its CSV, TSV and HTML hold the active sheet.
pub fn convert(input: &Path, label: &str) -> io::Result<Vec<u8>> {
    let ext = crate::catalog::ext_of(input);
    let family = Family::of(&ext).ok_or_else(|| io::Error::other("not an office document"))?;
    let target = label.to_lowercase();
    // The engines read the format from the name, and put it in titles and headers.
    let name = input
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("document.{ext}"));
    let output = format!("{}.{target}", crate::catalog::stem_of(input));
    match family {
        Family::Word => {
            let doc = if ext == "html" {
                // From the path, so a web page finds the pictures beside it.
                check_size(input)?;
                wordcraft_engine::io::open_path(input)
            } else {
                wordcraft_engine::io::open_bytes(&name, &read(input)?)
            };
            wordcraft_engine::io::save_bytes(&output, &doc.map_err(io::Error::other)?)
                .map_err(io::Error::other)
        }
        Family::Sheet => {
            let bytes = read(input)?;
            let (book, _) = gridcraft_engine::io::open_bytes(&name, &bytes).map_err(failed)?;
            if target == "pdf" {
                use gridcraft_engine::cmd::print;
                let options = print::PrintOptions {
                    all_sheets: true,
                    ..Default::default()
                };
                let layouts = print::layouts(&book, &options);
                let pages: usize = layouts.iter().map(|l| l.pages.len()).sum();
                // GridCraft stops there; better no PDF than one quietly cut short.
                if pages >= print::MAX_PAGES {
                    return Err(io::Error::other("this spreadsheet is too long to print"));
                }
                let path = input.to_string_lossy();
                return Ok(print::render_pdf(&book, &layouts, &name, &path).0);
            }
            gridcraft_engine::io::save_bytes(&book, &output).map_err(failed)
        }
        Family::Deck => {
            use deckcraft_engine::cmd::file;
            let bytes = read(input)?;
            let deck = file::open_presentation(&name, &bytes).map_err(failed)?;
            file::save_bytes(&deck, &target).map_err(failed)
        }
    }
}

/// What PdfCraft does to a PDF from the Tools wheel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PdfTool {
    Compress,
    Rotate,
    Split,
}

impl PdfTool {
    pub fn from_label(label: &str) -> Option<PdfTool> {
        match label {
            "Compress" => Some(PdfTool::Compress),
            "Rotate" => Some(PdfTool::Rotate),
            "Split" => Some(PdfTool::Split),
            _ => None,
        }
    }

    /// Added to the name of what it makes.
    pub fn suffix(self) -> &'static str {
        match self {
            PdfTool::Compress => "compressed",
            PdfTool::Rotate => "rotated",
            PdfTool::Split => "pages",
        }
    }
}

/// The PDF smaller: images downsampled and recompressed, unused and repeated objects dropped.
/// Fails when that saves nothing, rather than leaving a copy that is no smaller.
pub fn compress(input: &Path) -> io::Result<Vec<u8>> {
    let bytes = read(input)?;
    let before = bytes.len();
    let mut doc = open_pdf_bytes(bytes)?;
    pdfcraft_optimize::optimize(&mut doc, &Default::default()).map_err(failed)?;
    let all: Vec<_> = doc
        .object_numbers()
        .into_iter()
        .map(|n| pdfcraft_cos::ObjRef::new(n, doc.generation(n)))
        .collect();
    pdfcraft_organize::dedupe_resources(&mut doc, &all, false);
    let bytes = save_pdf(&doc)?;
    if bytes.len() >= before {
        return Err(io::Error::other("this PDF is already as small as it gets"));
    }
    Ok(bytes)
}

/// Every page turned a quarter clockwise.
pub fn rotate(input: &Path) -> io::Result<Vec<u8>> {
    let mut doc = open_pdf(input)?;
    let pages: Vec<usize> = (0..page_count(&doc)?).collect();
    pdfcraft_organize::rotate_pages(&mut doc, &pages, 90).map_err(failed)?;
    save_pdf(&doc)
}

/// One PDF per page, handed to `part` with its 0-based page number and the page count as soon
/// as it is made, so only one is held at a time. A protected PDF is refused: its parts would
/// come out unprotected.
pub fn split(
    input: &Path,
    mut part: impl FnMut(usize, usize, Vec<u8>) -> io::Result<()>,
) -> io::Result<()> {
    let doc = open_pdf(input)?;
    refuse_protected(&doc)?;
    let pages = page_count(&doc)?;
    for page in 0..pages {
        let one = pdfcraft_organize::extract_pages(&doc, &[page]).map_err(failed)?;
        part(page, pages, save_pdf(&one)?)?;
    }
    Ok(())
}

/// The pages of `inputs`, in order, in one PDF, with a bookmark for each file. They are read one
/// at a time, but the result is held whole, so together they may be no bigger than one input.
/// `progress` counts the files; a cancel stops the merge between two of them.
pub fn merge(inputs: &[PathBuf], progress: &Progress) -> io::Result<Vec<u8>> {
    let mut total = 0;
    let mut combiner = pdfcraft_organize::Combiner::new();
    // Each file, then writing the whole.
    progress.begin(inputs.len() as u64 + 1);
    for input in inputs {
        progress.check()?;
        // With several files, say which one failed.
        let named = |e: io::Error| {
            let name = crate::catalog::display_name(input);
            io::Error::new(e.kind(), format!("{name}: {e}"))
        };
        let bytes = read(input).map_err(named)?;
        total += bytes.len() as u64;
        if total > MAX_INPUT {
            return Err(io::Error::other("these files are too big to merge"));
        }
        let doc = open_pdf_bytes(bytes).map_err(named)?;
        refuse_protected(&doc).map_err(named)?;
        let title = crate::catalog::stem_of(input);
        combiner.add(&title, &doc, None).map_err(failed)?;
        progress.add(1);
    }
    progress.check()?;
    let bytes = save_pdf(&combiner.finish().map_err(failed)?)?;
    progress.add(1);
    Ok(bytes)
}

fn check_size(input: &Path) -> io::Result<()> {
    if std::fs::metadata(input)?.len() > MAX_INPUT {
        return Err(io::Error::other("this file is too big to convert"));
    }
    Ok(())
}

/// The whole file, as long as it is no bigger than `MAX_INPUT`, even if it grows while read.
fn read(input: &Path) -> io::Result<Vec<u8>> {
    use std::io::Read;
    check_size(input)?;
    let mut bytes = Vec::new();
    std::fs::File::open(input)?
        .take(MAX_INPUT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_INPUT {
        return Err(io::Error::other("this file is too big to convert"));
    }
    Ok(bytes)
}

fn open_pdf(input: &Path) -> io::Result<Document> {
    open_pdf_bytes(read(input)?)
}

/// A PDF to change. A signed one is refused: whatever is made from it would carry a broken
/// signature. One whose page tree is lost gets it rebuilt from the pages found in the file.
fn open_pdf_bytes(bytes: Vec<u8>) -> io::Result<Document> {
    let mut doc = Document::open(Arc::new(bytes)).map_err(failed)?;
    if is_signed(&doc) {
        return Err(io::Error::other(
            "this PDF is signed: changing it would break the signature",
        ));
    }
    pdfcraft_organize::recover_page_tree(&mut doc).map_err(failed)?;
    if page_count(&doc)? == 0 {
        return Err(io::Error::other("this PDF has no pages"));
    }
    Ok(doc)
}

/// Whether anything in the file is signed: a signature field with a value, or the catalog's
/// `/Perms` (certification and usage rights are signatures too). An empty "sign here" field
/// isn't.
fn is_signed(doc: &Document) -> bool {
    let Some(catalog) = doc.root().and_then(|root| doc.get(root).as_dict().cloned()) else {
        return false;
    };
    if catalog.get(b"Perms").is_some() {
        return true;
    }
    let fields = catalog
        .get(b"AcroForm")
        .and_then(|form| doc.dict(form))
        .and_then(|form| form.get(b"Fields").map(|f| doc.resolve(f)));
    let mut todo: Vec<pdfcraft_cos::Object> = match fields.as_deref() {
        Some(pdfcraft_cos::Object::Array(items)) => items.clone(),
        _ => return false,
    };
    // A malformed file could make the tree loop; this many fields is plenty.
    let mut budget = 100_000;
    while let Some(item) = todo.pop() {
        budget -= 1;
        if budget == 0 {
            break;
        }
        let Some(field) = doc.dict(&item) else {
            continue;
        };
        let value = field.get(b"V").map(|v| doc.resolve(v));
        if field.name(b"FT") == Some(b"Sig") && value.is_some_and(|v| v.as_dict().is_some()) {
            return true;
        }
        if let Some(pdfcraft_cos::Object::Array(kids)) =
            field.get(b"Kids").map(|k| doc.resolve(k)).as_deref()
        {
            todo.extend(kids.iter().cloned());
        }
    }
    false
}

/// Split and Merge make new documents, which would come out without the PDF's password or its
/// restrictions on printing and copying; such a PDF is left alone.
fn refuse_protected(doc: &Document) -> io::Result<()> {
    if doc.security().is_some() {
        return Err(io::Error::other(
            "this PDF is protected: what is made from it would not be",
        ));
    }
    Ok(())
}

fn page_count(doc: &Document) -> io::Result<usize> {
    pdfcraft_organize::page_count(doc).map_err(failed)
}

fn save_pdf(doc: &Document) -> io::Result<Vec<u8>> {
    pdfcraft_cos::write_full(doc, &Default::default()).map_err(failed)
}

fn failed(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
