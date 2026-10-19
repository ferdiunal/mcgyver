//! Documents, spreadsheets, presentations and PDFs to Markdown with anydoc: pure Rust, nothing
//! leaves the machine. Scanned PDFs would need OCR, which anydoc leaves to a cloud service that
//! isn't used here.

use std::io;
use std::path::Path;

use anydoc::{ConvertError, Format};

/// anydoc reads the whole file; larger documents than this are refused up front.
const MAX_INPUT: u64 = 256 << 20;

/// Whether anydoc reads files with this extension.
pub fn reads(ext: &str) -> bool {
    Format::from_extension(ext).is_some()
}

/// Writes `input` as Markdown to `output`.
pub fn convert(input: &Path, output: &Path) -> io::Result<()> {
    if std::fs::metadata(input)?.len() > MAX_INPUT {
        return Err(io::Error::other("this document is too big to convert"));
    }
    let markdown = anydoc::to_markdown(input).map_err(|e| match e {
        ConvertError::Io(e) => e,
        ConvertError::NeedsOcr { .. } => {
            io::Error::other("this PDF is scanned: it has no text to take out")
        }
        ConvertError::Encrypted => io::Error::other("the document is password-protected"),
        e => io::Error::other(e.to_string()),
    })?;
    std::fs::write(output, markdown)
}
