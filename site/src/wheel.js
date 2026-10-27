// McGyver's wheel, drawn in SVG with the app's geometry (src/wheel.rs) and its POP, SWELL and
// NUDGE springs (src/motion.rs). The page shows it on the laptop, in the walkthrough, beside the
// modes, in the demo and under the pointer while Shift is held: app.js makes each one with
// McWheel.create(svg) and says what it shows. Nothing here touches a file.
(() => {
  "use strict";

  const SVG = "http://www.w3.org/2000/svg";
  const TAU = Math.PI * 2;
  const still = window.matchMedia("(prefers-reduced-motion: reduce)");

  // ---------- Geometry, in the app's points ----------

  const HUB_R = 62;
  const INFO_R = 34;
  const MODE_GAP = 4;
  const MODE_CORNER = 7;
  const RING_R0 = 74;
  const RING_R1 = 160;
  const DISC_R = 172;
  const GAP = 7;
  const CORNER = 14;
  const FRAME_SWELL = 19;
  const WEDGE_LIFT = 8;
  const LOBE_MARGIN = 7;
  const WEDGE_SWELL = DISC_R - RING_R1 + FRAME_SWELL - LOBE_MARGIN;
  const LABEL_SWELL = (WEDGE_LIFT + WEDGE_SWELL) / 2;
  const UNFOLD_FROM = 0.6;
  const BUBBLE_R = 21;
  const BUBBLE_GAP = 7;
  const BUBBLE_AT = DISC_R + FRAME_SWELL + 6 + BUBBLE_R;
  const CAPTION_AT = DISC_R + 14 + 16;
  /**
   * A slice of the mode ring the pointer rests on this long becomes the mode. The app switches
   * after 50 ms (src/overlay.rs SETTLE) because it can tell a stop from a pass; this only times.
   */
  const MODE_DWELL = 260;

  // ---------- Springs (src/motion.rs) ----------

  const POP = { response: 0.36, damping: 0.62 };
  const SWELL = { response: 0.3, damping: 0.5 };
  const NUDGE = { response: 0.26, damping: 0.45 };
  /** The page's own: the raised slice of the mode ring going round. */
  const SLIDE = { response: 0.34, damping: 0.78 };

  /** Where a spring that started at `from` with `v`, heading for `to`, is `t` seconds later. */
  function springAt(feel, from, v, to, t) {
    if (t <= 0) return [from, v];
    const w = TAU / feel.response;
    const z = Math.min(Math.max(feel.damping, 0.05), 1);
    const a = from - to;
    const e = Math.exp(-z * w * t);
    if (z >= 1) {
      const b = v + w * a;
      return [to + e * (a + b * t), e * (b - w * (a + b * t))];
    }
    const wd = w * Math.sqrt(1 - z * z);
    const b = (v + z * w * a) / wd;
    const sin = Math.sin(wd * t);
    const cos = Math.cos(wd * t);
    return [
      to + e * (a * cos + b * sin),
      e * ((b * wd - z * w * a) * cos - (a * wd + z * w * b) * sin),
    ];
  }

  function settle(feel) {
    const z = Math.min(Math.max(feel.damping, 0.05), 1);
    return z >= 1 ? (6.64 * feel.response) / TAU : (4.6 * feel.response) / (TAU * z);
  }

  /** A value that springs toward wherever it is sent, without a jump when sent elsewhere halfway. */
  class Spring {
    constructor(feel, value = 0) {
      this.feel = feel;
      this.from = value;
      this.v = 0;
      this.to = value;
      this.since = 0;
    }
    value(now) {
      return springAt(this.feel, this.from, this.v, this.to, (now - this.since) / 1000)[0];
    }
    set(to, now, delay = 0) {
      if (to === this.to && !this.moving(now)) return;
      const [x, v] = springAt(this.feel, this.from, this.v, this.to, (now - this.since) / 1000);
      this.from = x;
      this.v = delay > 0 ? 0 : v;
      this.to = to;
      this.since = now + delay;
      if (still.matches) this.jump(to);
    }
    kick(now, velocity) {
      const [x] = springAt(this.feel, this.from, this.v, this.to, (now - this.since) / 1000);
      this.from = x;
      this.v = still.matches ? 0 : velocity;
      this.since = now;
    }
    jump(value) {
      this.from = value;
      this.to = value;
      this.v = 0;
      this.since = 0;
    }
    moving(now) {
      return (now - this.since) / 1000 < settle(this.feel) * 1.3;
    }
  }

  // ---------- Words: written in English, shown in Turkish on /tr/ (the app's own, src/i18n.rs) ----------

  const TURKISH = document.documentElement.lang === "tr";
  const TR = {
    // Modes, kinds of file, wedges and bubbles.
    Convert: "Dönüştür",
    Tools: "Araçlar",
    Actions: "Eylemler",
    Apps: "Uygulama",
    Media: "Medya",
    Everyday: "Günlük",
    System: "Sistem",
    "{} mode": "{} modu",
    Image: "Resim",
    Audio: "Ses",
    Document: "Belge",
    Archive: "Arşiv",
    Subtitles: "Altyazı",
    Share: "Paylaş",
    Phone: "Telefon",
    Copy: "Kopyala",
    Cut: "Kes",
    "Copy To": "Şuraya kopyala",
    "Move To": "Şuraya taşı",
    "Copy Path": "Yolu kopyala",
    Compress: "Sıkıştır",
    Resize: "Boyutlandır",
    Crop: "Kırp",
    Rotate: "Döndür",
    "Strip EXIF": "EXIF’i sil",
    Trim: "Kes",
    Mute: "Sessize al",
    Frames: "Kareler",
    Normalize: "Dengele",
    Split: "Böl",
    Merge: "Birleştir",
    Unzip: "Çıkar",
    Shift: "Kaydır",
    Files: "Dosyalar",
    Browser: "Tarayıcı",
    Music: "Müzik",
    Notes: "Notlar",
    Pause: "Duraklat",
    Play: "Oynat",
    Next: "Sonraki",
    Repeat: "Tekrarla",
    Player: "Oynatıcı",
    Shuffle: "Karıştır",
    Previous: "Önceki",
    Keyboard: "Klavye",
    Language: "Dil",
    Volume: "Ses",
    Brightness: "Parlaklık",
    Screenshot: "Ekran Görüntüsü",
    Record: "Ekran Kaydı",
    Monitor: "Sistem İzleyici",
    Area: "Alan",
    Screen: "Ekran",
    Window: "Pencere",
    Lock: "Kilitle",
    "Dark Mode": "Karanlık Mod",
    "Stay Awake": "Uyanık Kal",
    Microphone: "Mikrofon",
    "Empty Trash": "Çöpü Boşalt",
    Desktop: "Masaüstü",
    Sleep: "Uyku",
    On: "Açık",
    Off: "Kapalı",
    off: "kapalı",
    // The caption.
    "{} files": "{} dosya",
    "Move {} by {}": "{}, {} kaydırılacak",
    "Move {} in time": "{} zamanda kaydırılacak",
    "Copy {} to the clipboard": "{} panoya kopyalanacak",
    "Cut {}, to paste where it goes": "{} kesilecek, taşınacağı yere yapıştırın",
    "Terminal in {}": "{} içinde terminal",
    Downloads: "İndirilenler",
    Archive: "Arşiv",
    "unpacked here": "buraya çıkarıldı",
    "Copy {} to…": "{} kopyalanacak…",
    "Move {} to…": "{} taşınacak…",
    "Copy the path of {}": "Yolu kopyala: {}",
    "Send {} to your phone": "{} telefona gönderilecek",
    "Send {} with {}": "{} {} ile gönderilecek",
    "Switch to an app": "Bir uygulamaya geç",
    "Repeating all": "Tümü tekrarlanıyor",
    "Repeat is off": "Tekrar kapalı",
    "Shuffle is on": "Karıştırma açık",
    "Shuffle is off": "Karıştırma kapalı",
    Playing: "Çalıyor",
    Paused: "Duraklatıldı",
    "Track {} — Morning mix": "{}. parça — Sabah karışımı",
    "Next track": "Sonraki parça",
    "Previous track": "Önceki parça",
    "Open {}": "{} açılacak",
    "{} after you log in again": "{}, yeniden oturum açınca",
    "Volume {}": "Ses {}",
    Muted: "Ses kapalı",
    "Brightness {}": "Parlaklık {}",
    "Pick an area": "Bir alan seçin",
    "The whole screen": "Tüm ekran",
    "The window in front": "Öndeki pencere",
    "Record the screen": "Ekranı kaydet",
    "Processor {}": "İşlemci {}",
    "Memory {}": "Bellek {}",
    "Disk {} full": "Disk {} dolu",
    CPU: "İşlemci",
    RAM: "Bellek",
    // Windows' titles on the Apps wedges.
    Vacation: "Tatil",
    "Morning mix": "Sabah karışımı",
    Ideas: "Fikirler",
    "Lock the screen": "Ekranı kilitle",
    "The microphone is on": "Mikrofon açık",
    "The microphone is muted": "Mikrofon kapalı",
    "Empty the trash": "Çöp kutusunu boşalt",
    "Show the desktop": "Masaüstünü göster",
    "Put the computer to sleep": "Bilgisayarı uyut",
    "The wheel without files": "Dosyasız çark",
    "The wheel for {}": "{} için çark",
    " and ": " ve ",
    // The hint under the wheel.
    "Pick how far, on one of its bubbles.": "Ne kadar kayacağını balonlarından birinde seçin.",
    "{} would come to the front.": "{} öne gelirdi.",
    "The player would come to the front.": "Oynatıcı öne gelirdi.",
    "{}: {}. In the app, that is.": "{}: {}. Uygulamada, tabii.",
    "Pick one of its bubbles.": "Balonlarından birini seçin.",
    "The system monitor would open.": "Sistem İzleyici açılırdı.",
    "Locked. Well, it would be: this page is only a demo.": "Kilitlendi. Yani kilitlenirdi: bu sayfa yalnızca bir demo.",
    "The trash would be empty now.": "Çöp kutusu şimdi boş olurdu.",
    "Every window would step aside.": "Bütün pencereler kenara çekilirdi.",
    "The computer would go to sleep. The demo stays awake.": "Bilgisayar uyurdu. Demo uyanık kalıyor.",
    "Rest the file on the space a moment.": "Dosyayı bir an alanın üstünde tutun.",
    "Now hold Shift.": "Şimdi Shift’i basılı tutun.",
    "Dropped off the wheel: nothing happens.": "Çarkın dışına bırakıldı: bir şey olmaz.",
    "Rest the file on the space until the wheel opens.": "Çark açılana kadar dosyayı alanın üstünde tutun.",
    "Hold Shift while the file is over the space.": "Dosya alanın üstündeyken Shift’i basılı tutun.",
    "Let go of Shift on a wedge to do it.": "Yapmak için Shift’i bir dilimin üstünde bırakın.",
    "Tap a wedge, or drag a file onto the space.": "Bir dilime dokunun ya da bir dosyayı alana sürükleyin.",
    "Hover a wedge, then click it. Or drag a file onto the space and hold Shift.":
      "Bir dilimin üstüne gelip tıklayın. Ya da bir dosyayı alana sürükleyip Shift’i basılı tutun.",
    // The cards.
    "Sent to your phone": "Telefona gönderildi",
    "Through KDE Connect": "KDE Connect ile",
    "Pick a chat in Telegram": "Telegram’da bir sohbet seçin",
    "Copied: paste in {}": "Kopyalandı: {} içine yapıştırın",
    "Next to {}": "{} ile aynı klasörde",
    "Moved by {}": "{} kaydırıldı",
    "Unzipping {}": "{} çıkarılıyor",
    "Converting {}": "{} dönüştürülüyor",
    "Working on {}": "{} işleniyor",
    "A moment…": "Bir saniye…",
    Cancel: "İptal",
    "Put the card away": "Kartı kaldır",
    Cancelled: "İptal edildi",
    "Nothing was made": "Hiçbir şey yapılmadı",
  };

  /** `english` in the page's language, each `{}` replaced by the next of `args`. */
  function tr(english, ...args) {
    const text = (TURKISH && TR[english]) || english;
    let k = 0;
    return args.length ? text.replace(/\{\}/g, () => args[k++]) : text;
  }

  /** A share of something, as the app writes it in either language (src/desk/mod.rs percent). */
  const pct = (n) => `${n}%`;

  // ---------- What the wheel offers (the app's catalog, for these sample files) ----------

  const SHARE = { label: "Share", bubbles: ["Phone", "WhatsApp", "Telegram", "Slack", "Teams"] };
  const ACTIONS = ["Zip", "Copy", "Cut", "Copy To", "Move To", "Copy Path", "Terminal", SHARE];

  // Each scene is named by its file on the English page; the files it shows are the page's own.
  const SCENES = {
    "holiday.jpg": {
      files: TURKISH ? ["tatil.jpg"] : ["holiday.jpg"],
      kind: "Image",
      size: "2.4 MB",
      modes: {
        Convert: ["PNG", "WEBP", "TIFF", "GIF", "BMP", "ICO", "PDF"],
        Tools: ["Compress", "Resize", "Crop", "Rotate", "Strip EXIF"],
        Actions: ACTIONS,
      },
    },
    "trip.mov": {
      files: TURKISH ? ["gezi.mov"] : ["trip.mov"],
      kind: "Video",
      size: "184 MB",
      modes: {
        Convert: ["MP4", "MKV", "WEBM", "GIF", "MP3"],
        Tools: ["Compress", "Trim", "Mute", "Rotate", "Frames"],
        Actions: ACTIONS,
      },
    },
    "song.flac": {
      files: TURKISH ? ["şarkı.flac"] : ["song.flac"],
      kind: "Audio",
      size: "31 MB",
      modes: {
        Convert: ["MP3", "M4A", "WAV", "OGG", "OPUS", "AIFF"],
        Tools: ["Trim", "Normalize", "Compress"],
        Actions: ACTIONS,
      },
    },
    "report.pdf": {
      files: TURKISH ? ["rapor.pdf"] : ["report.pdf"],
      kind: "PDF",
      size: "1.2 MB",
      modes: {
        Convert: ["DOCX", "PPTX", "XLSX", "JPG", "PNG", "EPUB", "TXT", "MD"],
        Tools: ["Compress", "Split", "Rotate"],
        Actions: ACTIONS,
      },
    },
    "notes.docx": {
      files: TURKISH ? ["notlar.docx"] : ["notes.docx"],
      kind: "Document",
      size: "86 KB",
      modes: { Convert: ["PDF", "ODT", "RTF", "HTML", "TXT", "MD"], Actions: ACTIONS },
    },
    "photos.zip": {
      files: TURKISH ? ["fotolar.zip"] : ["photos.zip"],
      kind: "Archive",
      size: "48 MB",
      modes: {
        Convert: ["Unzip", "7Z", "TAR.GZ", "TAR.XZ", "TAR.ZST", "TAR.BZ2", "TAR"],
        Actions: ACTIONS,
      },
    },
    "film.srt": {
      files: ["film.srt"],
      kind: "Subtitles",
      size: "64 KB",
      modes: {
        Convert: ["VTT", "TXT"],
        Tools: [{ label: "Shift", bubbles: ["−1 s", "−0.5 s", "+0.5 s", "+1 s"] }],
        Actions: ACTIONS,
      },
    },
    chapters: {
      files: TURKISH ? ["bölüm-1.pdf", "bölüm-2.pdf"] : ["chapter-1.pdf", "chapter-2.pdf"],
      kind: "PDF",
      modes: {
        Convert: ["DOCX", "PPTX", "XLSX", "JPG", "PNG", "EPUB", "TXT", "MD"],
        Tools: ["Compress", "Split", "Merge", "Rotate"],
        Actions: ACTIONS,
      },
    },
    desk: {
      files: [],
      modes: {
        // The app shows each app's own name and icon, its window's title as the caption.
        Apps: [
          { label: "Files", tile: "F", title: "Vacation" },
          { label: "Browser", tile: "B", title: "McGyver" },
          { label: "Terminal", tile: "T", title: "~" },
          { label: "Music", tile: "M", title: "Morning mix" },
          { label: "Notes", tile: "N", title: "Ideas" },
        ],
        // Words only, as in the app; its player wedge carries the player's own name.
        Media: [
          { label: "Pause" },
          { label: "Next" },
          { label: "Repeat", toggle: "repeat" },
          { label: "Spotify" },
          { label: "Shuffle", toggle: "shuffle" },
          { label: "Previous" },
        ],
        Everyday: [
          { label: "Keyboard", bubbles: ["EN", "TR", "DE"], level: "layout" },
          { label: "Language", bubbles: ["EN", "TR"], level: "language" },
          { label: "Volume", bubbles: ["Mute", "25", "50", "75", "100"], level: "volume" },
          { label: "Brightness", bubbles: ["10", "25", "50", "75", "100"], level: "brightness" },
          { label: "Screenshot", bubbles: ["Area", "Screen", "Window"] },
          { label: "Record", bubbles: ["Area", "Screen"] },
          { label: "Monitor", bubbles: ["CPU", "RAM", "Disk"] },
        ],
        System: [
          { label: "Lock" },
          { label: "Dark Mode", toggle: "dark" },
          { label: "Stay Awake", toggle: "awake" },
          { label: "Microphone", toggle: "mic" },
          { label: "Empty Trash" },
          { label: "Desktop" },
          { label: "Sleep" },
        ],
      },
    },
  };

  /** The demo computer's switches and levels, as the System and Everyday wedges change them. */
  const desk = {
    playing: true,
    track: 3,
    repeat: false,
    shuffle: false,
    awake: false,
    mic: true,
    layout: TURKISH ? 1 : 0,
    language: TURKISH ? 1 : 0,
    volume: 3,
    brightness: 3,
  };
  /** The names behind the Keyboard and Language bubbles. */
  const LANGUAGES = { EN: "English", TR: "Türkçe", DE: "Deutsch" };

  const stemOf = (name) => name.replace(/\.[^.]+$/, "");
  const extOf = (name) => (name.match(/\.([^.]+)$/) || ["", ""])[1];
  const labelOf = (item) => (typeof item === "string" ? item : item.label);
  /** The label as the wedge shows it: Pause reads Play while nothing plays. */
  const shownLabel = (item) => tr(labelOf(item) === "Pause" && !desk.playing ? "Play" : labelOf(item));
  /** The page is dark, as the demo computer is. */
  const isDark = () => true;

  const TOOL_SUFFIX = {
    Compress: "compressed",
    Resize: "resized",
    Crop: "cropped",
    Rotate: "rotated",
    "Strip EXIF": "exif removed",
    Trim: "trimmed",
    Mute: "muted",
    Normalize: "normalized",
  };

  /** What the wheel's caption says: the files, or what aiming at `item` would make. */
  function caption(scene, mode, item, bubble) {
    const s = SCENES[scene];
    if (scene === "desk") return deskCaption(mode, item, bubble);
    const first = s.files[0];
    const what = s.files.length > 1 ? tr("{} files", s.files.length) : first;
    if (!item) return `${what} · ${tr(s.kind)}`;
    const label = labelOf(item);
    const stem = stemOf(first);
    const ext = extOf(first);
    if (mode === "Convert") {
      if (label === "Unzip") return `${what} › ${tr("unpacked here")}`;
      return `${what} › ${stem}.${label.toLowerCase()}`;
    }
    if (mode === "Tools") {
      if (label === "Merge") return `${what} › ${stem} (merged).pdf`;
      if (label === "Split") return `${what} › ${stem} (pages)`;
      if (label === "Frames") return `${what} › ${stem} (frames)`;
      if (label === "Shift") return bubble ? tr("Move {} by {}", what, bubble) : tr("Move {} in time", what);
      // Compressed pictures come out as WebP, and lossless audio as M4A (src/images, src/media.rs).
      let out = ext;
      if (label === "Compress" && s.kind === "Image") out = "webp";
      if (label === "Compress" && /^(flac|wav|aiff?)$/i.test(ext)) out = "m4a";
      return `${what} › ${stem} (${TOOL_SUFFIX[label] || label.toLowerCase()}).${out}`;
    }
    switch (label) {
      case "Zip": return `${what} › ${s.files.length > 1 ? tr("Archive") : first}.zip`;
      case "Copy": return tr("Copy {} to the clipboard", what);
      case "Cut": return tr("Cut {}, to paste where it goes", what);
      case "Copy To": return tr("Copy {} to…", what);
      case "Move To": return tr("Move {} to…", what);
      case "Copy Path": return tr("Copy the path of {}", what);
      case "Terminal": return tr("Terminal in {}", tr("Downloads"));
      default:
        if (!bubble || bubble === "Phone") return tr("Send {} to your phone", what);
        return tr("Send {} with {}", what, bubble);
    }
  }

  /** The demo computer's processor, memory and disk, as Monitor reads them. */
  const USAGE = { CPU: ["Processor {}", 12], RAM: ["Memory {}", 41], Disk: ["Disk {} full", 63] };

  /** What the wheel without files says, in the app's words (src/desk/mod.rs). */
  function deskCaption(mode, item, bubble) {
    const label = item ? labelOf(item) : null;
    const nowPlaying = tr("Track {} — Morning mix", desk.track);
    const usage = Object.values(USAGE).map(([words, n]) => tr(words, pct(n))).join(" · ");
    if (mode === "Apps") return item ? tr(item.title) : tr("Switch to an app");
    if (mode === "Media") {
      switch (label) {
        case "Repeat": return tr(desk.repeat ? "Repeating all" : "Repeat is off");
        case "Shuffle": return tr(desk.shuffle ? "Shuffle is on" : "Shuffle is off");
        case "Next": return tr("Next track");
        case "Previous": return tr("Previous track");
        case "Spotify": return tr("Open {}", label);
        default: return nowPlaying;
      }
    }
    if (mode === "Everyday") {
      const shot = { Area: "Pick an area", Screen: "The whole screen", Window: "The window in front" };
      const volume = (n) => (n === "Mute" ? tr("Muted") : tr("Volume {}", pct(n)));
      const levels = { volume: ["Mute", 25, 50, 75, 100], brightness: [10, 25, 50, 75, 100] };
      switch (label) {
        case "Keyboard": return LANGUAGES[bubble || item.bubbles[desk.layout]];
        case "Language": {
          const name = LANGUAGES[bubble || item.bubbles[desk.language]];
          return !bubble || item.bubbles.indexOf(bubble) === desk.language ? name : tr("{} after you log in again", name);
        }
        case "Volume": return volume(bubble || levels.volume[desk.volume]);
        case "Brightness": return tr("Brightness {}", pct(bubble || levels.brightness[desk.brightness]));
        case "Screenshot": return tr(shot[bubble || "Area"]);
        case "Record": return bubble ? tr(shot[bubble]) : tr("Record the screen");
        case "Monitor": return bubble ? tr(USAGE[bubble][0], pct(USAGE[bubble][1])) : usage;
        default: return `${volume(levels.volume[desk.volume])} · ${tr("Brightness {}", pct(levels.brightness[desk.brightness]))}`;
      }
    }
    switch (label) {
      case "Lock": return tr("Lock the screen");
      case "Dark Mode": return `${tr("Dark Mode")} · ${tr(isDark() ? "On" : "Off")}`;
      case "Stay Awake": return `${tr("Stay Awake")} · ${tr(desk.awake ? "On" : "Off")}`;
      case "Microphone": return tr(desk.mic ? "The microphone is on" : "The microphone is muted");
      case "Empty Trash": return tr("Empty the trash");
      case "Desktop": return tr("Show the desktop");
      case "Sleep": return tr("Put the computer to sleep");
      default: return usage;
    }
  }

  /** What the centre of the hub says: the aimed wedge, else the file's size or the desk's state. */
  function centreText(scene, mode, item, bubble) {
    const word = bubble && tr(bubble);
    if (item) return word && word.length <= 9 ? word : shownLabel(item);
    if (scene === "desk") {
      if (mode === "Media") return tr(desk.playing ? "Playing" : "Paused");
      if (mode === "Everyday") return SCENES.desk.modes.Everyday[0].bubbles[desk.layout];
      return tr(mode);
    }
    const s = SCENES[scene];
    return s.files.length > 1 ? tr("{} files", s.files.length) : s.size;
  }

  // ---------- Drawing ----------

  const el = (name, attrs = {}, parent) => {
    const node = document.createElementNS(SVG, name);
    for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, v);
    if (parent) parent.appendChild(node);
    return node;
  };

  const polar = (a, r) => [Math.sin(a) * r, -Math.cos(a) * r];
  const f = (n) => Math.round(n * 100) / 100;

  /**
   * An annular sector from angle a0 to a1 (radians, clockwise from twelve o'clock) between r0 and
   * r1, its sides pulled apart by `gap`, drawn inset by `corner`: stroked 2·corner wide with round
   * joins it fills the sector with every corner rounded.
   */
  function sector(a0, a1, r0, r1, gap, corner) {
    const d = gap / 2 + corner;
    const ri = r0 + corner;
    const ro = r1 - corner;
    if (ro <= ri + 0.5) return "";
    const di = Math.asin(Math.min(1, d / ri));
    const dO = Math.asin(Math.min(1, d / ro));
    const s0 = a0 + dO;
    const e0 = a1 - dO;
    const s1 = a0 + di;
    const e1 = a1 - di;
    if (e0 <= s0 || e1 <= s1) return "";
    const [x1, y1] = polar(s1, ri);
    const [x2, y2] = polar(s0, ro);
    const [x3, y3] = polar(e0, ro);
    const [x4, y4] = polar(e1, ri);
    const big = e0 - s0 > Math.PI ? 1 : 0;
    const bigI = e1 - s1 > Math.PI ? 1 : 0;
    return `M${f(x1)} ${f(y1)}L${f(x2)} ${f(y2)}A${f(ro)} ${f(ro)} 0 ${big} 1 ${f(x3)} ${f(y3)}L${f(x4)} ${f(y4)}A${f(ri)} ${f(ri)} 0 ${bigI} 0 ${f(x1)} ${f(y1)}Z`;
  }

  /** A label broken onto two lines when it is long and has a space, as the app does. */
  function lines(label) {
    if (label.length <= 9 || !label.includes(" ")) return [label];
    const mid = label.length / 2;
    let best = -1;
    for (let i = 0; i < label.length; i++) {
      if (label[i] === " " && (best < 0 || Math.abs(i - mid) < Math.abs(best - mid))) best = i;
    }
    return [label.slice(0, best), label.slice(best + 1)];
  }

  // ---------- One wheel ----------

  let made = 0;

  /**
   * A wheel in `svg`, whose viewBox is -210 -210 420 420 around the hub. show() puts one of the
   * scenes above on it, setItems() any other list. An interactive one is aimed by the page with
   * indexAt() and setHover(), and says what happened: onPick(index, label, bubble) when a wedge
   * or one of its bubbles is clicked, onHover(index) as the aim moves, onMode(name) when the ring
   * in the hub is clicked or rested on.
   */
  function create(svg, { interactive = false, onPick, onHover, onMode } = {}) {
    const id = `mcw${++made}`;
    svg.classList.add("wheel");
    svg.classList.toggle("is-interactive", interactive);

    const defs = el("defs", {}, svg);
    const aim = el("linearGradient", { id: `${id}-aim`, x1: "0", y1: "0", x2: "0", y2: "1" }, defs);
    el("stop", { offset: "0", class: "stop-light" }, aim);
    el("stop", { offset: "1", class: "stop-deep" }, aim);
    const goo = el("filter", {
      id: `${id}-goo`,
      filterUnits: "userSpaceOnUse",
      x: "-320",
      y: "-320",
      width: "640",
      height: "700",
      "color-interpolation-filters": "sRGB",
    }, defs);
    el("feGaussianBlur", { in: "SourceGraphic", stdDeviation: "6", result: "blur" }, goo);
    el("feColorMatrix", {
      in: "blur",
      type: "matrix",
      values: "1 0 0 0 0  0 1 0 0 0  0 0 1 0 0  0 0 0 24 -11",
      result: "goo",
    }, goo);
    const blur = el("filter", { id: `${id}-shadow`, filterUnits: "userSpaceOnUse", x: "-340", y: "-320", width: "680", height: "720" }, defs);
    el("feGaussianBlur", { stdDeviation: "16" }, blur);
    const AIM = `url(#${id}-aim)`;

    const body = el("g", { class: "wheel__body", visibility: "hidden" }, svg);
    const shadow = el("circle", { class: "wheel__shadow", r: DISC_R, cy: 14, filter: `url(#${id}-shadow)` }, body);
    const frame = el("g", { filter: `url(#${id}-goo)` }, body);
    el("circle", { class: "wheel__frame", r: DISC_R }, frame);
    const lobe = el("path", { class: "wheel__frame wheel__lobe", "stroke-width": (CORNER + 6) * 2, "stroke-linejoin": "round" }, frame);
    const wedgeLayer = el("g", {}, body);
    const bubbleLayer = el("g", {}, body);
    const hub = el("g", {}, body);
    el("circle", { class: "hub__track", r: HUB_R }, hub);
    const slice = el("path", { class: "hub__slice", "stroke-width": MODE_CORNER * 2, "stroke-linejoin": "round" }, hub);
    const hubLabels = el("g", {}, hub);
    const hubHits = el("g", {}, hub);
    el("circle", { class: "hub__centre", r: INFO_R }, hub);
    const centre = el("text", { class: "hub__text", "text-anchor": "middle", "dominant-baseline": "central" }, hub);
    const cap = el("g", { class: "caption" }, body);
    const capPill = el("rect", { class: "caption__pill", rx: 15, height: 30, y: -15 }, cap);
    const capText = el("text", { class: "caption__text", "text-anchor": "middle", "dominant-baseline": "central" }, cap);

    /** What the wheel shows, where it aims, and how far it is through opening. */
    const w = {
      open: false,
      closingAt: 0,
      scene: null,
      modes: [],
      mode: 0,
      items: [],
      wedges: [],
      bubbles: [],
      aim: -1,
      hover: -1,
      active: -1,
      bubble: -1,
      centre: "",
      sub: "",
      unfoldAt: 0,
      open$: new Spring(POP, 0),
      slide$: new Spring(SLIDE, 0),
      nudge$: new Spring(NUDGE, 0),
      nudgeAngle: 0,
      dwell: 0,
      dwellSlice: -1,
    };
    let frameId = 0;

    // Small on the page, its words grow a little, as the app's do at a small scale.
    const fit = () => {
      const width = svg.getBoundingClientRect().width;
      if (width) svg.style.setProperty("--boost", f(Math.min(1.35, Math.max(1, 0.86 / (width / 420)))));
    };
    if (window.ResizeObserver) new ResizeObserver(fit).observe(svg);

    function build() {
      wedgeLayer.replaceChildren();
      hubLabels.replaceChildren();
      hubHits.replaceChildren();
      bubbleLayer.replaceChildren();
      w.bubbles = [];
      const n = w.items.length;
      const span = TAU / Math.max(1, n);
      w.wedges = w.items.map((item, i) => {
        const g = el("g", { class: "wedge" }, wedgeLayer);
        if (interactive) {
          g.setAttribute("role", "menuitem");
          g.setAttribute("tabindex", "-1");
          g.setAttribute("aria-label", shownLabel(item));
          g.addEventListener("pointerenter", (e) => e.pointerType === "mouse" && setHover(i, true));
          g.addEventListener("click", () => pick(i));
          g.addEventListener("focus", () => setHover(i, true));
        }
        const ring = el("path", { class: "wedge__focus", "stroke-width": CORNER * 2 + 7, "stroke-linejoin": "round" }, g);
        const base = el("path", { class: "wedge__base", "stroke-width": CORNER * 2, "stroke-linejoin": "round" }, g);
        const tint = el("path", { class: "wedge__aim", fill: AIM, stroke: AIM, "stroke-width": CORNER * 2, "stroke-linejoin": "round", opacity: 0 }, g);
        const flash = el("path", { class: "wedge__flash", "stroke-width": CORNER * 2, "stroke-linejoin": "round", opacity: 0 }, g);
        const content = el("g", {}, g);
        const parts = lines(shownLabel(item));
        const hasMark = !!item.tile;
        const text = el("text", { class: "wedge__label", "text-anchor": "middle" }, content);
        // A word longer than any English one (Turkish's Boyutlandır) gets a smaller size, not the wedge's edge.
        if (Math.max(...parts.map((p) => p.length)) > 10) text.style.setProperty("--fit", "0.84");
        parts.forEach((part, k) => {
          const t = el("tspan", { x: 0, dy: k === 0 ? (hasMark ? 21 : parts.length > 1 ? "-0.2em" : "0.37em") : "1.13em" }, text);
          t.textContent = part;
        });
        if (item.tile) {
          el("rect", { class: "wedge__tile", x: -15, y: -26, width: 30, height: 30, rx: 8 }, content);
          const t = el("text", { class: "wedge__tile-text", "text-anchor": "middle", "dominant-baseline": "central", y: -11 }, content);
          t.textContent = TURKISH ? shownLabel(item)[0] : item.tile;
        }
        if (item.toggle) {
          const on = { dark: isDark(), awake: desk.awake, mic: desk.mic, repeat: desk.repeat, shuffle: desk.shuffle }[item.toggle];
          el("circle", { class: `wedge__dot${on ? " is-on" : ""}`, r: 3, cy: hasMark ? 33 : parts.length > 1 ? 27 : 19 }, content);
        }
        return {
          item,
          g,
          base,
          tint,
          flash,
          ring,
          content,
          angle: i * span,
          span,
          swell$: new Spring(SWELL, 0),
          flash$: new Spring(NUDGE, 0),
        };
      });
      // The hub: one slice per mode, the current one raised, each named along its curve.
      const m = w.modes.length;
      if (m > 1) {
        const mspan = TAU / m;
        w.modes.forEach((name, i) => {
          const a = i * mspan;
          const arc = `${id}-mode-${i}`;
          const r = (INFO_R + HUB_R) / 2 - 3;
          // Text runs left to right: along the top half clockwise, along the bottom half the other way.
          const bottom = Math.cos(a) < -0.2;
          const rr = bottom ? r + 6 : r;
          const from = bottom ? a + mspan / 2 - 0.05 : a - mspan / 2 + 0.05;
          const to = bottom ? a - mspan / 2 + 0.05 : a + mspan / 2 - 0.05;
          const [x0, y0] = polar(from, rr);
          const [x1, y1] = polar(to, rr);
          el("path", { id: arc, d: `M${f(x0)} ${f(y0)}A${f(rr)} ${f(rr)} 0 0 ${bottom ? 0 : 1} ${f(x1)} ${f(y1)}`, fill: "none" }, hubLabels);
          const t = el("text", { class: `hub__label${i === w.mode ? " is-current" : ""}`, "text-anchor": "middle" }, hubLabels);
          const tp = el("textPath", { href: `#${arc}`, startOffset: "50%" }, t);
          tp.textContent = tr(name);
          if (interactive && onMode) {
            const hit = el("path", { class: "hub__hit", d: sector(a - mspan / 2, a + mspan / 2, INFO_R, HUB_R, 0, 0.01) }, hubHits);
            hit.addEventListener("click", (e) => {
              e.stopPropagation();
              if (i !== w.mode) onMode(name);
            });
          }
        });
        slice.style.display = "";
      } else {
        slice.style.display = "none";
      }
      text();
    }

    /** The words: the centre and the caption, sized to what they say. */
    function text() {
      const item = w.aim >= 0 ? w.items[w.aim] : null;
      const bubble = item && item.bubbles && w.bubble >= 0 ? item.bubbles[w.bubble] : null;
      const mode = w.modes[w.mode];
      centre.textContent = w.scene ? centreText(w.scene, mode, item, bubble) : w.centre;
      const long = centre.textContent.length;
      centre.style.setProperty("--fit", long > 11 ? "0.7" : long > 8 ? "0.84" : "1");
      capText.textContent = w.scene ? caption(w.scene, mode, item, bubble) : w.sub;
      cap.style.display = capText.textContent ? "" : "none";
      let width = capText.textContent.length * 7 + 30;
      try {
        width = capText.getComputedTextLength() + 30 || width;
      } catch (e) {
        // Not laid out yet: a guess from the letters, measured again next time.
      }
      capPill.setAttribute("width", f(width));
      capPill.setAttribute("x", f(-width / 2));
    }

    /**
     * The bubbles' places past the aimed wedge: side by side along a circle, their inner edges on
     * BUBBLE_AT's, each as big as its word needs (the app's are icons; words want more room).
     */
    function bubbleCentres(wedge, radii) {
      const along = radii.map((r) => BUBBLE_AT - BUBBLE_R + r);
      const gaps = radii.slice(1).map((r, k) => (radii[k] + r + BUBBLE_GAP) / ((along[k] + along[k + 1]) / 2));
      const total = gaps.reduce((a, b) => a + b, 0);
      let a = wedge.angle - total / 2;
      return radii.map((_, k) => {
        if (k > 0) a += gaps[k - 1];
        return polar(a, along[k]);
      });
    }

    function showBubbles(i) {
      bubbleLayer.replaceChildren();
      w.bubbles = [];
      const wedge = w.wedges[i];
      if (!wedge || !wedge.item.bubbles || !w.open) return;
      const now = performance.now();
      const current = wedge.item.level ? desk[wedge.item.level] : -1;
      const drawn = wedge.item.bubbles.map((word, k) => {
        const g = el("g", { class: `bubble${k === current ? " is-current" : ""}` }, bubbleLayer);
        const discEl = el("circle", { class: "bubble__disc", r: BUBBLE_R }, g);
        const ringEl = el("circle", { class: "bubble__ring", r: BUBBLE_R + 3.5 }, g);
        const t = el("text", { class: "bubble__text", "text-anchor": "middle", "dominant-baseline": "central" }, g);
        t.textContent = tr(word);
        let r = BUBBLE_R;
        try {
          r = Math.max(BUBBLE_R, t.getComputedTextLength() / 2 + 7);
        } catch (e) {
          // Not laid out: the smallest bubble.
        }
        discEl.setAttribute("r", f(r));
        ringEl.setAttribute("r", f(r + 3.5));
        if (interactive) {
          g.addEventListener("pointerenter", () => aimBubble(k));
          g.addEventListener("click", (e) => {
            e.stopPropagation();
            pick(i, k);
          });
        }
        return { g, r };
      });
      const places = bubbleCentres(wedge, drawn.map((b) => b.r));
      drawn.forEach(({ g, r }, k) => {
        const pop = new Spring(POP, 0);
        pop.set(1, now, k * 40);
        w.bubbles.push({ g, x: places[k][0], y: places[k][1], r, pop });
      });
    }

    function aimBubble(k) {
      if (k === w.bubble) return;
      w.bubble = k;
      w.bubbles.forEach((b, j) => b.g.classList.toggle("is-aimed", j === k));
      text();
    }

    /** Aims at wedge `i` (-1, none): it swells, its bubbles come out, the words follow. */
    function setAim(i, delay = 0) {
      const now = performance.now();
      if (i !== w.aim) {
        const was = w.wedges[w.aim];
        if (was) {
          was.swell$.set(0, now);
          was.g.classList.remove("is-aimed");
        }
        w.aim = i;
        w.bubble = -1;
        const wedge = w.wedges[i];
        if (wedge) {
          wedge.swell$.set(1, now, delay);
          wedge.g.classList.add("is-aimed");
        }
        showBubbles(i);
      }
      text();
      run();
    }
    const syncAim = (delay) => setAim(w.active >= 0 ? w.active : w.hover, delay);

    /** Puts `list` on the wheel, the ring showing `modes` with `mode` raised. */
    function put(list, modes, mode, active) {
      const now = performance.now();
      const labels = (l) => l.map(labelOf).join("|");
      const sameModes = modes.join("|") === w.modes.join("|");
      let delay = 0;
      if (labels(list) !== labels(w.items) || !sameModes || mode !== w.mode) {
        const turned = sameModes && mode !== w.mode && modes.length > 1;
        const from = w.slide$.value(now);
        // The raised slice goes the short way round.
        let to = (mode * TAU) / Math.max(1, modes.length);
        while (to - from > Math.PI) to -= TAU;
        while (from - to > Math.PI) to += TAU;
        w.items = list;
        w.modes = modes;
        w.mode = mode;
        w.aim = -1;
        w.bubble = -1;
        w.hover = -1;
        build();
        if (w.open && !w.closingAt) {
          w.unfoldAt = now;
          w.slide$.set(to, now);
          if (turned) nudge(Math.PI + to);
          // The new wedges unfold first; the chosen one swells once they have.
          delay = 160 + list.length * 28;
        } else {
          w.slide$.jump(to);
        }
      }
      w.active = active;
      syncAim(delay);
    }

    /** One of the scenes above: `scene` names it, `modeName` the mode, `active` a wedge (or its label). */
    function show(scene, modeName, { active = -1 } = {}) {
      const s = SCENES[scene];
      const names = Object.keys(s.modes);
      const m = Math.max(0, names.indexOf(modeName));
      const list = s.modes[names[m]];
      w.scene = scene;
      put(list, names, m, typeof active === "string" ? list.findIndex((item) => labelOf(item) === active) : active);
    }

    /** Any other list: the hub says `center`, the caption `sub`. */
    function setItems(list, { center = "", sub = "", active = -1, modes = [], mode = 0 } = {}) {
      w.scene = null;
      w.centre = center;
      w.sub = sub;
      put(list, modes, mode, active);
    }

    function setHub(c, s) {
      w.centre = c;
      w.sub = s;
      text();
    }

    function setActive(i) {
      w.active = i;
      syncAim();
    }

    function setHover(i, fromInput) {
      if (w.hover === i) return;
      w.hover = i;
      if (interactive) w.active = i;
      syncAim();
      if (fromInput && onHover) onHover(i);
    }

    function open(on) {
      const now = performance.now();
      if (on) {
        if (w.open && !w.closingAt) return;
        w.open = true;
        w.closingAt = 0;
        svg.classList.add("is-open");
        body.removeAttribute("visibility");
        fit();
        w.open$.jump(0);
        w.open$.set(1, now);
        w.unfoldAt = now;
        w.slide$.jump((w.mode * TAU) / Math.max(1, w.modes.length));
        // The aimed wedge swells again, once the others are out.
        const was = w.aim;
        w.wedges.forEach((wedge) => {
          wedge.swell$.jump(0);
          wedge.g.classList.remove("is-aimed");
        });
        w.aim = -1;
        setAim(was, 160 + w.items.length * 28);
      } else {
        if (!w.open || w.closingAt) return;
        stopDwell();
        w.hover = -1;
        w.bubbles = [];
        bubbleLayer.replaceChildren();
        w.closingAt = still.matches ? now - 1000 : now;
        run();
      }
    }

    /** Where the pointer at `x`, `y` (the page's pixels) is, in the wheel's points. */
    function local(x, y) {
      const r = svg.getBoundingClientRect();
      if (!r.width) return null;
      const k = 420 / r.width;
      return [(x - (r.left + r.width / 2)) * k, (y - (r.top + r.height / 2)) * k];
    }

    /**
     * The wedge at `x`, `y`, or -1: the hub, or (with `cap`) past the disc. The aimed one holds
     * while the pointer is near it, or on one of its bubbles, so the aim feels magnetic. Resting
     * on the ring of modes changes the mode, as in the app.
     */
    function indexAt(x, y, { cap: capped = true } = {}) {
      const p = local(x, y);
      if (!p || !w.items.length) return -1;
      const [dx, dy] = p;
      const d = Math.hypot(dx, dy);
      dwellAt(dx, dy, d);
      if (d < HUB_R) return -1;
      if (w.aim >= 0 && w.bubbles.length) {
        const k = w.bubbles.findIndex((b) => Math.hypot(dx - b.x, dy - b.y) <= b.r + 7);
        if (k >= 0) {
          aimBubble(k);
          return w.aim;
        }
        aimBubble(-1);
      }
      // Past the disc is outside, as in the app (src/wheel.rs): a drop there cancels.
      if (capped && d > DISC_R + 10) return -1;
      const span = TAU / w.items.length;
      const a = (Math.atan2(dx, -dy) + TAU) % TAU;
      if (w.hover >= 0) {
        let delta = Math.abs(a - w.hover * span) % TAU;
        if (delta > Math.PI) delta = TAU - delta;
        if (delta < span / 2 + 0.1) return w.hover;
      }
      return Math.round(a / span) % w.items.length;
    }

    function dwellAt(dx, dy, d) {
      const m = w.modes.length;
      if (!interactive || !onMode || m < 2 || !w.open || d < INFO_R || d > HUB_R) {
        stopDwell();
        return;
      }
      const s = Math.round(((Math.atan2(dx, -dy) + TAU) % TAU) / (TAU / m)) % m;
      if (s === w.mode) {
        stopDwell();
        return;
      }
      if (s === w.dwellSlice) return;
      stopDwell();
      w.dwellSlice = s;
      w.dwell = setTimeout(() => {
        w.dwellSlice = -1;
        onMode(w.modes[s]);
      }, MODE_DWELL);
    }

    function stopDwell() {
      clearTimeout(w.dwell);
      w.dwellSlice = -1;
    }

    /** A point inside wedge `i`, `rr` of the way out from its inner edge, in the wheel's points. */
    function midpoint(i, rr = 0.62) {
      const span = TAU / Math.max(1, w.items.length);
      return polar(i * span, RING_R0 + (RING_R1 - RING_R0) * rr);
    }

    /** A pick: the wedge flashes and the wheel nods toward it. */
    function confirm(i) {
      const wedge = w.wedges[i];
      if (!wedge) return;
      wedge.flash$.kick(performance.now(), TAU / NUDGE.response);
      nudge(wedge.angle);
      run();
    }

    function pick(i, k = w.bubble) {
      const wedge = w.wedges[i];
      if (!wedge || !onPick) return;
      const item = wedge.item;
      onPick(i, labelOf(item), item.bubbles && k >= 0 ? item.bubbles[k] : null);
    }

    function nudge(angle) {
      w.nudgeAngle = angle;
      w.nudge$.kick(performance.now(), TAU / NUDGE.response);
    }

    // ---------- Every frame while something moves ----------

    function run() {
      if (!frameId) frameId = requestAnimationFrame(frame_);
    }

    function frame_(now) {
      frameId = 0;
      let moving = false;
      // Opening: the hub pops, the disc grows out of it; closing: everything folds in, faster.
      let openness;
      if (w.closingAt) {
        const t = Math.min(1, (now - w.closingAt) / 170);
        openness = 1 - easeIn(t);
        if (t >= 1) {
          w.open = false;
          w.closingAt = 0;
          svg.classList.remove("is-open");
          body.setAttribute("visibility", "hidden");
          return;
        }
        moving = true;
      } else if (w.open) {
        openness = w.open$.value(now);
        moving = w.open$.moving(now);
      } else {
        return;
      }
      const hubScale = Math.max(0, openness);
      const discT = w.closingAt ? openness : still.matches ? 1 : Math.max(0, w.open$.value(now - 60));
      const nudgeR = w.nudge$.value(now) * 3.5;
      moving = moving || w.nudge$.moving(now);
      const nx = Math.sin(w.nudgeAngle) * nudgeR;
      const ny = -Math.cos(w.nudgeAngle) * nudgeR;
      hub.setAttribute("transform", `translate(${f(nx)} ${f(ny)}) scale(${f(hubScale)})`);
      frame.setAttribute("transform", `scale(${f(Math.max(0, discT))})`);
      shadow.setAttribute("transform", `scale(${f(Math.max(0, discT))})`);
      cap.setAttribute("transform", `translate(0 ${f(CAPTION_AT)}) scale(${f(Math.max(0, Math.min(1.05, discT)))})`);
      cap.setAttribute("opacity", f(Math.max(0, Math.min(1, discT * 1.4 - 0.2))));

      // The raised slice of the mode ring.
      if (w.modes.length > 1) {
        const a = w.slide$.value(now);
        const mspan = TAU / w.modes.length;
        slice.setAttribute("d", sector(a - mspan / 2, a + mspan / 2, INFO_R + 1, HUB_R - 2, MODE_GAP, MODE_CORNER));
        moving = moving || w.slide$.moving(now);
      }

      // The wedges: unfolding one after another, the aimed one swelling with the frame around it.
      let lobePath = "";
      w.wedges.forEach((wedge, i) => {
        const since = (now - w.unfoldAt - 40 - i * 28) / 1000;
        const unfoldT = still.matches ? 1 : Math.max(0, springAt(POP, 0, 0, 1, since)[0]);
        if (since < settle(POP) * 1.3) moving = true;
        const unfold = w.closingAt ? openness : unfoldT;
        const s = wedge.swell$.value(now);
        if (wedge.swell$.moving(now)) moving = true;
        const lift = s * WEDGE_LIFT;
        const outer = RING_R1 + s * WEDGE_SWELL;
        const a0 = wedge.angle - wedge.span / 2;
        const a1 = wedge.angle + wedge.span / 2;
        const d = sector(a0, a1, RING_R0, outer, GAP, CORNER);
        const [lx, ly] = polar(wedge.angle, lift);
        const sc = UNFOLD_FROM + (1 - UNFOLD_FROM) * unfold;
        wedge.g.setAttribute("transform", `translate(${f(lx)} ${f(ly)}) scale(${f(Math.max(0, sc * discT))})`);
        wedge.g.setAttribute("opacity", f(Math.max(0, Math.min(1, unfold * 1.6))));
        wedge.base.setAttribute("d", d);
        wedge.tint.setAttribute("d", d);
        wedge.flash.setAttribute("d", d);
        wedge.ring.setAttribute("d", d);
        wedge.tint.setAttribute("opacity", f(Math.max(0, Math.min(1, s * 1.2))));
        const fl = Math.abs(wedge.flash$.value(now));
        if (wedge.flash$.moving(now)) moving = true;
        wedge.flash.setAttribute("opacity", f(Math.min(1, fl * 1.4)));
        const [tx, ty] = polar(wedge.angle, (RING_R0 + RING_R1) / 2 + 2 + s * LABEL_SWELL);
        wedge.content.setAttribute("transform", `translate(${f(tx)} ${f(ty)})`);
        if (s > 0.02) {
          const over = DISC_R - RING_R1;
          lobePath += sector(a0 - 0.05, a1 + 0.05, RING_R0, outer + over + LOBE_MARGIN * s, 0, CORNER + 6);
        }
      });
      lobe.setAttribute("d", lobePath);

      w.bubbles.forEach((b) => {
        const p = b.pop.value(now);
        if (b.pop.moving(now)) moving = true;
        b.g.setAttribute("transform", `translate(${f(b.x)} ${f(b.y)}) scale(${f(Math.max(0, p))})`);
      });

      if (moving) run();
    }

    return {
      show,
      setItems,
      setHub,
      setActive,
      setHover,
      open,
      indexAt,
      midpoint,
      confirm,
      get items() {
        return w.items.map(labelOf);
      },
      get nodes() {
        return w.wedges.map((wedge) => wedge.g);
      },
      get hover() {
        return w.hover;
      },
      get mode() {
        return w.modes[w.mode];
      },
    };
  }

  const easeIn = (t) => t * t * t;

  window.McWheel = { create, SCENES, ACTIONS, tr, pct, TURKISH };
})();
