# McGyver: dosyayı sürükle, Shift'i basılı tut, bir dilim seç

McGyver, dosya sürüklerken imlecin çevresinde bir çark açar. Dosyaları bir dilimin üstüne bırakın: resimler biçim değiştirir, videolar küçülür, PDF'ler Word dosyasına dönüşür, klasörler zip'lenir. Dönüştürmeler bulutta değil, bilgisayarınızda yapılır.

Sürüm 1.0.0, ilk kararlı sürüm. KDE Plasma 6, macOS ve Windows için.

## Tek çark, yedi mod

Merkezin çevresindeki halka modlar arasında geçiş yapar. Halkadaki bir bölümün üstünde durun ya da halka boyunca onun içine geçin.

### Dosyalarla

- **Dönüştür.** Çark Dönüştür modunda açılır. Bir biçime nişan alıp bırakın: tatil.webp, tatil.jpg'nin yanına gelir. Resimler, video, ses, PDF'ler, Office dosyaları, arşivler ve altyazılar için ayrı listeler var.
- **Araçlar.** Bir videoyu sıkıştırın, kesin, sessize alın, döndürün ya da içinden kareler çıkarın. Resimler boyutlanır, kırpılır, EXIF bilgilerinden arınır; PDF'ler bölünür ve döner, iki ya da daha fazlası tek dosyada birleşir.
- **Eylemler.** Zip'leyin, kopyalayın ya da bir yere taşıyın, yolunu kopyalayın, orada bir terminal açın ya da telefonunuza, WhatsApp'a, Telegram'a, Slack'e veya Teams'e gönderin.

### Dosyasız

Shift'i tek başına biraz basılı tutun; çark bu kez bilgisayar için açılır.

- **Uygulamalar.** Açık olanlar, en son kullandığınız en üstte; ardından ayarlardan seçtiğiniz uygulamalar. Çember biçiminde bir Alt+Tab.
- **Medya.** Ne çalıyorsa: oynatın ya da duraklatın, sonraki, önceki, tekrarla, karıştır ve oynatıcının kendisi.
- **Günlük.** Klavye düzeni, bilgisayarın dili, ses, parlaklık, ekran görüntüsü, ekran kaydı ve bilgisayarın ne kadar meşgul olduğu.
- **Sistem.** Kilit, Karanlık Mod, Uyanık Kal, mikrofon, çöp kutusu ve uyku.

## Her dosyaya ne sunulur

Çark yalnızca elinizdeki dosyaların dönüşebileceği şeyleri sunar. Zaten o biçimde olan bir dosyaya o biçim sunulmaz; sunacak bir şeyi olmayan modun dilimi olmaz.

