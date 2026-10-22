//! Subtitles: SubRip (.srt) and WebVTT (.vtt) read into cues, written as either or as plain
//! text, and moved in time.

use std::fs;
use std::io::{self, Read};
use std::path::Path;

/// A subtitle file larger than this is not one.
const MAX_INPUT: u64 = 64 << 20;

/// What Shift moves subtitles by, as its wedge's bubbles: the label, and milliseconds.
pub const SHIFTS: [(&str, i64); 4] = [
    ("−1 s", -1000),
    ("−0.5 s", -500),
    ("+0.5 s", 500),
    ("+1 s", 1000),
];

/// The milliseconds a Shift bubble moves subtitles by.
pub fn shift_by(label: &str) -> Option<i64> {
    SHIFTS.iter().find(|(l, _)| *l == label).map(|(_, ms)| *ms)
}

/// The bubble's label for a shift of `ms`.
pub fn shift_label(ms: i64) -> Option<&'static str> {
    SHIFTS
        .iter()
        .find(|(_, m)| *m == ms)
        .map(|(label, _)| *label)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Srt,
    Vtt,
    /// The words alone, a cue to a line.
    Txt,
}

impl Format {
    pub fn from_label(label: &str) -> Option<Format> {
        Some(match label {
            "SRT" => Format::Srt,
            "VTT" => Format::Vtt,
            "TXT" => Format::Txt,
            _ => return None,
        })
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Srt => "srt",
            Format::Vtt => "vtt",
            Format::Txt => "txt",
        }
    }
}

/// One subtitle: its name (a WebVTT cue's id; SubRip's number), when it shows and goes in
/// milliseconds, what follows the times on its line (WebVTT's settings, SubRip's position),
/// and its lines.
#[derive(Clone, Debug, Default, PartialEq)]
struct Cue {
    id: Option<String>,
    start: i64,
    end: i64,
    settings: String,
    text: String,
}

/// A subtitle file read: whether it is WebVTT, the blocks before its first cue (WebVTT's
/// header, styles and regions), and its cues.
#[derive(Debug, Default, PartialEq)]
struct Subtitles {
    vtt: bool,
    header: Vec<String>,
    cues: Vec<Cue>,
}

