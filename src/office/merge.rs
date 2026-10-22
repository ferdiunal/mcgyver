//! Documents and spreadsheets merged into one: WordCraft documents one after another, each from
//! a new page; GridCraft workbooks' sheets side by side in one workbook.

use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gridcraft_engine::model::Workbook;
use wordcraft_engine::doc::section::SectionStart;
use wordcraft_engine::doc::{Block, Document, InlineObject, Paragraph, SectionProps};

use super::{failed, read};
use crate::archive::Progress;

/// Tables nested deeper than this in a damaged file are left as they are.
const MAX_DEPTH: usize = 32;

/// What merging documents saves as: the first's format where WordCraft writes it, else Word.
pub fn document_extension(first: &str) -> &'static str {
    match first {
        "docx" => "docx",
        "odt" => "odt",
        "rtf" => "rtf",
        "html" => "html",
        "txt" => "txt",
        _ => "docx",
    }
}

fn open_document(input: &Path) -> io::Result<Document> {
    let name = crate::catalog::display_name(input);
    let doc = if crate::catalog::ext_of(input) == "html" {
        // From the path, so a web page finds the pictures beside it.
        super::check_size(input)?;
        wordcraft_engine::io::open_path(input)
    } else {
        wordcraft_engine::io::open_bytes(&name, &read(input)?)
    };
    doc.map_err(io::Error::other)
}

/// With several files, which one failed.
fn named(input: &Path) -> impl Fn(io::Error) -> io::Error + '_ {
    move |e| {
        let name = crate::catalog::display_name(input);
        io::Error::new(e.kind(), format!("{name}: {e}"))
    }
}

/// The documents `inputs`, in order, each from a new page, saved like the first (see
/// [`document_extension`]). What each brings is carried over: its pictures, lists, styles the
/// first lacks (where both have one, the first's wins), notes, comments, text boxes and tracked
/// changes. Its section breaks keep their pages, without its own headers and footers.
pub fn documents(inputs: &[PathBuf], progress: &Progress) -> io::Result<Vec<u8>> {
    let (first, rest) = inputs
        .split_first()
        .ok_or_else(|| io::Error::other("nothing to merge"))?;
    progress.begin(inputs.len() as u64 + 1);
    let mut merged = open_document(first).map_err(named(first))?;
    progress.add(1);
    for input in rest {
        progress.check()?;
        let doc = open_document(input).map_err(named(input))?;
        append(&mut merged, doc);
        progress.add(1);
    }
    progress.check()?;
    let ext = document_extension(&crate::catalog::ext_of(first));
    let bytes = wordcraft_engine::io::save_bytes(&format!("merged.{ext}"), &merged)
        .map_err(io::Error::other)?;
    progress.add(1);
    Ok(bytes)
}

/// `src`'s body after `dst`'s. `dst`'s last section is closed by a section break, so its pages
/// keep their size and headers, and `src`'s sections follow from a new page with their own
/// page setup, without headers and footers of their own.
fn append(dst: &mut Document, src: Document) {
    let mut adopt = Adopt::new(dst, &src);
    let mut blocks: Vec<Arc<Block>> = src.body.clone();
    adopt.blocks(&mut blocks, 0);
    let mut last = src.last_section.clone();
    adopt.section(&mut last);
    // `src`'s first section starts a new page.
    let first = blocks
        .iter()
        .position(|block| matches!(&**block, Block::Para(p) if p.section.is_some()));
    match first.map(|i| Arc::make_mut(&mut blocks[i])) {
        Some(Block::Para(p)) => {
            if let Some(section) = p.section.as_deref_mut() {
                section.start = SectionStart::NextPage;
            }
        }
        _ => last.start = SectionStart::NextPage,
    }
    let closing = Some(Box::new(dst.last_section.clone()));
    match dst.body.last_mut().map(Arc::make_mut) {
        Some(Block::Para(p)) if p.section.is_none() => p.section = closing,
        _ => {
            let mut p = Paragraph::new();
            p.section = closing;
            dst.body.push(Arc::new(Block::Para(p)));
        }
    }
    dst.last_section = last;
    dst.body.extend(blocks);
}

