//! Work started from the wheel runs on a background thread: every conversion, tool and action
//! the wheel offers is done for real.

use std::io;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::actions::{self, Action};
use crate::archive::Progress;
use crate::catalog::{self, Choice, Mode};
use crate::convert::{self, Task};
use crate::i18n::{tr, trf};

pub struct Job {
    pub batch: u32,
    pub input: String,
    pub output: String,
    work: Arc<Shared>,
    /// Cancelled from its card before it finished: done as far as the card is concerned,
    /// whatever the thread is still winding down.
    cancelled_at: Option<Instant>,
}

#[derive(Default)]
struct Shared {
    progress: Arc<Progress>,
    /// When it ended, what to say, and the file made (if any), or why it failed.
    result: Mutex<Option<(Instant, Result<Outcome, String>)>>,
}

impl Shared {
    fn result(&self) -> Option<(Instant, Result<Outcome, String>)> {
        self.result
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

#[derive(Clone)]
struct Outcome {
    message: String,
    selected_output: Option<PathBuf>,
}

impl Job {
    fn new(batch: u32, input: String, output: String, work: Arc<Shared>) -> Job {
        Job {
            batch,
            input,
            output,
            work,
            cancelled_at: None,
        }
    }

    /// Stops it, unless it is done: real work stops at its next check and puts nothing in place.
    pub fn cancel(&mut self, now: Instant) {
        if self.finished_at().is_some() {
            return;
        }
        self.cancelled_at = Some(now);
        self.work.progress.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled_at.is_some()
    }

    pub fn progress(&self) -> f32 {
        if self.work.result().is_some() {
            1.0
        } else {
            self.work.progress.fraction()
        }
    }

    /// When it finished, if it has.
    pub fn finished_at(&self) -> Option<Instant> {
        if self.cancelled_at.is_some() {
            return self.cancelled_at;
        }
        self.work.result().map(|(at, _)| at)
    }

    pub fn error(&self) -> Option<String> {
        if self.is_cancelled() {
            return Some("cancelled".into());
        }
        self.work.result().and_then(|(_, r)| r.err())
    }

    /// What it did, in a few words, once done.
    pub fn message(&self) -> Option<String> {
        if self.is_cancelled() {
            return None;
        }
        self.work
            .result()
            .and_then(|(_, r)| r.ok().map(|outcome| outcome.message))
    }

    /// A newly created real output, if this job completed successfully.
    pub fn successful_output(&self) -> Option<PathBuf> {
        if self.is_cancelled() {
            return None;
        }
        self.work
            .result()
            .and_then(|(_, r)| r.ok()?.selected_output)
    }
}

/// Whether `choice` does anything to `file`; mixed selections skip files it doesn't fit.
pub fn applies(file: &Path, choice: Choice) -> bool {
    match choice.mode {
        Mode::Actions => true,
        Mode::Media | Mode::Everyday | Mode::Apps | Mode::System => false,
        Mode::Tools => catalog::kind_of(file).is_some(),
        Mode::Convert => {
            catalog::kind_of(file).is_some() && catalog::ext_of(file).to_uppercase() != choice.label
        }
    }
}

/// Real work, done on the batch's thread.
enum Real {
    File(PathBuf, Task),
    Action(Vec<PathBuf>, Action),
    /// Files of one kind merged into one.
    Merge(Vec<PathBuf>, catalog::Kind),
}

/// One job per file, or one for the whole selection for an action or a merge. They run one after
/// another on a thread of their own.
pub fn start(files: &[PathBuf], choice: Choice, batch: u32) -> Vec<Job> {
    if choice.mode == Mode::Actions
        && let Some(action) = Action::from_label(choice.label)
    {
        let shared = Arc::new(Shared::default());
        let job = Job::new(
            batch,
            catalog::describe(files),
            actions::preview(files, action),
            shared.clone(),
        );
        spawn(vec![(Real::Action(files.to_vec(), action), shared)]);
        return vec![job];
    }
    // The wheel ignores files it doesn't recognise, and so does the merge.
    let known: Vec<PathBuf> = files
        .iter()
        .filter(|f| catalog::kind_of(f).is_some())
        .cloned()
        .collect();
    // Files all of one kind that merges, into one.
    let kind = known.first().and_then(|f| catalog::kind_of(f));
    if choice.mode == Mode::Tools
        && choice.label == catalog::MERGE
        && known.len() > 1
        && let Some(kind) = kind.filter(|kind| convert::merges(*kind))
        && known.iter().all(|f| catalog::kind_of(f) == Some(kind))
    {
        let shared = Arc::new(Shared::default());
        let job = Job::new(
            batch,
            catalog::describe(&known),
            outcome(&known[0], choice),
            shared.clone(),
        );
        spawn(vec![(Real::Merge(known, kind), shared)]);
        return vec![job];
    }
    let mut real = Vec::new();
    let jobs = files
        .iter()
        .filter(|file| applies(file, choice))
        .map(|file| {
            let shared = Arc::new(Shared::default());
            match convert::task(file, choice) {
                Some(task) => real.push((Real::File(file.clone(), task), shared.clone())),
                // The wheel only offers what is done; should anything else come, it says so.
                None => {
                    *shared.result.lock().unwrap_or_else(|e| e.into_inner()) =
                        Some((Instant::now(), Err("McGyver can't do that".into())));
                }
            }
            let work = shared;
            Job::new(
                batch,
                catalog::display_name(file),
                outcome(file, choice),
                work,
            )
        })
        .collect();
    spawn(real);
    jobs
}

fn spawn(work: Vec<(Real, Arc<Shared>)>) {
    if work.is_empty() {
        return;
    }
    let shared: Vec<Arc<Shared>> = work.iter().map(|(_, s)| s.clone()).collect();
    let spawned = std::thread::Builder::new()
        .name("mcgyver jobs".into())
        .spawn(move || {
            for (work, shared) in work {
                // Cancelled while waiting its turn: not started at all.
                let result = match shared.progress.check() {
                    Ok(()) => {
                        convert::as_job(shared.progress.clone(), || run(&work, &shared.progress))
                    }
                    Err(e) => Err(describe(&e)),
                };
                *shared.result.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some((Instant::now(), result));
            }
        });
    if let Err(e) = spawned {
        for shared in shared {
            *shared.result.lock().unwrap_or_else(|e| e.into_inner()) =
                Some((Instant::now(), Err(format!("couldn't start: {e}"))));
        }
    }
}

fn run(work: &Real, progress: &Progress) -> Result<Outcome, String> {
    // A codec that panics on a damaged file shouldn't take the other jobs down with it.
    let result = panic::catch_unwind(AssertUnwindSafe(|| match work {
        Real::File(file, task) => convert::run(file, *task, progress).map(|made| Outcome {
            message: trf("Saved as {}", &[&catalog::display_name(&made)]),
            selected_output: Some(made),
        }),
        Real::Merge(files, kind) => convert::merge(files, *kind, progress).map(|made| Outcome {
            message: trf("Saved as {}", &[&catalog::display_name(&made)]),
            selected_output: Some(made),
        }),
        Real::Action(files, action) => {
            actions::run(files, *action, progress).map(|message| Outcome {
                message,
                selected_output: None,
            })
        }
    }));
    match result {
        Ok(Ok(said)) => Ok(said),
        Ok(Err(e)) => Err(describe(&e)),
        Err(_) => Err("the file looks damaged".into()),
    }
}

/// The error in a few plain words; our own messages pass through.
fn describe(e: &io::Error) -> String {
    match e.kind() {
        io::ErrorKind::NotFound => "it isn't there any more".into(),
        io::ErrorKind::PermissionDenied => "no permission to do that".into(),
        io::ErrorKind::StorageFull => "the disk is full".into(),
        io::ErrorKind::InvalidData | io::ErrorKind::UnexpectedEof => {
            "the file looks damaged".into()
        }
        _ => e.to_string(),
    }
}

/// What the result will be called (or what happens, for actions).
pub fn outcome(file: &Path, choice: Choice) -> String {
    let stem = catalog::stem_of(file);
    let ext = file
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    if choice.mode == Mode::Tools
        && let Some(kind @ (catalog::Kind::Audio | catalog::Kind::Video)) = catalog::kind_of(file)
        && let Some(tool) =
            crate::media::Tool::from_label(choice.label, kind == catalog::Kind::Video)
    {
        let video = kind == catalog::Kind::Video;
        return match crate::media::tool_extension(&catalog::ext_of(file), video, tool) {
            Some(ext) => format!("{stem} ({}).{ext}", tool.suffix()),
            None => format!("{stem} ({})", tool.suffix()),
        };
    }
    match choice.mode {
        // What an archive holds isn't known until it's opened.
        Mode::Convert if choice.label == catalog::UNZIP => match catalog::kind_of(file) {
            Some(catalog::Kind::Compressed) => stem,
            _ => tr("unpacked here").into(),
        },
        Mode::Convert => format!("{stem}.{}", catalog::extension_for(choice.label)),
        Mode::Tools if catalog::kind_of(file) == Some(catalog::Kind::Image) => {
            let Some(tool) = crate::images::Tool::from_label(choice.label) else {
                return format!("{stem} ({}).{ext}", choice.label.to_lowercase());
            };
            let Some(target) = crate::images::tool_target(&catalog::ext_of(file), tool) else {
                return format!("{stem} ({}).{ext}", choice.label.to_lowercase());
            };
            format!("{stem} ({}).{}", tool.suffix(), target.extension())
        }
        Mode::Tools if catalog::kind_of(file) == Some(catalog::Kind::Pdf) => {
            match crate::office::PdfTool::from_label(choice.label) {
                Some(tool @ crate::office::PdfTool::Split) => format!("{stem} ({})", tool.suffix()),
                Some(tool) => format!("{stem} ({}).pdf", tool.suffix()),
                None if choice.label == catalog::MERGE => format!("{stem} (merged).pdf"),
                None => format!("{stem} ({}).{ext}", choice.label.to_lowercase()),
            }
        }
        Mode::Tools if choice.label == catalog::MERGE => match catalog::kind_of(file) {
            Some(kind) => format!(
                "{stem} (merged).{}",
                convert::merged_extension(kind, &catalog::ext_of(file))
            ),
            None => format!("{stem} (merged).{ext}"),
        },
        Mode::Tools if choice.label == catalog::FLATTEN => format!("{stem} (flat)"),
        Mode::Tools if choice.label == "Compress" => format!("{stem} (compressed).{ext}"),
        Mode::Tools => format!("{stem} ({}).{ext}", choice.label.to_lowercase()),
        Mode::Actions => tr(match choice.label {
            "Copy" | "Copy To" => "copied",
            "Cut" => "cut",
            "Move To" => "moved",
            "Copy Path" => "path copied",
            "Terminal" => "terminal opened",
            "Share" | "Telegram" | "WhatsApp" | "Teams" | "Slack" => "shared",
            _ => "floating",
        })
        .into(),
        Mode::Media | Mode::Everyday | Mode::Apps | Mode::System => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_cancelled_job_is_over_at_once_and_made_nothing() {
        let now = Instant::now();
        let ms = Duration::from_millis;
        let running = Arc::new(Shared::default());
        let mut job = Job::new(1, "a".into(), "b".into(), running.clone());
        assert_eq!(job.finished_at(), None);
        job.cancel(now + ms(5));
        assert_eq!(job.finished_at(), Some(now + ms(5)));
        assert_eq!(job.error().as_deref(), Some("cancelled"));
        assert_eq!(job.successful_output(), None);
        assert!(running.progress.is_cancelled());

        // Done is done.
        let finished = Arc::new(Shared::default());
        *finished.result.lock().unwrap() = Some((
            now,
            Ok(Outcome {
                message: "done".into(),
                selected_output: None,
            }),
        ));
        let mut done = Job::new(1, "a".into(), "b".into(), finished);
        done.cancel(now + ms(5));
        assert!(!done.is_cancelled() && done.error().is_none());
    }

    #[test]
    fn every_choice_the_wheel_offers_does_real_work() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("photos");
        std::fs::create_dir(&folder).unwrap();
        let mut files: Vec<PathBuf> = [
            "photo.jpg",
            "scan.png",
            "raw.cr2",
            "song.mp3",
            "voice.wav",
            "old.wma",
            "clip.mp4",
            "old.avi",
            "report.pdf",
            "letter.docx",
            "old.doc",
            "notes.md",
            "plain.txt",
            "book.epub",
            "page.html",
            "story.rtf",
            "text.odt",
            "mac.pages",
            "table.xlsx",
            "old.xls",
            "sheet.ods",
            "data.csv",
            "data.tsv",
            "mac.numbers",
            "deck.pptx",
            "old.ppt",
            "talk.odp",
            "show.key",
            "film.srt",
            "film.vtt",
            "backup.zip",
            "backup.tar.gz",
            "log.gz",
            "set.7z",
        ]
        .iter()
        .map(|name| dir.path().join(name))
        .collect();
        files.push(folder);
        for file in &files {
            for mode in [Mode::Convert, Mode::Tools] {
                for choice in catalog::choices(std::slice::from_ref(file), mode) {
                    let bubbles = catalog::subs(choice);
                    // Shift only holds its bubbles: how far is picked on them.
                    let wedge = choice.label != catalog::SHIFT;
                    for choice in bubbles.into_iter().chain(wedge.then_some(choice)) {
                        assert!(
                            convert::task(file, choice).is_some(),
                            "{} offers {:?} {}, which does nothing",
                            catalog::display_name(file),
                            mode,
                            choice.label
                        );
                    }
                }
            }
            // A second of the same kind: Merge, where offered, is one real job.
            let ext = catalog::ext_of(file);
            let pair = [file.clone(), dir.path().join(format!("other.{ext}"))];
            if let Some(merge) = catalog::choices(&pair, Mode::Tools)
                .into_iter()
                .find(|c| c.label == catalog::MERGE)
            {
                let kind = catalog::kind_of(file).unwrap();
                assert!(convert::merges(kind) && convert::merges_file(kind, &ext));
                // The files aren't there, so the job fails at once, touching nothing.
                let jobs = start(&pair, merge, 1);
                assert_eq!(jobs.len(), 1, "{ext}");
            }
        }
    }

    #[test]
    fn work_cancelled_before_its_turn_never_starts() {
        let dir = tempfile::tempdir().unwrap();
        let note = dir.path().join("note.txt");
        std::fs::write(&note, "n").unwrap();
        let shared = Arc::new(Shared::default());
        shared.progress.cancel();
        spawn(vec![(
            Real::Action(vec![note], Action::Zip),
            shared.clone(),
        )]);
        let deadline = Instant::now() + Duration::from_secs(10);
        while shared.result().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let (_, result) = shared.result().expect("the job thread answers");
        assert_eq!(result.err().as_deref(), Some("cancelled"));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
