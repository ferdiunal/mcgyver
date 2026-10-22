//! Office documents and PDFs through the real job path, with files built by hand: nothing has to
//! be installed.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mcgyver::archive::Progress;
use mcgyver::catalog::{self, Choice, Kind, Mode};
use mcgyver::convert::{self, Task};
use pdf_writer::types::{FieldType, SigFlags};
use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref, Str, TextStr};

fn choice(mode: Mode, label: &'static str) -> Choice {
    Choice {
        mode,
        label,
        hint: "",
    }
}

/// A Word document with a bold title and a line of Turkish.
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
<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Proje Kapsamı</w:t></w:r></w:p>
<w:p><w:r><w:t>Dönüştürücü testi: ğüşıöç.</w:t></w:r></w:p>
</w:body></w:document>"#,
    );
    zip.finish().unwrap();
}

/// A PDF with one line of text on each of `pages` pages.
fn write_pdf(path: &Path, pages: usize) {
    write_pdf_with(path, pages, None);
}

/// [`write_pdf`], with a signature field, signed or waiting to be, when `signature` says so.
fn write_pdf_with(path: &Path, pages: usize, signature: Option<bool>) {
    let mut pdf = Pdf::new();
    let (catalog_id, pages_id, font_id, field_id) =
        (Ref::new(1), Ref::new(2), Ref::new(3), Ref::new(4));
    let page_ids: Vec<Ref> = (0..pages).map(|i| Ref::new(10 + 2 * i as i32)).collect();
    let mut catalog = pdf.catalog(catalog_id);
    catalog.pages(pages_id);
    if signature.is_some() {
        catalog
            .form()
            .fields([field_id])
            .sig_flags(SigFlags::SIGNATURES_EXIST | SigFlags::APPEND_ONLY);
    }
    catalog.finish();
    if let Some(signed) = signature {
        let mut field = pdf.form_field(field_id);
        field
            .field_type(FieldType::Signature)
            .partial_name(TextStr("Sign here"));
        if signed {
            field
                .insert(Name(b"V"))
                .dict()
                .pair(Name(b"Type"), Name(b"Sig"));
        }
        field.finish();
    }
    pdf.pages(pages_id)
        .kids(page_ids.iter().copied())
        .count(pages as i32);
    pdf.type1_font(font_id).base_font(Name(b"Helvetica"));
    for (i, id) in page_ids.iter().enumerate() {
        let content_id = Ref::new(id.get() + 1);
        let mut page = pdf.page(*id);
        page.media_box(Rect::new(0.0, 0.0, 595.0, 842.0))
            .parent(pages_id)
            .contents(content_id);
        page.resources().fonts().pair(Name(b"F1"), font_id);
        page.finish();
        let mut content = Content::new();
        let line = format!("Page {}", i + 1);
        content
            .begin_text()
            .set_font(Name(b"F1"), 14.0)
            .next_line(72.0, 720.0)
            .show(Str(line.as_bytes()))
            .end_text();
        pdf.stream(content_id, &content.finish());
    }
    fs::write(path, pdf.finish()).unwrap();
}

fn pdf_pages(path: &Path) -> (usize, Vec<i64>) {
    let doc = pdfcraft_cos::Document::open(Arc::new(fs::read(path).unwrap())).unwrap();
    let n = pdfcraft_organize::page_count(&doc).unwrap();
    let rotations = (0..n)
        .map(|i| pdfcraft_organize::page_rotation(&doc, i).unwrap())
        .collect();
    (n, rotations)
}

fn run(file: &Path, choice: Choice) -> PathBuf {
    let task = convert::task(file, choice).expect("a real conversion");
    convert::run(file, task, &Progress::default()).unwrap()
}

fn labels(name: &str, mode: Mode) -> Vec<&'static str> {
    catalog::choices(&[PathBuf::from(name)], mode)
        .iter()
        .map(|c| c.label)
        .collect()
}

