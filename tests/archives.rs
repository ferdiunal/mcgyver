//! Real conversions of compressed files and folders, end to end through `convert::run`, plus
//! round trips with the system's own tools where they are installed.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

use mcgyver::archive::{self, Codec, Format, Progress};
use mcgyver::catalog::{self, Choice, Kind, Mode};
use mcgyver::convert::{self, Task};
use mcgyver::jobs;

const CODECS: [Codec; 8] = [
    Codec::Gz,
    Codec::Bz2,
    Codec::Xz,
    Codec::Zst,
    Codec::Lz4,
    Codec::Lzma,
    Codec::Lzip,
    Codec::Br,
];
const NOTES_TIME: u64 = 1_600_000_000;

/// A folder with what an archive has to keep: nesting, an empty folder, an empty file, a
/// non-ASCII name, an executable, a link, a fixed time and a file bigger than any buffer.
fn sample(dir: &Path) -> PathBuf {
    let root = dir.join("Photos");
    fs::create_dir_all(root.join("sub/deeper")).unwrap();
    fs::create_dir(root.join("empty")).unwrap();
    fs::write(root.join("notes.txt"), "hello\n").unwrap();
    fs::write(root.join("zero"), "").unwrap();
    fs::write(root.join("Ünïcödé ❤.txt"), "çok güzel").unwrap();
    let big: Vec<u8> = (0..700_000u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    fs::write(root.join("sub/deeper/big.bin"), big).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::write(root.join("run.sh"), "#!/bin/sh\necho hi\n").unwrap();
        fs::set_permissions(root.join("run.sh"), fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink("notes.txt", root.join("link")).unwrap();
    }
    let t = filetime::FileTime::from_unix_time(NOTES_TIME as i64, 0);
    filetime::set_file_mtime(root.join("notes.txt"), t).unwrap();
    root
}

/// Every entry under `root`, described by kind, contents and (for files) mode.
fn snapshot(root: &Path) -> BTreeMap<String, String> {
    fn visit(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let meta = fs::symlink_metadata(&path).unwrap();
            let what = if meta.file_type().is_symlink() {
                format!("link to {}", fs::read_link(&path).unwrap().display())
            } else if meta.is_dir() {
                visit(root, &path, out);
                "folder".to_owned()
            } else {
                let data = fs::read(&path).unwrap();
                let hash = data.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
                    (h ^ *b as u64).wrapping_mul(0x100_0000_01b3)
                });
                format!("{} bytes {hash:x} mode {:o}", data.len(), mode(&meta))
            };
            out.insert(name, what);
        }
    }
    let mut out = BTreeMap::new();
    visit(root, root, &mut out);
    out
}

#[cfg(unix)]
fn mode(meta: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o777
}

#[cfg(not(unix))]
fn mode(_: &fs::Metadata) -> u32 {
    0
}

fn notes_time(root: &Path) -> u64 {
    let t = fs::metadata(root.join("notes.txt"))
        .unwrap()
        .modified()
        .unwrap();
    t.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs()
}

fn run(file: &Path, task: Task) -> PathBuf {
    let progress = Progress::default();
    let made = convert::run(file, task, &progress)
        .unwrap_or_else(|e| panic!("{task:?} on {}: {e}", file.display()));
    assert_eq!(
        progress.fraction(),
        1.0,
        "{task:?} on {} didn't finish its progress",
        file.display()
    );
    made
}

fn writable_archives() -> Vec<Format> {
    let mut formats = vec![Format::Zip, Format::SevenZip, Format::Tar(None)];
    formats.extend(CODECS.map(|c| Format::Tar(Some(c))));
    formats
}

fn no_leftovers(dir: &Path) {
    for entry in fs::read_dir(dir).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        assert!(!name.starts_with(".mcgyver-"), "left behind {name}");
    }
}

#[test]
fn folders_pack_into_every_format_and_back() {
    let dir = tempfile::tempdir().unwrap();
    let folder = sample(dir.path());
    let want = snapshot(&folder);
    for format in writable_archives() {
        let packed = run(&folder, Task::Convert(format));
        assert_eq!(packed, dir.path().join(format!("Photos.{}", format.ext())));
        assert_eq!(
            Format::sniff(&packed).unwrap_or(format),
            format,
            "sniffing {}",
            packed.display()
        );
        let unpacked = run(&packed, Task::Extract);
        assert_eq!(unpacked, dir.path().join("Photos 2"), "{format:?}");
        assert_eq!(snapshot(&unpacked), want, "{format:?}");
        // Zip keeps times in two-second steps.
        assert!(
            notes_time(&unpacked).abs_diff(NOTES_TIME) <= 2,
            "{format:?} lost the time"
        );
        fs::remove_dir_all(&unpacked).unwrap();
    }
    no_leftovers(dir.path());
}