/// Bringing what one document refers to into another, renumbered so nothing collides.
struct Adopt<'a> {
    dst: &'a mut Document,
    src: &'a Document,
    /// Old media key → new.
    media: BTreeMap<String, String>,
    /// Old list (`Num`) id → new.
    lists: BTreeMap<u32, u32>,
    /// Old part id → new, for the notes, comments and text boxes taken so far.
    parts: BTreeMap<u32, u32>,
    /// Old comment id → new.
    comments: BTreeMap<u32, u32>,
    /// Where the source's tracked changes start among the merged document's.
    revisions: u32,
}

impl<'a> Adopt<'a> {
    fn new(dst: &'a mut Document, src: &'a Document) -> Adopt<'a> {
        let mut media = BTreeMap::new();
        for (key, data) in &src.media {
            let ext = key.rsplit_once('.').map_or("png", |(_, ext)| ext);
            media.insert(key.clone(), dst.add_media(data.to_vec(), ext));
        }
        let next_abstract = dst
            .numbering
            .abstracts
            .iter()
            .map(|a| a.id.saturating_add(1))
            .max()
            .unwrap_or(0);
        let mut abstracts = BTreeMap::new();
        for (i, a) in src.numbering.abstracts.iter().enumerate() {
            let id = next_abstract.saturating_add(i as u32);
            abstracts.insert(a.id, id);
            let mut a = a.clone();
            a.id = id;
            dst.numbering.abstracts.push(a);
        }
        let next_list = dst
            .numbering
            .nums
            .iter()
            .map(|n| n.id.saturating_add(1))
            .max()
            .unwrap_or(1)
            .max(1);
        let mut lists = BTreeMap::new();
        for (i, n) in src.numbering.nums.iter().enumerate() {
            let id = next_list.saturating_add(i as u32);
            lists.insert(n.id, id);
            let mut n = n.clone();
            n.id = id;
            n.abstract_id = abstracts
                .get(&n.abstract_id)
                .copied()
                .unwrap_or(n.abstract_id);
            dst.numbering.nums.push(n);
        }
        for style in &src.styles.styles {
            if dst.styles.get(&style.id).is_none() {
                let mut style = style.clone();
                // The first document's default paragraph style stays the only one.
                style.default = false;
                if let Some(list) = style.para.numbering.as_mut()
                    && list.num != 0
                {
                    list.num = lists.get(&list.num).copied().unwrap_or(0);
                }
                dst.styles.upsert(style);
            }
        }
        let revisions = u32::try_from(dst.revisions.len()).unwrap_or(u32::MAX);
        dst.revisions.extend(src.revisions.iter().cloned());
        let next_comment = dst.comments.keys().next_back().map_or(0, |id| id + 1);
        let comments = src
            .comments
            .keys()
            .enumerate()
            .map(|(i, id)| (*id, next_comment + i as u32))
            .collect();
        let mut adopt = Adopt {
            dst,
            src,
            media,
            lists,
            parts: BTreeMap::new(),
            comments,
            revisions,
        };
        for (old, comment) in &src.comments {
            let mut comment = comment.clone();
            comment.part = adopt.part(comment.part).unwrap_or(0);
            comment.parent = comment
                .parent
                .and_then(|parent| adopt.comments.get(&parent).copied());
            let id = adopt.comments[old];
            adopt.dst.comments.insert(id, comment);
        }
        adopt
    }

    /// The merged document's copy of the source's part `old` (made once), or `None` if the
    /// source has no such part.
    fn part(&mut self, old: u32) -> Option<u32> {
        if let Some(new) = self.parts.get(&old) {
            return Some(*new);
        }
        let part = self.src.parts.get(&old)?;
        // The id first, so a text box that shows itself doesn't go round for ever.
        let new = self.dst.try_add_part(part.kind, Vec::new()).ok()?;
        self.parts.insert(old, new);
        let mut blocks = part.blocks.clone();
        self.blocks(&mut blocks, 0);
        let mut body = part.body;
        // Linked text boxes stay linked only within their own document.
        body.next = None;
        if let Some(copy) = self.dst.parts.get_mut(&new) {
            copy.blocks = blocks;
            copy.body = body;
        }
        Some(new)
    }

    fn blocks(&mut self, blocks: &mut [Arc<Block>], depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        for block in blocks {
            match Arc::make_mut(block) {
                Block::Para(p) => self.paragraph(p),
                Block::Table(table) => {
                    self.change(&mut table.props.fmt_change);
                    for row in &mut table.rows {
                        self.change(&mut row.props.fmt_change);
                        for cell in &mut row.cells {
                            self.change(&mut cell.props.fmt_change);
                            self.blocks(&mut cell.blocks, depth + 1);
                        }
                    }
                }
            }
        }
    }

    fn paragraph(&mut self, p: &mut Paragraph) {
        if let Some(list) = p.props.numbering.as_mut()
            && list.num != 0
        {
            list.num = self.lists.get(&list.num).copied().unwrap_or(0);
        }
        if let Some(section) = p.section.as_deref_mut() {
            self.section(section);
        }
        for run in &mut p.runs {
            self.revision(&mut run.props.ins);
            self.revision(&mut run.props.del);
            self.change(&mut run.props.fmt_change);
        }
        self.revision(&mut p.mark.ins);
        self.revision(&mut p.mark.del);
        self.change(&mut p.mark.fmt_change);
        self.change(&mut p.props.fmt_change);
        let mut list_change = p.props.num_change.as_ref().map(|c| c.rev);
        self.revision(&mut list_change);
        match (list_change, p.props.num_change.as_mut()) {
            (Some(rev), Some(change)) => change.rev = rev,
            _ => p.props.num_change = None,
        }
        for object in &mut p.objects {
            self.object(object);
        }
        p.touch();
    }

    fn object(&mut self, object: &mut InlineObject) {
        match object {
            InlineObject::Image { media, .. } => {
                if let Some(new) = self.media.get(media.as_str()) {
                    *media = new.clone();
                }
            }
            InlineObject::Group { children, .. } => {
                for child in children {
                    self.object(&mut child.obj);
                }
            }
            InlineObject::Shape { story, .. } => {
                *story = story.and_then(|old| self.part(old));
            }
            // A note whose story is missing points at none, rather than at one of `dst`'s.
            InlineObject::NoteRef { id, .. } => *id = self.part(*id).unwrap_or(0),
            InlineObject::CommentStart { id } | InlineObject::CommentEnd { id } => {
                if let Some(new) = self.comments.get(id) {
                    *id = *new;
                }
            }
            _ => {}
        }
    }

    /// A section of the source: its headers and footers are its own document's, so it takes the
    /// ones before it.
    fn section(&self, section: &mut SectionProps) {
        section.headers = Default::default();
        section.footers = Default::default();
        section.title_page = false;
        self.change(&mut section.fmt_change);
    }

    /// A tracked change's place among the merged document's (gone if the source lacks it).
    fn revision(&self, rev: &mut Option<u32>) {
        let count = self.src.revisions.len();
        *rev = rev
            .filter(|r| (*r as usize) < count)
            .and_then(|r| self.revisions.checked_add(r));
    }

    fn change<T>(&self, change: &mut Option<Box<wordcraft_engine::doc::props::PropChange<T>>>) {
        let mut rev = change.as_ref().map(|c| c.rev);
        self.revision(&mut rev);
        match (rev, change.as_mut()) {
            (Some(rev), Some(c)) => c.rev = rev,
            _ => *change = None,
        }
    }
}

