/* McGyver website — interactions
   No dependencies. Every animated piece has a static reduced-motion state. */
(() => {
  'use strict';

  const $ = (s, r = document) => r.querySelector(s);
  const $$ = (s, r = document) => [...r.querySelectorAll(s)];
  const NS = 'http://www.w3.org/2000/svg';
  const reduceMQ = matchMedia('(prefers-reduced-motion: reduce)');
  const fineMQ = matchMedia('(hover: hover) and (pointer: fine)');
  const reduced = () => reduceMQ.matches;
  const wait = (ms) => new Promise((r) => setTimeout(r, reduced() ? 0 : ms));
  const EASE_MOVE = 'cubic-bezier(.45,.05,.25,1)';
  reduceMQ.addEventListener('change', () => {
    if (!reduced()) return;
    // CSS obeys the media query; complete any in-flight Web Animations too.
    document.getAnimations().forEach((a) => {
      if (a.constructor.name !== 'Animation') return;
      try { a.finish(); } catch { a.cancel(); }
    });
  });

  /* ------------------------------------------------------------------
     Analytics: event names from the brief. Never sends file names or
     anything the visitor typed beyond a known extension.
     ------------------------------------------------------------------ */
  function track(event, props = {}) {
    if (typeof window.gtag === 'function') window.gtag('event', event, props);
  }
  document.addEventListener('click', (e) => {
    const a = e.target.closest('[data-track]');
    if (a) track(a.dataset.track, a.dataset.platform ? { platform: a.dataset.platform } : {});
  });

  /* ------------------------------------------------------------------
     Words: written in English, shown in Turkish on /tr/. What the wheel
     already says (modes, wedges, its captions) comes from wheel.js.
     ------------------------------------------------------------------ */
  const TURKISH = document.documentElement.lang === 'tr';
  const TR = {
    // The scenes' desktop.
    'File': 'Dosya',
    'Edit': 'Düzen',
    'View': 'Görüntü',
    'Go': 'Git',
    'Help': 'Yardım',
    'Mon 12 May&nbsp;&nbsp;9:41': '12 May Pzt&nbsp;&nbsp;09:41',
    'Favorites': 'Favoriler',
    'Recents': 'Son Kullanılanlar',
    'Applications': 'Uygulamalar',
    'Documents': 'Belgeler',
    'Downloads': 'İndirilenler',
    'Pictures': 'Resimler',
    'Locations': 'Konumlar',
    'Vacation': 'Tatil',
    '· 1 item': '· 1 öğe',
    'Projects': 'Projeler',
    'Screenshots': 'Ekran Görüntüleri',
    'invoice.pdf': 'fatura.pdf',
    'held': 'basılı',
    // How it works.
    'Step 1 of 3: drag a file.': 'Adım 1/3: bir dosya sürükleyin.',
    'Step 2 of 3: hold Shift. The wheel opens around the cursor.': 'Adım 2/3: Shift’i basılı tutun. Çark imlecin çevresinde açılır.',
    'Step 3 of 3: choose WEBP. holiday.webp lands next to the original.': 'Adım 3/3: WEBP’yi seçin. tatil.webp, asıl dosyanın yanına gelir.',
    'Pause walkthrough': 'Gösterimi duraklat',
    'Play walkthrough': 'Gösterimi oynat',
    // Seven modes.
    'holiday.jpg in hand': 'tatil.jpg elinizde',
    'No file selected': 'Seçili dosya yok',
    '<b>Convert</b> opens first. Aim at a format and let go: holiday.webp lands next to holiday.jpg. A format the file already has is skipped.':
      '<b>Dönüştür</b> ilk açılır. Bir biçime nişan alıp bırakın: tatil.webp, tatil.jpg’nin yanına gelir. Dosyanın zaten olduğu biçim sunulmaz.',
    '<b>Tools</b> fit the file in hand. Pictures resize, crop and lose their EXIF. Videos compress, trim and mute. PDFs split and rotate.':
      '<b>Araçlar</b> elinizdeki dosyaya göre değişir. Resimler boyutlanır, kırpılır, EXIF bilgilerinden arınır. Videolar sıkıştırılır, kesilir, sessize alınır. PDF’ler bölünür ve döndürülür.',
    '<b>Actions</b> work on any file or folder: zip it, copy or move it, copy its path, open a terminal there, or share it.':
      '<b>Eylemler</b> her dosya ve klasörde çalışır: zip’leyin, kopyalayın ya da taşıyın, yolunu kopyalayın, orada bir terminal açın ya da paylaşın.',
    '<b>Apps</b> shows what\'s open, the last one you used on top, then the apps you picked in settings. Alt+Tab, in a circle.':
      '<b>Uygulamalar</b> açık olanları gösterir, en son kullandığınız en üstte; ardından ayarlardan seçtiğiniz uygulamalar. Çember biçiminde bir Alt+Tab.',
    '<b>Media</b> controls the player that is playing: play or pause, next, previous, repeat, shuffle, and the player itself. On a Mac it works with Spotify and Music, without repeat and shuffle.':
      '<b>Medya</b> çalan oynatıcıyı yönetir: oynatın ya da duraklatın, sonraki, önceki, tekrarla, karıştır ve oynatıcının kendisi. Mac’te Spotify ve Müzik ile çalışır, tekrarla ve karıştır olmadan.',
    '<b>Everyday</b> puts the keyboard layout, the language, volume, brightness, screenshots, screen recording and how busy the computer is in reach. Which of them show depends on the system: a Mac has no Brightness wedge.':
      '<b>Günlük</b> klavye düzenini, dili, sesi, parlaklığı, ekran görüntüsünü, ekran kaydını ve bilgisayarın ne kadar meşgul olduğunu elinizin altına getirir. Hangilerinin görüneceği sisteme bağlıdır: Mac’te Parlaklık dilimi yoktur.',
    '<b>System</b> switches: lock the screen, Dark Mode, Stay Awake, the microphone, the trash, the desktop and sleep. A Mac has no Desktop wedge.':
      '<b>Sistem</b> düğmeleri: ekran kilidi, Karanlık Mod, Uyanık Kal, mikrofon, çöp kutusu, masaüstü ve uyku. Mac’te Masaüstü dilimi yoktur.',
    '{} wedges': '{} dilim',
    'with a file': 'dosyayla',
    'hold Shift alone': 'Shift tek başına',
    // Try the wheel.
    'PDF document': 'PDF belgesi',
    'Audio · FFmpeg': 'Ses · FFmpeg',
    '{} has nothing in {}, so the real wheel shows no slice for it.': '{} için {} modunda bir şey yok; gerçek çarkta da bu modun dilimi olmaz.',
    '{} wheel for {}': '{} çarkı: {}',
    'Would create {}': '{} dosyasını oluşturur',
    '{} is in hand. Drag it onto the space and hold Shift, or press Open wheel.': '{} elinizde. Alana sürükleyip Shift’i basılı tutun ya da Çarkı aç düğmesine basın.',
    'Drag a sample file onto the space, then hold Shift.': 'Bir örnek dosyayı alana sürükleyin, sonra Shift’i basılı tutun.',
    'The wheel folds when Shift is let go. Hold it again to bring it back.': 'Shift bırakılınca çark kapanır. Geri getirmek için yeniden basılı tutun.',
    'Pick a sample file first. The wheel needs something in hand.': 'Önce bir örnek dosya seçin. Çarkın açılması için elinizde bir şey olmalı.',
    'Open wheel': 'Çarkı aç',
    'Close wheel': 'Çarkı kapat',
    '{} wheel open for {}. Point at a wedge or use the arrow keys. Esc closes.': '{} çarkı açık: {}. Bir dilime nişan alın ya da ok tuşlarını kullanın. Esc kapatır.',
    'Wheel closed. {} is still in hand.': 'Çark kapandı. {} hâlâ elinizde.',
    'MOVE': 'TAŞI',
    'Simulated: {} on {}. Try another, or reset.': 'Simülasyon: {}, {} için seçildi. Başka birini deneyin ya da sıfırlayın.',
    'Pick a sample file to begin.': 'Başlamak için bir örnek dosya seçin.',
    'Dragging {}. Hold Shift over the space to open the wheel.': '{} sürükleniyor. Çarkı açmak için alanın üstünde Shift’i basılı tutun.',
    'Drag cancelled. Pick a sample file to continue.': 'Sürükleme iptal edildi. Devam etmek için bir örnek dosya seçin.',
    'Dropped off the wheel, so nothing happens, as in the app.': 'Çarkın dışına bırakıldı; uygulamadaki gibi hiçbir şey olmaz.',
    'Drop the file inside the space to hold it.': 'Tutmak için dosyayı alanın içine bırakın.',
    // The Shift-anywhere wheel.
    'How': 'Nasıl',
    'How it works': 'Nasıl çalışır',
    'Modes': 'Modlar',
    'Seven modes': 'Yedi mod',
    'Try it': 'Deneyin',
    'Try the wheel': 'Çarkı deneyin',
    'Supported files': 'Desteklenen dosyalar',
    'Download': 'İndir',
    'Get McGyver': 'McGyver’ı indirin',
    'FAQ': 'SSS',
    'Questions': 'Sorular',
    'Jump to': 'Bölüme git',
    'Release on a wedge': 'Bir dilimin üstünde bırakın',
    '{} · release Shift to go': '{} · gitmek için Shift’i bırakın',
    // Supported files.
    'All': 'Tümü',
    'Images': 'Resimler',
    'Spreadsheets': 'Tablolar',
    'Presentations': 'Sunumlar',
    'Slides': 'Sunumlar',
    'Archives': 'Arşivler',
    'Compressed files': 'Sıkıştırılmış dosyalar',
    'Subtitles': 'Altyazılar',
    'Folders': 'Klasörler',
    'JPEG, PNG, camera RAW, PSD, SVG, HEIC with libheif…': 'JPEG, PNG, kamera RAW, PSD, SVG, libheif ile HEIC…',
    'PPTX; PPT and ODP to MD': 'PPTX; PPT ve ODP yalnızca MD’ye',
    'Any folder': 'Her klasör',
    'Needs FFmpeg': 'FFmpeg gerekir',
    'Flatten': 'Düzleştir',
    'Shift by half a second or a second': 'Yarım ya da bir saniye kaydır',
    'Convert to': 'Hedef biçimler',
    'Where libheif can write it': 'libheif yazabiliyorsa',
    ' (where the system supports it)': ' (sistem destekliyorsa)',
    'No match for “{}”. It may not be supported yet.': '“{}” için sonuç yok. Henüz desteklenmiyor olabilir.',
    'Ask for it on GitHub': 'GitHub’da isteyin',
    // `{} {} of file{}{}.` is the count, kind or kinds, then the two below.
    '{} {} of file{}{}.': '{2}{3}{0} dosya türü var.',
    ' in {}': '{} kategorisinde ',
    ' match “{}”': '“{}” ile eşleşen ',
    // Download and the header.
    'Your system': 'Sisteminiz',
    'Download for Mac': 'Mac için indir',
    'Download for Windows': 'Windows için indir',
    'Download for Linux': 'Linux için indir',
    'Open menu': 'Menüyü aç',
    'Close menu': 'Menüyü kapat',
    // The corner card, in the app's words (src/i18n.rs).
    'Working…': 'İşleniyor…',
    'Saved as {}': '{} olarak kaydedildi',
    'Copied; paste it anywhere': 'Kopyalandı; istediğiniz yere yapıştırın',
    'Cut; paste it where it goes': 'Kesildi; taşınacağı yere yapıştırın',
    'Copied to {}': '{} klasörüne kopyalandı',
    'Moved to {}': '{} klasörüne taşındı',
    Documents: 'Belgeler',
    'Path copied': 'Yol kopyalandı',
    'Opened {}': '{} açıldı',
    'Sent to {}': '{} cihazına gönderildi',
    Phone: 'Telefon',
    'Pick a chat in Telegram': 'Telegram’da bir sohbet seçin',
    'Copied: paste in {}': 'Kopyalandı: {} içine yapıştırın',
    Cancel: 'İptal',
    Cancelled: 'İptal edildi',
    Close: 'Kapat',
  };
  /** `english` in the page's language; each `{}` takes the next of `args`, `{1}` a given one. */
  function t(english, ...args) {
    const text = (TURKISH && TR[english]) || McWheel.tr(english);
    let k = 0;
    return text.replace(/\{(\d?)\}/g, (_, n) => args[n ? +n : k++]);
  }

  /* ------------------------------------------------------------------
     Shared artwork
     ------------------------------------------------------------------ */
  const THUMB = `<svg viewBox="0 0 84 64" preserveAspectRatio="xMidYMid slice" width="100%" height="100%" aria-hidden="true">
    <rect width="84" height="64" fill="#9DB7D8"/><rect y="20" width="84" height="44" fill="#F0B987" opacity=".55"/>
    <path d="M0 46 22 18l10 11 12-17 17 22 9-8 14 20v18H0z" fill="#2E3D55"/>
    <path d="M22 18l5 6-4 1-3 4-3-4zM44 12l6 8-4-1-3 4-2-5-3 3z" fill="#fff"/>
    <path d="M0 48h84v16H0z" fill="#3B6A87"/><path d="M0 50h84" stroke="#9DC1D8" stroke-width="1" opacity=".6"/></svg>`;
  $$('[data-thumb]').forEach((t) => (t.innerHTML = THUMB));

  // Shadow is baked into the cursor art (no CSS filter on a moving element).
  const CURSOR = `<svg viewBox="0 0 24 32" width="24" height="32"><path d="M3 4v22l6-5.5 4 9 4-1.8-4-8.7h8z" fill="#000" opacity=".35" transform="translate(1 2)"/><path d="M2 2v22l6-5.5 4 9 4-1.8-4-8.7h8z" fill="#fff" stroke="#111" stroke-width="1.5" stroke-linejoin="round"/></svg>`;

  /* ------------------------------------------------------------------
     Wheel: McGyver's own, from wheel.js, with the app's geometry,
     springs and wedges. viewBox is -210..210, centred on 0,0.
     ------------------------------------------------------------------ */
  const VB = 420;
  const createWheel = McWheel.create;
  const SCENES = McWheel.SCENES;

  /* ------------------------------------------------------------------
     Scene: a small desktop authored at 800×H.
     Hero: mapped onto the photo's screen. How it works: scaled to fit.
     ------------------------------------------------------------------ */
  const WEBP = SCENES['holiday.jpg'].modes.Convert.indexOf('WEBP');
  // The scene's photo as this page names it (tatil.jpg on /tr/), and what it becomes.
  const PHOTO = SCENES['holiday.jpg'].files[0];
  const PHOTO_WEBP = PHOTO.replace(/\.jpg$/, '.webp');

  function createScene(host, { finder, h = 480, fit = true, onWheel } = {}) {
    const L = finder
      ? { file: [192, 92], result: [312, 92], grab: [244, 132], center: [548, 270], start: [470, 450], away: [470, 330] }
      : { file: [110, 150], result: [110, 290], grab: [162, 190], center: [520, 240], start: [300, 420], away: [300, 400] };
    const scene = document.createElement('div');
    scene.className = 'scene desktop ' + (finder ? 'finder' : 'plain');
    scene.style.height = h + 'px';
    scene.innerHTML = `
      ${finder ? `<div class="s-menubar"><b>Finder</b><span>${t('File')}</span><span>${t('Edit')}</span><span>${t('View')}</span><span>${t('Go')}</span><span>${t('Window')}</span><span>${t('Help')}</span><span class="r">${t('Mon 12 May&nbsp;&nbsp;9:41')}</span></div>
      <div class="s-window" style="height:${h - 62}px"><div class="s-side"><div class="s-dots"><i></i><i></i><i></i></div><p>${t('Favorites')}</p><div>${t('Recents')}</div><div>${t('Applications')}</div><div class="on">${t('Desktop')}</div><div>${t('Documents')}</div><div>${t('Downloads')}</div><div>${t('Pictures')}</div><p>${t('Locations')}</p><div>Macintosh HD</div></div><div class="s-bar">${t('Vacation')} <span>${t('· 1 item')}</span></div></div>` : ''}
      ${finder ? '' : `<div class="s-icons" aria-hidden="true"><div class="s-ico"><i></i>${t('Projects')}</div><div class="s-ico"><i></i>${t('Screenshots')}</div><div class="s-ico doc"><i></i>${t('invoice.pdf')}</div></div><div class="s-dock" aria-hidden="true"><i></i><i></i><i></i><i></i></div>`}
      <div class="s-file s-orig" style="left:${L.file[0]}px;top:${L.file[1]}px"><div class="thumb">${THUMB}</div><span class="name">${PHOTO}</span></div>
      <div class="s-file s-result" style="left:${L.result[0]}px;top:${L.result[1]}px"><div class="thumb">${THUMB}</div><span class="name">${PHOTO_WEBP}</span><span class="tag">WEBP</span></div>
      <svg class="wheel s-wheel" viewBox="-210 -210 420 420" style="left:${L.center[0] - 150}px;top:${L.center[1] - 150}px" aria-hidden="true"></svg>
      <div class="s-ghost">${THUMB}</div>
      <div class="s-cursor">${CURSOR}</div>
      <div class="s-key"><kbd>⇧ Shift</kbd>${t('held')}</div>`;
    host.appendChild(scene);

    if (fit) {
      const compactMQ = matchMedia('(max-width: 640px)');
      function layoutScene() {
        // Preserve the 800px scene coordinates; frame the source and whole wheel on phones.
        const compact = compactMQ.matches && !finder;
        const s = host.getBoundingClientRect().width / (compact ? 560 : 800);
        const x = compact ? -110 * s : 0;
        scene.style.transform = `translateX(${x}px) scale(${s})`;
      }
      new ResizeObserver(layoutScene).observe(host);
      compactMQ.addEventListener('change', layoutScene);
    }

    const orig = $('.s-orig', scene), res = $('.s-result', scene), ghost = $('.s-ghost', scene),
      cursor = $('.s-cursor', scene), key = $('.s-key', scene);
    const wheel = createWheel($('.s-wheel', scene));
    wheel.show('holiday.jpg', 'Convert');

    const k = 300 / VB;
    const wedgePoint = (i) => { const [x, y] = wheel.midpoint(i); return [L.center[0] + x * k, L.center[1] + y * k]; };
    const tc = (p) => `translate(${p[0] - 2}px, ${p[1] - 2}px)`;
    const tg = (p) => `translate(${p[0] - 26}px, ${p[1] - 18}px)`;
    let pos = L.start;

    // Hands move in curves: sample a quadratic Bézier bent 18% off the straight line.
    function curve(from, to, n = 14) {
      const [x0, y0] = from, [x1, y1] = to;
      const len = Math.hypot(x1 - x0, y1 - y0) || 1;
      const cx = (x0 + x1) / 2 - ((y1 - y0) / len) * len * 0.18;
      const cy = (y0 + y1) / 2 + ((x1 - x0) / len) * len * 0.18;
      return Array.from({ length: n + 1 }, (_, i) => {
        const t = i / n, u = 1 - t;
        return [u * u * x0 + 2 * u * t * cx + t * t * x1, u * u * y0 + 2 * u * t * cy + t * t * y1];
      });
    }
    function moveTo(to, dur = 0) {
      const from = pos;
      pos = to;
      [cursor, ghost].forEach((n) => n.getAnimations().forEach((a) => a.cancel()));
      cursor.style.transform = tc(to);
      ghost.style.transform = tg(to);
      if (!dur || reduced()) return Promise.resolve();
      const pts = curve(from, to);
      const opts = { duration: dur, easing: EASE_MOVE, fill: 'both' };
      const a = cursor.animate(pts.map((p) => ({ transform: tc(p) })), opts);
      const b = ghost.animate(pts.map((p) => ({ transform: tg(p) })), { ...opts, delay: 40 });
      b.finished.then(() => b.cancel(), () => {});
      return a.finished.then(() => a.cancel(), () => {});
    }
    function press() {
      if (reduced()) return;
      cursor.firstElementChild.animate([{ transform: 'scale(1)' }, { transform: 'scale(.88)' }, { transform: 'scale(1)' }], { duration: 200, easing: 'ease-out' });
    }
    let wheelOpen = false, wheelActive = -1;
    function set(s) {
      if ('cursor' in s) moveTo(s.cursor, 0);
      if ('cursorOn' in s) cursor.classList.toggle('is-hidden', !s.cursorOn);
      if ('selected' in s) orig.classList.toggle('is-selected', s.selected);
      if ('dim' in s) orig.classList.toggle('is-dim', s.dim);
      if ('ghost' in s) ghost.classList.toggle('is-on', s.ghost);
      if ('key' in s) key.classList.toggle('is-on', s.key);
      if ('wheel' in s) { wheelOpen = s.wheel; wheel.open(s.wheel); }
      if ('hover' in s) wheel.setHover(s.hover);
      if ('active' in s) { wheelActive = s.active; wheel.setActive(s.active); if (s.active >= 0 && wheelOpen) wheel.confirm(s.active); }
      if ('result' in s) res.classList.toggle('is-shown', s.result);
      if (onWheel && ('wheel' in s || 'active' in s)) onWheel(wheelOpen, wheelOpen && wheelActive >= 0);
    }
    const reset = { cursor: L.start, cursorOn: true, selected: false, dim: false, ghost: false, key: false, wheel: false, hover: -1, active: -1, result: false };
    const showcase = { cursor: wedgePoint(WEBP), cursorOn: true, selected: true, dim: true, ghost: false, key: true, wheel: true, active: WEBP, result: false };
    set(reset);
    function pause() {
      [cursor, ghost].forEach((n) => n.getAnimations().forEach((a) => a.pause()));
    }
    return { set, moveTo, press, pause, L, wedgePoint, reset, showcase, el: scene };
  }

  /* ------------------------------------------------------------------
     01 Hero — the live scene is projected onto the laptop screen in the
     photo with a homography (matrix3d), so the UI on screen is real.
     ------------------------------------------------------------------ */
  const IMG_W = 1536, IMG_H = 864, SCR_W = 800, SCR_H = 500;
  // Screen corners measured in the photo (px): TL, TR, BR, BL. Expanded ~1px to cover the bezel edge.
  const SCREEN = [[752, 156], [1451, 141], [1438, 634], [725, 599]];

  function homography(w, h, [[x0, y0], [x1, y1], [x2, y2], [x3, y3]]) {
    // Unit square → quad (Heckbert), then scale the square to w×h.
    const sx = x0 - x1 + x2 - x3, sy = y0 - y1 + y2 - y3;
    const dx1 = x1 - x2, dx2 = x3 - x2, dy1 = y1 - y2, dy2 = y3 - y2;
    const den = dx1 * dy2 - dx2 * dy1;
    const g = (sx * dy2 - dx2 * sy) / den, hh = (dx1 * sy - sx * dy1) / den;
    const a = (x1 - x0 + g * x1) / w, b = (x3 - x0 + hh * x3) / h;
    const d = (y1 - y0 + g * y1) / w, e = (y3 - y0 + hh * y3) / h;
    const f = (n) => +n.toFixed(8);
    return `matrix3d(${f(a)},${f(d)},0,${f(g / w)},${f(b)},${f(e)},0,${f(hh / h)},0,0,1,0,${x0},${y0},0,1)`;
  }

  const heroMedia = $('#hero-media'), heroFrame = $('#hero-frame'), heroScreen = $('#hero-screen'), spill = $('#hero-spill');
  heroScreen.style.transform = homography(SCR_W, SCR_H, SCREEN);
  const hero = createScene($('#hero-scene'), {
    finder: true, h: SCR_H, fit: false,
    onWheel: (open, act) => spill.style.setProperty('--spill', act ? 1 : open ? 0.55 : 0),
  });
  const wideMQ = matchMedia('(min-width: 1100px)');

  function layoutHero() {
    const r = heroMedia.getBoundingClientRect();
    let k, left, top;
    if (wideMQ.matches) {
      // Cover the hero, anchored right so the laptop keeps clear of the copy.
      k = Math.max(r.width / IMG_W, r.height / IMG_H);
      left = r.width - IMG_W * k;
      top = (r.height - IMG_H * k) / 2;
    } else {
      // Phones and tablets: crop in on the screen so the product stays legible.
      k = Math.min((r.width * 1.12) / 697, (r.height * 0.86) / 470);
      left = r.width / 2 - 1080 * k;
      top = r.height / 2 - 430 * k;
    }
    heroFrame.style.transform = `translate(${left.toFixed(1)}px, ${top.toFixed(1)}px) scale(${k.toFixed(4)})`;
  }
  new ResizeObserver(layoutHero).observe(heroMedia);
  wideMQ.addEventListener('change', layoutHero);

  // Gentle parallax: the photo and the projected screen move as one plate.
  (() => {
    let tx = 0, ty = 0, x = 0, y = 0, raf = 0;
    const step = () => {
      x += (tx - x) * 0.08; y += (ty - y) * 0.08;
      heroMedia.style.translate = `${x.toFixed(2)}px ${y.toFixed(2)}px`;
      raf = Math.abs(tx - x) + Math.abs(ty - y) > 0.05 ? requestAnimationFrame(step) : 0;
    };
    function reset() {
      cancelAnimationFrame(raf);
      raf = 0; tx = ty = x = y = 0;
      heroMedia.style.removeProperty('translate');
    }
    $('.hero').addEventListener('pointerleave', reset);
    reduceMQ.addEventListener('change', reset);
    fineMQ.addEventListener('change', reset);
    $('.hero').addEventListener('pointermove', (e) => {
      if (!fineMQ.matches || reduced()) return;
      tx = (e.clientX / innerWidth - 0.5) * -12;
      ty = (e.clientY / innerHeight - 0.5) * -8;
      if (!raf) raf = requestAnimationFrame(step);
    });
  })();

  const heroToggle = $('#hero-toggle');
  let heroRun = 0, heroPaused = false, heroVisible = true;

  async function heroLoop(id) {
    const S = hero, on = () => id === heroRun;
    while (on()) {
      S.set({ ...S.reset, cursorOn: false });
      await wait(400); if (!on()) return;
      S.set({ cursorOn: true });
      await wait(200); if (!on()) return;
      await S.moveTo(S.L.grab, 900); if (!on()) return;
      S.press(); S.set({ selected: true });
      await wait(320); if (!on()) return;
      S.set({ ghost: true, dim: true });
      await S.moveTo(S.L.center, 1000); if (!on()) return;
      S.set({ key: true });
      await wait(120); if (!on()) return;
      S.set({ wheel: true, ghost: false });
      await wait(700); if (!on()) return;
      await S.moveTo(S.wedgePoint(WEBP), 420); if (!on()) return;
      S.set({ hover: WEBP });
      await wait(160); if (!on()) return;
      S.set({ active: WEBP });
      await wait(800); if (!on()) return;
      S.set({ wheel: false, key: false });
      await wait(200); if (!on()) return;
      S.set({ result: true, selected: false, dim: false, hover: -1, active: -1 });
      S.moveTo(S.L.away, 900);
      await wait(2600); if (!on()) return;
      // Fade out before anything resets, so the loop never visibly jumps.
      S.set({ result: false, cursorOn: false });
      await wait(450);
    }
  }
  function heroSync() {
    heroRun++;
    const play = !heroPaused && heroVisible && !reduced() && !document.hidden;
    if (play) heroLoop(heroRun);
    else hero.set(hero.showcase);
    heroToggle.hidden = reduced();
  }
  heroToggle.addEventListener('click', () => {
    heroPaused = !heroPaused;
    heroToggle.setAttribute('aria-pressed', String(heroPaused));
    heroToggle.querySelector('use').setAttribute('href', heroPaused ? '#i-play' : '#i-pause');
    heroToggle.querySelector('span').textContent = t(heroPaused ? 'Play' : 'Pause');
    heroSync();
  });
  new IntersectionObserver(([e]) => { const v = e.isIntersecting; if (v !== heroVisible) { heroVisible = v; heroSync(); } }, { threshold: 0.1 }).observe(heroMedia);
  document.addEventListener('visibilitychange', heroSync);
  reduceMQ.addEventListener('change', () => { heroSync(); howSync(); });
  // Page-load intro: the screen powers on with an empty desktop, then the loop starts.
  const root = document.documentElement;
  if (root.classList.contains('intro')) {
    hero.set({ ...hero.reset, cursorOn: false });
    heroToggle.hidden = reduced();
    setTimeout(heroSync, 820);
    setTimeout(() => root.classList.remove('intro'), 1250);
  } else heroSync();

  /* ------------------------------------------------------------------
     02 How it works — three steps, auto-advances, fully manual too.
     ------------------------------------------------------------------ */
  const how = createScene($('#how-scene'), { finder: false });
  const steps = $$('#how-steps .step');
  const howPlay = $('#how-play');
  const STEP_TEXT = ['Step 1 of 3: drag a file.', 'Step 2 of 3: hold Shift. The wheel opens around the cursor.', 'Step 3 of 3: choose WEBP. holiday.webp lands next to the original.'];
  let step = 0, howRun = 0, howAuto = true, howVisible = false, howTimer = null;

  async function playStep(n) {
    const id = ++howRun, on = () => id === howRun, S = how;
    if (n === 1) {
      S.set({ ...S.reset, cursor: S.L.grab, selected: true });
      await wait(320); if (!on()) return;
      S.set({ ghost: true, dim: true });
      S.moveTo([S.L.center[0] - 120, S.L.center[1]], 1100);
    } else if (n === 2) {
      S.set({ selected: true, dim: true, result: false, hover: -1, active: -1, ghost: true });
      await S.moveTo(S.L.center, 500); if (!on()) return;
      S.set({ key: true, wheel: true, ghost: false });
    } else {
      S.set({ selected: true, dim: true, result: false, ghost: false, key: true, wheel: true });
      await S.moveTo(S.wedgePoint(WEBP), 450); if (!on()) return;
      S.set({ active: WEBP });
      await wait(reduced() ? 0 : 1000); if (!on()) return;
      S.set({ wheel: false, key: false, dim: false, selected: false, result: true, active: -1 });
    }
  }
  function goStep(n, user) {
    step = ((n - 1 + 3) % 3) + 1;
    steps.forEach((s, i) => {
      const on = i === step - 1;
      s.classList.toggle('is-active', on);
      s.querySelector('button').setAttribute('aria-current', on ? 'step' : 'false');
    });
    $('#how-count').textContent = `0${step} / 03`;
    $('#how-bar').style.transform = `scaleX(${step / 3})`;
    if (user) { $('#how-live').textContent = t(STEP_TEXT[step - 1]); setAuto(false); }
    playStep(step);
    schedule();
  }
  function schedule() {
    clearTimeout(howTimer);
    if (howAuto && howVisible && !document.hidden && !reduced()) howTimer = setTimeout(() => goStep(step + 1), step === 3 ? 4200 : 3200);
  }
  function setAuto(on) {
    howAuto = on;
    if (!on) { howRun++; how.pause(); }
    howPlay.hidden = reduced();
    howPlay.setAttribute('aria-pressed', String(!on));
    howPlay.setAttribute('aria-label', t(on ? 'Pause walkthrough' : 'Play walkthrough'));
    howPlay.querySelector('use').setAttribute('href', on ? '#i-pause' : '#i-play');
    schedule();
  }
  function howSync() {
    howPlay.hidden = reduced();
    if (reduced()) {
      setAuto(false);
      goStep(step || 1);
    } else if (document.hidden || !howVisible) {
      howRun++;
      how.pause();
      schedule();
    } else if (howAuto) {
      goStep(step || 1);
    } else {
      // A manually selected step returns as a complete still, never a stranded cursor.
      const still = step === 1
        ? { cursor: [how.L.center[0] - 120, how.L.center[1]], selected: true, dim: true, ghost: true }
        : step === 2
          ? { cursor: how.L.center, selected: true, dim: true, wheel: true, key: true }
          : { cursor: how.L.away, result: true };
      how.set({ ...how.reset, ...still });
      schedule();
    }
  }
  document.addEventListener('visibilitychange', howSync);
  steps.forEach((s, i) => s.querySelector('button').addEventListener('click', () => goStep(i + 1, true)));
  $('#how-prev').addEventListener('click', () => goStep(step - 1, true));
  $('#how-next').addEventListener('click', () => goStep(step + 1, true));
  howPlay.addEventListener('click', () => { setAuto(!howAuto); if (howAuto) goStep(step || 1); });
  new IntersectionObserver(([e]) => { howVisible = e.isIntersecting; howSync(); }, { threshold: 0.15 }).observe($('.how'));
  if (reduced()) setAuto(false);
  goStep(1);

  /* ------------------------------------------------------------------
     03 Seven modes
     ------------------------------------------------------------------ */
  const MODES = {
    convert: { ctx: 'holiday.jpg in hand', withFile: true, scene: 'holiday.jpg', mode: 'Convert', active: 'WEBP',
      text: '<b>Convert</b> opens first. Aim at a format and let go: holiday.webp lands next to holiday.jpg. A format the file already has is skipped.' },
    tools: { ctx: 'holiday.jpg in hand', withFile: true, scene: 'holiday.jpg', mode: 'Tools', active: 'Resize',
      text: '<b>Tools</b> fit the file in hand. Pictures resize, crop and lose their EXIF. Videos compress, trim and mute. PDFs split and rotate.' },
    actions: { ctx: 'holiday.jpg in hand', withFile: true, scene: 'holiday.jpg', mode: 'Actions', active: 'Copy Path',
      text: '<b>Actions</b> work on any file or folder: zip it, copy or move it, copy its path, open a terminal there, or share it.' },
    apps: { ctx: 'No file selected', scene: 'desk', mode: 'Apps', active: 'Files',
      text: '<b>Apps</b> shows what\'s open, the last one you used on top, then the apps you picked in settings. Alt+Tab, in a circle.' },
    media: { ctx: 'No file selected', scene: 'desk', mode: 'Media', active: 'Pause',
      text: '<b>Media</b> controls the player that is playing: play or pause, next, previous, repeat, shuffle, and the player itself. On a Mac it works with Spotify and Music, without repeat and shuffle.' },
    everyday: { ctx: 'No file selected', scene: 'desk', mode: 'Everyday', active: 'Volume',
      text: '<b>Everyday</b> puts the keyboard layout, the language, volume, brightness, screenshots, screen recording and how busy the computer is in reach. Which of them show depends on the system: a Mac has no Brightness wedge.' },
    system: { ctx: 'No file selected', scene: 'desk', mode: 'System', active: 'Dark Mode',
      text: '<b>System</b> switches: lock the screen, Dark Mode, Stay Awake, the microphone, the trash, the desktop and sleep. A Mac has no Desktop wedge.' },
  };
  const modeWheel = createWheel($('#mode-wheel'));
  const tabs = $$('#mode-tabs .mode');
  // One indicator slides between tabs instead of each tab drawing its own.
  const ind = Object.assign(document.createElement('i'), { className: 'mode-ind' });
  ind.setAttribute('aria-hidden', 'true');
  $('#mode-tabs').prepend(ind);
  function moveInd() {
    const t = tabs.find((x) => x.getAttribute('aria-selected') === 'true');
    if (!t) return;
    if (matchMedia('(max-width: 1024px)').matches) { ind.style.height = ''; ind.style.transform = `translateX(${t.offsetLeft + 10}px) scaleX(${(t.offsetWidth - 20) / 100})`; }
    else { ind.style.height = t.offsetHeight - 20 + 'px'; ind.style.transform = `translateY(${t.offsetTop + 10}px)`; }
  }
  new ResizeObserver(() => moveInd()).observe($('#mode-tabs'));
  requestAnimationFrame(() => requestAnimationFrame(() => ind.classList.add('is-ready')));
  const modeText = $('#mode-text');
  let modeTextT;
  function showMode(id, focus) {
    const m = MODES[id];
    tabs.forEach((t) => {
      const on = t.dataset.mode === id;
      t.setAttribute('aria-selected', on);
      t.tabIndex = on ? 0 : -1;
      if (on && focus) { t.focus(); t.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: reduced() ? 'auto' : 'smooth' }); }
    });
    $('#mode-panel').setAttribute('aria-labelledby', 'tab-' + id);
    modeWheel.show(m.scene, m.mode, { active: m.active });
    clearTimeout(modeTextT);
    modeText.style.opacity = 0;
    modeTextT = setTimeout(() => { modeText.innerHTML = t(m.text); modeText.style.opacity = 1; }, reduced() ? 0 : 140);
    $('#mode-ctx').innerHTML = `<i class="dot ${m.withFile ? 'g' : 'off'}"></i>${t(m.ctx)}`;
    $('#mode-count').textContent = `${t('{} wedges', SCENES[m.scene].modes[m.mode].length)} · ${t(m.withFile ? 'with a file' : 'hold Shift alone')}`;
    moveInd();
  }
  tabs.forEach((t, i) => {
    t.addEventListener('click', () => showMode(t.dataset.mode));
    t.addEventListener('keydown', (e) => {
      const d = { ArrowDown: 1, ArrowRight: 1, ArrowUp: -1, ArrowLeft: -1 }[e.key];
      if (e.key === 'Home' || e.key === 'End') { e.preventDefault(); showMode(tabs[e.key === 'Home' ? 0 : tabs.length - 1].dataset.mode, true); }
      if (!d) return;
      e.preventDefault();
      showMode(tabs[(i + d + tabs.length) % tabs.length].dataset.mode, true);
    });
  });
  const narrowMQ = matchMedia('(max-width: 1024px)');
  const syncOrientation = () => $('#mode-tabs').setAttribute('aria-orientation', narrowMQ.matches ? 'horizontal' : 'vertical');
  narrowMQ.addEventListener('change', syncOrientation);
  syncOrientation();
  showMode('convert');
  new IntersectionObserver(([e], o) => { if (e.isIntersecting) { modeWheel.open(true); o.disconnect(); } }, { threshold: 0.3 }).observe($('#mode-wheel'));

  /* ------------------------------------------------------------------
     06 Try McGyver — simulated, no real files involved.
     ------------------------------------------------------------------ */
  // Each one is a scene in wheel.js, which holds the wedges the app shows for it. `name` is the
  // scene's key; the page shows the scene's own file name (tatil.jpg on /tr/).
  const FILES = [
    { id: 'jpg', name: 'holiday.jpg', ext: 'JPG', kind: 'Image' },
    { id: 'mov', name: 'trip.mov', ext: 'MOV', kind: 'Video · FFmpeg' },
    { id: 'pdf', name: 'report.pdf', ext: 'PDF', kind: 'PDF document' },
    { id: 'docx', name: 'notes.docx', ext: 'DOCX', kind: 'Document' },
    { id: 'zip', name: 'photos.zip', ext: 'ZIP', kind: 'Archive' },
    { id: 'flac', name: 'song.flac', ext: 'FLAC', kind: 'Audio · FFmpeg' },
  ];
  const shown = (f) => SCENES[f.name].files[0];
  const MODE_NAMES = { convert: 'Convert', tools: 'Tools', actions: 'Actions' };

  const space = $('#space'), dwheelBox = $('#dwheel'), held = $('#held'), status = $('#demo-status'),
    openBtn = $('#open-wheel'), ghostEl = $('#dghost'), demoPanel = $('#demo'), demoSvg = $('#demo-wheel');
  const filesBox = $('#demo-files');
  const D = { file: null, mode: 'convert', open: false, started: false, dragging: null, lastPt: null, pointerIn: false, input: 'pointer', justDragged: false, committing: false, touchClickUntil: 0 };
  let pickTimer = null, pickVersion = 0;
  function cancelPick() {
    clearTimeout(pickTimer);
    pickTimer = null;
    pickVersion++;
    D.committing = false;
  }

  filesBox.innerHTML = FILES.map((f) => `<button class="dfile" data-id="${f.id}" aria-pressed="false"><span class="ext" aria-hidden="true">${f.ext}</span><span><b>${shown(f)}</b><small>${t(f.kind)}</small></span></button>`).join('');

  const demoWheel = createWheel(demoSvg, {
    interactive: true,
    onPick: (i, label, bubble) => pick(i, label, bubble),
    onHover: (i) => previewHub(i),
    onMode: (name) => setMode(name.toLowerCase(), true),
  });

  const itemsFor = (f, mode) => SCENES[f.name].modes[MODE_NAMES[mode]] || [];
  function say(msg, warn) { status.textContent = msg; status.classList.toggle('warn', !!warn); }
  const baseName = (f) => shown(f).replace(/\.[^.]+$/, '');

  // The wheel's caption already says what the aimed wedge would make; a finger feels the aim too.
  function previewHub(i) {
    if (!D.file || !D.open || i < 0) return;
    if (D.input === 'touch' && navigator.vibrate) navigator.vibrate(8);
  }

  function syncModes() {
    $$('#demo-modes button').forEach((b) => {
      const m = b.dataset.mode;
      const empty = !!(D.file && !itemsFor(D.file, m).length);
      b.setAttribute('aria-disabled', String(empty));
      b.setAttribute('aria-pressed', String(m === D.mode));
    });
  }
  function setMode(m, fromUser) {
    if (D.file && !itemsFor(D.file, m).length) {
      say(t('{} has nothing in {}, so the real wheel shows no slice for it.', shown(D.file), t(MODE_NAMES[m])), true);
      return;
    }
    cancelPick();
    D.mode = m;
    syncModes();
    if (fromUser) track('demo_mode_change', { mode: m });
    if (D.open) renderWheel(D.input === 'key');
  }
  function renderWheel(focusFirst) {
    demoWheel.show(D.file.name, MODE_NAMES[D.mode]);
    demoSvg.setAttribute('aria-label', t('{} wheel for {}', t(MODE_NAMES[D.mode]), shown(D.file)));
    demoWheel.nodes.forEach((n, i) => {
      const l = demoWheel.items[i];
      if (D.mode === 'convert' && l !== 'Unzip') n.setAttribute('aria-description', t('Would create {}', `${baseName(D.file)}.${l.toLowerCase()}`));
    });
    if (focusFirst) requestAnimationFrame(() => focusItem(0));
  }

  function selectFile(f, quiet) {
    closeWheel(true);
    D.file = f;
    $$('.dfile', filesBox).forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.id === f.id)));
    if (!itemsFor(f, D.mode).length) D.mode = 'convert';
    syncModes();
    held.hidden = false;
    $('#held-tile').textContent = f.ext;
    $('#held-name').textContent = shown(f);
    $('#space-empty').hidden = true;
    if (!D.started) { D.started = true; track('demo_start'); }
    if (!quiet) say(t('{} is in hand. Drag it onto the space and hold Shift, or press Open wheel.', shown(f)));
  }

  function openWheel(x, y, focusFirst) {
    if (!D.file) { say(t('Pick a sample file first. The wheel needs something in hand.'), true); return; }
    const r = space.getBoundingClientRect();
    const half = dwheelBox.offsetWidth / 2 || 200;
    // Keep the whole wheel inside the space; centre it when the space is too small.
    const fit = (v, size) => (v == null || size < half * 2 + 16 ? size / 2 : Math.min(Math.max(v, half + 8), size - half - 8));
    dwheelBox.style.left = fit(x, r.width) + 'px';
    dwheelBox.style.top = fit(y, r.height) + 'px';
    dwheelBox.hidden = false;
    dwheelBox.inert = false;
    D.open = true;
    renderWheel(false);
    space.classList.add('is-open');
    requestAnimationFrame(() => demoWheel.open(true));
    openBtn.setAttribute('aria-expanded', 'true');
    openBtn.textContent = t('Close wheel');
    say(t('{} wheel open for {}. Point at a wedge or use the arrow keys. Esc closes.', t(MODE_NAMES[D.mode]), shown(D.file)));
    if (focusFirst) { D.input = 'key'; requestAnimationFrame(() => focusItem(0)); }
  }
  function closeWheel(silent) {
    cancelPick();
    if (!D.open) return;
    D.open = false;
    demoWheel.open(false);
    demoWheel.setActive(-1);
    demoWheel.nodes.forEach((n) => n.setAttribute('tabindex', '-1'));
    dwheelBox.inert = true;
    space.classList.remove('is-open', 'is-dragging');
    openBtn.setAttribute('aria-expanded', 'false');
    openBtn.textContent = t('Open wheel');
    if (!silent && D.file) say(t('Wheel closed. {} is still in hand.', shown(D.file)));
  }
  function focusItem(i) {
    const nodes = demoWheel.nodes;
    if (!nodes.length) return;
    i = (i + nodes.length) % nodes.length;
    nodes.forEach((n, j) => n.setAttribute('tabindex', j === i ? '0' : '-1'));
    nodes[i].focus({ preventScroll: true });
  }

  // What the app's card says once the pick is done (src/jobs.rs, src/actions.rs, src/desk/mod.rs).
  function doneText(f, mode, label, bubble, made) {
    const name = shown(f);
    if (label === 'Unzip') return t('Saved as {}', baseName(f));
    if (mode !== 'actions') return t('Saved as {}', made);
    switch (label) {
      case 'Zip': return t('Saved as {}', `${name}.zip`);
      case 'Copy': return t('Copied; paste it anywhere');
      case 'Cut': return t('Cut; paste it where it goes');
      case 'Copy To': return t('Copied to {}', t('Documents'));
      case 'Move To': return t('Moved to {}', t('Documents'));
      case 'Copy Path': return t('Path copied');
      case 'Terminal': return t('Opened {}', 'Terminal');
      default:
        if (!bubble || bubble === 'Phone') return t('Sent to {}', t('Phone'));
        return bubble === 'Telegram' ? t('Pick a chat in Telegram') : t('Copied: paste in {}', bubble);
    }
  }

  // The card the app puts up at the bottom-right corner after a pick (src/overlay.rs paint_card):
  // a ring while it works, a tick once done; the pick on top, how it went below. It stays 1.6 s
  // (longer under the pointer), then fades; its cross cancels it, or puts it away.
  const cards = Object.assign(document.createElement('ol'), { className: 'dcards' });
  cards.setAttribute('aria-live', 'polite');
  space.appendChild(cards);
  function addCard(title, done, quick) {
    const li = Object.assign(document.createElement('li'), { className: 'dcard' });
    li.innerHTML = `<svg class="dcard-mark" viewBox="0 0 30 30" aria-hidden="true"><circle class="track" cx="15" cy="15" r="11"/><circle class="ring" cx="15" cy="15" r="11" pathLength="100" stroke-dasharray="100" stroke-dashoffset="100"/><path class="tick" d="M9.5 15.5l3.6 3.6 7.4-7.6"/><path class="cross" d="M10.5 10.5l9 9M19.5 10.5l-9 9"/></svg><span class="dcard-text"><b></b><small></small></span><button class="dcard-close" type="button" aria-label="${t('Cancel')}"><svg viewBox="0 0 12 12" aria-hidden="true"><path d="M2.5 2.5l7 7M9.5 2.5l-7 7"/></svg></button>`;
    const sub = $('small', li), ring = $('.ring', li), close = $('.dcard-close', li);
    $('b', li).textContent = title;
    sub.textContent = t('Working…');
    cards.appendChild(li);
    while (cards.children.length > 3) cards.firstElementChild.remove();
    let finished = false, leaveT = 0;
    const length = reduced() ? 0 : quick ? 260 : 900 + Math.random() * 700, start = performance.now();
    const leave = () => { li.classList.add('is-leaving'); setTimeout(() => li.remove(), reduced() ? 0 : 300); };
    const later = () => {
      clearTimeout(leaveT);
      leaveT = setTimeout(() => (li.matches(':hover') ? later() : leave()), 1600);
    };
    const end = (cancelled) => {
      finished = true;
      li.classList.add(cancelled ? 'is-cancelled' : 'is-done');
      sub.textContent = cancelled ? t('Cancelled') : done;
      close.setAttribute('aria-label', t('Close'));
      later();
    };
    const tick = (now) => {
      if (finished || !li.isConnected) return;
      const k = length ? Math.min(1, (now - start) / length) : 1;
      ring.setAttribute('stroke-dashoffset', String(100 * (1 - k * (2 - k))));
      if (k < 1) requestAnimationFrame(tick); else end(false);
    };
    requestAnimationFrame(tick);
    close.addEventListener('click', (e) => { e.stopPropagation(); if (finished) leave(); else end(true); });
  }

  function pick(i, label, bubble) {
    if (!D.open || !D.file || D.committing) return;
    label = label || demoWheel.items[i];
    if (!label) return;
    D.committing = true;
    const version = ++pickVersion;
    demoWheel.setActive(i);
    demoWheel.confirm(i);
    // The card's title is what the wheel's caption said the pick would do.
    const title = $('.caption__text', demoSvg).textContent;
    const made = title.includes('› ') ? title.split('› ')[1] : '';
    const f = D.file, mode = D.mode;
    track('demo_action_select', { mode, action: label, kind: f.ext });
    pickTimer = setTimeout(() => {
      if (version !== pickVersion) return;
      pickTimer = null;
      closeWheel(true);
      addCard(title, doneText(f, mode, label, bubble, made), mode === 'actions');
      say(t('Simulated: {} on {}. Try another, or reset.', t(label), shown(f)));
      track('demo_complete', { mode, action: label, kind: f.ext });
    }, reduced() ? 0 : 130);
  }
  function reset() {
    closeWheel(true);
    cards.replaceChildren();
    D.file = null; D.mode = 'convert';
    $$('.dfile', filesBox).forEach((b) => b.setAttribute('aria-pressed', 'false'));
    held.hidden = true;
    $('#space-empty').hidden = false;
    syncModes();
    say(t('Pick a sample file to begin.'));
  }

  $$('#demo-modes button').forEach((b) => b.addEventListener('click', () => setMode(b.dataset.mode, true)));
  openBtn.addEventListener('click', () => (D.open ? closeWheel() : openWheel(null, null, true)));
  $('#demo-reset').addEventListener('click', (e) => {
    reset();
    if (!reduced()) $('svg', e.currentTarget).animate([{ transform: 'rotate(0)' }, { transform: 'rotate(-360deg)' }], { duration: 480, easing: 'cubic-bezier(.16,1,.3,1)' });
  });

  // Wheel keyboard: arrows, Home/End, number keys, Enter/Space, Esc.
  demoSvg.addEventListener('keydown', (e) => {
    const nodes = demoWheel.nodes, cur = nodes.indexOf(document.activeElement);
    D.input = 'key';
    if (['ArrowRight', 'ArrowDown'].includes(e.key)) { e.preventDefault(); focusItem(cur + 1); }
    else if (['ArrowLeft', 'ArrowUp'].includes(e.key)) { e.preventDefault(); focusItem(cur - 1); }
    else if (e.key === 'Home') { e.preventDefault(); focusItem(0); }
    else if (e.key === 'End') { e.preventDefault(); focusItem(nodes.length - 1); }
    else if (/^[1-9]$/.test(e.key) && +e.key <= nodes.length) { e.preventDefault(); pick(+e.key - 1); }
    else if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); if (cur >= 0) pick(cur); }
  });
  // Focus leaving the wheel closes it.
  demoSvg.addEventListener('focusout', (e) => {
    if (D.open && D.input === 'key' && !demoSvg.contains(e.relatedTarget) && e.relatedTarget !== openBtn) closeWheel();
  });
  space.addEventListener('click', (e) => {
    // A long-press release can synthesize a click on the captured space.
    if (performance.now() < D.touchClickUntil) return;
    if (D.open && !e.target.closest('.wheel__body')) closeWheel();
  });
  space.addEventListener('pointerenter', () => (D.pointerIn = true));
  space.addEventListener('pointerleave', () => (D.pointerIn = false));
  // Aim at a wedge on the disc; past its edge nothing is aimed at, as in the app.
  space.addEventListener('pointermove', (e) => {
    D.lastPt = [e.clientX, e.clientY];
    if (e.pointerType === 'mouse') D.input = 'pointer';
    if (D.open && !D.dragging && D.input === 'pointer') demoWheel.setHover(demoWheel.indexAt(e.clientX, e.clientY), true);
  });

  // Touch: long-press in the space opens the wheel under the finger; slide to choose, lift to commit.
  space.addEventListener('pointerdown', (e) => {
    if (e.pointerType !== 'touch' || !D.file || D.open || e.target.closest('button, .dcards')) return;
    const id = e.pointerId, start = [e.clientX, e.clientY];
    let opened = false;
    const t = setTimeout(() => {
      opened = true;
      D.input = 'touch';
      space.classList.add('is-dragging');
      const r = space.getBoundingClientRect();
      openWheel(start[0] - r.left, start[1] - r.top);
      try { space.setPointerCapture(id); } catch (_) {}
    }, 350);
    const move = (ev) => {
      if (ev.pointerId !== id) return;
      if (!opened && Math.hypot(ev.clientX - start[0], ev.clientY - start[1]) > 10) cancel();
      if (opened) demoWheel.setHover(demoWheel.indexAt(ev.clientX, ev.clientY), true);
    };
    const end = (ev) => {
      if (ev.pointerId !== id) return;
      cancel();
      if (opened) {
        D.touchClickUntil = performance.now() + 500;
        space.classList.remove('is-dragging');
        const i = demoWheel.indexAt(ev.clientX, ev.clientY);
        if (ev.type === 'pointerup' && i >= 0) pick(i);
        else closeWheel();
      }
    };
    function cancel() {
      clearTimeout(t);
      space.removeEventListener('pointermove', move);
      space.removeEventListener('pointerup', end);
      space.removeEventListener('pointercancel', end);
    }
    space.addEventListener('pointermove', move);
    space.addEventListener('pointerup', end);
    space.addEventListener('pointercancel', end);
  });

  const overSpace = () => D.pointerIn || space.matches(':hover');
  function demoActive() { return D.dragging || overSpace() || demoPanel.contains(document.activeElement); }
  function localPt() {
    if (!D.lastPt) return [null, null];
    const r = space.getBoundingClientRect();
    const x = D.lastPt[0] - r.left, y = D.lastPt[1] - r.top;
    return x >= 0 && y >= 0 && x <= r.width && y <= r.height ? [x, y] : [null, null];
  }

  // Drag a sample file into the space. A click without movement selects it (click handler below).
  filesBox.addEventListener('pointerdown', (e) => {
    const b = e.target.closest('.dfile');
    if (!b || e.button !== 0 || e.pointerType === 'touch') return;
    const f = FILES.find((x) => x.id === b.dataset.id);
    const start = [e.clientX, e.clientY];
    let moved = false;
    let gx = e.clientX, gy = e.clientY, tx = gx, ty = gy, raf = 0;
    const follow = () => {
      gx += (tx - gx) * 0.3; gy += (ty - gy) * 0.3;
      const tilt = Math.max(-8, Math.min(8, (tx - gx) * 0.4));
      ghostEl.style.transform = `translate(${gx + 14}px, ${gy + 14}px) rotate(${tilt.toFixed(1)}deg)`;
      raf = D.dragging ? requestAnimationFrame(follow) : 0;
    };
    b.setPointerCapture(e.pointerId);
    const move = (ev) => {
      D.lastPt = [ev.clientX, ev.clientY];
      tx = ev.clientX; ty = ev.clientY;
      if (!moved && Math.hypot(ev.clientX - start[0], ev.clientY - start[1]) > 6) {
        moved = true;
        D.dragging = { file: f };
        D.input = 'pointer';
        ghostEl.querySelector('i').textContent = f.ext;
        ghostEl.querySelector('span').textContent = shown(f);
        ghostEl.style.display = 'flex';
        if (reduced()) gx = tx, gy = ty;
        raf = requestAnimationFrame(follow);
        say(t('Dragging {}. Hold Shift over the space to open the wheel.', shown(f)));
      }
      if (!moved) return;
      const [lx] = localPt();
      D.pointerIn = lx != null;
      space.classList.toggle('is-over', D.pointerIn && !D.open);
      ghostEl.classList.toggle('is-over', D.pointerIn && !D.open);
      if (ev.shiftKey && !D.open && D.pointerIn) { selectFile(f, true); const [x, y] = localPt(); openWheel(x, y); }
      if (D.open) demoWheel.setHover(demoWheel.indexAt(ev.clientX, ev.clientY), true);
    };
    const up = (ev) => {
      b.removeEventListener('pointermove', move);
      b.removeEventListener('pointerup', up);
      b.removeEventListener('pointercancel', up);
      ghostEl.style.display = 'none';
      ghostEl.classList.remove('is-over');
      space.classList.remove('is-over');
      cancelAnimationFrame(raf);
      if (!moved) return;
      D.justDragged = true;
      setTimeout(() => (D.justDragged = false), 0);
      const [lx] = localPt();
      D.dragging = null;
      D.pointerIn = lx != null;
      if (ev.type === 'pointercancel') {
        closeWheel(true);
        say(t('Drag cancelled. Pick a sample file to continue.'));
        return;
      }
      if (D.open) {
        const i = demoWheel.indexAt(ev.clientX, ev.clientY);
        if (i >= 0 && ev.type === 'pointerup') pick(i);
        else {
          closeWheel(true);
          if (lx != null) selectFile(f, true);
          say(t('Dropped off the wheel, so nothing happens, as in the app.'));
        }
      } else if (lx != null) selectFile(f);
      else say(t('Drop the file inside the space to hold it.'));
    };
    b.addEventListener('pointermove', move);
    b.addEventListener('pointerup', up);
    b.addEventListener('pointercancel', up);
  });
  // Click covers mouse, touch, keyboard and screen-reader activation.
  filesBox.addEventListener('click', (e) => {
    const b = e.target.closest('.dfile');
    if (!b || D.justDragged) return;
    selectFile(FILES.find((x) => x.id === b.dataset.id));
  });
  dwheelBox.inert = true;
  syncModes();

  /* ------------------------------------------------------------------
     Signature: hold Shift anywhere on the page and a wheel opens at the
     pointer with the page's sections. Release on a wedge to go there.
     Shift held with any other key (Shift+Tab, Shift+click) never triggers.
     ------------------------------------------------------------------ */
  const NAV = [['how', 'How', 'How it works'], ['modes', 'Modes', 'Seven modes'], ['try', 'Try it', 'Try the wheel'], ['files', 'Files', 'Supported files'], ['download', 'Download', 'Get McGyver'], ['faq', 'FAQ', 'Questions']];
  const navBox = $('#navwheel'), navSvg = $('#nav-wheel-svg');
  const navWheel = createWheel(navSvg, {
    interactive: true,
    onPick: (i) => navGo(i),
    onHover: (i) => navWheel.setHub(i >= 0 ? t(NAV[i][1]) : t('Jump to'), i >= 0 ? t('{} · release Shift to go', t(NAV[i][2])) : t('Release on a wedge')),
  });
  navWheel.setItems(NAV.map((n) => t(n[1])), { center: t('Jump to'), sub: t('Release on a wedge') });
  let navOpen = false, mouse = null;
  addEventListener('pointermove', (e) => {
    if (e.pointerType !== 'mouse') return;
    mouse = [e.clientX, e.clientY];
    if (navOpen) navWheel.setHover(navWheel.indexAt(e.clientX, e.clientY, { cap: false }), true);
  }, { passive: true });
  function openNav() {
    if (!mouse) return;
    const half = navBox.offsetWidth / 2 || 160;
    const x = Math.min(Math.max(mouse[0], half + 8), innerWidth - half - 8);
    const y = Math.min(Math.max(mouse[1], half + 8), innerHeight - half - 8);
    navBox.style.left = x + 'px';
    navBox.style.top = y + 'px';
    navBox.hidden = false;
    navOpen = true;
    navWheel.setHover(-1);
    requestAnimationFrame(() => navWheel.open(true));
    $('#shift-tip')?.classList.add('is-used');
    track('nav_wheel_open');
  }
  function closeNav() {
    if (!navOpen) return;
    navOpen = false;
    navWheel.open(false);
    setTimeout(() => { if (!navOpen) navBox.hidden = true; }, reduced() ? 0 : 200);
  }
  function navGo(i) {
    const id = NAV[i] && NAV[i][0];
    navWheel.confirm(i);
    setTimeout(closeNav, reduced() ? 0 : 120);
    if (!id) return;
    const target = document.getElementById(id);
    target.scrollIntoView({ behavior: reduced() ? 'auto' : 'smooth' });
    target.setAttribute('tabindex', '-1');
    target.focus({ preventScroll: true });
  }
  navBox.addEventListener('pointerdown', (e) => { if (!e.target.closest('.wheel__body')) closeNav(); });

  const isShift = (e) => e.key === 'Shift' || /^Shift/.test(e.code || '') || e.keyCode === 16;
  const isTyping = (t) => t && t.matches && t.matches('input, textarea, select, [contenteditable="true"]');
  let shiftT = null, shiftDirty = false, shiftOwner = null;
  document.addEventListener('keydown', (e) => {
    if (!isShift(e)) {
      shiftDirty = true;
      clearTimeout(shiftT);
      if (e.key === 'Escape') {
        if (navOpen) closeNav();
        if (D.open) { closeWheel(); openBtn.focus(); }
      }
      return;
    }
    if (e.repeat) return;
    shiftDirty = false;
    if (D.dragging) {
      selectFile(D.dragging.file, true);
      if (!D.open) { const [x, y] = localPt(); openWheel(x, y); }
      shiftOwner = 'demo';
      return;
    }
    if (isTyping(e.target)) return;
    clearTimeout(shiftT);
    shiftT = setTimeout(() => {
      if (shiftDirty) return;
      if (demoActive()) {
        // Without a drag, Shift opens no files wheel in the app either; Open wheel stands in for a tap.
        if (!D.open) say(D.file ? t('Drag a sample file onto the space, then hold Shift.') : t('Pick a sample file first. The wheel needs something in hand.'), !D.file);
      } else if (fineMQ.matches && !navOpen) {
        openNav();
        shiftOwner = 'nav';
      }
    }, 220);
  });
  document.addEventListener('keyup', (e) => {
    if (!isShift(e)) return;
    clearTimeout(shiftT);
    if (shiftOwner === 'nav') { const i = navWheel.hover; if (i >= 0) navGo(i); else closeNav(); }
    else if (shiftOwner === 'demo' && D.dragging && D.open) {
      closeWheel(true);
      say(t('The wheel folds when Shift is let go. Hold it again to bring it back.'));
    }
    shiftOwner = null;
  });
  document.addEventListener('pointerdown', () => { shiftDirty = true; clearTimeout(shiftT); }, true);
  addEventListener('blur', () => { clearTimeout(shiftT); closeNav(); shiftOwner = null; });

  /* ------------------------------------------------------------------
     07 Supported files
     ------------------------------------------------------------------ */
  const FT = [
    { cat: 'Images', name: 'Pictures', inputs: 'JPEG, PNG, camera RAW, PSD, SVG, HEIC with libheif…', exts: ['jpg', 'jpeg', 'png', 'heic', 'heif', 'webp', 'avif', 'tiff', 'tif', 'gif', 'bmp', 'ico', 'psd', 'svg', 'raw', 'dng', 'cr2', 'cr3', 'nef', 'arw'], to: ['JPG', 'PNG', 'WEBP', 'HEIC*', 'AVIF*', 'TIFF', 'GIF', 'BMP', 'ICO', 'PDF'], tools: 'Compress · Resize · Crop · Rotate · Strip EXIF' },
    { cat: 'Video', name: 'Video', inputs: 'MP4, MOV, MKV, WEBM…', dep: 'Needs FFmpeg', exts: ['mp4', 'mov', 'mkv', 'webm', 'avi', 'm4v'], to: ['MP4', 'MOV', 'MKV', 'WEBM', 'GIF', 'MP3'], tools: 'Compress · Trim · Mute · Rotate · Frames' },
    { cat: 'Audio', name: 'Audio', inputs: 'MP3, M4A, WAV, FLAC, OGG…', dep: 'Needs FFmpeg', exts: ['mp3', 'm4a', 'wav', 'flac', 'ogg', 'opus', 'aiff', 'aif'], to: ['MP3', 'M4A', 'WAV', 'FLAC', 'OGG', 'OPUS', 'AIFF'], tools: 'Trim · Normalize · Compress · Merge' },
    { cat: 'PDF', name: 'PDF', inputs: 'PDF', exts: ['pdf'], to: ['DOCX', 'PPTX', 'XLSX', 'JPG', 'PNG', 'EPUB', 'TXT', 'MD'], tools: 'Compress · Split · Rotate · Merge' },
    { cat: 'Documents', name: 'Documents', inputs: 'DOCX, DOC, ODT, RTF', exts: ['docx', 'doc', 'odt', 'rtf'], to: ['PDF', 'DOCX', 'ODT', 'RTF', 'HTML', 'TXT', 'MD'], tools: 'Merge' },
    { cat: 'Spreadsheets', name: 'Spreadsheets', inputs: 'XLSX, XLS, ODS, CSV', exts: ['xlsx', 'xls', 'ods', 'csv'], to: ['PDF', 'XLSX', 'CSV', 'TSV', 'HTML', 'MD'], tools: 'Merge' },
    { cat: 'Presentations', name: 'Slides', inputs: 'PPTX; PPT and ODP to MD', exts: ['pptx', 'ppt', 'odp'], to: ['PDF', 'PPTX', 'TXT', 'MD'], tools: 'Compress' },
    { cat: 'Archives', name: 'Archives', inputs: 'ZIP, 7Z, RAR, TAR…', exts: ['zip', '7z', 'rar', 'tar'], to: ['Unzip', 'ZIP', '7Z', 'TAR.GZ', 'TAR.XZ', 'TAR.ZST', 'TAR.BZ2', 'TAR'], tools: '—' },
    { cat: 'Archives', name: 'Compressed files', inputs: 'GZ, XZ, ZST, BZ2, LZ4', exts: ['gz', 'xz', 'zst', 'bz2', 'lz4'], to: ['Unzip', 'GZ', 'XZ', 'ZST', 'BZ2', 'LZ4', 'ZIP', '7Z'], tools: '—' },
    { cat: 'Subtitles', name: 'Subtitles', inputs: 'SRT, VTT', exts: ['srt', 'vtt'], to: ['VTT', 'SRT', 'TXT'], tools: 'Shift by half a second or a second' },
    { cat: 'Folders', name: 'Folders', inputs: 'Any folder', exts: ['folder', 'directory'], to: ['ZIP', '7Z', 'TAR.GZ', 'TAR.XZ', 'TAR.ZST', 'TAR.BZ2', 'TAR'], tools: 'Flatten' },
  ];
  const CATS = ['All', 'Images', 'Video', 'Audio', 'PDF', 'Documents', 'Spreadsheets', 'Presentations', 'Archives', 'Subtitles', 'Folders'];
  const filt = $('#file-filters'), rows = $('#file-rows'), search = $('#file-search');
  let cat = 'All';
  filt.innerHTML = CATS.map((c) => `<button aria-pressed="${c === cat}" data-cat="${c}">${t(c)}</button>`).join('');
  const esc = (s) => s.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));

  function renderFiles() {
    const q = search.value.trim().toLowerCase().replace(/^\*?\./, '');
    const match = (r) => !q || r.exts.some((e) => e.startsWith(q)) || r.to.some((t) => t.toLowerCase().replace('*', '').startsWith(q)) || t(r.name).toLowerCase().includes(q);
    const list = FT.filter((r) => (cat === 'All' || r.cat === cat) && match(r));
    rows.innerHTML = list.length
      ? list.map((r) => `<div class="frow" role="row">
          <div class="k" role="cell"><b>${t(r.name)}</b><small>${esc(t(r.inputs))}</small>${r.dep ? `<span class="dep">${t(r.dep)}</span>` : ''}</div>
          <div role="cell"><span class="sub-l" aria-hidden="true">${t('Convert to')}</span><div class="pills">${r.to.map((to) => {
            const cond = to.endsWith('*'), lbl = to.replace('*', '');
            const hit = q && lbl.toLowerCase().startsWith(q);
            return `<span class="pill${cond ? ' cond' : ''}${hit ? ' hit' : ''}"${cond ? ` title="${t('Where libheif can write it')}"` : ''}>${t(lbl)}${cond ? `<span class="sr-only">${t(' (where the system supports it)')}</span>` : ''}</span>`;
          }).join('')}</div></div>
          <div role="cell" class="tool-list"><span class="sub-l" aria-hidden="true">${t('Tools')}</span>${r.tools.split(' · ').map((x) => t(x)).join(' · ')}</div>
        </div>`).join('')
      : `<div class="empty-row" role="row"><span role="cell" aria-colspan="3">${t('No match for “{}”. It may not be supported yet.', esc(q))} <a href="https://github.com/ferdiunal/mcgyver/issues" rel="noopener">${t('Ask for it on GitHub')}</a>.</span></div>`;
    const scope = cat === 'All' ? '' : t(' in {}', t(cat));
    const found = q ? t(' match “{}”', q) : '';
    $('#files-count').textContent = q || cat !== 'All' ? t('{} {} of file{}{}.', list.length, list.length === 1 ? 'kind' : 'kinds', scope, found) : '';
  }
  filt.addEventListener('click', (e) => {
    const b = e.target.closest('button'); if (!b) return;
    cat = b.dataset.cat;
    $$('button', filt).forEach((x) => x.setAttribute('aria-pressed', String(x === b)));
    renderFiles();
    $$('.frow', rows).forEach((r, i) => r.style.setProperty('--i', Math.min(i, 8)));
    rows.classList.remove('is-swap'); void rows.offsetWidth; rows.classList.add('is-swap');
  });
  let sTimer;
  search.addEventListener('input', () => {
    renderFiles();
    clearTimeout(sTimer);
    sTimer = setTimeout(() => {
      const q = search.value.trim().toLowerCase().replace(/^\*?\./, '');
      if (q) track('supported_files_search', { term: FT.some((r) => r.exts.includes(q)) ? q : 'other' });
    }, 800);
  });
  renderFiles();

  /* ------------------------------------------------------------------
     08 Download — default CTA follows the visitor's platform.
     ------------------------------------------------------------------ */
  function detectOS() {
    const p = (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || '';
    const ua = navigator.userAgent;
    if (/android|iphone|ipad|ipod/i.test(ua)) return null;
    if (/mac/i.test(p) && navigator.maxTouchPoints > 1) return null; // iPadOS reports as Mac
    if (/mac/i.test(p)) return 'mac';
    if (/win/i.test(p)) return 'windows';
    if (/linux/i.test(p)) return 'linux';
    return null;
  }
  const os = detectOS();
  if (os) {
    const card = $(`.os[data-os="${os}"]`);
    card.classList.add('is-you');
    card.insertAdjacentHTML('afterbegin', `<span class="you">${t('Your system')}</span>`);
    card.parentNode.prepend(card); // DOM order matches visual order
    const label = { mac: 'Download for Mac', windows: 'Download for Windows', linux: 'Download for Linux' }[os];
    const icon = { mac: '#i-apple', windows: '#i-windows', linux: '#i-kde' }[os];
    $('#hero-dl-label').textContent = t(label);
    $('#hero-dl-icon use').setAttribute('href', icon);
    $('#hero-dl').dataset.platform = os;
  } else if (!/mac/i.test(navigator.platform || '')) {
    $('#hero-dl-label').textContent = t('Download');
    $('#hero-dl-icon use').setAttribute('href', '#i-download');
  }

  /* ------------------------------------------------------------------
     Page chrome: header, menu, reveals
     ------------------------------------------------------------------ */
  const header = $('.site-header');
  const onScroll = () => header.classList.toggle('is-scrolled', scrollY > 8);
  addEventListener('scroll', onScroll, { passive: true });
  onScroll();
  const menuBtn = $('.menu-btn');
  function setMenu(open, restoreFocus = false) {
    menuBtn.setAttribute('aria-expanded', String(open));
    menuBtn.setAttribute('aria-label', t(open ? 'Close menu' : 'Open menu'));
    header.classList.toggle('menu-open', open);
    if (restoreFocus) menuBtn.focus();
  }
  menuBtn.addEventListener('click', () => setMenu(menuBtn.getAttribute('aria-expanded') !== 'true'));
  $('#nav-links').addEventListener('click', (e) => {
    if (e.target.closest('a')) setMenu(false);
  });
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && header.classList.contains('menu-open')) {
      e.preventDefault();
      setMenu(false, true);
    }
  });
  document.addEventListener('pointerdown', (e) => {
    if (header.classList.contains('menu-open') && !header.contains(e.target)) setMenu(false);
  });
  document.addEventListener('focusin', (e) => {
    if (header.classList.contains('menu-open') && !header.contains(e.target)) setMenu(false);
  });
  narrowMQ.addEventListener('change', () => setMenu(false));
  // A language picked on either page sticks, so the English page stops sending a Turkish browser to /tr/.
  document.addEventListener('click', (e) => {
    const link = e.target.closest('[data-lang]');
    if (link) try { localStorage.setItem('mcgyver-lang', link.dataset.lang); } catch {}
  });

  // Section headlines reveal line by line from behind a mask.
  $$('.reveal .h2').forEach((h) => {
    let i = 0;
    [...h.childNodes].forEach((n) => {
      if (n.nodeType === 3 && !n.textContent.trim()) return;
      const mask = document.createElement('span'), inner = document.createElement('span');
      mask.className = 'l' + (n.classList && n.classList.contains('accent') ? ' accent' : '');
      mask.style.setProperty('--i', i++);
      inner.append(...(n.nodeType === 3 ? [n.textContent] : [...n.childNodes]));
      mask.append(inner);
      n.replaceWith(mask);
    });
  });

  // Workflow cards: a soft light follows the pointer (moved with transform, no repaint).
  if (fineMQ.matches) $$('.flow').forEach((c) => {
    let raf = 0, x = 0, y = 0;
    c.addEventListener('pointermove', (e) => {
      if (reduced() || !fineMQ.matches) return;
      const r = c.getBoundingClientRect();
      x = e.clientX - r.left; y = e.clientY - r.top;
      if (!raf) raf = requestAnimationFrame(() => { raf = 0; c.style.setProperty('--mx', x + 'px'); c.style.setProperty('--my', y + 'px'); });
    });
  });

  // FAQ: animated open/close; native behaviour under reduced motion and find-in-page.
  $$('.faq details').forEach((d) => {
    const a = $('.a', d);
    if (d.open) d.classList.add('is-open');
    d.addEventListener('toggle', () => d.classList.toggle('is-open', d.open));
    $('summary', d).addEventListener('click', (e) => {
      if (reduced()) return;
      e.preventDefault();
      if (!d.classList.contains('is-open')) { d.open = true; void a.offsetHeight; d.classList.add('is-open'); }
      else { d.classList.remove('is-open'); setTimeout(() => { if (!d.classList.contains('is-open')) d.open = false; }, 300); }
    });
  });

  const io = new IntersectionObserver((es) => es.forEach((e) => { if (e.isIntersecting) { e.target.classList.add('is-in'); io.unobserve(e.target); } }), { threshold: 0.12, rootMargin: '0px 0px -40px 0px' });
  $$('.reveal').forEach((n) => io.observe(n));
  // Enable hiding only after every enhancement has initialized successfully.
  root.classList.add('reveal-ready');
  // Keyboard navigation must never leave a focused control in a hidden panel.
  document.addEventListener('focusin', (e) => {
    const panel = e.target.closest('.reveal');
    if (panel) { panel.classList.add('is-in'); io.unobserve(panel); }
  });
})();