#[test]
fn office_files_offer_their_engines_targets() {
    assert_eq!(
        labels("report.docx", Mode::Convert),
        ["PDF", "ODT", "RTF", "HTML", "TXT", "MD"]
    );
    assert_eq!(
        labels("budget.xlsx", Mode::Convert),
        ["PDF", "CSV", "TSV", "HTML", "MD"]
    );
    assert_eq!(labels("deck.pptx", Mode::Convert), ["PDF", "TXT", "MD"]);
    // Markdown and plain text lay out into a document too.
    assert!(labels("notes.md", Mode::Convert).contains(&"PDF"));
    for tool in ["Compress", "Split", "Rotate"] {
        assert!(labels("scan.pdf", Mode::Tools).contains(&tool), "{tool}");
    }
}

#[test]
fn a_word_document_saves_as_every_target() {
    let dir = tempfile::tempdir().unwrap();
    let docx = dir.path().join("Kapsam.docx");
    write_docx(&docx);
    for label in ["PDF", "ODT", "RTF", "HTML", "TXT"] {
        let made = run(&docx, choice(Mode::Convert, label));
        let ext = label.to_lowercase();
        assert_eq!(made, dir.path().join(format!("Kapsam.{ext}")));
        let bytes = fs::read(&made).unwrap();
        match label {
            "PDF" => assert_eq!(pdf_pages(&made).0, 1),
            "ODT" => assert!(bytes.starts_with(b"PK"), "ODT is a zip"),
            "RTF" => assert!(bytes.starts_with(b"{\\rtf"), "RTF header"),
            _ => {
                let text = String::from_utf8_lossy(&bytes);
                assert!(text.contains("Proje Kapsam"), "{label}: {text}");
            }
        }
    }
}

#[test]
fn a_spreadsheet_round_trips_and_prints() {
    let dir = tempfile::tempdir().unwrap();
    let csv = dir.path().join("rows.csv");
    fs::write(&csv, "name,qty\napple,3\npear,5\n").unwrap();
    let xlsx = run(&csv, choice(Mode::Convert, "XLSX"));
    let back = run(&xlsx, choice(Mode::Convert, "CSV"));
    assert_eq!(back, dir.path().join("rows 2.csv"));
    let text = fs::read_to_string(&back).unwrap();
    assert!(text.contains("pear,5"), "{text}");
    let pdf = run(&xlsx, choice(Mode::Convert, "PDF"));
    assert!(pdf_pages(&pdf).0 >= 1);
}

#[test]
fn a_workbook_prints_every_sheet() {
    use gridcraft_engine::core::{CellRef, Value};
    use gridcraft_engine::model::{Sheet, Workbook};
    let dir = tempfile::tempdir().unwrap();
    let mut book = Workbook::new();
    let mut second = Sheet::new("Totals");
    second.set_value(CellRef { row: 0, col: 0 }, Value::Text("sum".into()));
    book.sheets.push(Arc::new(second));
    let first = Arc::make_mut(&mut book.sheets[0]);
    first.set_value(CellRef { row: 0, col: 0 }, Value::Number(3.0));
    let xlsx = dir.path().join("book.xlsx");
    fs::write(
        &xlsx,
        gridcraft_engine::io::save_bytes(&book, "book.xlsx").unwrap(),
    )
    .unwrap();
    let pdf = run(&xlsx, choice(Mode::Convert, "PDF"));
    assert_eq!(pdf_pages(&pdf).0, 2);
}

#[test]
fn a_presentation_becomes_a_pdf() {
    let dir = tempfile::tempdir().unwrap();
    let pptx = dir.path().join("deck.pptx");
    let deck = deckcraft_engine::model::Presentation::default();
    fs::write(
        &pptx,
        deckcraft_engine::cmd::file::save_bytes(&deck, "pptx").unwrap(),
    )
    .unwrap();
    let pdf = run(&pptx, choice(Mode::Convert, "PDF"));
    assert!(fs::read(&pdf).unwrap().starts_with(b"%PDF"));
}

