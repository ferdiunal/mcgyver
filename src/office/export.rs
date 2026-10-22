//! A PDF saved as something else: Word and PowerPoint (PdfCraft's export, from each page's
//! paragraphs and pictures), an EPUB e-book and a workbook of its tables (from the same export as
//! a web page), plain text, and page images (drawn by PdfCraft's renderer).

use std::io::{self, Cursor, Write};
use std::path::Path;
use std::sync::Arc;

use pdfcraft_cos::Document;

use super::{failed, read, refuse_protected};
use crate::archive::Progress;

/// What a PDF can be saved as here, as the wheel's labels (Markdown is `markdown`'s).
pub const TARGETS: [&str; 7] = ["DOCX", "PPTX", "XLSX", "EPUB", "TXT", "JPG", "PNG"];

/// Page images are drawn at this many dots an inch.
const DPI: f32 = 150.0;

/// A PDF to read from. What comes out of a protected one would be unprotected, so it is
/// refused; a signed one is fine, as it is only read.
fn open(bytes: Arc<Vec<u8>>) -> io::Result<Document> {
    let mut doc = Document::open(bytes).map_err(failed)?;
    refuse_protected(&doc)?;
    pdfcraft_organize::recover_page_tree(&mut doc).map_err(failed)?;
    if super::page_count(&doc)? == 0 {
        return Err(io::Error::other("this PDF has no pages"));
    }
    Ok(doc)
}

/// `input` saved as `label`: "DOCX", "PPTX", "XLSX", "EPUB" or "TXT".
pub fn convert(input: &Path, label: &str) -> io::Result<Vec<u8>> {
    let doc = open(Arc::new(read(input)?))?;
    let title = crate::catalog::stem_of(input);
    if label == "TXT" {
        return text(&doc);
    }
    // Tables need no pictures, and a scan's are large.
    let pages = pages(&doc, label != "XLSX");
    match label {
        "DOCX" => Ok(pdfcraft_export::docx(&pages, &title)),
        "PPTX" => Ok(pdfcraft_export::pptx(&pages, &title)),
        "EPUB" => epub(&pdfcraft_export::html(&pages, &title), &title),
        "XLSX" => workbook(&pdfcraft_export::html(&pages, &title)),
        _ => Err(io::Error::other("a PDF can't be saved as that")),
    }
}

/// The paragraphs of every page, a blank line between them.
fn text(doc: &Document) -> io::Result<Vec<u8>> {
    let mut paragraphs = Vec::new();
    for page in 0..super::page_count(doc)? {
        // The text a scan's recognition left invisible over it counts too; a page that can't
        // be read is left out, as the other exports leave it.
        for block in pdfcraft_edit::reading_blocks(doc, page).unwrap_or_default() {
            let text = block.text.trim();
            if !text.is_empty() {
                paragraphs.push(text.to_owned());
            }
        }
    }
    if paragraphs.is_empty() {
        return Err(io::Error::other(
            "this PDF is scanned: it has no text to take out",
        ));
    }
    Ok((paragraphs.join("\n\n") + "\n").into_bytes())
}

/// Each page as PdfCraft's export wants it: paragraphs and (with `pictures`) pictures placed on
/// the page as it is shown (after its crop, `/Rotate` and `/UserUnit`), in points with y up.
fn pages(doc: &Document, pictures: bool) -> Vec<pdfcraft_export::Page> {
    pdfcraft_model::pages(doc)
        .iter()
        .enumerate()
        .map(|(i, page)| {
            let crop = page.crop(doc);
            let rotation = page.rotation(doc);
            let unit = page.user_unit(doc);
            let (width, height) = page.display_size(doc);
            // A point in user space on the page as shown, y down.
            let shown = |x: f64, y: f64| {
                let u = (x - crop[0]) / (crop[2] - crop[0]).max(1e-3);
                let v = (crop[3] - y) / (crop[3] - crop[1]).max(1e-3);
                let (a, b) = match rotation {
                    90 => (1.0 - v, u),
                    180 => (1.0 - u, 1.0 - v),
                    270 => (v, 1.0 - u),
                    _ => (u, v),
                };
                (a * width, b * height)
            };
            let rect = |r: [f64; 4]| {
                let (ax, ay) = shown(r[0], r[1]);
                let (bx, by) = shown(r[2], r[3]);
                [
                    ax.min(bx),
                    height - ay.max(by),
                    ax.max(bx),
                    height - ay.min(by),
                ]
            };
            let blocks = pdfcraft_edit::reading_blocks(doc, i)
                .unwrap_or_default()
                .into_iter()
                .filter(|b| !b.text.trim().is_empty())
                .map(|b| {
                    let font = b.base_font.to_ascii_lowercase();
                    pdfcraft_export::Block {
                        rect: rect(b.rect),
                        size: b.size * unit,
                        bold: b.bold || ["bold", "black", "heavy"].iter().any(|w| font.contains(w)),
                        italic: b.italic || font.contains("italic") || font.contains("oblique"),
                        color: b.color,
                        rotation: b.rotation - rotation as f64,
                        text: b.text,
                    }
                })
                .collect();
            let found = if pictures {
                pdfcraft_edit::reading_images(doc, i).unwrap_or_default()
            } else {
                Vec::new()
            };
            let images = found
                .iter()
                .filter_map(|image| {
                    let (ext, bytes) = pdfcraft_create::image_file(doc, image.object?).ok()?;
                    let m = image.matrix;
                    Some(pdfcraft_export::Image {
                        ext: if ext == "jpg" { "jpg" } else { "png" },
                        bytes,
                        rect: rect(image.rect),
                        size: [m[0].hypot(m[1]) * unit, m[2].hypot(m[3]) * unit],
                        rotation: m[1].atan2(m[0]).to_degrees() - rotation as f64,
                    })
                })
                .collect();
            pdfcraft_export::Page {
                width,
                height,
                blocks,
                images,
            }
        })
        .collect()
}