/// The subtitles in `input` as `format`, moved `shift` milliseconds later (earlier if
/// negative); ones moved before the start are dropped. Always UTF-8. Kept in their own format,
/// only the times change: positions, settings and styling stay as they are.
pub fn convert(input: &Path, format: Format, shift: i64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(input)?
        .take(MAX_INPUT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_INPUT {
        return Err(io::Error::other("this file is too big to convert"));
    }
    let mut subtitles = parse(&decode(&bytes));
    subtitles.cues = std::mem::take(&mut subtitles.cues)
        .into_iter()
        .filter_map(|cue| {
            let end = cue.end + shift;
            (end > 0).then(|| Cue {
                start: (cue.start + shift).max(0),
                end,
                ..cue
            })
        })
        .collect();
    if subtitles.cues.is_empty() {
        return Err(io::Error::other("there are no subtitles in this file"));
    }
    Ok(write(&subtitles, format).into_bytes())
}

/// The text of a subtitle file: as its byte order mark says, as UTF-8, or else as the
/// encoding it looks like (older subtitles are often Windows-1254, -1252 and the like).
fn decode(bytes: &[u8]) -> String {
    if let Some((encoding, mark)) = encoding_rs::Encoding::for_bom(bytes) {
        return encoding
            .decode_without_bom_handling(&bytes[mark..])
            .0
            .into_owned();
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_owned();
    }
    let mut detector = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    detector.feed(bytes, true);
    detector
        .guess(None, chardetng::Utf8Detection::Deny)
        .decode_without_bom_handling(bytes)
        .0
        .into_owned()
}

/// A timing line ("00:00:01,000 --> 00:00:02,500 align:start"): the times, and what follows.
fn timing(line: &str) -> Option<(i64, i64, String)> {
    let (start, rest) = line.split_once("-->")?;
    let rest = rest.trim_start();
    let (end, settings) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    Some((time(start.trim())?, time(end)?, settings.trim().to_owned()))
}

/// SubRip or WebVTT text, line by line: a timing line starts a cue, a blank line (spaces
/// count as blank) ends it, and what stands before a timing line is the cue's name. A number
/// left at the end of a cue that runs straight into the next is the next one's.
fn parse(text: &str) -> Subtitles {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut subtitles = Subtitles {
        vtt: text.trim_start().starts_with("WEBVTT"),
        ..Default::default()
    };
    let mut cue: Option<Cue> = None;
    let mut before: Vec<&str> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            if let Some(done) = cue.take() {
                subtitles.cues.push(done);
            } else if !before.is_empty() && subtitles.cues.is_empty() && subtitles.vtt {
                // WebVTT's header, styles, regions and notes before the first cue.
                subtitles.header.push(before.join("\n"));
            }
            before.clear();
            continue;
        }
        let Some((start, end, settings)) = timing(line) else {
            match cue.as_mut() {
                Some(cue) if cue.text.is_empty() => cue.text.push_str(line.trim_end()),
                Some(cue) => {
                    cue.text.push('\n');
                    cue.text.push_str(line.trim_end());
                }
                None => before.push(line),
            }
            continue;
        };
        let mut id = before.pop().map(|line| line.trim().to_owned());
        before.clear();
        if let Some(mut done) = cue.take() {
            if let Some((rest, last)) = done.text.rsplit_once('\n')
                && last.trim().bytes().all(|b| b.is_ascii_digit())
            {
                id = Some(last.trim().to_owned());
                done.text = rest.to_owned();
            }
            subtitles.cues.push(done);
        }
        cue = Some(Cue {
            id,
            start,
            end,
            settings,
            text: String::new(),
        });
    }
    subtitles.cues.extend(cue);
    subtitles.cues.retain(|cue| !cue.text.trim().is_empty());
    subtitles
}

/// "01:02:03,450", "01:02:03.450" or "02:03.450" in milliseconds.
fn time(text: &str) -> Option<i64> {
    let (clock, fraction) = text.split_once([',', '.']).unwrap_or((text, "0"));
    let mut seconds = 0i64;
    let parts: Vec<&str> = clock.split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    for part in parts {
        seconds = seconds * 60 + part.trim().parse::<i64>().ok()?;
    }
    // Milliseconds, however many digits are written.
    let digits: String = fraction.chars().chain("000".chars()).take(3).collect();
    Some(seconds * 1000 + digits.parse::<i64>().ok()?)
}

fn clock(ms: i64, separator: char) -> String {
    let (hours, rest) = (ms / 3_600_000, ms % 3_600_000);
    let (minutes, rest) = (rest / 60_000, rest % 60_000);
    let (seconds, ms) = (rest / 1000, rest % 1000);
    format!("{hours:02}:{minutes:02}:{seconds:02}{separator}{ms:03}")
}

/// The times and what follows them, as `format` writes a timing line.
fn timing_line(cue: &Cue, separator: char, settings: bool) -> String {
    let mut line = format!(
        "{} --> {}",
        clock(cue.start, separator),
        clock(cue.end, separator)
    );
    if settings && !cue.settings.is_empty() {
        line.push(' ');
        line.push_str(&cue.settings);
    }
    line
}

