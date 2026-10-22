// The demo wheel: McGyver's radial menu drawn in SVG, with the app's geometry (src/wheel.rs) and
// its POP, SWELL and NUDGE springs (src/motion.rs). Nothing here touches a file: a drop makes a
// card, not a conversion.
// Also the page's small chores: the banner, the platform picked for Download, the tour, the
// language picked.
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
  /** How far the wheel reaches from its centre, kept on the stage: with its bubbles where it can. */
  const REACH_SIDE = DISC_R + FRAME_SWELL + 8;
  const REACH_BUBBLES = BUBBLE_AT + BUBBLE_R + 4;
  const REACH_DOWN = CAPTION_AT + 22;
  /**
   * A slice of the mode ring the pointer rests on this long becomes the mode. The app switches
   * after 50 ms (src/overlay.rs SETTLE) because it can tell a stop from a pass; this only times.
   */
  const MODE_DWELL = 260;
  /** Shift held on its own this long opens the wheel without files (the app's default). */
  const HOLD_MS = 450;

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
    "Copy {}": "{} kopyalanacak",
    "Cut {}": "{} kesilecek",
    "Copy {} to…": "{} kopyalanacak…",
    "Move {} to…": "{} taşınacak…",
    "Copy the path of {}": "Yolu kopyala: {}",
    "Open a terminal there": "Orada bir terminal açılacak",
    "Send {} to your phone": "{} telefona gönderilecek",
    "Send {} with {}": "{} {} ile gönderilecek",
    "Switch to {}": "Geç: {}",
    "Switch to an app": "Bir uygulamaya geç",
    "Repeating all": "Tümü tekrarlanıyor",
    "Repeat is off": "Tekrar kapalı",
    "Shuffle is on": "Karıştırma açık",
    "Shuffle is off": "Karıştırma kapalı",
    "Morning mix · track {}": "Sabah karışımı · {}. parça",
    " · paused": " · duraklatıldı",
    "Language · Türkçe after you log in again": "Dil · Türkçe, yeniden oturum açınca",
    "Language · English": "Dil · English",
    "Volume {}": "Ses {}",
    "Brightness {}": "Parlaklık {}",
    "pick an area": "bir alan seçin",
    "the whole screen": "tüm ekran",
    "the window in front": "öndeki pencere",
    "Record the screen": "Ekranı kaydet",
    "CPU {} · memory {} · disk {}": "İşlemci {} · bellek {} · disk {}",
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

  /** A share of something, as each language writes it: 25 % and %25. */
  const pct = (n) => (TURKISH ? `%${n}` : `${n} %`);

  // ---------- What the wheel offers (the app's catalog, for these sample files) ----------

  const SHARE = { label: "Share", bubbles: ["Phone", "WhatsApp", "Telegram", "Slack", "Teams"] };
  const ACTIONS = ["Zip", "Copy", "Cut", "Copy To", "Move To", "Copy Path", "Terminal", SHARE];

  // Each scene is named by its file on the English page; the files it shows are the page's own.
  const SCENES = {
    "holiday.jpg": {
      files: TURKISH ? ["tatil.jpg"] : ["holiday.jpg"],
      kind: "Image",
      modes: {
        Convert: ["PNG", "WEBP", "AVIF", "TIFF", "GIF", "BMP", "ICO", "PDF"],
        Tools: ["Compress", "Resize", "Crop", "Rotate", "Strip EXIF"],
        Actions: ACTIONS,
      },
    },
    "trip.mov": {
      files: TURKISH ? ["gezi.mov"] : ["trip.mov"],
      kind: "Video",
      modes: {
        Convert: ["MP4", "MKV", "WEBM", "GIF", "MP3"],
        Tools: ["Compress", "Trim", "Mute", "Rotate", "Frames"],
        Actions: ACTIONS,
      },
    },
    "song.flac": {
      files: TURKISH ? ["şarkı.flac"] : ["song.flac"],
      kind: "Audio",
      modes: {
        Convert: ["MP3", "M4A", "WAV", "OGG", "OPUS", "AIFF"],
        Tools: ["Trim", "Normalize", "Compress"],
        Actions: ACTIONS,
      },
    },
    "report.pdf": {
      files: TURKISH ? ["rapor.pdf"] : ["report.pdf"],
      kind: "PDF",
      modes: {
        Convert: ["DOCX", "PPTX", "XLSX", "JPG", "PNG", "EPUB", "TXT", "MD"],
        Tools: ["Compress", "Split", "Rotate"],
        Actions: ACTIONS,
      },
    },
    "notes.docx": {
      files: TURKISH ? ["notlar.docx"] : ["notes.docx"],
      kind: "Document",
      modes: { Convert: ["PDF", "ODT", "RTF", "HTML", "TXT", "MD"], Actions: ACTIONS },
    },
    "photos.zip": {
      files: TURKISH ? ["fotolar.zip"] : ["photos.zip"],
      kind: "Archive",
      modes: {
        Convert: ["Unzip", "7Z", "TAR.GZ", "TAR.XZ", "TAR.ZST", "TAR.BZ2", "TAR"],
        Actions: ACTIONS,
      },
    },
    "film.srt": {
      files: ["film.srt"],
      kind: "Subtitles",
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
        Apps: [
          { label: "Files", tile: "F" },
          { label: "Browser", tile: "B" },
          { label: "Terminal", tile: "T" },
          { label: "Music", tile: "M" },
          { label: "Notes", tile: "N" },
        ],
        Media: [
          { label: "Pause", glyph: "pause" },
          { label: "Next", glyph: "next" },
          { label: "Repeat", glyph: "repeat", toggle: "repeat" },
          { label: "Player", glyph: "player" },
          { label: "Shuffle", glyph: "shuffle", toggle: "shuffle" },
          { label: "Previous", glyph: "prev" },
        ],
        Everyday: [
          { label: "Keyboard", bubbles: ["EN", "TR", "DE"], level: "layout" },
          { label: "Language" },
          { label: "Volume", bubbles: ["Mute", "25", "50", "75", "100"], level: "volume" },
          { label: "Brightness", bubbles: ["25", "50", "75", "100"], level: "brightness" },
          { label: "Screenshot", bubbles: ["Area", "Screen", "Window"] },
          { label: "Record", bubbles: ["Area", "Screen"] },
          { label: "Monitor" },
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
    layout: 0,
    volume: 3,
    brightness: 2,
    turkish: false,
  };

  const stemOf = (name) => name.replace(/\.[^.]+$/, "");
  const extOf = (name) => (name.match(/\.([^.]+)$/) || ["", ""])[1];
  const labelOf = (item) => (typeof item === "string" ? item : item.label);
  /** The label as the wedge shows it: Pause reads Play while nothing plays. */
  const shownLabel = (item) => tr(item && item.glyph === "pause" && !desk.playing ? "Play" : labelOf(item));
  const isDark = () => document.documentElement.dataset.theme === "dark";

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
      if (label === "Unzip") return `${what} › ${stem}/`;
      return `${what} › ${stem}.${label.toLowerCase()}`;
    }
    if (mode === "Tools") {
      if (label === "Merge") return `${what} › ${stem} (merged).pdf`;
      if (label === "Split") return `${what} › ${stem} (pages)`;
      if (label === "Frames") return `${what} › ${stem} (frames)`;
      if (label === "Shift") return bubble ? tr("Move {} by {}", what, bubble) : tr("Move {} in time", what);
      return `${what} › ${stem} (${TOOL_SUFFIX[label] || label.toLowerCase()}).${ext}`;
    }
    switch (label) {
      case "Zip": return `${what} › ${stem}.zip`;
      case "Copy": return tr("Copy {}", what);
      case "Cut": return tr("Cut {}", what);
      case "Copy To": return tr("Copy {} to…", what);
      case "Move To": return tr("Move {} to…", what);
      case "Copy Path": return tr("Copy the path of {}", what);
      case "Terminal": return tr("Open a terminal there");
      default:
        if (!bubble || bubble === "Phone") return tr("Send {} to your phone", what);
        return tr("Send {} with {}", what, bubble);
    }
  }

  function deskCaption(mode, item, bubble) {
    const label = item ? labelOf(item) : null;
    if (mode === "Apps") return label ? tr("Switch to {}", tr(label)) : tr("Switch to an app");
    if (mode === "Media") {
      if (label === "Repeat") return tr(desk.repeat ? "Repeating all" : "Repeat is off");
      if (label === "Shuffle") return tr(desk.shuffle ? "Shuffle is on" : "Shuffle is off");
      return tr("Morning mix · track {}", desk.track) + (desk.playing ? "" : tr(" · paused"));
    }
    if (mode === "Everyday") {
      const shot = { Area: "pick an area", Screen: "the whole screen", Window: "the window in front" };
      switch (label) {
        case "Keyboard": return `${tr("Keyboard")} · ${bubble || ["EN", "TR", "DE"][desk.layout]}`;
        case "Language": return tr(desk.turkish ? "Language · Türkçe after you log in again" : "Language · English");
        case "Volume": return tr("Volume {}", bubble ? (bubble === "Mute" ? tr("off") : pct(bubble)) : [tr("off"), pct(25), pct(50), pct(75), pct(100)][desk.volume]);
        case "Brightness": return tr("Brightness {}", pct(bubble || [25, 50, 75, 100][desk.brightness]));
        case "Screenshot": return bubble ? `${tr("Screenshot")} · ${tr(shot[bubble])}` : tr("Screenshot");
        case "Record": return bubble ? `${tr("Record")} · ${tr(shot[bubble])}` : tr("Record the screen");
        case "Monitor": return tr("CPU {} · memory {} · disk {}", pct(12), pct(41), pct(63));
        default: return tr("Everyday");
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
      default: return tr("System");
    }
  }

  /** What the centre of the hub says. */
  function centreText(scene, mode, item, bubble) {
    const word = bubble && tr(bubble);
    if (item) return word && word.length <= 9 ? word : shownLabel(item);
    if (scene === "desk") return tr(mode);
    const s = SCENES[scene];
    return s.files.length > 1 ? tr("{} files", s.files.length) : extOf(s.files[0]).toUpperCase();
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

  const GLYPHS = {
    pause: { fill: "M-6 -8h4v16h-4zM2 -8h4v16h-4z" },
    play: { fill: "M-5 -8L8 0L-5 8z" },
    next: { fill: "M-7 -7L3 0L-7 7zM4 -7h3v14h-3z" },
    prev: { fill: "M7 -7L-3 0L7 7zM-7 -7h3v14h-3z" },
    repeat: { line: "M-7 0V-3a4 4 0 0 1 4 -4H6M3 -10L6 -7L3 -4M7 0V3a4 4 0 0 1 -4 4H-6M-3 10L-6 7L-3 4" },
    shuffle: { line: "M-8 -6H-4L4 6H8M5 3L8 6L5 9M-8 6H-4L-1 1.5M2 -2.5L4 -6H8M5 -9L8 -6L5 -3" },
    player: { line: "M-8 -6h16v12h-16z", fill: "M-2 -3.5L3.5 0L-2 3.5z" },
  };

  // ---------- The wheel on the stage ----------

  const stage = document.querySelector("[data-stage]");
  const svg = document.querySelector("[data-wheel]");
  const cardsList = document.querySelector("[data-cards]");
  const hint = document.querySelector("[data-hint]");
  const status = document.querySelector("[data-status]");
  if (!stage || !svg) return;
  const desktop = stage.querySelector(".desktop");

  const defs = el("defs", {}, svg);
  const aim = el("linearGradient", { id: "wheel-aim", x1: "0", y1: "0", x2: "0", y2: "1" }, defs);
  el("stop", { offset: "0", class: "stop-light" }, aim);
  el("stop", { offset: "1", class: "stop-deep" }, aim);
  const goo = el("filter", {
    id: "wheel-goo",
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
  const shadowFilter = el("filter", {
    id: "wheel-shadow",
    filterUnits: "userSpaceOnUse",
    x: "-340",
    y: "-320",
    width: "680",
    height: "720",
  }, defs);
  el("feGaussianBlur", { stdDeviation: "16" }, shadowFilter);

  const body = el("g", { class: "wheel__body" }, svg);
  const shadow = el("circle", { class: "wheel__shadow", r: DISC_R, cy: 14, filter: "url(#wheel-shadow)" }, body);
  const frame = el("g", { class: "wheel__frame-group", filter: "url(#wheel-goo)" }, body);
  const disc = el("circle", { class: "wheel__frame", r: DISC_R }, frame);
  const lobe = el("path", { class: "wheel__frame wheel__lobe", "stroke-width": (CORNER + 6) * 2, "stroke-linejoin": "round" }, frame);
  const wedgeLayer = el("g", { class: "wheel__wedges" }, body);
  const bubbleLayer = el("g", { class: "wheel__bubbles" }, body);
  const hub = el("g", { class: "wheel__hub" }, body);
  el("circle", { class: "hub__track", r: HUB_R }, hub);
  const slice = el("path", { class: "hub__slice", "stroke-width": MODE_CORNER * 2, "stroke-linejoin": "round" }, hub);
  const hubLabels = el("g", {}, hub);
  const hubHits = el("g", {}, hub);
  el("circle", { class: "hub__centre", r: INFO_R }, hub);
  const centre = el("text", { class: "hub__text", "text-anchor": "middle", "dominant-baseline": "central" }, hub);
  const cap = el("g", { class: "caption", transform: `translate(0 ${CAPTION_AT})` }, body);
  const capPill = el("rect", { class: "caption__pill", rx: 15, height: 30, y: -15 }, cap);
  const capText = el("text", { class: "caption__text", "text-anchor": "middle", "dominant-baseline": "central" }, cap);

  /** The wheel's state: what it is for, how it opened, where it is, and what it aims at. */
  const w = {
    open: false,
    how: "click",
    scene: "holiday.jpg",
    mode: 0,
    modes: [],
    items: [],
    wedges: [],
    bubbles: [],
    aim: -1,
    bubble: -1,
    x: 0,
    y: 0,
    openedAt: 0,
    unfoldAt: 0,
    closingAt: 0,
    open$: new Spring(POP, 0),
    slide$: new Spring(SLIDE, 0),
    nudge$: new Spring(NUDGE, 0),
    nudgeAngle: 0,
    dwell: 0,
    dwellSlice: -1,
    opener: null,
    acted: false,
    done: false,
    atHome: true,
  };
  let scale = 1;
  let box = { w: 600, h: 600 };
  /** Whether the keyboard or a pointer was used last: a click focuses a wedge too. */
  let byKeyboard = false;
  window.addEventListener("keydown", () => (byKeyboard = true), true);
  window.addEventListener("pointerdown", () => (byKeyboard = false), true);
  const keysInWheel = () => byKeyboard && svg.contains(document.activeElement);
  let frameId = 0;
  let pointer = null;

  function layoutStage() {
    const r = stage.getBoundingClientRect();
    // Small enough that the bubbles stay on the stage, large enough to aim at with a thumb.
    scale = Math.min(1, Math.max(0.56, Math.min(r.width / 486, r.height / 600)));
    box = { w: r.width / scale, h: r.height / scale };
    svg.style.setProperty("--boost", f(Math.min(1.35, Math.max(1, 0.86 / scale))));
    svg.setAttribute("viewBox", `0 0 ${f(box.w)} ${f(box.h)}`);
    stage.style.setProperty("--band", `${f(desktop.getBoundingClientRect().bottom - r.top)}px`);
    if (!w.open) return;
    if (w.atHome) place(...home());
    else place(w.x, w.y);
    run();
  }

  /** Where the wheel goes when nothing says where: beside the files (or below their row). */
  function home() {
    const band = desktop.getBoundingClientRect();
    const st = stage.getBoundingClientRect();
    const column = getComputedStyle(desktop).display === "grid";
    const left = column ? (band.right - st.left) / scale : 0;
    const top = column ? 0 : (band.bottom - st.top) / scale;
    return [(left + box.w) / 2, (top + box.h) / 2 - 10];
  }

  function place(x, y) {
    const side = Math.max(REACH_SIDE, Math.min(REACH_BUBBLES, box.w / 2));
    const top = Math.max(REACH_SIDE, Math.min(REACH_BUBBLES, box.h / 2 - 40));
    const minX = side;
    const maxX = box.w - side;
    const minY = top;
    const maxY = box.h - REACH_DOWN;
    w.x = minX > maxX ? box.w / 2 : Math.min(Math.max(x, minX), maxX);
    w.y = minY > maxY ? box.h / 2 : Math.min(Math.max(y, minY), maxY);
  }

  function toLocal(clientX, clientY) {
    const r = stage.getBoundingClientRect();
    return [(clientX - r.left) / scale, (clientY - r.top) / scale];
  }

  function onStage(clientX, clientY) {
    const r = stage.getBoundingClientRect();
    return clientX >= r.left && clientX <= r.right && clientY >= r.top && clientY <= r.bottom;
  }

  /** Builds the wedges, the hub's slices and the captions for the current scene and mode. */
  function build() {
    const s = SCENES[w.scene];
    w.modes = Object.keys(s.modes);
    w.items = s.modes[w.modes[w.mode]];
    wedgeLayer.replaceChildren();
    hubLabels.replaceChildren();
    hubHits.replaceChildren();
    const n = w.items.length;
    const span = TAU / n;
    w.wedges = w.items.map((item, i) => {
      const label = labelOf(item);
      const g = el("g", {
        class: "wedge",
        role: "button",
        tabindex: w.open ? "0" : "-1",
        "aria-label": shownLabel(item),
      }, wedgeLayer);
      const ring = el("path", { class: "wedge__focus", "stroke-width": CORNER * 2 + 7, "stroke-linejoin": "round" }, g);
      const base = el("path", { class: "wedge__base", "stroke-width": CORNER * 2, "stroke-linejoin": "round" }, g);
      const tint = el("path", { class: "wedge__aim", "stroke-width": CORNER * 2, "stroke-linejoin": "round", opacity: 0 }, g);
      const flash = el("path", { class: "wedge__flash", "stroke-width": CORNER * 2, "stroke-linejoin": "round", opacity: 0 }, g);
      const content = el("g", {}, g);
      const parts = lines(shownLabel(item));
      const hasMark = item.tile || item.glyph;
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
      if (item.glyph) drawGlyph(content, item.glyph === "pause" && !desk.playing ? "play" : item.glyph);
      let dot = null;
      if (item.toggle) {
        dot = el("circle", { class: "wedge__dot", r: 3, cy: hasMark ? 33 : parts.length > 1 ? 27 : 19 }, content);
      }
      g.addEventListener("focus", () => {
        if (w.open && byKeyboard && w.aim !== i) setAim(i, -1);
      });
      g.addEventListener("keydown", (e) => wedgeKey(e, i));
      return {
        item,
        g,
        base,
        tint,
        flash,
        ring,
        content,
        dot,
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
        const id = `mode-arc-${i}`;
        const r = (INFO_R + HUB_R) / 2 - 3;
        // Text runs left to right: along the top half clockwise, along the bottom half the other way.
        const bottom = Math.cos(a) < -0.2;
        const from = bottom ? a + mspan / 2 - 0.05 : a - mspan / 2 + 0.05;
        const to = bottom ? a - mspan / 2 + 0.05 : a + mspan / 2 - 0.05;
        const [x0, y0] = polar(from, bottom ? r + 6 : r);
        const [x1, y1] = polar(to, bottom ? r + 6 : r);
        const rr = bottom ? r + 6 : r;
        el("path", {
          id,
          d: `M${f(x0)} ${f(y0)}A${f(rr)} ${f(rr)} 0 0 ${bottom ? 0 : 1} ${f(x1)} ${f(y1)}`,
          fill: "none",
        }, hubLabels);
        const t = el("text", { class: `hub__label${i === w.mode ? " is-current" : ""}`, "text-anchor": "middle" }, hubLabels);
        const tp = el("textPath", { href: `#${id}`, startOffset: "50%" }, t);
        tp.textContent = tr(name);
        const hit = el("path", {
          class: "hub__hit",
          d: sector(a - mspan / 2, a + mspan / 2, INFO_R, HUB_R, 0, 0.01),
          role: "button",
          tabindex: w.open ? "0" : "-1",
          "aria-label": tr("{} mode", tr(name)),
        }, hubHits);
        hit.addEventListener("click", (e) => {
          e.stopPropagation();
          switchMode(i);
        });
        hit.addEventListener("keydown", (e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            switchMode(i);
            focusWedge(0);
          }
        });
      });
      slice.style.display = "";
    } else {
      slice.style.display = "none";
    }
    w.bubbles = [];
    bubbleLayer.replaceChildren();
    svg.setAttribute("aria-label", w.scene === "desk" ? tr("The wheel without files") : tr("The wheel for {}", SCENES[w.scene].files.join(tr(" and "))));
    refreshDots();
    text();
  }

  function drawGlyph(parent, name) {
    const glyph = GLYPHS[name];
    const g = el("g", { transform: "translate(0 -10)" }, parent);
    if (glyph.line) el("path", { class: "wedge__glyph wedge__glyph--line", d: glyph.line }, g);
    if (glyph.fill) el("path", { class: "wedge__glyph", d: glyph.fill }, g);
  }

  function refreshDots() {
    for (const wedge of w.wedges) {
      if (!wedge.dot) continue;
      const on = {
        dark: isDark(),
        awake: desk.awake,
        mic: desk.mic,
        repeat: desk.repeat,
        shuffle: desk.shuffle,
      }[wedge.item.toggle];
      wedge.dot.classList.toggle("is-on", !!on);
      wedge.g.setAttribute("aria-pressed", on ? "true" : "false");
    }
  }

  /** The words: the centre and the caption, sized to what they say. */
  function text() {
    const item = w.aim >= 0 ? w.items[w.aim] : null;
    const bubble = item && item.bubbles && w.bubble >= 0 ? item.bubbles[w.bubble] : null;
    centre.textContent = centreText(w.scene, w.modes[w.mode], item, bubble);
    centre.style.setProperty("--fit", centre.textContent.length > 8 ? "0.84" : "1");
    capText.textContent = caption(w.scene, w.modes[w.mode], item, bubble);
    let width = 160;
    try {
      width = capText.getComputedTextLength() + 30;
    } catch (e) {
      // Not laid out yet (hidden): the next call measures it.
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
    if (!wedge || !wedge.item.bubbles) return;
    const now = performance.now();
    const current = wedge.item.level ? desk[wedge.item.level] : -1;
    const made = wedge.item.bubbles.map((word, k) => {
      const g = el("g", {
        class: `bubble${k === current ? " is-current" : ""}`,
        role: "button",
        tabindex: "0",
        "aria-label": tr(word),
      }, bubbleLayer);
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
      g.addEventListener("keydown", (e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          w.bubble = k;
          pick();
        } else if (e.key === "Escape") {
          e.preventDefault();
          focusWedge(i);
        }
      });
      g.addEventListener("focus", () => {
        w.bubble = k;
        markBubbles();
        text();
      });
      return { g, r };
    });
    const places = bubbleCentres(wedge, made.map((b) => b.r));
    made.forEach(({ g, r }, k) => {
      const pop = new Spring(POP, 0);
      pop.set(1, now, k * 40);
      w.bubbles.push({ g, x: places[k][0], y: places[k][1], r, pop });
    });
  }

  function markBubbles() {
    w.bubbles.forEach((b, k) => b.g.classList.toggle("is-aimed", k === w.bubble));
  }

  function setAim(i, bubble) {
    const now = performance.now();
    if (i !== w.aim) {
      if (w.aim >= 0 && w.wedges[w.aim]) {
        w.wedges[w.aim].swell$.set(0, now);
        w.wedges[w.aim].g.classList.remove("is-aimed");
      }
      w.aim = i;
      if (i >= 0) {
        w.wedges[i].swell$.set(1, now);
        w.wedges[i].g.classList.add("is-aimed");
      }
      showBubbles(i);
      w.acted = false;
    }
    if (bubble !== w.bubble) {
      w.bubble = bubble;
      markBubbles();
    }
    text();
    run();
  }

  /** Opens the wheel for `scene` on `mode`, at a spot on the stage (or its usual place). */
  function open(scene, how, at, modeName) {
    const now = performance.now();
    const wasOpen = w.open && !w.closingAt;
    const names = Object.keys(SCENES[scene].modes);
    const want = Math.max(0, modeName ? names.indexOf(modeName) : 0);
    stage.classList.toggle("is-busy", how !== "click");
    if (wasOpen && w.scene === scene) {
      w.how = how;
      if (at) {
        w.atHome = false;
        place(at[0], at[1]);
      }
      if (want !== w.mode) switchMode(want);
      run();
      return;
    }
    w.scene = scene;
    w.how = how;
    w.mode = want;
    w.aim = -1;
    w.bubble = -1;
    w.acted = false;
    w.done = false;
    w.closingAt = 0;
    w.open = true;
    svg.classList.add("is-open");
    svg.removeAttribute("aria-hidden");
    build();
    if (!wasOpen) {
      w.atHome = !at;
      const [hx, hy] = at || home();
      place(hx, hy);
      w.open$.jump(0);
      w.open$.set(1, now);
      w.openedAt = now;
    } else if (at) {
      w.atHome = false;
      place(at[0], at[1]);
    }
    w.unfoldAt = now;
    w.slide$.jump((w.mode * TAU) / Math.max(1, w.modes.length));
    setFocusable(true);
    run();
  }

  function close(instant) {
    if (!w.open) return;
    stopDwell();
    if (w.aim >= 0 && w.wedges[w.aim]) w.wedges[w.aim].g.classList.remove("is-aimed");
    w.aim = -1;
    w.bubble = -1;
    bubbleLayer.replaceChildren();
    w.bubbles = [];
    w.closingAt = instant || still.matches ? performance.now() - 1000 : performance.now();
    setFocusable(false);
    stage.classList.remove("is-busy");
    run();
  }

  function setFocusable(on) {
    svg.querySelectorAll("[tabindex]").forEach((node) => node.setAttribute("tabindex", on ? "0" : "-1"));
  }

  function switchMode(i) {
    if (i === w.mode || !w.open) return;
    const now = performance.now();
    const m = w.modes.length;
    // The raised slice goes the short way round.
    const from = w.slide$.value(now);
    let to = (i * TAU) / m;
    while (to - from > Math.PI) to -= TAU;
    while (from - to > Math.PI) to += TAU;
    w.mode = i;
    w.aim = -1;
    w.bubble = -1;
    build();
    w.slide$.set(to, now);
    w.unfoldAt = now;
    setFocusable(true);
    nudge(Math.PI + (i * TAU) / m);
    run();
  }

  function nudge(angle) {
    w.nudgeAngle = angle;
    w.nudge$.kick(performance.now(), TAU / NUDGE.response);
  }

  // ---------- Every frame while something moves ----------

  function run() {
    if (!frameId) frameId = requestAnimationFrame(frame_);
  }

  const easeIn = (t) => t * t * t;

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
        w.how = "click";
        w.done = false;
        svg.classList.remove("is-open");
        svg.setAttribute("aria-hidden", "true");
        body.setAttribute("transform", "translate(-9999 -9999)");
        return;
      }
      moving = true;
    } else {
      openness = w.open$.value(now);
      moving = moving || w.open$.moving(now);
    }
    const hubScale = Math.max(0, openness);
    const discT = w.closingAt ? openness : still.matches ? 1 : Math.max(0, w.open$.value(now - 60));
    const nudgeR = w.nudge$.value(now) * 3.5;
    moving = moving || w.nudge$.moving(now);
    const nx = Math.sin(w.nudgeAngle) * nudgeR;
    const ny = -Math.cos(w.nudgeAngle) * nudgeR;
    body.setAttribute("transform", `translate(${f(w.x)} ${f(w.y)})`);
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
      const unfoldT = still.matches ? 1 : Math.max(0, springAt(POP, 0, 0, 1, (now - w.unfoldAt - 40 - i * 28) / 1000)[0]);
      if ((now - w.unfoldAt - 40 - i * 28) / 1000 < settle(POP) * 1.3) moving = true;
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

  // ---------- Aiming, from wherever the pointer is ----------

  function aimAt(lx, ly) {
    if (!w.open || w.closingAt) return;
    const dx = lx - w.x;
    const dy = ly - w.y;
    const r = Math.hypot(dx, dy);
    // Bubbles first: they sit outside the disc.
    if (w.aim >= 0 && w.bubbles.length) {
      const k = w.bubbles.findIndex((b) => Math.hypot(dx - b.x, dy - b.y) <= b.r + 7);
      if (k >= 0) {
        stopDwell();
        setAim(w.aim, k);
        return;
      }
    }
    if (r < INFO_R) {
      stopDwell();
      setAim(-1, -1);
      return;
    }
    let a = Math.atan2(dx, -dy);
    if (a < 0) a += TAU;
    if (r < HUB_R + 4) {
      setAim(-1, -1);
      const m = w.modes.length;
      if (m > 1) {
        const i = Math.floor(((a + Math.PI / m) % TAU) / (TAU / m)) % m;
        if (i !== w.mode && w.dwellSlice !== i) {
          clearTimeout(w.dwell);
          w.dwellSlice = i;
          w.dwell = setTimeout(() => {
            w.dwellSlice = -1;
            switchMode(i);
          }, MODE_DWELL);
        } else if (i === w.mode) {
          stopDwell();
        }
      }
      return;
    }
    stopDwell();
    const n = w.items.length;
    const span = TAU / n;
    const i = Math.floor(((a + span / 2) % TAU) / span) % n;
    if (r > DISC_R + FRAME_SWELL + 10) {
      // Past the disc: on the aimed wedge's way out to its bubbles, or off the wheel.
      const toBubbles = i === w.aim && w.bubbles.length && r <= BUBBLE_AT + BUBBLE_R + 10;
      setAim(toBubbles ? i : -1, -1);
      return;
    }
    setAim(i, -1);
  }

  function stopDwell() {
    clearTimeout(w.dwell);
    w.dwellSlice = -1;
  }

  /** Where the pointer is on the wheel: a wedge, a bubble, the hub, or off it. */
  function hitAt(lx, ly) {
    const dx = lx - w.x;
    const dy = ly - w.y;
    const r = Math.hypot(dx, dy);
    if (w.bubbles.some((b) => Math.hypot(dx - b.x, dy - b.y) <= b.r + 7)) return "bubble";
    if (r <= INFO_R) return "centre";
    if (r <= HUB_R + 4) return "hub";
    if (r <= DISC_R + FRAME_SWELL + 6) return "wedge";
    return "off";
  }

  // ---------- Doing what is picked ----------

  function pick() {
    if (w.aim < 0 || w.done) return;
    const wedge = w.wedges[w.aim];
    const item = wedge.item;
    const bubble = item.bubbles && w.bubble >= 0 ? item.bubbles[w.bubble] : null;
    if (item.bubbles && !bubble && w.scene !== "desk" && labelOf(item) !== "Share") {
      // Shift: its bubbles say how far; the wedge on its own is not an answer.
      say(tr("Pick how far, on one of its bubbles."));
      return;
    }
    const now = performance.now();
    wedge.flash$.kick(now, TAU / NUDGE.response);
    nudge(wedge.angle);
    run();
    if (w.scene === "desk") {
      deskPick(item, bubble);
      return;
    }
    const mode = w.modes[w.mode];
    const said = caption(w.scene, mode, item, bubble);
    const scene = w.scene;
    const at = [w.x, w.y];
    const how = w.how;
    const byKeys = keysInWheel();
    const index = w.aim;
    w.done = true;
    setTimeout(() => close(), 130);
    addCard(scene, mode, item, bubble, said);
    // Opened by a click, it comes back, so the next wedge is a click away.
    if (how === "click") {
      setTimeout(() => {
        if (w.open || drag) return;
        open(scene, "click", at, mode);
        if (byKeys && focusStayed()) focusWedge(index);
      }, 1100);
    }
  }

  /** Focus left in the closed wheel, or dropped to the page: nobody has taken it elsewhere. */
  function focusStayed() {
    const now = document.activeElement;
    return !now || now === document.body || svg.contains(now);
  }

  /** Says what a desk wedge did to anyone listening, as the caption shows it to everyone else. */
  function announce() {
    if (status) status.textContent = capText.textContent;
  }

  function deskPick(item, bubble) {
    const label = labelOf(item);
    const mode = w.modes[w.mode];
    w.acted = true;
    if (mode === "Apps") {
      say(tr("{} would come to the front.", tr(label)));
      foldAndReturn();
      return;
    }
    if (mode === "Media") {
      if (label === "Pause") desk.playing = !desk.playing;
      else if (label === "Next") desk.track += 1;
      else if (label === "Previous") desk.track = Math.max(1, desk.track - 1);
      else if (label === "Repeat") desk.repeat = !desk.repeat;
      else if (label === "Shuffle") desk.shuffle = !desk.shuffle;
      else if (label === "Player") {
        say(tr("The player would come to the front."));
        foldAndReturn();
        return;
      }
      const aimed = w.aim;
      const byKeys = keysInWheel();
      build();
      setFocusable(true);
      w.aim = -1;
      setAim(aimed, -1);
      w.acted = true;
      w.wedges[aimed].swell$.jump(1);
      if (byKeys) w.wedges[aimed].g.focus({ preventScroll: true });
      announce();
      run();
      return;
    }
    if (mode === "Everyday") {
      const k = w.bubble;
      if (item.level && k >= 0) {
        const byKeys = keysInWheel();
        desk[item.level] = k;
        showBubbles(w.aim);
        w.bubble = k;
        markBubbles();
        if (byKeys && w.bubbles[k]) w.bubbles[k].g.focus({ preventScroll: true });
      } else if (label === "Language") {
        desk.turkish = !desk.turkish;
      } else if (label === "Screenshot" || label === "Record") {
        say(bubble ? tr("{}: {}. In the app, that is.", tr(label), caption("desk", "Everyday", item, bubble).split("· ")[1]) : tr("Pick one of its bubbles."));
        if (bubble) foldAndReturn();
        return;
      } else if (label === "Monitor") {
        say(tr("The system monitor would open."));
        foldAndReturn();
        return;
      } else if (item.bubbles) {
        say(tr("Pick one of its bubbles."));
        text();
        return;
      }
      text();
      announce();
      return;
    }
    // System.
    switch (item.toggle) {
      case "dark":
        // The page turns after the crossfade has its picture: say so once it has.
        turnDark(!isDark(), announce);
        break;
      case "awake":
        desk.awake = !desk.awake;
        break;
      case "mic":
        desk.mic = !desk.mic;
        break;
      default:
        say(tr({
          Lock: "Locked. Well, it would be: this page is only a demo.",
          "Empty Trash": "The trash would be empty now.",
          Desktop: "Every window would step aside.",
          Sleep: "The computer would go to sleep. The demo stays awake.",
        }[label]));
        if (label !== "Empty Trash") foldAndReturn();
        text();
        return;
    }
    refreshDots();
    text();
    if (item.toggle !== "dark") announce();
  }

  function foldAndReturn() {
    const at = [w.x, w.y];
    const mode = w.modes[w.mode];
    const how = w.how;
    const byKeys = keysInWheel();
    const index = w.aim;
    w.done = true;
    setTimeout(() => close(), 140);
    if (how !== "click") return;
    setTimeout(() => {
      if (w.open || drag) return;
      open("desk", "click", at, mode);
      if (byKeys && focusStayed()) focusWedge(index);
    }, 1500);
  }

  let turning = false;

  /** The browser's bar in the page's paper (tokens.css --color-paper, as theme-color reads it). */
  function paintBar(dark) {
    document.querySelectorAll('meta[name="theme-color"]').forEach((meta) => meta.setAttribute("content", dark ? "#140e0b" : "#fbf5ed"));
  }

  /** The page turns as the app's Dark Mode does: one crossfade, the wheel with it. */
  function turnDark(dark, then, remember = true) {
    const apply = () => {
      if (dark) document.documentElement.dataset.theme = "dark";
      else delete document.documentElement.dataset.theme;
      paintBar(dark);
      if (remember) {
        try {
          localStorage.setItem("mcgyver-theme", dark ? "dark" : "light");
        } catch (e) {
          // Private windows may refuse: the page still turns, it just won't remember.
        }
      }
      refreshDots();
      text();
      if (then) then();
    };
    if (document.startViewTransition && !still.matches) {
      turning = true;
      document.startViewTransition(apply).finished.finally(() => (turning = false));
    } else {
      apply();
    }
  }

  let hintTimer = 0;
  function hideHint() {
    clearTimeout(hintTimer);
    if (hint) hint.classList.remove("is-shown");
  }

  function say(message) {
    if (!hint || !message) return;
    hint.textContent = message;
    hint.classList.add("is-shown");
    clearTimeout(hintTimer);
    hintTimer = setTimeout(() => hint.classList.remove("is-shown"), 2600);
  }

  // ---------- Cards: what a drop makes, at the bottom right as the app's do ----------

  // In each language whole: "Cut" is the wedge's word and the card's, but not in Turkish.
  const DONE = TURKISH
    ? {
        Copy: ["Kopyalandı", "Bir klasöre yapıştırılmaya hazır"],
        Cut: ["Kesildi", "Taşınacağı yere yapıştırın"],
        "Copy To": ["Bir klasör seçin", "Uygulamada klasör seçici açılır"],
        "Move To": ["Bir klasör seçin", "Uygulamada klasör seçici açılır"],
        "Copy Path": ["Yol kopyalandı", "Yapıştırılmaya hazır"],
        Terminal: ["Terminal açıldı", "Dosyanın bulunduğu klasörde"],
      }
    : {
        Copy: ["Copied", "Ready to paste in a folder"],
        Cut: ["Cut", "Paste it where it goes"],
        "Copy To": ["Pick a folder", "In the app, a folder picker opens"],
        "Move To": ["Pick a folder", "In the app, a folder picker opens"],
        "Copy Path": ["Path copied", "Ready to paste"],
        Terminal: ["Terminal opened", "In the folder the file is in"],
      };

  function addCard(scene, mode, item, bubble, said) {
    if (!cardsList) return;
    const s = SCENES[scene];
    const label = labelOf(item);
    const what = s.files.length > 1 ? tr("{} files", s.files.length) : s.files[0];
    let title;
    let sub;
    const made = said.includes("›") ? said.split("› ")[1] : null;
    if (mode === "Actions") {
      if (label === "Share") {
        if (!bubble || bubble === "Phone") [title, sub] = [tr("Sent to your phone"), tr("Through KDE Connect")];
        else if (bubble === "Telegram") [title, sub] = [tr("Pick a chat in Telegram"), what];
        else [title, sub] = [tr("Copied: paste in {}", bubble), what];
      } else if (label === "Zip") {
        [title, sub] = [made, tr("Next to {}", what)];
      } else {
        [title, sub] = DONE[label];
      }
    } else if (label === "Shift") {
      [title, sub] = [`${stemOf(s.files[0])}.srt`, tr("Moved by {}", bubble)];
    } else {
      [title, sub] = [made, tr("Next to {}", what)];
    }
    const working = mode === "Actions" ? title : tr(label === "Unzip" ? "Unzipping {}" : mode === "Convert" ? "Converting {}" : "Working on {}", what);
    const li = document.createElement("li");
    li.className = "card";
    li.innerHTML = `
      <svg class="card__ring" viewBox="0 0 30 30" aria-hidden="true">
        <circle class="card__track" cx="15" cy="15" r="12"/>
        <circle class="card__progress" cx="15" cy="15" r="12" stroke-dasharray="75.4" stroke-dashoffset="75.4"/>
        <path class="card__done" d="M9.5 15.5l3.6 3.6 7.4-7.6"/>
      </svg>
      <div class="card__text"><span class="card__title"></span><span class="card__sub"></span></div>
      <button class="card__close" type="button" aria-label="${tr("Cancel")}"><svg viewBox="0 0 12 12" aria-hidden="true"><path d="M2 2l8 8M10 2l-8 8"/></svg></button>`;
    const titleEl = li.querySelector(".card__title");
    const subEl = li.querySelector(".card__sub");
    const ring = li.querySelector(".card__progress");
    const closeBtn = li.querySelector(".card__close");
    titleEl.textContent = working;
    subEl.textContent = mode === "Actions" ? sub : tr("A moment…");
    cardsList.appendChild(li);
    while (cardsList.children.length > 3) cardsList.firstElementChild.remove();
    const quick = mode === "Actions";
    const length = quick ? 260 : 900 + Math.random() * 700;
    let finished = false;
    let leaveTimer = 0;
    const start = performance.now();
    const tick = (now) => {
      if (!li.isConnected || finished) return;
      const t = Math.min(1, (now - start) / length);
      ring.setAttribute("stroke-dashoffset", f(75.4 * (1 - (1 - Math.pow(1 - t, 2)))));
      if (t < 1) requestAnimationFrame(tick);
      else finish();
    };
    const leave = () => {
      li.classList.add("is-leaving");
      setTimeout(() => li.remove(), 320);
    };
    const later = () => {
      clearTimeout(leaveTimer);
      leaveTimer = setTimeout(() => {
        if (li.matches(":hover") || li.contains(document.activeElement)) later();
        else leave();
      }, 3600);
    };
    const finish = () => {
      finished = true;
      ring.setAttribute("stroke-dashoffset", "0");
      li.classList.add("is-done");
      titleEl.textContent = title;
      subEl.textContent = sub;
      closeBtn.setAttribute("aria-label", tr("Put the card away"));
      later();
    };
    closeBtn.addEventListener("click", () => {
      if (!finished) {
        finished = true;
        titleEl.textContent = tr("Cancelled");
        subEl.textContent = tr("Nothing was made");
        closeBtn.setAttribute("aria-label", tr("Put the card away"));
        later();
        return;
      }
      leave();
    });
    requestAnimationFrame(tick);
  }

  // ---------- Pointer: clicking, aiming, dragging files ----------

  let drag = null;

  stage.addEventListener("pointermove", (e) => {
    pointer = [e.clientX, e.clientY];
    if (drag) return;
    if (w.open && e.pointerType !== "touch") {
      const [lx, ly] = toLocal(e.clientX, e.clientY);
      aimAt(lx, ly);
    }
  });

  stage.addEventListener("pointerleave", () => {
    // A theme's crossfade covers the page for a moment; the pointer hasn't gone anywhere.
    if (!drag && !turning && w.open && w.how === "click") setAim(-1, -1);
  });

  // A click on the wheel picks; a click on the stage off it closes it, as the app's does.
  // A finger: where it went down, and what was aimed before it did.
  let press = null;
  stage.addEventListener("pointerdown", (e) => {
    press = null;
    if (drag || e.target.closest(".file, .keycap, .card")) return;
    if (!w.open || w.closingAt) return;
    if (e.button === 2) {
      close();
      return;
    }
    const [lx, ly] = toLocal(e.clientX, e.clientY);
    const touch = e.pointerType === "touch";
    if (touch) {
      press = { x: e.clientX, y: e.clientY, aim: w.aim };
      aimAt(lx, ly);
    }
    const hit = hitAt(lx, ly);
    if (hit === "off") {
      // A finger may be starting to scroll the page: it closes the wheel on a tap, at pointerup.
      if (!touch) close();
      return;
    }
    if (hit === "centre") nudge(Math.atan2(lx - w.x, -(ly - w.y)));
  });

  stage.addEventListener("pointerup", (e) => {
    if (drag || !w.open || w.closingAt || e.button !== 0) return;
    if (e.target.closest(".file, .keycap, .card, .hub__hit")) return;
    const [lx, ly] = toLocal(e.clientX, e.clientY);
    const hit = hitAt(lx, ly);
    const tap = e.pointerType === "touch" && press && Math.hypot(e.clientX - press.x, e.clientY - press.y) < 10;
    if (hit === "off") {
      if (tap) close();
      return;
    }
    if (hit === "wedge" || hit === "bubble") {
      aimAt(lx, ly);
      // No hover on a finger: a wedge with bubbles shows them first, and the next tap picks.
      if (press && hit === "wedge" && w.aim >= 0 && w.aim !== press.aim && w.items[w.aim].bubbles) return;
      pick();
    }
  });

  stage.addEventListener("contextmenu", (e) => {
    if (w.open) {
      e.preventDefault();
      close();
    }
  });

  // Files: a click opens their wheel; a drag carries them, and Shift opens it under the pointer.
  stage.querySelectorAll(".file").forEach((button) => {
    button.addEventListener("pointerdown", (e) => {
      if (e.button !== 0) return;
      clearTimeout(holdTimer);
      button.setPointerCapture(e.pointerId);
      drag = {
        button,
        scene: button.dataset.file,
        startX: e.clientX,
        startY: e.clientY,
        moved: false,
        ghost: null,
        // A pen has no Shift either: it rests, as a finger does.
        touch: e.pointerType !== "mouse",
        restTimer: 0,
      };
    });
    button.addEventListener("pointermove", (e) => {
      if (!drag || drag.button !== button) return;
      pointer = [e.clientX, e.clientY];
      const dx = e.clientX - drag.startX;
      const dy = e.clientY - drag.startY;
      const swiping = drag.touch && desktop.scrollWidth > desktop.clientWidth + 1;
      if (!drag.moved && (swiping ? Math.hypot(dx, dy) > 12 && Math.abs(dy) >= Math.abs(dx) : Math.hypot(dx, dy) > 6)) {
        drag.moved = true;
        if (w.open) close(true);
        startGhost(button);
        stage.classList.add("is-busy");
        say(tr(drag.touch ? "Rest the file on the space a moment." : "Now hold Shift."));
      }
      if (!drag.moved) return;
      moveGhost(e.clientX, e.clientY);
      const [lx, ly] = toLocal(e.clientX, e.clientY);
      if (w.open && w.how === "drag") {
        aimAt(lx, ly);
      } else if (drag.touch) {
        clearTimeout(drag.restTimer);
        if (onStage(e.clientX, e.clientY)) {
          drag.restTimer = setTimeout(() => {
            if (!drag || w.open) return;
            open(drag.scene, "drag", toLocal(...pointer));
            hideHint();
          }, 420);
        }
      } else if (shiftDown && onStage(e.clientX, e.clientY)) {
        open(drag.scene, "drag", [lx, ly]);
        hideHint();
      }
    });
    const end = (e) => {
      if (!drag || drag.button !== button) return;
      const d = drag;
      drag = null;
      clearTimeout(d.restTimer);
      stopGhost();
      stage.classList.remove("is-busy");
      if (!d.moved) {
        if (e.type === "pointerup") open(d.scene, "click", null);
        return;
      }
      if (w.open && w.how === "drag") {
        const [lx, ly] = toLocal(e.clientX, e.clientY);
        aimAt(lx, ly);
        if (w.aim >= 0 && e.type === "pointerup") {
          w.how = "drop";
          pick();
        } else {
          say(tr("Dropped off the wheel: nothing happens."));
          close();
        }
      } else if (e.type === "pointerup") {
        say(tr(d.touch ? "Rest the file on the space until the wheel opens." : "Hold Shift while the file is over the space."));
      }
    };
    button.addEventListener("pointerup", end);
    button.addEventListener("pointercancel", end);
    // Keyboard: Enter or Space opens the wheel and moves into it.
    button.addEventListener("click", (e) => {
      if (e.detail === 0) {
        open(button.dataset.file, "click", null);
        w.opener = button;
        focusWedge(0);
      }
    });
  });

  let ghost = null;
  function startGhost(button) {
    ghost = document.createElement("div");
    ghost.className = "ghost";
    ghost.innerHTML = button.innerHTML;
    document.body.appendChild(ghost);
    button.classList.add("is-held");
  }
  function moveGhost(x, y) {
    if (!ghost) return;
    ghost.style.setProperty("--x", `${x - 42}px`);
    ghost.style.setProperty("--y", `${y - 30}px`);
  }
  function stopGhost() {
    if (ghost) ghost.remove();
    ghost = null;
    stage.querySelectorAll(".file.is-held").forEach((b) => b.classList.remove("is-held"));
  }

  // ---------- Shift: held during a drag it opens the files' wheel; on its own, the desk's ----------

  let shiftDown = false;
  let holdTimer = 0;
  let stageVisible = false;

  new IntersectionObserver(
    (entries) => {
      stageVisible = entries[0].isIntersecting;
    },
    { threshold: 0.35 },
  ).observe(stage);

  window.addEventListener("keydown", (e) => {
    if (e.key !== "Shift") {
      // Another key with Shift (Shift+Tab, capitals) is not Shift on its own.
      clearTimeout(holdTimer);
      if (w.open && w.how === "hold") {
        w.acted = true;
        close();
        return;
      }
      // Escape a bubble handled itself; elsewhere on the page Escape isn't the wheel's.
      if (e.key === "Escape" && w.open && !e.defaultPrevented && stage.contains(document.activeElement)) {
        close();
        (w.opener || stage).focus({ preventScroll: true });
      }
      return;
    }
    if (e.repeat || shiftDown) return;
    shiftDown = true;
    if (drag && drag.moved && pointer && onStage(...pointer)) {
      open(drag.scene, "drag", toLocal(...pointer));
      hideHint();
      return;
    }
    if (!drag && stageVisible && !(w.open && w.how === "hold")) {
      holdTimer = setTimeout(() => {
        if (drag) return;
        const at = pointer && onStage(...pointer) ? toLocal(...pointer) : null;
        open("desk", "hold", at, w.scene === "desk" ? w.modes[w.mode] : undefined);
        say(tr("Let go of Shift on a wedge to do it."));
      }, HOLD_MS);
    }
  });

  window.addEventListener("keyup", (e) => {
    if (e.key !== "Shift") return;
    shiftDown = false;
    clearTimeout(holdTimer);
    if (!w.open) return;
    if (w.how === "drag") {
      // The drag carries on; Shift again reopens the wheel.
      close();
    } else if (w.how === "hold") {
      if (w.aim >= 0 && !w.acted) pick();
      setTimeout(() => {
        if (w.how === "hold") close();
      }, 160);
    }
  });

  window.addEventListener("blur", () => {
    shiftDown = false;
    clearTimeout(holdTimer);
    // Shift let go in another window never comes back here as a keyup.
    if (w.open && w.how === "hold") close();
  });

  const keycap = stage.querySelector("[data-shift]");
  if (keycap) {
    keycap.addEventListener("pointerdown", () => keycap.classList.add("is-down"));
    ["pointerup", "pointerleave", "pointercancel"].forEach((type) =>
      keycap.addEventListener(type, () => keycap.classList.remove("is-down")),
    );
    keycap.addEventListener("click", (e) => {
      open("desk", "click", null);
      w.opener = keycap;
      if (e.detail === 0) focusWedge(0);
    });
  }

  // ---------- Keyboard on the wheel ----------

  function focusWedge(i) {
    const wedge = w.wedges[(i + w.wedges.length) % w.wedges.length];
    if (wedge) wedge.g.focus({ preventScroll: true });
  }

  function wedgeKey(e, i) {
    const n = w.wedges.length;
    switch (e.key) {
      case "ArrowRight":
      case "ArrowDown":
        e.preventDefault();
        focusWedge(i + 1);
        break;
      case "ArrowLeft":
      case "ArrowUp":
        e.preventDefault();
        focusWedge(i - 1 + n);
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        setAim(i, -1);
        if (w.wedges[i].item.bubbles && w.bubbles.length) {
          w.bubbles[0].g.focus({ preventScroll: true });
        } else {
          pick();
        }
        break;
      default:
    }
  }

  // ---------- The tour drives the stage ----------

  function show(spec, focus) {
    const [scene, mode] = spec.split("|");
    open(scene, "click", null, mode);
    document.querySelectorAll(".step").forEach((step) => step.classList.toggle("is-current", step.dataset.step === spec));
    if (focus) focusWedge(0);
  }

  document.querySelectorAll("[data-show]").forEach((button) => {
    button.addEventListener("click", (e) => {
      const narrow = window.matchMedia("(max-width: 59.99rem)").matches;
      if (narrow) stage.scrollIntoView({ behavior: still.matches ? "auto" : "smooth", block: "center" });
      show(button.dataset.show, e.detail === 0);
      w.opener = button;
    });
  });

  document.querySelectorAll("[data-try]").forEach((link) => {
    link.addEventListener("click", (e) => {
      e.preventDefault();
      stage.scrollIntoView({ behavior: still.matches ? "auto" : "smooth", block: "center" });
      if (e.detail === 0) {
        show("holiday.jpg|Convert", true);
      }
    });
  });

  // On a wide screen the stage stays put while the steps go by, each setting it as it arrives.
  const wide = window.matchMedia("(min-width: 60rem)");
  const steps = new IntersectionObserver(
    (entries) => {
      // Keys working the wheel keep it: a rebuild would take their focus away.
      if (!wide.matches || drag || w.how === "hold" || w.how === "drag" || keysInWheel()) return;
      const seen = entries.filter((x) => x.isIntersecting);
      if (!seen.length) return;
      const spec = seen[seen.length - 1].target.dataset.step;
      if (w.open && !w.closingAt && `${w.scene}|${w.modes[w.mode]}` === spec) return;
      show(spec);
    },
    { rootMargin: "-48% 0px -48% 0px" },
  );
  document.querySelectorAll(".step").forEach((step) => steps.observe(step));

  // ---------- Start: the wheel opens on its own, as on the app's first start ----------

  const resized = new ResizeObserver(() => layoutStage());
  resized.observe(stage);
  resized.observe(desktop);
  layoutStage();
  body.setAttribute("transform", "translate(-9999 -9999)");
  const begin = () => {
    open("holiday.jpg", "click", null, "Convert");
    const touch = window.matchMedia("(hover: none)").matches;
    say(tr(touch ? "Tap a wedge, or drag a file onto the space." : "Hover a wedge, then click it. Or drag a file onto the space and hold Shift."));
  };
  if (document.fonts && document.fonts.ready) document.fonts.ready.then(() => setTimeout(begin, 350));
  else setTimeout(begin, 350);

  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", (e) => {
    let saved = null;
    try {
      saved = localStorage.getItem("mcgyver-theme");
    } catch (err) {
      // No storage: follow the system.
    }
    if (!saved) turnDark(e.matches, null, false);
  });
})();