#[test]
fn pdf_tools_rotate_split_compress_and_merge() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.pdf");
    let b = dir.path().join("b.pdf");
    write_pdf(&a, 3);
    write_pdf(&b, 2);

    let rotated = run(&a, choice(Mode::Tools, "Rotate"));
    assert_eq!(rotated, dir.path().join("a (rotated).pdf"));
    assert_eq!(pdf_pages(&rotated), (3, vec![90, 90, 90]));

    let pages = run(&a, choice(Mode::Tools, "Split"));
    assert_eq!(pages, dir.path().join("a (pages)"));
    for n in 1..=3 {
        assert_eq!(pdf_pages(&pages.join(format!("a {n}.pdf"))).0, 1);
    }

    let compressed = run(&a, choice(Mode::Tools, "Compress"));
    assert_eq!(pdf_pages(&compressed).0, 3);

    let merged = convert::merge(&[a.clone(), b.clone()], Kind::Pdf, &Progress::default()).unwrap();
    assert_eq!(merged, dir.path().join("a (merged).pdf"));
    assert_eq!(pdf_pages(&merged).0, 5);
    // The originals stay as they were.
    assert_eq!(pdf_pages(&a), (3, vec![0, 0, 0]));
}

#[test]
fn merging_counts_files_and_a_cancel_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.pdf"), dir.path().join("b.pdf"));
    write_pdf(&a, 1);
    write_pdf(&b, 2);
    let progress = Progress::default();
    let merged = convert::merge(&[a.clone(), b.clone()], Kind::Pdf, &progress).unwrap();
    assert_eq!(progress.fraction(), 1.0);
    fs::remove_file(merged).unwrap();

    let cancelled = Progress::default();
    cancelled.cancel();
    let error = convert::merge(&[a, b], Kind::Pdf, &cancelled).unwrap_err();
    assert_eq!(error.to_string(), "cancelled");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
}

#[test]
fn signed_pdfs_are_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    // A field waiting for a signature is fine to change.
    let unsigned = dir.path().join("form.pdf");
    write_pdf_with(&unsigned, 2, Some(false));
    let rotated = run(&unsigned, choice(Mode::Tools, "Rotate"));
    fs::remove_file(rotated).unwrap();
    fs::remove_file(&unsigned).unwrap();

    let signed = dir.path().join("contract.pdf");
    write_pdf_with(&signed, 2, Some(true));
    for tool in ["Rotate", "Split", "Compress"] {
        let task = convert::task(&signed, choice(Mode::Tools, tool)).unwrap();
        let error = convert::run(&signed, task, &Progress::default()).unwrap_err();
        assert!(error.to_string().contains("signed"), "{tool}: {error}");
    }
    let other = dir.path().join("other.pdf");
    write_pdf(&other, 1);
    assert!(convert::merge(&[other, signed], Kind::Pdf, &Progress::default()).is_err());
    // Only the two originals are there.
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
}

#[test]
fn damaged_documents_fail_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["broken.docx", "broken.pptx", "broken.pdf"] {
        let file = dir.path().join(name);
        fs::write(&file, b"PK\x03\x04 this is not a document").unwrap();
        let label = if name.ends_with(".pdf") {
            "Rotate"
        } else {
            "PDF"
        };
        let mode = if name.ends_with(".pdf") {
            Mode::Tools
        } else {
            Mode::Convert
        };
        let task = convert::task(&file, choice(mode, label)).unwrap();
        assert!(matches!(task, Task::Office(_) | Task::Pdf(_)));
        assert!(
            convert::run(&file, task, &Progress::default()).is_err(),
            "{name}"
        );
    }
    // Nothing half-written was left behind.
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
}