/// Each page drawn at 150 dpi on white, as a JPEG or a PNG, handed to `picture` with its 0-based
/// number as soon as it is drawn, so only one is held at a time. `progress` counts the pages.
pub fn pictures(
    input: &Path,
    jpeg: bool,
    progress: &Progress,
    mut picture: impl FnMut(usize, Vec<u8>) -> io::Result<()>,
) -> io::Result<()> {
    use pdfcraft_render::{PageRenderer, RenderConfig, RenderRequest, RequestKind};
    let bytes = Arc::new(read(input)?);
    open(bytes.clone())?;
    let mut renderer = PageRenderer::new(bytes, RenderConfig::default());
    let pages = renderer.page_count();
    if pages == 0 {
        return Err(io::Error::other("this PDF has no pages"));
    }
    progress.begin(pages as u64);
    for page in 0..pages {
        progress.check()?;
        let drawn = renderer.render(RenderRequest {
            page,
            kind: RequestKind::Pixels,
            scale: DPI / 72.0,
            ..Default::default()
        });
        if let Some(error) = drawn.error {
            return Err(io::Error::other(error));
        }
        // Premultiplied, so a pixel on white is its colour plus what it lets through.
        let rgb: Vec<u8> = AsRef::<[u8]>::as_ref(&drawn.rgba)
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[r, g, b, a]| {
                let clear = 255 - a;
                [
                    r.saturating_add(clear),
                    g.saturating_add(clear),
                    b.saturating_add(clear),
                ]
            })
            .collect();
        let image = image::RgbImage::from_raw(drawn.width, drawn.height, rgb)
            .ok_or_else(|| io::Error::other("the page was drawn at the wrong size"))?;
        let mut out = Vec::new();
        if jpeg {
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
                .encode_image(&image)
                .map_err(failed)?;
        } else {
            image
                .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
                .map_err(failed)?;
        }
        picture(page, out)?;
        progress.add(1);
    }
    Ok(())
}

/// What PdfCraft's web page export holds between `<body>` and `</body>`.
fn body(html: &str) -> &str {
    let start = html.find("<body>").map_or(0, |i| i + "<body>".len());
    let end = html.rfind("</body>").unwrap_or(html.len());
    &html[start..end.max(start)]
}