| Dosya | Dönüştür | Araçlar |
| --- | --- | --- |
| Resimler (JPEG, PNG, HEIC, kamera RAW, PSD, SVG…) | JPG · PNG · WEBP · HEIC · AVIF · TIFF · GIF · BMP · ICO · PDF | Sıkıştır · Boyutlandır · Kırp · Döndür · EXIF'i sil |
| Video (FFmpeg gerekir) | MP4 · MOV · MKV · WEBM · GIF · MP3 | Sıkıştır · Kes · Sessize al · Döndür · Kareler |
| Ses (FFmpeg gerekir) | MP3 · M4A · WAV · FLAC · OGG · OPUS · AIFF | Kes · Dengele · Sıkıştır · Birleştir |
| PDF | DOCX · PPTX · XLSX · JPG · PNG · EPUB · TXT · MD | Sıkıştır · Böl · Döndür · Birleştir |
| Belgeler (DOCX, DOC, ODT, RTF) | PDF · DOCX · ODT · RTF · HTML · TXT · MD | Birleştir |
| Tablolar (XLSX, XLS, ODS, CSV) | PDF · XLSX · CSV · TSV · HTML · MD | Birleştir |
| Sunumlar (PPTX; PPT ve ODP yalnızca MD'ye) | PDF · PPTX · TXT · MD | Sıkıştır |
| Arşivler (ZIP, 7Z, RAR, TAR…) | Çıkar · ZIP · 7Z · TAR.GZ · TAR.XZ · TAR.ZST · TAR.BZ2 · TAR | — |
| Sıkıştırılmış dosyalar (GZ, XZ, ZST, BZ2, LZ4) | Çıkar · GZ · XZ · ZST · BZ2 · LZ4 · ZIP · 7Z | — |
| Altyazılar (SRT, VTT) | VTT · SRT · TXT | Yarım ya da bir saniye kaydır |
| Klasörler | ZIP · 7Z · TAR.GZ · TAR.XZ · TAR.ZST · TAR.BZ2 · TAR | Düzleştir |

Her dosya ve klasöre ayrıca **Eylemler** de sunulur: Zip, Kopyala, Kes, Şuraya kopyala, Şuraya taşı, Yolu kopyala, Terminal ve Paylaş. Birleştir, elinizde iki ya da daha fazla dosya olunca görünür. HEIC ve AVIF, bilgisayardaki libheif bunları yazabiliyorsa görünür.

## McGyver'ı indirin

Bir kez indirin; sonra siz Shift'e basana kadar sistem tepsisinde ya da menü çubuğunda bekler.

- **KDE Plasma 6.** Linux, Wayland üzerinde. McGyver'ın doğduğu yer. İlk açılıştan sonra ayarlarında *Kur*'a basın; böylece sürüklerken Shift'e bastığınız anda çark açılır. Debian ve Ubuntu için [.deb'i indirin](https://mcgyver.ferdiunal.tr/download/deb) ya da Fedora için [.rpm'i](https://mcgyver.ferdiunal.tr/download/rpm).
- **macOS.** macOS 11 veya üstü. Apple Silicon ve Intel için tek uygulama. Disk görüntüsünü açın; uygulama kendini Uygulamalar klasörüne taşımayı önerir. Henüz Apple'ın onayından (notarization) geçmedi: ilk seferde Control tuşuna basarak tıklayıp *Aç*'ı seçin (macOS 15'ten itibaren Gizlilik ve Güvenlik'te *Yine de Aç*). [Mac için indirin](https://mcgyver.ferdiunal.tr/download/macos) ya da [yalnızca Apple Silicon için](https://mcgyver.ferdiunal.tr/download/macos-arm64) (daha küçük).
- **Windows.** Windows 10 ve 11, x64 ve ARM64 bilgisayarlarda. Ekran kartı sürücüsü olmayan bir bilgisayarda, örneğin bir sanal makinede, zip'in içinde gelen Mesa'nın yazılım OpenGL'i ile çizer. SmartScreen bir kez sorar: *Ek bilgi*, ardından *Yine de çalıştır*. [x64 bilgisayarlar için indirin](https://mcgyver.ferdiunal.tr/download/windows-x64) ya da [ARM64 için](https://mcgyver.ferdiunal.tr/download/windows-arm64).

## Gizlilik

Uygulama günde bir kez mcgyver.ferdiunal.tr'ye yeni bir sürüm çıkıp çıkmadığını sorar. Bu istek ayrıca uygulamanın sürümünü, işletim sistemini ve sürümünü, işlemci türünü, uygulamanın dilini ve sistemin kendi kimliğinin tek yönlü özetinden (hash) oluşan bir makine kimliğini taşır (bu kimlikten sistemin kimliğine geri dönülemez). Sunucu ayrıca IP adresini ve isteğin geldiği ülkeyi görür ve bunları o kimlikle birlikte saklar.

Hiçbir dosya, dosya adı ya da çarkla yaptıklarınız bilgisayardan çıkmaz. Uygulamanın hiç sormaması için onu ortamında `MCGYVER_OFFLINE=1` ile başlatın.

## Ücretsiz; sponsorların desteğiyle sürüyor

McGyver'ı Ferdi ÜNAL geliştiriyor. Size günde birkaç tıklama kazandırıyorsa, [GitHub'da sponsor olmanız](https://github.com/sponsors/ferdiunal) işin sürmesini sağlar.

- [Ferdi GitHub'da](https://github.com/ferdiunal)
- [ferdiunal.tr](https://ferdiunal.tr/)
- [mail@ferdiunal.tr](mailto:mail@ferdiunal.tr)

McGyver 1.0.0 · MIT veya Apache-2.0 · © 2026 McGyver yazarları

[English](https://mcgyver.ferdiunal.tr/index.md)