#[test]
fn archives_repack_through_every_format() {
    let dir = tempfile::tempdir().unwrap();
    let folder = sample(dir.path());
    let want = snapshot(&folder);
    let mut current = run(&folder, Task::Convert(Format::Zip));
    let mut chain = writable_archives();
    chain.rotate_left(1);
    for to in chain {
        let next = run(&current, Task::Convert(to));
        let unpacked = run(&next, Task::Extract);
        assert_eq!(snapshot(&unpacked), want, "{current:?} → {to:?}");
        fs::remove_dir_all(&unpacked).unwrap();
        current = next;
    }
    no_leftovers(dir.path());
}

#[test]
fn single_files_recompress_and_unpack() {
    let dir = tempfile::tempdir().unwrap();
    let text = "line\n".repeat(50_000);
    fs::write(dir.path().join("log.txt"), &text).unwrap();
    let progress = Progress::default();
    let gz = dir.path().join("log.txt.gz");
    let items = [(dir.path().join("log.txt"), "log.txt".to_owned())];
    archive::pack(
        &items,
        Format::Single(Codec::Gz),
        fs::File::create(&gz).unwrap(),
        &progress,
    )
    .unwrap();
    fs::remove_file(dir.path().join("log.txt")).unwrap();
    assert_eq!(catalog::kind_of(&gz), Some(Kind::Compressed));

    let mut current = gz;
    for codec in &CODECS[1..] {
        let next = run(&current, Task::Convert(Format::Single(*codec)));
        assert_eq!(
            next,
            dir.path()
                .join(format!("log.txt.{}", Format::Single(*codec).ext()))
        );
        current = next;
    }
    let plain = run(&current, Task::Extract);
    assert_eq!(plain, dir.path().join("log.txt"));
    assert_eq!(fs::read_to_string(&plain).unwrap(), text);

    // Into an archive, holding the one file.
    let zipped = run(&current, Task::Convert(Format::Zip));
    assert_eq!(zipped, dir.path().join("log.txt.zip"));
    let unzipped = run(&zipped, Task::Extract);
    assert_eq!(unzipped, dir.path().join("log 2.txt"));
    assert_eq!(fs::read_to_string(unzipped).unwrap(), text);
    no_leftovers(dir.path());
}

#[cfg(feature = "rar")]
#[test]
fn rar_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let rar = dir.path().join("heart.rar");
    fs::copy("tests/fixtures/heart.rar", &rar).unwrap();
    assert_eq!(catalog::kind_of(&rar), Some(Kind::Archive));
    let file = run(&rar, Task::Extract);
    assert_eq!(file, dir.path().join("unicodefilename❤️.txt"));
    assert_eq!(fs::read_to_string(&file).unwrap(), "foobar\n");

    let xz = run(&rar, Task::Convert(Format::Tar(Some(Codec::Xz))));
    let again = run(&xz, Task::Extract);
    assert_eq!(again, dir.path().join("unicodefilename❤️ 2.txt"));
    assert_eq!(fs::read_to_string(again).unwrap(), "foobar\n");

    let locked = dir.path().join("locked.rar");
    fs::copy("tests/fixtures/locked.rar", &locked).unwrap();
    let err = convert::run(&locked, Task::Extract, &Progress::default()).unwrap_err();
    assert!(err.to_string().contains("password"), "{err}");
    no_leftovers(dir.path());
}

