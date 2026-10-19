//! Documents to Markdown through `convert::run`: built by hand, so the test needs nothing
//! installed.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use image::{Rgb, RgbImage};
use mcgyver::archive::Progress;
use mcgyver::catalog::{self, Choice, Mode};
use mcgyver::convert::{self, Task};
use mcgyver::images::{self, Target};
use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref, Str};

const MD: Choice = Choice {
    mode: Mode::Convert,
    label: "MD",
    hint: "",
};

/// The smallest Word document: a bold run and a plain one in one paragraph, then another.
fn write_docx(path: &Path) {
    let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    let mut part = |name: &str, body: &str| {
        zip.start_file(name, options).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    };
    part(
        "[Content_Types].xml",
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#,
    );
    part(
        "_rels/.rels",
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#,
    );
    part(
        "word/document.xml",
        r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Kapsam</w:t></w:r><w:r><w:t xml:space="preserve"> ve hedefler</w:t></w:r></w:p>
<w:p><w:r><w:t>İkinci paragraf.</w:t></w:r></w:p>
</w:body></w:document>"#,
    );
    zip.finish().unwrap();
}

/// One page with a line of text in Helvetica.
fn write_text_pdf(path: &Path) {
    let mut pdf = Pdf::new();
    let (catalog_id, pages_id, page_id, font_id, content_id) = (
        Ref::new(1),
        Ref::new(2),
        Ref::new(3),
        Ref::new(4),
        Ref::new(5),
    );
    pdf.catalog(catalog_id).pages(pages_id);
    pdf.pages(pages_id).kids([page_id]).count(1);
    let mut page = pdf.page(page_id);
    page.media_box(Rect::new(0.0, 0.0, 595.0, 842.0))
        .parent(pages_id)
        .contents(content_id);
    page.resources().fonts().pair(Name(b"F1"), font_id);
    page.finish();
    pdf.type1_font(font_id).base_font(Name(b"Helvetica"));
    let mut content = Content::new();
    content
        .begin_text()
        .set_font(Name(b"F1"), 14.0)
        .next_line(72.0, 720.0)
        .show(Str(b"Quarterly report"))
        .end_text();
    pdf.stream(content_id, &content.finish());
    fs::write(path, pdf.finish()).unwrap();
}

fn to_markdown(file: &Path) -> std::io::Result<PathBuf> {
    let task = convert::task(file, MD).expect("MD is a real conversion");
    assert!(matches!(task, Task::Markdown));
    convert::run(file, task, &Progress::default())
}

#[test]
fn documents_offer_markdown() {
    let labels = |name: &str| -> Vec<&str> {
        catalog::choices(&[PathBuf::from(name)], Mode::Convert)
            .iter()
            .map(|c| c.label)
            .collect()
    };
    for name in [
        "Kapsam 3.docx",
        "budget.xlsx",
        "deck.pptx",
        "notes.odt",
        "rows.csv",
        "scan.pdf",
    ] {
        assert!(labels(name).contains(&"MD"), "{name}");
    }
    // Already Markdown, or plain text anydoc doesn't read.
    assert!(!labels("README.md").contains(&"MD"));
    assert!(!labels("notes.txt").contains(&"MD"));
}

#[test]
fn a_word_document_becomes_markdown() {
    let dir = tempfile::tempdir().unwrap();
    let docx = dir.path().join("Kapsam 3.docx");
    write_docx(&docx);
    let made = to_markdown(&docx).unwrap();
    assert_eq!(made, dir.path().join("Kapsam 3.md"));
    let text = fs::read_to_string(&made).unwrap();
    assert!(text.contains("**Kapsam**"), "{text}");
    assert!(text.contains("ve hedefler"), "{text}");
    assert!(text.contains("İkinci paragraf."), "{text}");
    // The original stays, and nothing else is left behind.
    assert!(docx.exists());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
}

#[test]
fn a_spreadsheet_becomes_a_table() {
    let dir = tempfile::tempdir().unwrap();
    let csv = dir.path().join("rows.csv");
    fs::write(&csv, "name,qty\napple,3\npear,5\n").unwrap();
    let text = fs::read_to_string(to_markdown(&csv).unwrap()).unwrap();
    assert!(text.contains("| name | qty |"), "{text}");
    assert!(text.contains("| pear | 5 |"), "{text}");
}

#[test]
fn a_pdf_gives_its_text() {
    let dir = tempfile::tempdir().unwrap();
    let pdf = dir.path().join("report.pdf");
    write_text_pdf(&pdf);
    let text = fs::read_to_string(to_markdown(&pdf).unwrap()).unwrap();
    assert!(text.contains("Quarterly report"), "{text}");
}

#[test]
fn scanned_and_broken_files_fail_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    // A picture saved as PDF has no text, like a scan.
    let png = dir.path().join("scan.png");
    RgbImage::from_pixel(32, 32, Rgb([200, 30, 30]))
        .save(&png)
        .unwrap();
    let pdf = dir.path().join("scan.pdf");
    images::encode(&images::decode(&png).unwrap(), Target::Pdf, &pdf).unwrap();
    let error = to_markdown(&pdf).unwrap_err().to_string();
    assert!(error.contains("scanned"), "{error}");

    let broken = dir.path().join("broken.docx");
    fs::write(&broken, b"PK\x03\x04 not really a document").unwrap();
    assert!(to_markdown(&broken).is_err());
    // Neither left a half-written file.
    assert!(!dir.path().join("scan.md").exists());
    assert!(!dir.path().join("broken.md").exists());
}
