//! Work started from the wheel runs on a background thread. Images, archives, media conversions,
//! Office-to-PDF exports and actions have real implementations; remaining catalog entries are
//! simulated, with a made-up duration and no file touched.

use std::io;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::actions::{self, Action};
use crate::archive::Progress;
use crate::catalog::{self, Choice, Mode};
use crate::convert::{self, Task};
use crate::i18n::{tr, trf};

pub struct Job {
    pub batch: u32,
    pub input: String,
    pub output: String,
    work: Work,
    /// Cancelled from its card before it finished: done as far as the card is concerned,
    /// whatever the thread is still winding down.
    cancelled_at: Option<Instant>,
}

enum Work {
    Simulated { start: Instant, duration: Duration },
    Real(Arc<Shared>),
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
    fn new(batch: u32, input: String, output: String, work: Work) -> Job {
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
        if self.finished_at(now).is_some() {
            return;
        }
        self.cancelled_at = Some(now);
        if let Work::Real(shared) = &self.work {
            shared.progress.cancel();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled_at.is_some()
    }

    pub fn progress(&self, now: Instant) -> f32 {
        match &self.work {
            Work::Simulated { start, duration } => {
                let elapsed = now.saturating_duration_since(*start).as_secs_f32();
                (elapsed / duration.as_secs_f32()).clamp(0.0, 1.0)
            }
            Work::Real(shared) if shared.result().is_some() => 1.0,
            Work::Real(shared) => shared.progress.fraction(),
        }
    }

    /// When it finished, if it has.
    pub fn finished_at(&self, now: Instant) -> Option<Instant> {
        if self.cancelled_at.is_some() {
            return self.cancelled_at;
        }
        match &self.work {
            Work::Simulated { start, duration } => {
                Some(*start + *duration).filter(|end| now >= *end)
            }
            Work::Real(shared) => shared.result().map(|(at, _)| at),
        }
    }

    pub fn error(&self) -> Option<String> {
        if self.is_cancelled() {
            return Some("cancelled".into());
        }
        match &self.work {
            Work::Real(shared) => shared.result().and_then(|(_, r)| r.err()),
            Work::Simulated { .. } => None,
        }
    }

    /// What it did, in a few words, once done.
    pub fn message(&self) -> Option<String> {
        if self.is_cancelled() {
            return None;
        }
        match &self.work {
            Work::Real(shared) => shared
                .result()
                .and_then(|(_, r)| r.ok().map(|outcome| outcome.message)),
            Work::Simulated { .. } => None,
        }
    }

    /// A newly created real output, if this job completed successfully.
    pub fn successful_output(&self) -> Option<PathBuf> {
        if self.is_cancelled() {
            return None;
        }
        match &self.work {
            Work::Real(shared) => shared.result().and_then(|(_, r)| r.ok()?.selected_output),
            Work::Simulated { .. } => None,
        }
    }

    pub fn is_simulated(&self) -> bool {
        matches!(self.work, Work::Simulated { .. })
    }
}

/// Whether `choice` does anything to `file`; mixed selections skip files it doesn't fit.
pub fn applies(file: &Path, choice: Choice) -> bool {
    match choice.mode {
        Mode::Actions => true,
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
    /// PDFs merged into one.
    Merge(Vec<PathBuf>),
}

/// One job per file, or one for the whole selection for an action or a merge. Real ones run one after
/// another on a thread of their own; simulated ones are queued back to back from `now`.
pub fn start(files: &[PathBuf], choice: Choice, batch: u32, now: Instant) -> Vec<Job> {
    if choice.mode == Mode::Actions
        && let Some(action) = Action::from_label(choice.label)
    {
        let shared = Arc::new(Shared::default());
        let job = Job::new(
            batch,
            catalog::describe(files),
            actions::preview(files, action),
            Work::Real(shared.clone()),
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
    let all_pdfs = known
        .iter()
        .all(|f| catalog::kind_of(f) == Some(catalog::Kind::Pdf));
    if choice.mode == Mode::Tools && choice.label == catalog::MERGE && all_pdfs && known.len() > 1 {
        let shared = Arc::new(Shared::default());
        let job = Job::new(
            batch,
            catalog::describe(&known),
            outcome(&known[0], choice),
            Work::Real(shared.clone()),
        );
        spawn(vec![(Real::Merge(known), shared)]);
        return vec![job];
    }
    let mut queued = now;
    let mut real = Vec::new();
    let jobs = files
        .iter()
        .filter(|file| applies(file, choice))
        .map(|file| {
            let work = match convert::task(file, choice) {
                Some(task) => {
                    let shared = Arc::new(Shared::default());
                    real.push((Real::File(file.clone(), task), shared.clone()));
                    Work::Real(shared)
                }
                None => {
                    let duration = fake_duration(file, choice.mode);
                    queued += duration;
                    Work::Simulated {
                        start: queued - duration,
                        duration,
                    }
                }
            };
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
        Real::Merge(files) => convert::merge_pdfs(files, progress).map(|made| Outcome {
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
        Mode::Tools => format!("{stem} ({}).{ext}", choice.label.to_lowercase()),
        Mode::Actions => tr(match choice.label {
            "Copy" | "Copy To" => "copied",
            "Cut" => "cut",
            "Move To" => "moved",
            "Copy Path" => "path copied",
            "Terminal" => "terminal opened",
            "Share" => "shared",
            _ => "floating",
        })
        .into(),
    }
}

fn fake_duration(file: &Path, mode: Mode) -> Duration {
    if mode == Mode::Actions {
        return Duration::from_millis(250);
    }
    let megabytes = std::fs::metadata(file)
        .map(|m| m.len() as f32 / 1_000_000.0)
        .unwrap_or_else(|_| (catalog::display_name(file).len() % 7) as f32 * 4.0 + 6.0);
    Duration::from_secs_f32((0.6 + megabytes * 0.05).min(4.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cancelled_job_is_over_at_once_and_made_nothing() {
        let now = Instant::now();
        let ms = Duration::from_millis;
        let simulated = |duration| Work::Simulated {
            start: now,
            duration,
        };
        let mut job = Job::new(1, "a".into(), "b".into(), simulated(ms(10_000)));
        assert_eq!(job.finished_at(now), None);
        job.cancel(now + ms(5));
        assert_eq!(job.finished_at(now + ms(5)), Some(now + ms(5)));
        assert_eq!(job.error().as_deref(), Some("cancelled"));
        assert_eq!(job.successful_output(), None);

        // Done is done.
        let mut done = Job::new(1, "a".into(), "b".into(), simulated(ms(1)));
        done.cancel(now + ms(5));
        assert!(!done.is_cancelled() && done.error().is_none());
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