#[test]
fn entries_cannot_escape() {
    let dir = tempfile::tempdir().unwrap();
    let inside = dir.path().join("inside");
    fs::create_dir(&inside).unwrap();

    let zip_path = inside.join("evil.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&zip_path).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    for name in ["../escaped.txt", "/absolute.txt", "fine.txt"] {
        zip.start_file(name, options).unwrap();
        zip.write_all(b"x").unwrap();
    }
    zip.finish().unwrap();
    let unpacked = run(&zip_path, Task::Extract);
    assert!(!dir.path().join("escaped.txt").exists());
    assert!(unpacked.join("absolute.txt").is_file());
    assert!(unpacked.join("fine.txt").is_file());

    let tar_path = inside.join("evil.tar");
    let mut tar = tar::Builder::new(fs::File::create(&tar_path).unwrap());
    for name in ["../escaped.txt", "fine.txt"] {
        let mut header = tar::Header::new_gnu();
        header.as_old_mut().name[..name.len()].copy_from_slice(name.as_bytes());
        header.set_size(1);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append(&header, &b"x"[..]).unwrap();
    }
    tar.finish().unwrap();
    drop(tar);
    run(&tar_path, Task::Extract);
    assert!(!dir.path().join("escaped.txt").exists());
    assert!(inside.join("fine.txt").is_file());
}

#[test]
fn damaged_files_fail_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["broken.zip", "broken.7z", "broken.tar.xz", "broken.gz"] {
        let path = dir.path().join(name);
        fs::write(&path, b"this is not what the name says").unwrap();
        assert!(
            convert::run(&path, Task::Extract, &Progress::default()).is_err(),
            "{name}"
        );
        assert!(
            convert::run(&path, Task::Convert(Format::Zip), &Progress::default()).is_err(),
            "{name}"
        );
    }
    let names: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 4, "{names:?}");
}

#[test]
fn unnamed_archives_are_sniffed() {
    let dir = tempfile::tempdir().unwrap();
    let folder = sample(dir.path());
    let packed = run(&folder, Task::Convert(Format::Tar(Some(Codec::Zst))));
    let backup = dir.path().join("backup");
    fs::rename(&packed, &backup).unwrap();
    assert_eq!(Format::detect(&backup), Some(Format::Tar(Some(Codec::Zst))));
    assert_eq!(catalog::kind_of(&backup), Some(Kind::Archive));
    let unpacked = run(&backup, Task::Extract);
    assert_eq!(snapshot(&unpacked), snapshot(&folder));
}

#[test]
fn the_wheel_offers_archive_targets() {
    let dir = tempfile::tempdir().unwrap();
    let folder = sample(dir.path());
    let zip = run(&folder, Task::Convert(Format::Zip));
    let labels = |files: &[PathBuf], mode| {
        catalog::choices(files, mode)
            .iter()
            .map(|c| c.label)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        labels(std::slice::from_ref(&zip), Mode::Convert),
        [
            "Unzip", "7Z", "TAR.GZ", "TAR.XZ", "TAR.ZST", "TAR.BZ2", "TAR"
        ]
    );
    assert_eq!(
        labels(std::slice::from_ref(&folder), Mode::Convert),
        ["ZIP", "7Z", "TAR.GZ", "TAR.XZ", "TAR.ZST", "TAR.BZ2", "TAR"]
    );
    assert!(labels(std::slice::from_ref(&zip), Mode::Tools).is_empty());
    for choice in catalog::choices(&[zip.clone(), folder.clone()], Mode::Convert) {
        for file in [&zip, &folder] {
            if jobs::applies(file, choice) {
                assert!(
                    convert::task(file, choice).is_some(),
                    "{} has no real {}",
                    file.display(),
                    choice.label
                );
            }
        }
    }
}