fn write(subtitles: &Subtitles, format: Format) -> String {
    let from_vtt = subtitles.vtt;
    let mut out = String::new();
    match format {
        Format::Srt => {
            for (i, cue) in subtitles.cues.iter().enumerate() {
                // Kept SubRip, a cue is only re-timed; from WebVTT its text is turned over.
                let text = if from_vtt {
                    rewrite(&cue.text, true, true, false)
                } else {
                    cue.text.clone()
                };
                let timing = timing_line(cue, ',', !from_vtt);
                out.push_str(&format!("{}\n{timing}\n{text}\n\n", i + 1));
            }
        }
        Format::Vtt => {
            if from_vtt && !subtitles.header.is_empty() {
                for block in &subtitles.header {
                    out.push_str(block);
                    out.push_str("\n\n");
                }
            } else {
                out.push_str("WEBVTT\n\n");
            }
            for cue in &subtitles.cues {
                let text = if from_vtt {
                    if let Some(id) = &cue.id {
                        out.push_str(id);
                        out.push('\n');
                    }
                    cue.text.clone()
                } else {
                    rewrite(&cue.text, false, true, true)
                };
                let timing = timing_line(cue, '.', from_vtt);
                out.push_str(&format!("{timing}\n{text}\n\n"));
            }
        }
        Format::Txt => {
            let mut last = String::new();
            for cue in &subtitles.cues {
                let line = rewrite(&cue.text, from_vtt, false, false)
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                // A line held over several cues is written once.
                if !line.is_empty() && line != last {
                    out.push_str(&line);
                    out.push('\n');
                    last = line;
                }
            }
        }
    }
    out
}

/// Whether `text`, after a `<`, is a tag: a name ("i", "/b", "c.yellow", "v Ali") or a
/// karaoke time ("00:01.500"). "<3" and "< 5" are just text.
fn is_tag(inside: &str) -> bool {
    match inside.chars().next() {
        Some(c) if c.is_ascii_alphabetic() || c == '/' => true,
        Some(c) if c.is_ascii_digit() => inside
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b':' || b == b'.'),
        _ => false,
    }
}