#[test]
fn merging_pdfs_is_one_job_for_the_selection() {
    let dir = tempfile::tempdir().unwrap();
    let files: Vec<PathBuf> = ["one.pdf", "two.pdf", "three.pdf"]
        .iter()
        .map(|name| dir.path().join(name))
        .collect();
    for file in &files {
        write_pdf(file, 1);
    }
    let merge = catalog::choices(&files, Mode::Tools)
        .into_iter()
        .find(|c| c.label == catalog::MERGE)
        .expect("Merge is offered for several PDFs");
    let jobs = mcgyver::jobs::start(&files, merge, 1);
    assert_eq!(jobs.len(), 1);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while jobs[0].finished_at().is_none() {
        assert!(
            std::time::Instant::now() < deadline,
            "the merge didn't finish"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(jobs[0].error(), None);
    let merged = jobs[0].successful_output().unwrap();
    assert_eq!(merged, dir.path().join("one (merged).pdf"));
    assert_eq!(pdf_pages(&merged).0, 3);
}

#[test]
fn a_merge_skips_files_the_wheel_ignores() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.pdf");
    let b = dir.path().join("b.pdf");
    write_pdf(&a, 2);
    write_pdf(&b, 1);
    let stray = dir.path().join("notes.mcgyver-unknown");
    fs::write(&stray, b"not a document").unwrap();
    let files = vec![a, stray, b];
    let merge = catalog::choices(&files, Mode::Tools)
        .into_iter()
        .find(|c| c.label == catalog::MERGE)
        .expect("Merge is offered for the two PDFs");
    let jobs = mcgyver::jobs::start(&files, merge, 1);
    assert_eq!(jobs.len(), 1);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while jobs[0].finished_at().is_none() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(jobs[0].error(), None);
    assert_eq!(pdf_pages(&jobs[0].successful_output().unwrap()).0, 3);
}

#[test]
fn a_pdf_saves_as_documents_text_an_ebook_and_page_images() {
    let dir = tempfile::tempdir().unwrap();
    let pdf = dir.path().join("report.pdf");
    write_pdf(&pdf, 2);

    let docx = run(&pdf, choice(Mode::Convert, "DOCX"));
    assert_eq!(docx, dir.path().join("report.docx"));
    let doc = wordcraft_engine::io::open_bytes("report.docx", &fs::read(&docx).unwrap()).unwrap();
    assert!(wordcraft_engine::io::save_bytes("x.txt", &doc).is_ok());

    let pptx = run(&pdf, choice(Mode::Convert, "PPTX"));
    let deck = deckcraft_engine::cmd::file::open_presentation("r.pptx", &fs::read(&pptx).unwrap());
    assert!(deck.is_ok());

    let txt = run(&pdf, choice(Mode::Convert, "TXT"));
    assert_eq!(fs::read_to_string(&txt).unwrap(), "Page 1\n\nPage 2\n");

    let epub = run(&pdf, choice(Mode::Convert, "EPUB"));
    let mut book = zip::ZipArchive::new(fs::File::open(&epub).unwrap()).unwrap();
    let mut text = String::new();
    std::io::Read::read_to_string(&mut book.by_name("OEBPS/text.xhtml").unwrap(), &mut text)
        .unwrap();
    assert!(text.contains("Page 1") && text.contains("Page 2"));

    // No tables in it: said so, rather than an empty workbook.
    let task = convert::task(&pdf, choice(Mode::Convert, "XLSX")).unwrap();
    let error = convert::run(&pdf, task, &Progress::default()).unwrap_err();
    assert_eq!(error.to_string(), "there are no tables in this PDF");

    // Two pages are a folder of pictures, at 150 dpi; one page a picture of its own.
    let pages = run(&pdf, choice(Mode::Convert, "PNG"));
    assert_eq!(pages, dir.path().join("report (pages)"));
    let first = image::open(pages.join("report 1.png")).unwrap();
    assert_eq!(first.width(), 1240);
    assert!((1754..=1755).contains(&first.height()));
    assert!(pages.join("report 2.png").is_file());
    // White paper, and the line of text drawn dark on it.
    let first = first.to_rgb8();
    assert_eq!(first.get_pixel(5, 5).0, [255, 255, 255]);
    assert!(first.pixels().any(|p| p.0[0] < 100));

    let single = dir.path().join("cover.pdf");
    write_pdf(&single, 1);
    let jpg = run(&single, choice(Mode::Convert, "JPG"));
    assert_eq!(jpg, dir.path().join("cover.jpg"));
    assert!(image::open(&jpg).is_ok());
}

#[test]
fn a_pdfs_table_becomes_a_sheet() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("prices.pdf");
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
    pdf.type1_font(font_id).base_font(Name(b"Helvetica"));
    let mut page = pdf.page(page_id);
    page.media_box(Rect::new(0.0, 0.0, 595.0, 842.0))
        .parent(pages_id)
        .contents(content_id);
    page.resources().fonts().pair(Name(b"F1"), font_id);
    page.finish();
    let rows = [
        ["Item", "Count", "Price"],
        ["Apples", "12", "3.5"],
        ["Pears", "7", "4.25"],
        ["Plums", "30", "1.75"],
        ["Figs", "5", "9.5"],
    ];
    let mut content = Content::new();
    for (r, row) in rows.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            content
                .begin_text()
                .set_font(Name(b"F1"), 11.0)
                .next_line(72.0 + 150.0 * c as f32, 720.0 - 18.0 * r as f32)
                .show(Str(cell.as_bytes()))
                .end_text();
        }
    }
    pdf.stream(content_id, &content.finish());
    fs::write(&path, pdf.finish()).unwrap();

    let xlsx = run(&path, choice(Mode::Convert, "XLSX"));
    let (book, _) =
        gridcraft_engine::io::open_bytes("prices.xlsx", &fs::read(&xlsx).unwrap()).unwrap();
    let sheet = &book.sheets[0];
    let text = |row, col| {
        sheet
            .value(gridcraft_engine::core::CellRef::new(row, col))
            .display()
    };
    assert_eq!(text(0, 0), "Item");
    assert_eq!(text(2, 1), "7");
    assert_eq!(text(4, 2), "9.5");
}