#[test]
fn jobs_run_in_the_background() {
    let dir = tempfile::tempdir().unwrap();
    let folder = sample(dir.path());
    let choice = Choice {
        mode: Mode::Convert,
        label: "TAR.ZST",
        hint: "",
    };
    let started = jobs::start(&[folder], choice, 1, Instant::now());
    let [job] = &started[..] else {
        panic!("one job")
    };
    assert!(!job.is_simulated());
    let deadline = Instant::now() + Duration::from_secs(30);
    while job.finished_at(Instant::now()).is_none() {
        assert!(Instant::now() < deadline, "job never finished");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(job.error(), None);
    assert_eq!(job.message().as_deref(), Some("Saved as Photos.tar.zst"));
    assert_eq!(job.progress(Instant::now()), 1.0);
}

fn have(tool: &str) -> bool {
    Command::new(tool).arg("--help").output().is_ok()
}

fn sh(cmd: &mut Command) {
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "{cmd:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// What we write, the usual tools read; what they write, we read.
#[test]
fn interoperates_with_system_tools() {
    let dir = tempfile::tempdir().unwrap();
    let folder = sample(dir.path());
    let want = snapshot(&folder);
    let mut checked = Vec::new();

    if have("tar") {
        for (format, flag) in [
            (Format::Tar(Some(Codec::Gz)), "z"),
            (Format::Tar(Some(Codec::Xz)), "J"),
            (Format::Tar(Some(Codec::Bz2)), "j"),
        ] {
            let ours = run(&folder, Task::Convert(format));
            let out = dir.path().join(format!("tar-{flag}"));
            fs::create_dir(&out).unwrap();
            sh(Command::new("tar")
                .arg(format!("-x{flag}f"))
                .arg(&ours)
                .arg("-C")
                .arg(&out));
            assert_eq!(
                snapshot(&out.join("Photos")),
                want,
                "tar reading our {format:?}"
            );

            let theirs = dir.path().join(format!("theirs.{}", format.ext()));
            sh(Command::new("tar")
                .arg(format!("-c{flag}f"))
                .arg(&theirs)
                .arg("-C")
                .arg(dir.path())
                .arg("Photos"));
            let unpacked = run(&theirs, Task::Extract);
            assert_eq!(snapshot(&unpacked), want, "our reading of tar's {format:?}");
            fs::remove_dir_all(unpacked).unwrap();
        }
        checked.push("tar");
    }
    if have("zstd") {
        let ours = run(&folder, Task::Convert(Format::Tar(Some(Codec::Zst))));
        sh(Command::new("zstd").arg("-tq").arg(&ours));
        checked.push("zstd");
    }
    if have("unzip") {
        let ours = run(&folder, Task::Convert(Format::Zip));
        let out = dir.path().join("unzip");
        sh(Command::new("unzip")
            .arg("-q")
            .arg(&ours)
            .arg("-d")
            .arg(&out));
        assert_eq!(snapshot(&out.join("Photos")), want, "unzip reading ours");
        checked.push("unzip");
    }
    if have("zip") {
        let theirs = dir.path().join("theirs.zip");
        sh(Command::new("zip")
            .arg("-qry")
            .arg(&theirs)
            .arg("Photos")
            .current_dir(dir.path()));
        let unpacked = run(&theirs, Task::Extract);
        assert_eq!(snapshot(&unpacked), want, "our reading of zip's");
        fs::remove_dir_all(unpacked).unwrap();
        checked.push("zip");
    }
    if have("7z") {
        let ours = run(&folder, Task::Convert(Format::SevenZip));
        let out = dir.path().join("7z-out");
        sh(Command::new("7z")
            .arg("x")
            .arg("-bso0")
            .arg("-snl")
            .arg(format!("-o{}", out.display()))
            .arg(&ours));
        assert_eq!(snapshot(&out.join("Photos")), want, "7-Zip reading ours");

        let theirs = dir.path().join("theirs.7z");
        sh(Command::new("7z")
            .arg("a")
            .arg("-bso0")
            .arg("-snl")
            .arg(&theirs)
            .arg("Photos")
            .current_dir(dir.path()));
        let unpacked = run(&theirs, Task::Extract);
        assert_eq!(snapshot(&unpacked), want, "our reading of 7-Zip's");
        checked.push("7z");
    }
    eprintln!("checked against: {}", checked.join(", "));
}

/// Python's zipfile, like Windows, writes only the local DOS time, no UTC field.
#[test]
fn zips_without_utc_times_read_as_local_time() {
    if !have("python3") {
        eprintln!("no python3, skipped");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let zip = dir.path().join("py.zip");
    let script = "import sys, time, zipfile
info = zipfile.ZipInfo('a.txt', date_time=time.localtime(1600000000)[:6])
with zipfile.ZipFile(sys.argv[1], 'w') as z:
    z.writestr(info, 'x')";
    sh(Command::new("python3")
        .arg("-I")
        .arg("-c")
        .arg(script)
        .arg(&zip));
    let file = run(&zip, Task::Extract);
    let mtime = fs::metadata(file).unwrap().modified().unwrap();
    assert_eq!(
        mtime
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        NOTES_TIME
    );
}

#[test]
fn damaged_compression_under_a_tar_is_noticed() {
    let dir = tempfile::tempdir().unwrap();
    let folder = sample(dir.path());
    let packed = run(&folder, Task::Convert(Format::Tar(Some(Codec::Gz))));
    // The gzip trailer: CRC-32, then the size. The tar inside ends before it.
    let mut bytes = fs::read(&packed).unwrap();
    let crc = bytes.len() - 8;
    bytes[crc] ^= 0xFF;
    fs::write(&packed, bytes).unwrap();
    assert!(convert::run(&packed, Task::Extract, &Progress::default()).is_err());
    assert!(convert::run(&packed, Task::Convert(Format::Zip), &Progress::default()).is_err());
    no_leftovers(dir.path());
}

#[cfg(unix)]
#[test]
fn a_linked_folder_is_packed_by_its_contents() {
    let dir = tempfile::tempdir().unwrap();
    let folder = sample(dir.path());
    let link = dir.path().join("Shortcut");
    std::os::unix::fs::symlink(&folder, &link).unwrap();
    assert_eq!(catalog::kind_of(&link), Some(Kind::Folder));
    let packed = run(&link, Task::Convert(Format::Zip));
    assert_eq!(packed, dir.path().join("Shortcut.zip"));
    let unpacked = run(&packed, Task::Extract);
    assert_eq!(snapshot(&unpacked), snapshot(&folder));
}

#[cfg(unix)]
#[test]
fn unpacked_modes_are_tidied() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let tar_path = dir.path().join("modes.tar");
    let mut tar = tar::Builder::new(fs::File::create(&tar_path).unwrap());
    for (name, mode) in [
        ("open.sh", 0o4777),
        ("locked.txt", 0o000),
        ("shared.txt", 0o666),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(1);
        header.set_mode(mode);
        tar.append_data(&mut header, name, &b"x"[..]).unwrap();
    }
    tar.into_inner().unwrap();
    let unpacked = run(&tar_path, Task::Extract);
    let mode = |n: &str| fs::metadata(unpacked.join(n)).unwrap().permissions().mode() & 0o7777;
    assert_eq!(mode("open.sh"), 0o755, "no setuid, no group/other write");
    assert_eq!(mode("locked.txt"), 0o400, "readable by its owner");
    assert_eq!(mode("shared.txt"), 0o644);
    // And it can be packed again.
    run(&tar_path, Task::Convert(Format::Zip));
}

#[cfg(unix)]
#[test]
fn pipes_named_like_archives_are_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join("stream.gz");
    sh(Command::new("mkfifo").arg(&fifo));
    assert_eq!(catalog::kind_of(&fifo), None);
    let choice = Choice {
        mode: Mode::Convert,
        label: catalog::UNZIP,
        hint: "",
    };
    assert_eq!(convert::task(&fifo, choice), None);
}

#[test]
fn jobs_finishing_together_keep_both_results() {
    let dir = tempfile::tempdir().unwrap();
    let folder = sample(dir.path());
    let choice = Choice {
        mode: Mode::Convert,
        label: "ZIP",
        hint: "",
    };
    let now = Instant::now();
    let batches: Vec<_> = (0..4)
        .map(|batch| jobs::start(std::slice::from_ref(&folder), choice, batch, now))
        .collect();
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut made: Vec<String> = Vec::new();
    for job in batches.iter().flatten() {
        while job.finished_at(Instant::now()).is_none() {
            assert!(Instant::now() < deadline, "job never finished");
            std::thread::sleep(Duration::from_millis(10));
        }
        made.push(job.message().unwrap());
    }
    made.sort();
    assert_eq!(
        made,
        [
            "Saved as Photos 2.zip",
            "Saved as Photos 3.zip",
            "Saved as Photos 4.zip",
            "Saved as Photos.zip"
        ]
    );
    no_leftovers(dir.path());
}

#[test]
fn a_vanished_file_fails_instead_of_pretending() {
    let dir = tempfile::tempdir().unwrap();
    let gone = dir.path().join("gone.zip");
    let choice = Choice {
        mode: Mode::Convert,
        label: "7Z",
        hint: "",
    };
    let started = jobs::start(&[gone], choice, 1, Instant::now());
    let [job] = &started[..] else {
        panic!("one job")
    };
    assert!(!job.is_simulated());
    let deadline = Instant::now() + Duration::from_secs(10);
    while job.finished_at(Instant::now()).is_none() {
        assert!(Instant::now() < deadline, "job never finished");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(job.error().as_deref(), Some("it isn't there any more"));
}