/// The web page export as an EPUB 3 e-book: its text in one XHTML chapter, its pictures as
/// files beside it, and a contents page of its headings.
fn epub(html: &str, title: &str) -> io::Result<Vec<u8>> {
    let mut text = String::new();
    let mut pictures: Vec<(String, &'static str, Vec<u8>)> = Vec::new();
    let mut headings: Vec<(String, String)> = Vec::new();
    let mut rest = body(html);
    // The export writes HTML: XHTML closes every element, and keeps pictures as files.
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("<img alt=\"\" src=\"data:")
            && let Some(end) = after.find("\">")
        {
            let (mime, data) = after[..end].split_once(";base64,").unwrap_or_default();
            let (ext, mime) = if mime == "image/jpeg" {
                ("jpg", "image/jpeg")
            } else {
                ("png", "image/png")
            };
            let name = format!("images/picture{}.{ext}", pictures.len() + 1);
            text.push_str(&format!("<img alt=\"\" src=\"{name}\"/>"));
            pictures.push((name, mime, base64(data)));
            rest = &after[end + 2..];
        } else if let Some(after) = rest.strip_prefix("<hr>") {
            text.push_str("<hr/>");
            rest = after;
        } else if let Some(level) = ["<h1", "<h2"].iter().position(|h| rest.starts_with(h))
            && let Some(close) = rest.find(&format!("</h{}>", level + 1))
        {
            let id = format!("heading{}", headings.len() + 1);
            let open = rest.find('>').unwrap_or(close);
            headings.push((id.clone(), strip_tags(&rest[open + 1..close])));
            text.push_str(&format!("<h{} id=\"{id}\"", level + 1));
            text.push_str(&rest[3..close]);
            rest = &rest[close..];
        } else {
            let first = rest.chars().next().map_or(1, char::len_utf8);
            let next = rest[first..].find('<').map_or(rest.len(), |i| i + first);
            text.push_str(&rest[..next]);
            rest = &rest[next..];
        }
    }
    let title = escape(title);
    let chapter = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<!DOCTYPE html>\n<html \
         xmlns=\"http://www.w3.org/1999/xhtml\">\n<head>\n<meta charset=\"utf-8\"/>\n\
         <title>{title}</title>\n<style>img{{max-width:100%}}table{{border-collapse:collapse}}\
         td{{border:1px solid #999;padding:2px 6px;vertical-align:top}}</style>\n</head>\n\
         <body>\n{text}</body>\n</html>\n"
    );
    let contents: String = if headings.is_empty() {
        format!("<li><a href=\"text.xhtml\">{title}</a></li>\n")
    } else {
        headings
            .iter()
            .map(|(id, heading)| format!("<li><a href=\"text.xhtml#{id}\">{heading}</a></li>\n"))
            .collect()
    };
    let nav = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<!DOCTYPE html>\n<html \
         xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\">\n\
         <head>\n<meta charset=\"utf-8\"/>\n<title>{title}</title>\n</head>\n<body>\n\
         <nav epub:type=\"toc\" id=\"toc\">\n<ol>\n{contents}</ol>\n</nav>\n</body>\n</html>\n"
    );
    let now = jiff::Timestamp::now();
    let items: String = pictures
        .iter()
        .enumerate()
        .map(|(i, (name, mime, _))| {
            format!(
                "<item id=\"picture{}\" href=\"{name}\" media-type=\"{mime}\"/>\n",
                i + 1
            )
        })
        .collect();
    let package = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" \
         version=\"3.0\" unique-identifier=\"id\">\n<metadata \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n<dc:identifier \
         id=\"id\">urn:mcgyver:{}</dc:identifier>\n<dc:title>{title}</dc:title>\n\
         <dc:language>und</dc:language>\n<meta property=\"dcterms:modified\">{}</meta>\n\
         </metadata>\n<manifest>\n<item id=\"nav\" href=\"nav.xhtml\" \
         media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n<item id=\"text\" \
         href=\"text.xhtml\" media-type=\"application/xhtml+xml\"/>\n{items}</manifest>\n\
         <spine>\n<itemref idref=\"text\"/>\n</spine>\n</package>\n",
        now.as_nanosecond(),
        now.strftime("%Y-%m-%dT%H:%M:%SZ"),
    );
    let container = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<container version=\"1.0\" \
                     xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\n<rootfiles>\n\
                     <rootfile full-path=\"OEBPS/content.opf\" \
                     media-type=\"application/oebps-package+xml\"/>\n</rootfiles>\n</container>\n";

    use zip::write::SimpleFileOptions;
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    // The mimetype comes first and uncompressed, so a reader can tell what the file is.
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut add = |name: &str, bytes: &[u8], options| -> io::Result<()> {
        zip.start_file(name, options).map_err(failed)?;
        zip.write_all(bytes)
    };
    add("mimetype", b"application/epub+zip", stored)?;
    add("META-INF/container.xml", container.as_bytes(), deflated)?;
    add("OEBPS/content.opf", package.as_bytes(), deflated)?;
    add("OEBPS/nav.xhtml", nav.as_bytes(), deflated)?;
    add("OEBPS/text.xhtml", chapter.as_bytes(), deflated)?;
    for (name, _, bytes) in &pictures {
        // Pictures are compressed already.
        add(&format!("OEBPS/{name}"), bytes, stored)?;
    }
    Ok(zip.finish().map_err(failed)?.into_inner())
}

