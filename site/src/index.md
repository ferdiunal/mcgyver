# McGyver: drag a file, hold Shift, pick a wedge

McGyver opens a wheel around your pointer while you drag files. Drop them on a wedge: pictures change format, videos shrink, PDFs turn into Word files, folders get zipped. Conversions run on your computer, not in a cloud.

Version 1.0.0, the first stable release. For KDE Plasma 6, macOS and Windows.

## One wheel, seven modes

The ring around the centre switches between them. Stop on a slice, or move along the ring into it.

### With files

- **Convert.** The wheel opens on Convert. Aim at a format and let go: holiday.webp lands next to holiday.jpg. Pictures, video, audio, PDFs, Office files, archives and subtitles each get their own list.
- **Tools.** Compress, trim, mute or rotate a video, or pull stills out of it. Pictures resize, crop and lose their EXIF; PDFs split and rotate, and two or more merge into one.
- **Actions.** Zip it, copy or move it somewhere, copy its path, open a terminal there, or send it to your phone, WhatsApp, Telegram, Slack or Teams.

### Without files

Hold Shift on its own for a moment and the wheel is for the computer instead.

- **Apps.** What is open, the one you used last on top, then the apps you picked in the settings. Alt+Tab, in a circle.
- **Media.** Whatever is playing: play or pause, next, previous, repeat, shuffle, and the player itself.
- **Everyday.** Keyboard layout, the computer's language, volume, brightness, screenshots, screen recording, and how busy the computer is.
- **System.** Lock, Dark Mode, Stay Awake, the microphone, the trash and sleep.

## What each file gets

The wheel only offers what the files in your hand can become. A file already in a format skips it; a mode with nothing to offer has no slice.

| File | Convert to | Tools |
| --- | --- | --- |
| Pictures (JPEG, PNG, HEIC, camera RAW, PSD, SVG…) | JPG · PNG · WEBP · HEIC · AVIF · TIFF · GIF · BMP · ICO · PDF | Compress · Resize · Crop · Rotate · Strip EXIF |
| Video (needs FFmpeg) | MP4 · MOV · MKV · WEBM · GIF · MP3 | Compress · Trim · Mute · Rotate · Frames |
| Audio (needs FFmpeg) | MP3 · M4A · WAV · FLAC · OGG · OPUS · AIFF | Trim · Normalize · Compress · Merge |
| PDF | DOCX · PPTX · XLSX · JPG · PNG · EPUB · TXT · MD | Compress · Split · Rotate · Merge |
| Documents (DOCX, DOC, ODT, RTF) | PDF · DOCX · ODT · RTF · HTML · TXT · MD | Merge |
| Spreadsheets (XLSX, XLS, ODS, CSV) | PDF · XLSX · CSV · TSV · HTML · MD | Merge |
| Slides (PPTX; PPT and ODP to MD) | PDF · PPTX · TXT · MD | Compress |
| Archives (ZIP, 7Z, RAR, TAR…) | Unzip · ZIP · 7Z · TAR.GZ · TAR.XZ · TAR.ZST · TAR.BZ2 · TAR | — |
| Compressed files (GZ, XZ, ZST, BZ2, LZ4) | Unzip · GZ · XZ · ZST · BZ2 · LZ4 · ZIP · 7Z | — |
| Subtitles (SRT, VTT) | VTT · SRT · TXT | Shift by half a second or a second |
| Folders | ZIP · 7Z · TAR.GZ · TAR.XZ · TAR.ZST · TAR.BZ2 · TAR | Flatten |

Every file and folder also gets **Actions**: Zip, Copy, Cut, Copy To, Move To, Copy Path, Terminal and Share. Merge appears once two or more files are in hand. HEIC and AVIF appear where the computer's libheif can write them.

## Get McGyver

One download, then it waits in the tray or the menu bar until you hold Shift.

- **KDE Plasma 6.** Linux, on Wayland. Where McGyver started. After the first start, press *Set up* in its settings, so the wheel opens the moment you press Shift during a drag. [Download the .deb](https://mcgyver.ferdiunal.tr/download/deb) for Debian and Ubuntu, or [the .rpm](https://mcgyver.ferdiunal.tr/download/rpm) for Fedora.
- **macOS.** macOS 11 or later. One app for Apple Silicon and Intel. Open the disk image and the app offers to move itself to Applications. It isn't notarized yet: the first time, Control-click it and choose *Open* (from macOS 15 on, *Open Anyway* in Privacy & Security). [Download for Mac](https://mcgyver.ferdiunal.tr/download/macos).
- **Windows.** Windows 10 and 11, on x64 and ARM64 PCs. On a PC without a graphics driver, like a virtual machine, it draws with Mesa's software OpenGL, which comes in the zip. SmartScreen asks once: *More info*, then *Run anyway*. [Download for x64 PCs](https://mcgyver.ferdiunal.tr/download/windows-x64), or [for ARM64](https://mcgyver.ferdiunal.tr/download/windows-arm64).

## Privacy

Once a day the app asks mcgyver.ferdiunal.tr whether a new version is out. That request also carries the app version, the operating system and its version, the processor type, the app's language, and a machine ID that is a one-way hash of the system's own ID (it can't be turned back into it). The server also sees the IP address and the country it comes from, and stores them with that ID.

No files, file names, or what you do with the wheel ever leave the computer. To keep the app from asking at all, start it with `MCGYVER_OFFLINE=1` in its environment.

## Free to use, kept going by sponsors

McGyver is built by Ferdi ÜNAL. If it saves you a few clicks a day, [sponsoring it on GitHub](https://github.com/sponsors/ferdiunal) keeps the work going.

- [Ferdi on GitHub](https://github.com/ferdiunal)
- [ferdiunal.tr](https://ferdiunal.tr/)
- [mail@ferdiunal.tr](mailto:mail@ferdiunal.tr)

McGyver 1.0.0 · MIT or Apache-2.0 · © 2026 The McGyver authors

[Türkçe](https://mcgyver.ferdiunal.tr/tr/index.md)