// ---------- The page around the stage ----------
(() => {
  "use strict";

  // N12: the banner retracts as the page goes down and comes back as it goes up.
  const masthead = document.querySelector("[data-masthead]");
  const banner = document.querySelector("[data-banner]");
  const root = document.documentElement;
  const measure = () => {
    if (!masthead) return;
    const bannerH = banner && !banner.hidden ? banner.offsetHeight : 0;
    root.style.setProperty("--banner-h", `${bannerH}px`);
    root.style.setProperty("--nav-h", `${masthead.offsetHeight - bannerH}px`);
  };
  let dismissed = false;
  try {
    dismissed = localStorage.getItem("mcgyver-banner") === "closed";
  } catch (e) {
    // No storage: the banner shows each visit.
  }
  if (banner && dismissed) banner.hidden = true;
  document.querySelector("[data-banner-close]")?.addEventListener("click", () => {
    banner.hidden = true;
    try {
      localStorage.setItem("mcgyver-banner", "closed");
    } catch (e) {
      // Closed for this visit only.
    }
    measure();
  });
  measure();
  new ResizeObserver(measure).observe(masthead);
  let lastY = window.scrollY;
  window.addEventListener(
    "scroll",
    () => {
      const y = window.scrollY;
      masthead.classList.toggle("is-scrolled", y > 8);
      if (Math.abs(y - lastY) > 6) {
        masthead.classList.toggle("is-retracted", y > lastY && y > 120);
        lastY = y;
      }
    },
    { passive: true },
  );

  const strip = document.querySelector("[data-marquee]");
  document.querySelector("[data-marquee-toggle]")?.addEventListener("click", (e) => {
    const paused = strip.classList.toggle("is-paused");
    e.currentTarget.setAttribute("aria-pressed", paused ? "true" : "false");
  });

  // Download: the visitor's platform first and biggest, and named on the hero's button.
  const ua = (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || navigator.userAgent;
  const phone = /iPhone|iPad|Android/i.test(navigator.userAgent) || (/mac/i.test(ua) && navigator.maxTouchPoints > 1);
  const os = phone ? null : /mac/i.test(ua) ? "mac" : /win/i.test(ua) ? "windows" : /linux|x11|cros/i.test(ua) ? "linux" : null;
  const platforms = document.querySelector("[data-platforms]");
  if (os && platforms) {
    const yours = platforms.querySelector(`[data-os="${os}"]`);
    if (yours) {
      platforms.prepend(yours);
      yours.classList.add("is-yours");
      platforms.querySelectorAll(".platform__get").forEach((a) => a.classList.add("btn--soft"));
      yours.querySelector(".platform__get").classList.remove("btn--soft");
      const cta = document.querySelector("[data-download-cta]");
      const names = document.documentElement.lang === "tr"
        ? { mac: "Mac için indir", windows: "Windows için indir", linux: "Linux için indir" }
        : { mac: "Download for Mac", windows: "Download for Windows", linux: "Download for Linux" };
      if (cta) cta.textContent = names[os];
    }
  }

  // A language picked here is kept, so the English page stops sending a Turkish browser to /tr/.
  document.querySelectorAll("[data-lang]").forEach((link) => {
    link.addEventListener("click", () => {
      try {
        localStorage.setItem("mcgyver-lang", link.dataset.lang);
      } catch (e) {
        // No storage: the next visit is guessed from the browser again.
      }
    });
  });
})();
