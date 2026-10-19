//! The words the app shows, in English or Turkish. Text is written in English throughout and
//! looked up here when it is shown; anything without a translation (a format name, a file
//! name, an engine's own error) is shown as it is.

use std::fmt::Display;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    English,
    Turkish,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::English, Lang::Turkish];

    /// How the settings file names it.
    pub fn code(self) -> &'static str {
        match self {
            Lang::English => "en",
            Lang::Turkish => "tr",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        Lang::ALL.into_iter().find(|l| l.code() == code)
    }

    /// Its own name, as the list of languages shows it.
    pub fn name(self) -> &'static str {
        match self {
            Lang::English => "English",
            Lang::Turkish => "Türkçe",
        }
    }

    /// The desktop's language: the first of `LANGUAGE` (where Plasma keeps it; a list, the
    /// first entry counts), `LC_ALL`, `LC_MESSAGES` and `LANG` that is set.
    pub fn from_env() -> Lang {
        let locale = ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .filter_map(std::env::var_os)
            .find(|v| !v.is_empty())
            .unwrap_or_default();
        if locale.to_string_lossy().starts_with("tr") {
            Lang::Turkish
        } else {
            Lang::English
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Switches everything shown from now on to `lang`.
pub fn set(lang: Lang) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

pub fn current() -> Lang {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Lang::Turkish,
        _ => Lang::English,
    }
}

/// `english` in the current language.
pub fn tr(english: &str) -> &str {
    translate(current(), english)
}

/// `template` in the current language, each `{}` replaced by the next of `args`.
pub fn trf(template: &str, args: &[&dyn Display]) -> String {
    fill(translate(current(), template), args)
}

pub fn translate(lang: Lang, english: &str) -> &str {
    match lang {
        Lang::English => english,
        Lang::Turkish => turkish(english).unwrap_or(english),
    }
}

fn fill(template: &str, args: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut args = args.iter();
    let mut pieces = template.split("{}");
    if let Some(first) = pieces.next() {
        out.push_str(first);
    }
    for piece in pieces {
        if let Some(arg) = args.next() {
            out.push_str(&arg.to_string());
        }
        out.push_str(piece);
    }
    out
}

fn turkish(english: &str) -> Option<&'static str> {
    Some(match english {
        // The wheel's modes.
        "CONVERT" => "DÖNÜŞTÜR",
        "TOOLS" => "ARAÇLAR",
        "ACTIONS" => "EYLEMLER",
        "Nothing to do with these" => "Bunlarla yapılacak bir şey yok",

        // Kinds of file.
        "Image" => "Resim",
        "Audio" => "Ses",
        "Video" => "Video",
        "Document" => "Belge",
        "Spreadsheet" => "Tablo",
        "Slides" => "Sunum",
        "Subtitles" => "Altyazı",
        "Archive" => "Arşiv",
        "Compressed" => "Sıkıştırılmış",
        "Folder" => "Klasör",
        "{} files" => "{} dosya",
        "{} · {}" => "{} · {}",

        // Conversions and their hints.
        "Unzip" => "Çıkar",
        "photos" => "fotoğraflar",
        "lossless" => "kayıpsız",
        "for the web" => "web için",
        "Apple photos" => "Apple fotoğrafları",
        "smallest" => "en küçük",
        "print" => "baskı",
        "share & print" => "paylaş ve yazdır",
        "plays anywhere" => "her yerde oynatılır",
        "uncompressed" => "sıkıştırmasız",
        "open codec" => "açık kodek",
        "256 colours" => "256 renk",
        "app icon" => "uygulama simgesi",
        "unpack here" => "buraya çıkar",
        "e-book" => "e-kitap",
        "plain text" => "düz metin",
        "web page" => "web sayfası",
        "rich text" => "zengin metin",
        "raw table" => "ham tablo",
        "opens anywhere" => "her yerde açılır",
        "small" => "küçük",
        "fast and small" => "hızlı ve küçük",
        "no compression" => "sıkıştırmasız",
        "fastest" => "en hızlı",

        // Tools.
        "Compress" => "Sıkıştır",
        "smaller file" => "daha küçük dosya",
        "Resize" => "Boyutlandır",
        "fit a size" => "boyuta sığdır",
        "Crop" => "Kırp",
        "trim edges" => "kenarları kırp",
        "Rotate" => "Döndür",
        "Strip EXIF" => "EXIF'i sil",
        "remove photo details" => "fotoğraf bilgilerini sil",
        "Trim" => "Kes",
        "cut silence" => "sessizliği kes",
        "cut the ends" => "baştan ve sondan kes",
        "Normalize" => "Dengele",
        "even volume" => "eşit ses",
        "Merge" => "Birleştir",
        "join tracks" => "parçaları birleştir",
        "join PDFs" => "PDF'leri birleştir",
        "join files" => "dosyaları birleştir",
        "Mute" => "Sessize al",
        "drop audio" => "sesi kaldır",
        "Frames" => "Kareler",
        "export stills" => "kareleri çıkar",
        "Split" => "Böl",
        "one per page" => "sayfa başına bir",
        "Shift" => "Kaydır",
        "re-time" => "zamanlamayı kaydır",
        "Flatten" => "Düzleştir",
        "pull files up" => "dosyaları yukarı al",

        // Actions.
        "into one archive" => "tek arşive",
        "Copy" => "Kopyala",
        "Cut" => "Kes",
        "Copy To" => "Şuraya kopyala",
        "Move To" => "Şuraya taşı",
        "to a folder" => "bir klasöre",
        "Copy Path" => "Yolu kopyala",
        "to the clipboard" => "panoya",
        "open here" => "burada aç",
        "Share" => "Paylaş",
        "to your phone" => "telefonuna",
        "Copy {} to…" => "{} kopyalanacak…",
        "Move {} to…" => "{} taşınacak…",
        "Copy {} paths" => "{} yol kopyalanacak",
        "Copy the path of {}" => "Yolu kopyala: {}",
        "Terminal in {}" => "{} içinde terminal",
        "Send {} to your phone" => "{} telefona gönderilecek",
        "Copy {} to" => "{} nereye kopyalansın",
        "Move {} to" => "{} nereye taşınsın",
        "Saved as {}" => "{} olarak kaydedildi",
        "Copied to {}" => "{} klasörüne kopyalandı",
        "Moved to {}" => "{} klasörüne taşındı",
        "{} paths copied" => "{} yol kopyalandı",
        "Path copied" => "Yol kopyalandı",
        "Opened {}" => "{} açıldı",
        "Sent to {}" => "{} cihazına gönderildi",
        "copied" => "kopyalandı",
        "cut" => "kesildi",
        "moved" => "taşındı",
        "Copy {} to the clipboard" => "{} panoya kopyalanacak",
        "Cut {}, to paste where it goes" => "{} kesilecek, taşınacağı yere yapıştırın",
        "Copied; paste it anywhere" => "Kopyalandı; istediğiniz yere yapıştırın",
        "{} files copied; paste them anywhere" => {
            "{} dosya kopyalandı; istediğiniz yere yapıştırın"
        }
        "Cut; paste it where it goes" => "Kesildi; taşınacağı yere yapıştırın",
        "{} files cut; paste them where they go" => "{} dosya kesildi; taşınacağı yere yapıştırın",
        "path copied" => "yol kopyalandı",
        "terminal opened" => "terminal açıldı",
        "shared" => "paylaşıldı",
        "unpacked here" => "buraya çıkarıldı",

        // The card.
        "{} · nothing to change" => "{} · değişecek bir şey yok",
        "Done · simulated" => "Bitti · simülasyon",
        "Done · {} files" => "Bitti · {} dosya",
        "Working on {} of {}…" => "{} / {} işleniyor…",
        "Working…" => "İşleniyor…",
        "{} of {} failed · {}" => "{} / {} başarısız · {}",

        // Why something failed.
        "it isn't there any more" => "artık orada değil",
        "no permission to do that" => "bunun için izin yok",
        "the disk is full" => "disk dolu",
        "the file looks damaged" => "dosya bozuk görünüyor",
        "cancelled" => "vazgeçildi",
        "Cancelled" => "İptal edildi",
        "nothing to merge" => "birleştirilecek bir şey yok",
        "nothing to work on" => "üzerinde çalışılacak bir şey yok",
        "no terminal found" => "terminal bulunamadı",
        "no folder picker (kdialog or zenity) found" => {
            "klasör seçici (kdialog ya da zenity) bulunamadı"
        }
        "no phone is connected (KDE Connect)" => "bağlı telefon yok (KDE Connect)",
        "folders can't be sent; zip them first" => "klasörler gönderilemez; önce zip'le",
        "couldn't reach the clipboard" => "panoya ulaşılamadı",
        "not an office document" => "ofis belgesi değil",
        "this file is too big to convert" => "bu dosya dönüştürmek için çok büyük",
        "this document is too big to convert" => "bu belge dönüştürmek için çok büyük",
        "this picture is too big to convert" => "bu resim dönüştürmek için çok büyük",
        "these files are too big to merge" => "bu dosyalar birleştirmek için çok büyük",
        "this PDF is signed: changing it would break the signature" => {
            "bu PDF imzalı: değiştirmek imzayı bozar"
        }
        "this PDF has no pages" => "bu PDF'te sayfa yok",
        "this PDF is already as small as it gets" => "bu PDF zaten olabildiğince küçük",
        "this PDF is scanned: it has no text to take out" => {
            "bu PDF taranmış: içinden alınacak metin yok"
        }
        "the document is password-protected" => "belge parolayla korunuyor",
        "this archive is password-protected" => "arşiv parolayla korunuyor",
        "this RAR archive is damaged" => "bu RAR arşivi bozuk",
        "RAR archives can only be read" => "RAR arşivleri yalnızca okunabilir",
        "this build can't open RAR archives" => "bu sürüm RAR arşivlerini açamaz",
        "a folder isn't an archive" => "klasör bir arşiv değil",
        "only a file can be compressed on its own" => "tek başına yalnızca dosya sıkıştırılabilir",
        "a compressed file holds exactly one file" => {
            "sıkıştırılmış dosya yalnızca tek bir dosya içerir"
        }
        "FFmpeg produced an empty file" => "FFmpeg boş bir dosya üretti",
        "HEIC and AVIF need libheif, which isn't installed" => {
            "HEIC ve AVIF için libheif gerekir, kurulu değil"
        }
        "{} can't go inside itself" => "{} kendi içine giremez",
        "couldn't make sure the copy is on disk" => "kopyanın diske yazıldığı doğrulanamadı",
        "this spreadsheet is too long to print" => "bu tablo yazdırmak için çok uzun",
        "this PDF is protected: what is made from it would not be" => {
            "bu PDF korumalı: ondan yapılanlar korumasız kalırdı"
        }
        "a file changed while it was being packed" => "paketlenirken bir dosya değişti",
        "this archive has a broken link entry" => "bu arşivde bozuk bir bağlantı kaydı var",
        "this archive refers to a file it doesn't hold" => {
            "bu arşiv, içinde olmayan bir dosyaya başvuruyor"
        }
        "can't open the RAR archive (or one of its parts)" => {
            "RAR arşivi (ya da parçalarından biri) açılamıyor"
        }
        "not a compressed file or folder" => "sıkıştırılmış dosya ya da klasör değil",
        "this image format cannot be written on this system" => {
            "bu resim biçimi bu sistemde yazılamıyor"
        }

        // The tray.
        "Pause" => "Duraklat",
        "Settings…" => "Ayarlar…",
        "Quit" => "Çık",
        "Paused" => "Duraklatıldı",
        "{} — ready" => "{} — hazır",
        "{} — paused" => "{} — duraklatıldı",
        "Hold {} while dragging files, or tap it with files selected in Dolphin" => {
            "Dosya sürüklerken {} tuşunu basılı tut ya da Dolphin'de dosya seçip tek başına bas"
        }

        // The settings window.
        "Settings" => "Ayarlar",
        "Language" => "Dil",
        "Trigger key" => "Tetik tuşu",
        "Hold it while dragging files to open the wheel. Tapped on its own, it opens the wheel \
         for the files selected in Dolphin." => {
            "Dosya sürüklerken basılı tutunca çark açılır. Tek başına basılınca Dolphin'de seçili \
             dosyalar için açılır."
        }
        "Start at login" => "Oturum açılınca başlat",
        "Set up" => "Kur",
        "Remove" => "Kaldır",
        "On" => "Açık",
        "Permissions" => "İzinler",
        "Checking…" => "Kontrol ediliyor…",
        "Everything is ready" => "Her şey hazır",
        "Opens at once under a drag" => "Sürüklerken hemen açılsın",
        "Without it, the wheel opens once the mouse moves." => {
            "Olmazsa çark fare kıpırdayınca açılır."
        }
        "Opens when the trigger key is tapped alone" => "Tetik tuşuna tek başına basınca açılsın",
        "For the files selected in Dolphin." => "Dolphin'de seçili dosyalar için.",
        "Not set up" => "Kurulmadı",
        "Needs a restart" => "Yeniden başlatma gerekiyor",
        "Restarting…" => "Yeniden başlatılıyor…",
        "{} isn't running" => "{} çalışmıyor",
        "{} alone is taken by another shortcut" => "{} tek başına başka bir kısayolda",
        "Restart" => "Yeniden başlat",
        "Start" => "Başlat",
        "Open shortcuts" => "Kısayolları aç",
        "Try again" => "Tekrar dene",
        "Hold it while dragging files to open the wheel." => {
            "Dosya sürüklerken basılı tutunca çark açılır."
        }
        "Input Monitoring" => "Girdi İzleme",
        "Lets McGyver notice the trigger key the moment you press it, and saves battery. \
         McGyver works without it too." => {
            "McGyver tetik tuşuna bastığın anı hemen fark eder, pilin de daha az harcanır. İzin \
             vermesen de çalışır."
        }
        "If it is already switched on there, remove McGyver with − and allow it again." => {
            "Orada zaten açık görünüyorsa McGyver'ı − ile kaldırıp yeniden izin ver."
        }
        "Allowed. McGyver starts using it once it restarts." => {
            "İzin verildi. McGyver yeniden başlayınca kullanmaya başlar."
        }
        "Restart McGyver" => "McGyver'ı yeniden başlat",
        "Windows asks no permission for McGyver to notice the trigger key and the drag." => {
            "Windows, McGyver'ın tetik tuşunu ve sürüklemeyi fark etmesi için izin istemez."
        }
        "Command Prompt" => "Komut İstemi",
        "Unzip McGyver to a folder first." => "Önce McGyver'ı zip'ten bir klasöre çıkarın.",
        "Not allowed" => "İzin verilmedi",
        "Ask" => "İzin iste",
        "Open System Settings" => "Sistem Ayarları'nı aç",
        "Still not allowed" => "Hâlâ izin verilmedi",
        "This desktop can't share Dolphin's selection" => {
            "Bu masaüstü Dolphin'in seçimini paylaşamıyor"
        }
        "it stopped unexpectedly" => "beklenmedik şekilde durdu",
        "couldn't set up the {} shortcut: {}" => "{} kısayolu kurulamadı: {}",
        "{} alone is taken by another global shortcut" => "{} tek başına başka bir genel kısayolda",
        "couldn't move the shortcut: {}" => "kısayol taşınamadı: {}",
        "couldn't remove the shortcut: {}" => "kısayol kaldırılamadı: {}",
        "Saved" => "Kaydedildi",
        "Couldn't save: {}" => "Kaydedilemedi: {}",
        "Move McGyver to Applications first." => "Önce McGyver'ı Uygulamalar klasörüne taşıyın.",
        "{} isn't running; it will use these when it starts." => {
            "{} çalışmıyor; açıldığında bunları kullanacak."
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{self, Mode};
    use std::path::PathBuf;

    /// Labels that are names of formats, the same in every language.
    fn is_format(label: &str) -> bool {
        label
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '.')
    }

    #[test]
    fn every_label_and_hint_on_the_wheel_has_a_translation() {
        let files = [
            "a.jpg", "a.mp3", "a.mp4", "a.pdf", "a.docx", "a.xlsx", "a.pptx", "a.srt", "a.zip",
            "a.gz",
        ];
        let mut missing = Vec::new();
        for file in files {
            let two = [PathBuf::from(file), PathBuf::from(format!("b{file}"))];
            for mode in Mode::ALL {
                for choice in catalog::choices(&two, mode) {
                    for text in [choice.label, choice.hint] {
                        if !text.is_empty() && !is_format(text) && turkish(text).is_none() {
                            missing.push(text);
                        }
                    }
                }
                if turkish(mode.title()).is_none() {
                    missing.push(mode.title());
                }
            }
        }
        // Actions the test machine may lack the tools for.
        for action in crate::actions::Action::ALL {
            for text in [action.label(), action.hint()] {
                if turkish(text).is_none() {
                    missing.push(text);
                }
            }
        }
        missing.sort();
        missing.dedup();
        // Brand names and the like read the same in Turkish.
        missing.retain(|t| {
            ![
                "Word",
                "Excel",
                "PowerPoint",
                "Apple Pages",
                "Markdown",
                "OpenDocument",
                "QuickTime",
                "Matroska",
                "AAC",
                "gzip",
                "bzip2",
                "Zip",
                "Terminal",
                "90°",
            ]
            .contains(t)
        });
        assert!(missing.is_empty(), "no Turkish for {missing:?}");
    }

    #[test]
    fn templates_fill_in_order() {
        assert_eq!(
            fill(translate(Lang::Turkish, "Working on {} of {}…"), &[&2, &5]),
            "2 / 5 işleniyor…"
        );
        assert_eq!(
            fill(translate(Lang::English, "Working on {} of {}…"), &[&2, &5]),
            "Working on 2 of 5…"
        );
        // Untranslated text and extra arguments pass through.
        assert_eq!(translate(Lang::Turkish, "PNG"), "PNG");
        assert_eq!(fill("{} files", &[&3, &4]), "3 files");
    }

    #[test]
    fn languages_round_trip_through_their_codes() {
        for lang in Lang::ALL {
            assert_eq!(Lang::from_code(lang.code()), Some(lang));
        }
        assert_eq!(Lang::from_code("de"), None);
    }
}