/// The sheets of the workbooks `inputs`, in order, in one workbook (XLSX). A sheet whose name
/// is taken gets a number ("Sheet1 (2)"), and what its workbook says of it follows: formulas,
/// charts, names, sparklines, validations and conditional formats. Tables, pivot tables and
/// slicers stay behind (their names must be unique in a workbook); their cells come.
pub fn workbooks(inputs: &[PathBuf], progress: &Progress) -> io::Result<Vec<u8>> {
    let open = |input: &Path| -> io::Result<Workbook> {
        let name = crate::catalog::display_name(input);
        let bytes = read(input)?;
        let (book, _) = gridcraft_engine::io::open_bytes(&name, &bytes).map_err(failed)?;
        Ok(book)
    };
    let (first, rest) = inputs
        .split_first()
        .ok_or_else(|| io::Error::other("nothing to merge"))?;
    progress.begin(inputs.len() as u64 + 1);
    let mut merged = open(first).map_err(named(first))?;
    progress.add(1);
    for input in rest {
        progress.check()?;
        let book = open(input).map_err(named(input))?;
        add_book(&mut merged, &book);
        progress.add(1);
    }
    progress.check()?;
    let bytes = gridcraft_engine::io::save_bytes(&merged, "merged.xlsx").map_err(failed)?;
    progress.add(1);
    Ok(bytes)
}

