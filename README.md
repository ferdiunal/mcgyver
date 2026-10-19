<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/logo-on-dark.svg">
  <img alt="McGyver" src="assets/logo.svg" height="96">
</picture>

# McGyver (prototype)

A file wheel for KDE Plasma 6, macOS and Windows, written in Rust with
[egui](https://github.com/emilk/egui). Drag files in your file manager and hold Shift: a wheel
opens around the cursor with what those files can become: other formats, smaller, resized,
merged, zipped, shared. Drop them on a wedge and it's done, without opening another app.

**This is a prototype.** Done for real:
- [pictures](#pictures)
- image tools (compress, resize, square crop, rotate, strip EXIF)
- [compressed files and folders](#compressed-files)
- audio and video format conversions (with FFmpeg installed)
- [office documents and PDFs](#office-documents-and-pdfs) without LibreOffice: Word, Excel and
  PowerPoint files to PDF and to other formats of their own kind (Word to ODT, Excel to CSV…); PDFs
  compressed, rotated, split and merged
- Word (DOCX, DOC, ODT, RTF), Excel (XLSX, XLS, ODS, CSV), PowerPoint, EPUB and text PDFs to
  Markdown (MD), with
  [anydoc](https://github.com/firecrawl/anydoc): pure Rust, nothing leaves the machine; a scanned
  PDF says it has no text, since OCR would mean a cloud service
- every [action](#actions)

Media editing, PDF conversions other than to Markdown, document and spreadsheet Merge, and
presentation Compress are still simulated. It runs only on **KDE Plasma 6 (Wayland)** for now.

## Run

```sh
cargo run --release                # the app, in the background with a tray icon; MCGYVER_DEBUG=1 logs
cargo run --release -- --settings  # the settings window (also from the tray, or by starting it again)
cargo run --release -- --quit      # quits the running app
cargo run --release -- --install   # once: wheel opens at once; tapping Shift opens it without a drag
```

Packages for the Snap Store, Debian/Ubuntu and Fedora: see [PACKAGING.md](PACKAGING.md).

### Tray and settings

The app runs in the background with an icon in Plasma's system tray. Its menu has **Pause** (the
trigger key does nothing until unpaused; the icon fades), **Settings…** and **Quit**; clicking the
icon opens the settings too. Starting the app while it runs opens its settings instead of a
second copy (it holds the session-bus name `io.github.McGyver`), and only one settings
window opens at a time. Quit waits for running conversions to finish.

The settings window saves each change at once, to
`~/.config/mcgyver/settings.conf`, and the running app picks it up straight away:

- **Language:** English or Türkçe; the first time, the desktop's language. The wheel, the cards,
  the tray menu and the settings change; format names, file names and an engine's own error
  messages stay as they are.
- **Trigger key:** Shift (the default), Ctrl or Alt: held during a drag it opens the wheel, tapped
  on its own it opens the wheel for Dolphin's selection. Changing it also moves the KDE shortcut
  for the tap. Meta isn't offered: Plasma opens its launcher when Meta is tapped.
- **Start at login:** writes `~/.config/autostart/mcgyver.desktop`.
- **Permissions:** what KWin and KDE actually granted the running app, checked every second,
  each with the button that fixes it. "Opens at once under a drag" is KWin's fake input;
  "Opens when the trigger key is tapped alone" is the KDE shortcut. Not set up → **Set up**
  (what `--install` does, below); set up but not granted yet → **Restart**; still not granted
  after that restart → "Still not allowed" and **Set up** again; app not running → **Start**;
  key held by another shortcut → **Open shortcuts** (Plasma's page) and **Try again**; a
  compositor that can't share Dolphin's selection → said so, nothing to press. **Remove** is
  `--uninstall`. While something is missing the card comes first; once
  all is granted it moves to the bottom and says so. KWin reads a new desktop file a few
  seconds late, so Set up and Remove wait until a fresh Wayland connection shows the change
  before restarting the app (once nothing is converting). KDE turned out to give a modifier
  tapped alone to two shortcuts at once, so "held by another shortcut" may never show there.

The rest of this README says Shift for the trigger key.

`--install` does two things, both undone by `--uninstall`:

- It writes `~/.local/share/applications/mcgyver-kwin.desktop` so KWin offers
  this binary its fake-input protocol (see below). If the binary moves, the app rewrites it when
  it starts (a snap's points at its `current` revision). Without it, the wheel opens as soon as
  the mouse moves after you press the key.
- It binds a KDE global shortcut, "McGyver", to the trigger key on its own. It shows up in
  System Settings → Shortcuts, but the trigger key in the app's settings wins: the app binds it
  again when it starts.

Start dragging a file, then hold Shift. The wheel opens on Convert; the ring around its centre
switches between the three modes:

| Mode | Wheel |
|------|-------|
| Convert | To another format; Unzip first for archives |
| Tools | Compress, trim, rotate, split, merge… |
| Actions | Zip, copy, move, copy path, terminal, share |

- **The mode ring:** the hub is a ring of three slices around a white centre: Convert at the top,
  Tools at the lower right, Actions at the lower left. The current mode is raised white. Stop on
  another slice, or move along the ring into it, and the wheel switches to that mode at once, so
  you can start with Shift on a picture and still zip it. Passing over a slice on the way out to
  a wedge (or back in) doesn't switch. After a switch, move a little before dropping.
- **Drop on an option wedge:** the wedge flashes and the wheel folds into a small card where you dropped.
  The card shows progress, then a check mark and the new file's name (or a red mark and the
  reason it failed), then fades. Simulated work says "simulated". A drop made while an earlier
  card is still up pushes that card upward; both stay.
- **Finished outputs:** real conversion and tool jobs ask the active Dolphin window, or the window
  already showing an output folder, to select the files they made. A batch sends one request for all
  successful outputs; simulated and failed jobs are skipped. If needed, Dolphin opens their folders
  in tabs in that same window. If no matching existing window is available, selection is skipped.
- **Drop on the centre, the ring or off the wheel:** the drag is cancelled.
- **The centre** shows the file's size (or how many files); while aiming at an option, the format
  or action you'd get.
- **Letting go of Shift** closes the wheel; the drag carries on, and pressing Shift again
  reopens it.
- **Only modes with options:** a mode with nothing for these files gets no slice, and the wheel
  opens on the first mode that has something. With three modes the ring is in thirds, with two
  in halves (the first on top), and with one there is no ring at all: a JSON file, say, opens
  straight on its Actions.
- **Sound:** the wheel opening, a mode switch, moving onto another wedge and a pick each play
  `assets/sounds/click.mp3` through `pw-play` (or `paplay`). `MCGYVER_SOUND=0` turns it off.
- **Merge** (documents, spreadsheets, PDFs, audio) shows up only with two or more files; a single
  document has no other tool, so it gets no Tools slice at all.
- **Mixed selections:** the wheel offers what every recognised file supports. A file already in
  the target format counts as supporting it. Unrecognised files are skipped.
- **Without a drag:** select files in Dolphin and tap Shift on its own (down and up, nothing else
  in between). The wheel opens at the pointer for those files; click an option, or click a ring
  slice to switch modes. A click anywhere else, a right click or another Shift tap closes it.
  Typing capitals and Shift-clicking don't count as taps.

## Pictures

Hold Shift over a picture to see JPG, PNG, WEBP, HEIC, AVIF, TIFF,
GIF, BMP, ICO and PDF. Its own format is left out, and so is any format this system can't write
(see HEIC below). The result goes next to the original, as for compressed files.

- **Read:**
  - JPEG, PNG, GIF, WebP, BMP, TIFF, ICO, TGA, QOI, PNM (`.ppm`, `.pgm`, `.pbm`, `.pam`), HDR,
    OpenEXR and DDS
  - HEIC/HEIF and AVIF
  - JPEG XL
  - SVG/SVGZ, drawn at 1024 px on the long side or more
  - Photoshop (PSD/PSB, the merged image)
  - camera RAW: CR2, CR3, NEF, ARW, DNG, RAF, ORF, RW2 and about 20 more, developed with
    rawler's defaults
- **Kept:**
  - Photos taken sideways come out upright, and their Exif no longer asks to turn them again.
  - The colour profile (a Display P3 iPhone photo stays P3) goes to JPG, PNG, WebP, TIFF, HEIC,
    AVIF and PDF.
  - Exif (date, camera, location) goes to JPG, PNG, WebP, HEIC and AVIF.
- **Choices:**
  - JPG uses quality 90. Transparency becomes white.
  - WebP is lossy (quality 82) from photos and lossless from graphics.
  - HEIC uses quality 80 and AVIF 70.
  - TIFF is lossless (Deflate) and keeps 16 bits.
  - ICO holds 16–256 px squares.
  - PDF is one page at 96 dpi. A JPEG goes in unchanged, other photos as JPEG, graphics
    losslessly with their transparency.
- **Tools:** Compress exports WebP; Resize caps the long edge at 2048 px; Crop makes a centered
  square; Rotate turns the image 90°; Strip EXIF removes the EXIF block while preserving the
  colour profile. Pixel edits keep the source format when it has a writer and otherwise export
  PNG. Each tool writes a separate file beside the original.
- **HEIC and AVIF** go through the system's libheif, loaded at run time, so building needs
  nothing. Fedora's libheif can't open or save HEIC (the HEVC codec is patented):
  `sudo dnf install libheif-freeworld` (RPM Fusion) adds it. Until then the app says so at
  start, and the HEIC wedge stays hidden. AVIF works with the stock plugins.
- **Safety:**
  - A picture may decode to at most 1 GiB. A file whose header claims more is refused, so a
    tiny crafted file can't exhaust memory.
  - An SVG may only use pictures from its own folder.
- **Not yet:** animated GIF and WebP keep only their first frame. Multi-page TIFF keeps only
  its first page.

## Office documents and PDFs

The engines of four open-source "craft" apps do this work, in pure Rust (MIT OR Apache-2.0), so
nothing needs to be installed:
[WordCraft](https://github.com/storytold/wordcraft),
[GridCraft](https://github.com/storytold/gridcraft),
[DeckCraft](https://github.com/storytold/deckcraft) and
[PdfCraft](https://github.com/storytold/pdfcraft).

| Drag | Convert | Tools |
|------|---------|-------|
| Word: DOCX, DOC, ODT, RTF, HTML, Markdown, TXT | PDF, DOCX, ODT, RTF, HTML, TXT; MD for DOCX, DOC, ODT, RTF | Merge (simulated) |
| Spreadsheet: XLSX, XLS, ODS, CSV, TSV | PDF, XLSX, CSV, TSV, HTML; MD for all but TSV | Merge (simulated) |
| Presentation: PPTX | PDF, PPTX, TXT (the outline), MD | Compress (simulated) |
| PDF | MD (the rest simulated) | Compress, Rotate, Split, Merge |

- **Names:** the result goes next to the original as for pictures: "report.pdf",
  "report (compressed).pdf", "report (rotated).pdf". Split makes a folder "report (pages)" holding
  "report 1.pdf", "report 2.pdf"…; Merge makes "first (merged).pdf" from the PDFs in the order
  they were dragged, with a bookmark for each, and skips files the wheel doesn't recognise.
- **PDF tools leave signed PDFs alone** (a signature field with a value, or a certified PDF):
  what they made would carry a broken signature. Split and Merge also leave password-protected
  PDFs alone, since the new files would come out unprotected. Compress fails rather than make a
  file that is no smaller, and a PDF whose page list is lost has it rebuilt from the pages found
  in the file. A failed Merge names the file that stopped it.
- **Spreadsheets:** CSV, TSV and HTML hold the active sheet; PDF prints every visible sheet with
  something on it. GridCraft prints at most 2000 pages, so a longer one fails instead of being
  cut short.
- **They are alpha software.** Layout is close to Office but not identical (their own estimates
  put them at 50–60% of the way to replacing Office), and a file one can't read fails with its
  message. Each reads the whole file into memory, so files over 512 MiB are refused, and so is a
  Merge of PDFs that are larger than that together.
- **Build:** they aren't on crates.io, so Cargo fetches them from GitHub at pinned commits. The
  first build takes a few minutes longer, and the release binary grows from about 47 MB to
  about 99 MB (79 MB stripped).
- **`vendor/epaint`:** WordCraft and DeckCraft switch on vello_cpu's multithreading, which
  breaks how egui's epaint 0.37.0 draws glyphs (it never flushes). The copy there draws them
  single-threaded; drop it once epaint does.

## Actions

The Actions slice of the ring opens the actions. They work on the whole selection, files and
folders alike:

| Action | What it does |
|--------|--------------|
| Zip | Packs everything into one ZIP next to it: `name.zip` for one item, `Archive.zip` for several |
| Copy, Move | Asks for a folder (kdialog or zenity) and copies or moves there. Names that are taken get a number. Copying into the same folder duplicates; moving there changes nothing. A move to another disk copies, syncs to disk, then removes the original |
| Copy Path | Puts the full paths on the clipboard, one per line (wl-copy, or Klipper) |
| Terminal | Opens Konsole (or the first terminal found) in the folder |
| Share | Sends the files to the first phone KDE Connect can reach |

An action only shows up if its tool is installed. Pinning a preview above other windows isn't
done yet.

## Compressed files

Archives, single compressed files and folders are converted for real. The result goes next to
the original. If that name is taken, a number is added ("photos 2.zip"). Work happens in a
hidden `.mcgyver-…` file or folder beside it and moves into place only when complete. A
failure cleans it up; only killing the app mid-way can leave one behind.

| Drag | Convert |
|------|---------|
| An archive | **Unzip**, ZIP, 7Z, TAR.GZ, TAR.XZ, TAR.ZST, TAR.BZ2, TAR |
| A compressed file | **Unzip**, GZ, XZ, ZST, BZ2, LZ4, or into a ZIP or 7Z |
| A folder | ZIP, 7Z, TAR.GZ, TAR.XZ, TAR.ZST, TAR.BZ2, TAR |

Archives have no tools, so their wheel has Convert and Actions only.

- **Read:**
  - ZIP (also `.jar`, `.apk`, `.cbz`…), 7Z (`.cb7`) and RAR (`.cbr`).
  - TAR on its own or compressed with gz, bz2, xz, zst, lz4, lzma, lz or br, including the short
    names (`.tgz`, `.tbz2`, `.txz`, `.tzst`, `.tlz`…).
  - The same eight codecs as single compressed files.
  - A file whose name says nothing (a `backup` with no extension, say) is recognised by its first
    bytes.
- **Written:** all of the above except RAR, whose format only RAR's own tools may write. The
  wheel shows the common targets; the rest (TAR.LZ4, TAR.LZMA, TAR.LZ, TAR.BR, LZMA, LZ, BR)
  work in code but have no wedge yet.
- **Unzip:** if the archive holds one file or folder at the top (or it's a `.gz` and the like),
  that goes next to the archive. Several go into a folder named after the archive.
- **Kept:** folders (empty ones too), the executable bit, modification times and symbolic links.
  Unpacked files end up as with `tar` and the usual umask: readable by their owner, writable by
  nobody else, never setuid.
- **Safety:**
  - Entry names that would leave the destination (`../…`) are skipped, and nothing is written
    through a link.
  - RAR hard links and file copies are made only from files the archive itself holds. unRAR
    alone would resolve them against the app's working folder.
  - Checksums are verified, including those after the end of a compressed tar.
  - An `.xz`/`.lzma` that asks for more than 1 GiB of memory is refused (`.lz` is capped at
    512 MiB by its format). Nothing limits how much disk an archive fills when unpacked.
- **Dragging a link to a folder** packs the folder it points to.
- **Changing only the compression** (TAR.GZ → TAR.ZST, GZ → XZ) streams the data without
  unpacking it.
- **Not yet:**
  - Password-protected archives fail with a message saying so.
  - File names that aren't valid UTF-8 are packed with `�` in place of the bad bytes.
  - PDF conversions other than to Markdown, and Merge for documents and spreadsheets, are still
    simulated.

Most image and archive processing uses Rust crates. WebP, zstd and unRAR include native code;
libheif is loaded at run time. Audio and video conversions use an installed `ffmpeg`. RAR support uses the official unRAR code,
whose licence allows use and sharing but not writing a RAR compressor. Build without it with
`cargo build --no-default-features`.

## How it works on KDE

No app can see another app's drag on Wayland. Plasma has two protocols that get around this
without root:

1. `org_kde_kwin_keystate` (v5) reports modifier presses to every client, whichever app has
   focus.
2. `zwlr_layer_shell_v1` lets a client put a fullscreen surface above everything.

As soon as Shift goes down, the app maps an invisible fullscreen overlay.

- **A file drag is in progress:** the drag now passes over the overlay. The compositor reports
  the position (`wl_data_device.enter`), and the app reads the dragged paths right away so the
  wheel can offer the right options.
- **No drag:** the pointer simply enters the overlay. The app removes the overlay again within
  a few milliseconds.

KWin only re-checks what is under a drag when the pointer moves, so a drag held still never
reaches the overlay. If neither answer comes within 15 ms of the overlay being mapped, the app
moves the pointer one pixel and back with `org_kde_kwin_fake_input` (up to three times), and the
drag finds the overlay at once. KWin only offers that protocol to programs whose desktop file
asks for it with `X-KDE-Wayland-Interfaces=org_kde_kwin_fake_input` and whose `Exec` is the
running binary, which is what `--install` sets up.

The app accepts the drag only while it points at a wedge. The cursor shows what will happen, and
releasing anywhere else cancels the drag. After a drop, the overlay's input region is emptied,
so the progress card lets clicks through.

A Shift tap without a drag comes from KDE's global shortcuts (KGlobalAccel, hosted by KWin),
which fire a modifier-only shortcut only when nothing else was pressed in between. The app
listens for the shortcut's D-Bus signal (`src/tap.rs`). Dolphin has no call that lists its
selection, so the app checks that the active Dolphin window's Copy action is enabled, triggers
it over D-Bus and reads the `text/uri-list` it puts on the clipboard. Before that it saves every
type the clipboard holds (text first, at most 0.4 s), and afterwards it serves them again,
through `ext-data-control` (`src/clipboard.rs`). The clipboard ends up as it was, with two
exceptions: Klipper's history keeps an entry for the copied files, and if the clipboard was
empty, Klipper may fill it with that entry. Then it maps the overlay; the pointer entering it says where to open the
wheel.

Rendering: one EGL context (glutin) and one `egui_glow` painter, drawn onto the layer surface
with [smithay-client-toolkit](https://github.com/Smithay/client-toolkit). There is no winit.
Quality: the context asks for 4× multisampling, since egui doesn't anti-alias the wedges' meshes
itself; the overlay draws at twice the screen's resolution on a 1× screen and KWin averages it
down, which smooths text as well; dithering keeps gradients and shadows free of banding; and
glyphs sit on whole pixels instead of egui's quarter-pixel positions.

The look (`src/glass.rs`):

- **Wheel:** a frosted light grey frame holding white, rounded wedges with gaps between them,
  around the hub: a grey mode ring, its current slice raised white, around a white centre.
- **Accent:** orange, only for the wedge you aim at.
- **Labels:** in Noto Sans Bold, bundled in `assets/fonts` under the OFL.

A client can't read the pixels behind it, so the compositor does the blur. Each frame the
overlay reports where it drew glass: the disc, the caption and the progress card. That region
goes to `ext_background_effect_v1`, approximated by 2 px horizontal bands. Compositors without
that protocol get the same look without the blur.

GNOME has neither protocol. macOS, Windows and X11 can do the same with their own APIs: poll the
modifier state, detect a drag, and show a topmost window at the cursor. None of that is written
yet.

## UI development

The overlay UI (`src/overlay.rs`, `src/wheel.rs`) doesn't depend on the platform.
`examples/preview.rs` renders it in an ordinary window with a scripted drag:

```sh
cargo run --example preview -- hover     # wheel|hover|tools|actions|mixed|dwell|card|cards|archive|folder|failed|settings
EFRAME_SCREENSHOT_TO=out.png MCGYVER_LANG=tr WAYLAND_DISPLAY= xvfb-run -a cargo run --example preview -- card
```

`MCGYVER_LANG=tr` shows it in Turkish; `settings` opens the settings window. Clear `WAYLAND_DISPLAY`
for screenshots, or the window opens on the desktop rather than under xvfb.

### End-to-end test

`dev/e2e/run.sh` tests the real path headlessly:

1. Starts a nested KWin (`kwin_wayland --virtual`) with its own D-Bus and config directories.
2. A GTK window drags a file.
3. KWin's fake-input protocol moves the pointer and presses the modifiers, following
   `dev/e2e/scenarios/full.txt` (or another scenario: `run.sh still` presses Shift with the mouse
   held still, `FILE=photos.zip run.sh archive` converts and unpacks a real ZIP, and
   `run.sh real` turns a JPEG into a PNG, zips it, copies its path and opens Konsole,
   `run.sh ring` switches from Convert to Actions on the mode ring and zips the file, and
   `SOURCE=dolphin run.sh tap` selects a file in Dolphin, taps Shift, clicks PNG and saves the
   clipboard afterwards in `out/clipboard.txt`, which should read "before the tap", and
   `run.sh settings` starts the app a second time (its settings open instead), switches the
   trigger to Ctrl and the language to Turkish, checks that Shift no longer opens the wheel and
   Ctrl does, pauses from the tray menu and quits with `--quit`, and
   `PERMISSIONS=1 run.sh permissions` keeps KWin's permission checks on, as on a real desktop
   (the other runs switch them off), and sets up and removes the KDE integration from the
   settings' Permissions card).
4. spectacle takes screenshots.

Logs, screenshots and the files the app made go to `dev/e2e/out/`. This test caught a bug that only showed on NVIDIA:
the context was first made current without a surface, which left the draw buffer at
`GL_NONE`, so nothing was drawn.

## Layout

| File | What |
|------|------|
| `src/kde.rs` | KDE Wayland backend: keystate, layer-shell overlay, drag and drop, clicks after a Shift tap, EGL rendering |
| `src/tap.rs` | The trigger-key tap global shortcut (KGlobalAccel over D-Bus) |
| `src/tray.rs` | The tray icon and its menu (StatusNotifierItem, through ksni) |
| `src/service.rs` | The app's session-bus name: one instance, and what the settings window and `--quit` ask it |
| `src/settings.rs` | The settings file, the trigger keys, starting at login |
| `src/settings_window.rs` | The settings window (eframe), run as `--settings` |
| `src/i18n.rs` | English and Turkish text; everything shown is looked up there |
| `src/icon.rs` | The app icon (`assets/icon.svg`) drawn for the tray and the window |
| `assets/icon.svg`, `assets/logo.svg`, `assets/logo-on-dark.svg` | The icon (a paper clip bent into an M) and the logo with the name, its text turned into paths so it needs no font |
| `src/clipboard.rs` | Saving and restoring the clipboard with `ext-data-control` |
| `src/file_manager.rs` | Dolphin over D-Bus: select finished files, find and copy the active window's selection |
| `src/overlay.rs` | What the overlay shows: wheel state, drop handling, progress card |
| `src/wheel.rs` | Wheel geometry, hit testing, painting |
| `src/glass.rs` | Colours, fonts, the frosted frame and capsules, blur-region geometry |
| `src/catalog.rs` | What each file kind offers (formats, tools, actions) |
| `src/jobs.rs` | Runs the work: real jobs on a background thread, the rest simulated |
| `src/convert.rs` | What a wedge does to one file (picture, compressed file, folder); output names, scratch files |
| `src/images/` | Reading and writing pictures; libheif at run time (`heif`), WebP metadata (`webp`), PDF, PSD |
| `src/media.rs` | Audio and video conversion through FFmpeg |
| `src/office.rs` | Office documents and PDF tools through the WordCraft, GridCraft, DeckCraft and PdfCraft engines |
| `src/markdown.rs` | Documents, spreadsheets, presentations and PDFs to Markdown through anydoc |
| `src/actions.rs` | The Actions wheel: zip, copy, move, copy path, terminal, share |
| `src/archive/` | Reading and writing archives (`zip_io`, `sevenz_io`, `tar_io`, `rar_io`), codecs, safe unpacking |
| `tests/archives.rs` | Every format round trip, plus checks against tar, zip/unzip, 7z and zstd |
| `tests/images.rs` | Every picture target, orientation, profiles, Exif, crafted files; checks against ImageMagick; the Zip action |
| `tests/actions.rs` | Copy and Move through their real entry point, with a stand-in folder picker |
| `tests/documents.rs` | Word, CSV and PDF to Markdown from hand-built files; scanned and broken files |
| `tests/office.rs` | Word, spreadsheet and presentation conversions; PDF rotate, split, compress, merge |