/// A cue's text for another format: italic, bold and underline kept if `styled`, other tags
/// (colours, voices, classes, karaoke times) and SubRip's `{\an8}` positions dropped. WebVTT
/// escapes `&`, `<` and `>`: `from_vtt` unescapes them, `to_vtt` escapes them.
fn rewrite(text: &str, from_vtt: bool, styled: bool, to_vtt: bool) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(first) = rest.chars().next() {
        if first == '<'
            && let Some(close) = rest.find('>')
            && is_tag(&rest[1..close])
        {
            let tag = rest[1..close].trim().to_ascii_lowercase();
            let (closing, name) = match tag.strip_prefix('/') {
                Some(name) => ("/", name),
                None => ("", tag.as_str()),
            };
            let name = name.split(['.', ' ']).next().unwrap_or_default();
            if styled && matches!(name, "i" | "b" | "u") {
                out.push_str(&format!("<{closing}{name}>"));
            }
            rest = &rest[close + 1..];
        } else if rest.starts_with("{\\")
            && let Some(close) = rest.find('}')
        {
            rest = &rest[close + 1..];
        } else {
            let next = rest[first.len_utf8()..]
                .find(['<', '{'])
                .map_or(rest.len(), |i| i + first.len_utf8());
            let mut plain = rest[..next].to_owned();
            if from_vtt {
                plain = plain
                    .replace("&lt;", "<")
                    .replace("&gt;", ">")
                    .replace("&nbsp;", "\u{a0}")
                    .replace("&lrm;", "\u{200e}")
                    .replace("&rlm;", "\u{200f}")
                    .replace("&amp;", "&");
            }
            if to_vtt {
                plain = plain
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;");
            }
            out.push_str(&plain);
            rest = &rest[next..];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRT: &str = "1\r\n00:00:01,000 --> 00:00:02,500\r\n<i>Merhaba</i> & hoş geldin\r\n\
                       {\\an8}İkinci satır\r\n \r\n2\r\n00:00:03,000 --> 00:00:04,000 X1:10\r\n\
                       <font color=\"red\">Kırmızı</font> <3\r\n3\r\n00:00:05,000 --> \
                       00:00:06,000\r\nBitişik\r\n";

    #[test]
    fn subrip_becomes_webvtt_and_text() {
        let subtitles = parse(SRT);
        assert!(!subtitles.vtt);
        let cues = &subtitles.cues;
        // A blank line holding a space ends a cue; a cue with none after it runs into the next.
        assert_eq!(cues.len(), 3);
        assert_eq!((cues[0].start, cues[0].end), (1000, 2500));
        assert_eq!(cues[1].settings, "X1:10");
        assert_eq!(cues[2].id.as_deref(), Some("3"));
        let vtt = write(&subtitles, Format::Vtt);
        assert_eq!(
            vtt,
            "WEBVTT\n\n00:00:01.000 --> 00:00:02.500\n<i>Merhaba</i> &amp; hoş geldin\n\
             İkinci satır\n\n00:00:03.000 --> 00:00:04.000\nKırmızı &lt;3\n\n00:00:05.000 \
             --> 00:00:06.000\nBitişik\n\n"
        );
        let back = write(&parse(&vtt), Format::Srt);
        assert!(back.starts_with("1\n00:00:01,000 --> 00:00:02,500\n<i>Merhaba</i> & hoş"));
        assert!(back.contains("\nKırmızı <3\n"));
        assert_eq!(
            write(&subtitles, Format::Txt),
            "Merhaba & hoş geldin İkinci satır\nKırmızı <3\nBitişik\n"
        );
    }

    #[test]
    fn webvtt_reads_with_its_header_settings_and_short_times() {
        let vtt = "WEBVTT - a film\n\nSTYLE\n::cue { color: yellow }\n\nintro\n01:02.5 --> \
                   01:04.250 align:start\n<v Ali>Selam &lt;3</v>\n\nNOTE between\n\n01:05.000 \
                   --> 01:06.000\nİki\n";
        let subtitles = parse(vtt);
        assert_eq!(
            subtitles.header,
            ["WEBVTT - a film", "STYLE\n::cue { color: yellow }"]
        );
        assert_eq!(
            subtitles.cues[0],
            Cue {
                id: Some("intro".into()),
                start: 62_500,
                end: 64_250,
                settings: "align:start".into(),
                text: "<v Ali>Selam &lt;3</v>".into()
            }
        );
        assert_eq!(subtitles.cues.len(), 2);
        assert_eq!(write(&subtitles, Format::Txt), "Selam <3\nİki\n");
        // Kept WebVTT, all of it stays but the times.
        assert_eq!(
            write(&subtitles, Format::Vtt),
            "WEBVTT - a film\n\nSTYLE\n::cue { color: yellow }\n\nintro\n00:01:02.500 --> \
             00:01:04.250 align:start\n<v Ali>Selam &lt;3</v>\n\n00:01:05.000 --> \
             00:01:06.000\nİki\n\n"
        );
    }

    #[test]
    fn a_shift_moves_every_cue_and_drops_what_falls_before_the_start() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("film.srt");
        let words = "Işıklı şehrin üstünde güneş doğarken çocuklar ağaçların gölgesinde \
                     uyuyordu; öğretmen ödevleri düzeltip çayını içti.";
        let film = format!(
            "1\n00:00:00,400 --> 00:00:00,900\nGone\n\n2\n00:00:05,000 --> 00:00:06,000 X1:5\n\
             {{\\an8}}<font color=\"red\">{words}</font>\n"
        );
        // Windows-1254, as older Turkish subtitles are.
        let (bytes, _, lossy) = encoding_rs::WINDOWS_1254.encode(&film);
        assert!(!lossy && std::str::from_utf8(&bytes).is_err());
        fs::write(&input, &bytes).unwrap();
        let shifted = String::from_utf8(convert(&input, Format::Srt, -1000).unwrap()).unwrap();
        // Re-timed and renumbered; its position and colour stay.
        assert_eq!(
            shifted,
            format!(
                "1\n00:00:04,000 --> 00:00:05,000 X1:5\n{{\\an8}}<font color=\"red\">{words}</font>\n\n"
            )
        );
        assert_eq!(shift_by(shift_label(500).unwrap()), Some(500));

        fs::write(&input, "no times here").unwrap();
        let error = convert(&input, Format::Vtt, 0).unwrap_err();
        assert_eq!(error.to_string(), "there are no subtitles in this file");
    }
}