#[test]
fn documents_merge_each_from_a_new_page_with_their_lists() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.docx");
    write_docx(&a);
    let list = wordcraft_engine::io::open_bytes("b.md", b"# Second\n\n- one\n- two\n").unwrap();
    let b = dir.path().join("b.docx");
    fs::write(
        &b,
        wordcraft_engine::io::save_bytes("b.docx", &list).unwrap(),
    )
    .unwrap();

    let merged = convert::merge(&[a.clone(), b], Kind::Document, &Progress::default()).unwrap();
    assert_eq!(merged, dir.path().join("a (merged).docx"));
    let doc = wordcraft_engine::io::open_bytes("m.docx", &fs::read(&merged).unwrap()).unwrap();
    let paragraphs: Vec<&wordcraft_engine::doc::Paragraph> =
        doc.body.iter().filter_map(|b| b.as_para()).collect();
    let texts: Vec<&str> = paragraphs.iter().map(|p| p.text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "Proje Kapsamı",
            "Dönüştürücü testi: ğüşıöç.",
            "Second",
            "one",
            "two"
        ]
    );
    // The first's last section is closed where it ends; the second starts on a new page.
    assert!(paragraphs[1].section.is_some());
    assert_eq!(
        paragraphs[1].section.as_ref().unwrap().start,
        wordcraft_engine::doc::section::SectionStart::NextPage
    );
    let list = paragraphs[3]
        .props
        .numbering
        .as_ref()
        .expect("still a list");
    assert!(doc.numbering.nums.iter().any(|n| n.id == list.num));

    // The wheel offers Merge for documents WordCraft opens, not for others.
    let labels = |names: &[&str]| -> Vec<&'static str> {
        let files: Vec<PathBuf> = names.iter().map(PathBuf::from).collect();
        catalog::choices(&files, Mode::Tools)
            .iter()
            .map(|c| c.label)
            .collect()
    };
    assert!(labels(&["a.docx", "b.md"]).contains(&"Merge"));
    assert!(!labels(&["a.docx", "b.pages"]).contains(&"Merge"));
    // Nor for kinds that don't merge with each other.
    assert!(!labels(&["a.docx", "b.xlsx"]).contains(&"Merge"));
    assert!(!labels(&["a.pdf", "b.docx"]).contains(&"Merge"));
}