/// `book`'s sheets and names after `merged`'s.
fn add_book(merged: &mut Workbook, book: &Workbook) {
    use gridcraft_engine::core::{DateSystem, Value};
    use gridcraft_engine::model::{CfRule, DefinedName, Formula, Sheet};
    use gridcraft_engine::numfmt::NumberFormat;
    let base = merged.sheets.len();
    // Dates count days from 1900 or from 1904: a date from the other kind is moved.
    let days = match (book.date_system, merged.date_system) {
        (DateSystem::D1904, DateSystem::D1900) => 1462.0,
        (DateSystem::D1900, DateSystem::D1904) => -1462.0,
        _ => 0.0,
    };
    let mut styles = HashMap::new();
    let mut renamed: Vec<(String, String)> = Vec::new();
    let mut sheets: Vec<Sheet> = Vec::new();
    for sheet in &book.sheets {
        let mut sheet = (**sheet).clone();
        let taken = |name: &str, sheets: &[Sheet]| {
            among(
                name,
                merged
                    .sheets
                    .iter()
                    .map(|s| s.name.as_str())
                    .chain(sheets.iter().map(|s| s.name.as_str())),
            )
        };
        if taken(&sheet.name, &sheets) {
            // Not one of this workbook's own names either, so renaming one sheet's references
            // can't catch another's.
            let name = (2..)
                .map(|n| {
                    let suffix = format!(" ({n})");
                    // Sheet names are at most 31 characters.
                    let keep = 31 - suffix.chars().count();
                    let stem: String = sheet.name.chars().take(keep).collect();
                    format!("{stem}{suffix}")
                })
                .find(|name| {
                    !taken(name, &sheets)
                        && !among(name, book.sheets.iter().map(|s| s.name.as_str()))
                })
                .expect("a free sheet name");
            renamed.push((sheet.name.clone(), name.clone()));
            sheet.name = name;
        }
        let cells: Vec<_> = sheet
            .cells
            .iter()
            .map(|(at, cell)| (at, cell.clone()))
            .collect();
        for (at, mut cell) in cells {
            let style = book.styles.get(cell.style);
            // A time of day alone (below 1) is the same in both.
            if days != 0.0
                && let Value::Number(n) = cell.value
                && n >= 1.0
                && cell.formula.is_none()
                && NumberFormat::parse_cached(&style.num_fmt.0).is_date()
            {
                cell.value = Value::Number(n + days);
            }
            cell.style = *styles
                .entry(cell.style)
                .or_insert_with(|| merged.styles.intern(style.clone()));
            sheet.cells.set(at, cell);
        }
        for line in sheet.rows.values_mut().chain(sheet.cols.values_mut()) {
            if let Some(style) = line.style {
                line.style = Some(
                    *styles
                        .entry(style)
                        .or_insert_with(|| merged.styles.intern(book.styles.get(style).clone())),
                );
            }
        }
        sheet.tables.clear();
        sheet.pivots.clear();
        sheet.slicers.clear();
        sheets.push(sheet);
    }
    // A formula's text with the renamed sheets' new names (as it was if it can't be read).
    let fix = |text: &str| -> String {
        if renamed.is_empty() {
            return text.to_owned();
        }
        gridcraft_engine::formula::parse(text).map_or_else(
            |_| text.to_owned(),
            |expr| gridcraft_engine::formula::print(&rename(expr, &renamed)),
        )
    };
    if !renamed.is_empty() {
        for sheet in &mut sheets {
            let cells: Vec<_> = sheet
                .cells
                .iter()
                .filter(|(_, cell)| cell.formula.is_some())
                .map(|(at, cell)| (at, cell.clone()))
                .collect();
            for (at, mut cell) in cells {
                let Some(expr) = cell.formula.as_ref().and_then(|f| f.expr()) else {
                    continue;
                };
                let array = cell.formula.as_ref().and_then(|f| f.array);
                let mut formula = Formula::from_expr(rename(expr, &renamed));
                formula.array = array;
                cell.formula = Some(Arc::new(formula));
                sheet.cells.set(at, cell);
            }
            for chart in &mut sheet.charts {
                for series in &mut chart.series {
                    series.values = fix(&series.values);
                    series.categories = series.categories.as_deref().map(fix);
                    series.name = series.name.as_deref().map(fix);
                }
            }
            for sparkline in &mut sheet.sparklines {
                sparkline.source = fix(&sparkline.source);
            }
            for validation in &mut sheet.validations {
                validation.f1 = fix(&validation.f1);
                validation.f2 = validation.f2.as_deref().map(fix);
            }
            for format in &mut sheet.cond_formats {
                match &mut format.rule {
                    CfRule::CellIs { a, b, .. } => {
                        *a = fix(a);
                        *b = b.as_deref().map(fix);
                    }
                    CfRule::Expression { formula, .. } => *formula = fix(formula),
                    _ => {}
                }
            }
        }
    }
    merged.sheets.extend(sheets.into_iter().map(Arc::new));
    // Names scoped to a sheet come along with it. A workbook-wide one too, unless the merged
    // workbook has one by that name: then each of the book's sheets gets its own copy, which
    // its formulas find first.
    let mut names = Vec::new();
    for name in &book.names {
        let formula = fix(&name.formula);
        let shared = |scope| DefinedName {
            scope,
            formula: formula.clone(),
            ..name.clone()
        };
        match name.scope {
            Some(scope) => names.push(shared(Some(base + scope))),
            None if !merged
                .names
                .iter()
                .any(|n| n.scope.is_none() && n.name.eq_ignore_ascii_case(&name.name)) =>
            {
                names.push(shared(None));
            }
            None => {
                for i in 0..book.sheets.len() {
                    let own = book
                        .names
                        .iter()
                        .any(|n| n.scope == Some(i) && n.name.eq_ignore_ascii_case(&name.name));
                    if !own {
                        names.push(shared(Some(base + i)));
                    }
                }
            }
        }
    }
    merged.names.extend(names);
}

/// `expr` with each renamed sheet's new name.
fn rename(
    mut expr: gridcraft_engine::formula::Expr,
    renamed: &[(String, String)],
) -> gridcraft_engine::formula::Expr {
    for (old, new) in renamed {
        expr = gridcraft_engine::formula::adjust::rename_sheet(expr, old, new);
    }
    expr
}

/// Whether `name` is one of `names`, as sheet names compare: case aside.
fn among<'n>(name: &str, mut names: impl Iterator<Item = &'n str>) -> bool {
    let name = name.to_lowercase();
    names.any(|n| n.to_lowercase() == name)
}