/// The tables PdfCraft found on the pages (as its web page export writes them), a sheet each.
fn workbook(html: &str) -> io::Result<Vec<u8>> {
    use gridcraft_engine::core::{CellRef, Value};
    use gridcraft_engine::model::{Sheet, Workbook};
    let mut book = Workbook::new();
    book.sheets.clear();
    for (i, table) in body(html).split("<table>").skip(1).enumerate() {
        let table = table.split("</table>").next().unwrap_or_default();
        let mut sheet = Sheet::new(format!("Table {}", i + 1));
        for (row, cells) in table.split("<tr>").skip(1).enumerate() {
            let mut col = 0;
            for cell in cells.split("<td").skip(1) {
                let Some(open) = cell.find('>') else {
                    continue;
                };
                let span = cell[..open]
                    .split("colspan=\"")
                    .nth(1)
                    .and_then(|rest| rest.split('"').next()?.parse::<u32>().ok())
                    .unwrap_or(1);
                let end = cell.find("</td>").unwrap_or(cell.len());
                let text = unescape(&strip_tags(&cell[open + 1..end.max(open + 1)]));
                let text = text.trim();
                if !text.is_empty() {
                    let value = match number(text) {
                        Some(n) => Value::number(n),
                        None => Value::text(text),
                    };
                    sheet.set_value(CellRef::new(row as u32, col), value);
                }
                col += span.max(1);
            }
        }
        book.sheets.push(Arc::new(sheet));
    }
    if book.sheets.is_empty() {
        return Err(io::Error::other("there are no tables in this PDF"));
    }
    book.active_sheet = 0;
    gridcraft_engine::io::save_bytes(&book, "tables.xlsx").map_err(failed)
}

/// A cell that is plainly a number ("12", "-3.5"): not one with a leading zero, like a code.
fn number(text: &str) -> Option<f64> {
    let digits = text.trim_start_matches('-');
    let leading_zero = digits.len() > 1 && digits.starts_with('0') && !digits.starts_with("0.");
    if leading_zero
        || !text
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'.' || b == b'-')
    {
        return None;
    }
    text.parse::<f64>().ok().filter(|n| n.is_finite())
}

fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in html.chars() {
        match c {
            '<' => inside = true,
            '>' => inside = false,
            c if !inside => out.push(c),
            _ => {}
        }
    }
    out
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Standard base64, as the export writes pictures into its web page.
fn base64(text: &str) -> Vec<u8> {
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut bits, mut count) = (0u32, 0);
    for v in text.bytes().filter_map(value) {
        bits = bits << 6 | v;
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_decodes_with_and_without_padding() {
        assert_eq!(base64("aGVsbG8="), b"hello");
        assert_eq!(base64("aGk"), b"hi");
        assert_eq!(base64(""), b"");
    }

    #[test]
    fn tables_become_sheets_with_numbers_as_numbers() {
        let html = "<body>\n<p>x</p>\n<table>\n<tr><td><strong>Name</strong></td><td \
                    colspan=\"2\">Score</td></tr>\n<tr><td>A &amp; B</td><td>12.5</td><td>007</td>\
                    </tr>\n</table>\n</body>";
        let bytes = workbook(html).unwrap();
        let (book, _) = gridcraft_engine::io::open_bytes("t.xlsx", &bytes).unwrap();
        assert_eq!(book.sheets.len(), 1);
        assert_eq!(book.sheets[0].name, "Table 1");
        assert!(workbook("<body><p>none</p></body>").is_err());
        assert_eq!(number("12.5"), Some(12.5));
        assert_eq!(number("007"), None);
        assert_eq!(number("1,5"), None);
    }

    #[test]
    fn an_epub_is_well_formed_with_its_pictures_apart() {
        let html = "<!DOCTYPE html>\n<html><body>\n<h1 style=\"color:#FF0000\">Başlık &amp; \
                    <em>alt</em></h1>\n<p>Metin</p>\n<hr>\n<p><img alt=\"\" \
                    src=\"data:image/png;base64,aGVsbG8=\"></p>\n</body>\n</html>\n";
        let bytes = epub(html, "Kitap <1>").unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert_eq!(zip.by_index(0).unwrap().name().unwrap(), "mimetype");
        let mut text = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("OEBPS/text.xhtml").unwrap(), &mut text)
            .unwrap();
        assert!(text.contains("<h1 id=\"heading1\" style=\"color:#FF0000\">Başlık"));
        assert!(
            text.contains("<hr/>") && text.contains("<img alt=\"\" src=\"images/picture1.png\"/>")
        );
        assert!(text.contains("<title>Kitap &lt;1&gt;</title>"));
        let mut picture = Vec::new();
        std::io::Read::read_to_end(
            &mut zip.by_name("OEBPS/images/picture1.png").unwrap(),
            &mut picture,
        )
        .unwrap();
        assert_eq!(picture, b"hello");
        let mut nav = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("OEBPS/nav.xhtml").unwrap(), &mut nav)
            .unwrap();
        assert!(nav.contains("<a href=\"text.xhtml#heading1\">Başlık &amp; alt</a>"));
    }
}