#[test]
fn workbooks_merge_their_sheets_and_formulas_follow_a_renamed_one() {
    use gridcraft_engine::core::{CellRef, DateSystem, Value};
    use gridcraft_engine::model::{Cell, DefinedName, Formula, NumFmt, Sheet, Style, Workbook};
    let dir = tempfile::tempdir().unwrap();
    let book = |value: f64, dates: DateSystem| {
        let mut book = Workbook::new();
        book.date_system = dates;
        let mut sheet = Sheet::new("Data");
        sheet.set_value(CellRef::new(0, 0), Value::number(value));
        sheet.cells.set(
            CellRef::new(0, 1),
            Cell {
                formula: Some(Arc::new(Formula::new("=Data!A1*Rate"))),
                ..Default::default()
            },
        );
        // 1 January 2024 as either kind of workbook counts it.
        let date = book.styles.intern(Style {
            num_fmt: NumFmt("yyyy-mm-dd".into()),
            ..Default::default()
        });
        let day = if dates == DateSystem::D1904 {
            43_829.0
        } else {
            45_291.0
        };
        sheet.cells.set(
            CellRef::new(1, 0),
            Cell {
                value: Value::number(day),
                style: date,
                ..Default::default()
            },
        );
        book.sheets = vec![Arc::new(sheet)];
        book.names.push(DefinedName {
            name: "Rate".into(),
            scope: None,
            formula: "Data!$A$1".into(),
            comment: String::new(),
            hidden: false,
        });
        gridcraft_engine::io::save_bytes(&book, "book.xlsx").unwrap()
    };
    let a = dir.path().join("a.xlsx");
    let b = dir.path().join("b.xlsx");
    fs::write(&a, book(2.0, DateSystem::D1900)).unwrap();
    fs::write(&b, book(5.0, DateSystem::D1904)).unwrap();
    let csv = dir.path().join("c.csv");
    fs::write(&csv, "x,y\n1,2\n").unwrap();

    let merged = convert::merge(&[a, b, csv], Kind::Spreadsheet, &Progress::default()).unwrap();
    assert_eq!(merged, dir.path().join("a (merged).xlsx"));
    let (book, _) =
        gridcraft_engine::io::open_bytes("m.xlsx", &fs::read(&merged).unwrap()).unwrap();
    let names: Vec<&str> = book.sheets.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names.len(), 3);
    assert_eq!(&names[..2], ["Data", "Data (2)"]);
    let formula = book.sheets[1]
        .cell(CellRef::new(0, 1))
        .and_then(|c| c.formula.as_ref())
        .map(|f| f.text.clone())
        .unwrap();
    assert!(formula.contains("'Data (2)'!A1"), "{formula}");
    assert_eq!(book.sheets[1].value(CellRef::new(0, 0)).display(), "5");
    // The second book's date is moved to the first's count of days.
    assert_eq!(
        book.sheets[1].value(CellRef::new(1, 0)),
        Value::number(45_291.0)
    );
    // Both have a workbook-wide Rate: the second's stays with its sheet, pointing at it.
    let rate = book
        .names
        .iter()
        .find(|n| n.name == "Rate" && n.scope == Some(1))
        .expect("the second book's Rate");
    assert!(rate.formula.contains("'Data (2)'"), "{}", rate.formula);
    assert!(
        book.names
            .iter()
            .any(|n| n.name == "Rate" && n.scope.is_none())
    );
}
