use quick_xml::escape::unescape;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::error::Error;

use crate::domain::{LyricLine, LyricWord, LyricsDocument, LyricsSource, TrackIdentity};

pub fn parse_ttml(
    track: TrackIdentity,
    source: LyricsSource,
    xml: &str,
) -> Result<LyricsDocument, Box<dyn Error + Send + Sync>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut lines = Vec::new();
    let mut current: Option<PendingLine> = None;
    let mut active_word: Option<PendingWord> = None;
    let mut active_word_depth = 0usize;
    let mut in_body = false;
    let mut duration_ms = None;

    loop {
        match reader.read_event()? {
            Event::Start(element) if local_name(element.name().as_ref()) == b"body" => {
                in_body = true;
                duration_ms = optional_time(&element, b"dur", &reader)?;
            }
            Event::Start(element) if in_body && local_name(element.name().as_ref()) == b"p" => {
                if current.is_some() {
                    return Err("nested TTML lyric line is invalid".into());
                }
                let begin = required_time(&element, b"begin", &reader)?;
                let end = optional_time(&element, b"end", &reader)?;
                current = Some(PendingLine::new(begin, end));
            }
            Event::Start(element)
                if current.is_some() && local_name(element.name().as_ref()) == b"span" =>
            {
                if active_word.is_none() {
                    active_word = Some(PendingWord::new(
                        optional_time(&element, b"begin", &reader)?
                            .unwrap_or_else(|| current.as_ref().unwrap().begin),
                        optional_time(&element, b"end", &reader)?
                            .unwrap_or_else(|| current.as_ref().unwrap().end.unwrap_or(0)),
                    ));
                    active_word_depth = 1;
                } else {
                    active_word_depth += 1;
                }
            }
            Event::Empty(element)
                if current.is_some() && local_name(element.name().as_ref()) == b"br" =>
            {
                push_text(&mut current, &mut active_word, "\n");
            }
            Event::Text(text) => {
                let decoded = text.decode()?;
                let value = unescape(&decoded)?.into_owned();
                push_text(&mut current, &mut active_word, &value);
            }
            Event::CData(text) => {
                let value = text.decode()?;
                push_text(&mut current, &mut active_word, &value);
            }
            Event::End(element) if local_name(element.name().as_ref()) == b"span" => {
                active_word_depth = active_word_depth.saturating_sub(1);
                if active_word_depth == 0 {
                    let Some(word) = active_word.take() else {
                        continue;
                    };
                    if let Some(line) = current.as_mut() {
                        if !word.text.trim().is_empty() && word.end > word.begin {
                            line.words.push(LyricWord {
                                text: word.text,
                                begin_ms: word.begin,
                                end_ms: word.end,
                            });
                        }
                    }
                }
            }
            Event::End(element) if local_name(element.name().as_ref()) == b"p" => {
                if let Some(line) = current.take() {
                    if let Some(end) = line.end {
                        if end <= line.begin {
                            return Err("TTML line end must be after begin".into());
                        }
                    }
                    let text = if line.text.trim().is_empty() {
                        line.words
                            .iter()
                            .map(|word| word.text.as_str())
                            .collect::<String>()
                    } else {
                        line.text.clone()
                    };
                    if !text.trim().is_empty() {
                        lines.push(line.finish(text));
                    }
                }
            }
            Event::End(element) if local_name(element.name().as_ref()) == b"body" => {
                in_body = false
            }
            Event::Eof => break,
            _ => {}
        }
    }

    lines.sort_by_key(|line: &LyricLine| line.begin_ms);
    for index in 0..lines.len() {
        if lines[index].end_ms.is_none() {
            lines[index].end_ms = lines[index]
                .words
                .last()
                .map(|word| word.end_ms)
                .or_else(|| lines.get(index + 1).map(|line| line.begin_ms))
                .or(duration_ms);
        }
        if lines[index]
            .end_ms
            .is_some_and(|end| end <= lines[index].begin_ms)
        {
            return Err("invalid TTML line range".into());
        }
    }
    Ok(LyricsDocument {
        track,
        source,
        lines,
        duration_ms,
    })
}

fn push_text(
    current: &mut Option<PendingLine>,
    active_word: &mut Option<PendingWord>,
    value: &str,
) {
    if let Some(word) = active_word.as_mut() {
        word.text.push_str(value);
    } else if let Some(line) = current.as_mut() {
        line.text.push_str(value);
    }
}

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or(name)
}

fn attr_value<'a>(
    element: &'a BytesStart<'a>,
    key: &[u8],
    reader: &Reader<&[u8]>,
) -> Result<Option<String>, Box<dyn Error + Send + Sync>> {
    for attr in element.attributes().with_checks(false) {
        let attr = attr?;
        if local_name(attr.key.as_ref()) == key {
            return Ok(Some(
                attr.decode_and_unescape_value(reader.decoder())?
                    .into_owned(),
            ));
        }
    }
    Ok(None)
}

fn required_time(
    element: &BytesStart<'_>,
    key: &[u8],
    reader: &Reader<&[u8]>,
) -> Result<u64, Box<dyn Error + Send + Sync>> {
    attr_value(element, key, reader)?
        .ok_or_else(|| format!("TTML element missing {:?}", String::from_utf8_lossy(key)).into())
        .and_then(|value| {
            parse_time(&value).ok_or_else(|| format!("invalid TTML time: {value}").into())
        })
}

fn optional_time(
    element: &BytesStart<'_>,
    key: &[u8],
    reader: &Reader<&[u8]>,
) -> Result<Option<u64>, Box<dyn Error + Send + Sync>> {
    attr_value(element, key, reader)?
        .map(|value| parse_time(&value).ok_or_else(|| format!("invalid TTML time: {value}").into()))
        .transpose()
}

fn parse_time(value: &str) -> Option<u64> {
    let value = value.trim();
    if !value.contains(':') {
        let seconds = value.strip_suffix('s').unwrap_or(value);
        return seconds
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && *v >= 0.0)
            .map(|v| (v * 1000.0).round() as u64);
    }
    let parts: Vec<_> = value.split(':').collect();
    let seconds = match parts.as_slice() {
        [m, s] => m.parse::<u64>().ok()? as f64 * 60.0 + s.parse::<f64>().ok()?,
        [h, m, s] => {
            h.parse::<u64>().ok()? as f64 * 3600.0
                + m.parse::<u64>().ok()? as f64 * 60.0
                + s.parse::<f64>().ok()?
        }
        _ => return None,
    };
    Some((seconds * 1000.0).round() as u64)
}

struct PendingLine {
    begin: u64,
    end: Option<u64>,
    text: String,
    words: Vec<LyricWord>,
}
impl PendingLine {
    fn new(begin: u64, end: Option<u64>) -> Self {
        Self {
            begin,
            end,
            text: String::new(),
            words: Vec::new(),
        }
    }
    fn finish(self, text: String) -> LyricLine {
        LyricLine {
            begin_ms: self.begin,
            end_ms: self.end,
            text,
            words: self.words,
        }
    }
}
struct PendingWord {
    begin: u64,
    end: u64,
    text: String,
}
impl PendingWord {
    fn new(begin: u64, end: u64) -> Self {
        Self {
            begin,
            end,
            text: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lines_and_words() {
        let doc = parse_ttml(TrackIdentity { title: "T".into(), artist: "A".into(), album: None }, LyricsSource::AmllDb, r#"<tt><body><p begin="1.2s" end="2.5s"><span begin="1.2s" end="1.8s">Hello </span><span begin="1.8s" end="2.5s">world</span></p></body></tt>"#).unwrap();
        assert_eq!(doc.lines[0].begin_ms, 1200);
        assert_eq!(doc.lines[0].words.len(), 2);
    }

    #[test]
    fn parses_amll_time_formats() {
        assert_eq!(parse_time("48.739"), Some(48_739));
        assert_eq!(parse_time("48.739s"), Some(48_739));
        assert_eq!(parse_time("00:48.739"), Some(48_739));
        assert_eq!(parse_time("01:00:48.739"), Some(3_648_739));
    }

    #[test]
    fn parses_duration_cdata_and_infers_line_end() {
        let xml = r#"<tt><body dur="10s"><div><p begin="1s"><![CDATA[First & line]]></p><p begin="4s">Second<br/>line</p></div></body></tt>"#;
        let doc = parse_ttml(
            TrackIdentity {
                title: "T".into(),
                artist: "A".into(),
                album: None,
            },
            LyricsSource::AmllDb,
            xml,
        )
        .unwrap();

        assert_eq!(doc.duration_ms, Some(10_000));
        assert_eq!(doc.lines[0].text, "First & line");
        assert_eq!(doc.lines[0].end_ms, Some(4_000));
        assert_eq!(doc.lines[1].text, "Second\nline");
        assert_eq!(doc.lines[1].end_ms, Some(10_000));
    }

    #[test]
    fn keeps_nested_span_text_in_one_word() {
        let xml = r#"<tt><body><p begin="1s" end="2s"><span begin="1s" end="2s">A<span>B</span>C</span></p></body></tt>"#;
        let doc = parse_ttml(TrackIdentity::default(), LyricsSource::AmllDb, xml).unwrap();

        assert_eq!(doc.lines[0].words[0].text, "ABC");
    }

    #[test]
    fn preserves_overlapping_lines() {
        let xml = r#"<tt><body><div><p begin="3:20.025" end="3:29.289">First</p><p begin="3:21.022" end="3:24.366">Second</p><p begin="3:21.405" end="3:24.063">Third</p></div></body></tt>"#;
        let doc = parse_ttml(TrackIdentity::default(), LyricsSource::AppleMusicCache, xml).unwrap();

        assert_eq!(doc.lines.len(), 3);
        assert_eq!(doc.lines[0].begin_ms, 200_025);
        assert_eq!(doc.lines[0].end_ms, Some(209_289));
        assert_eq!(doc.lines[1].begin_ms, 201_022);
        assert_eq!(doc.lines[2].begin_ms, 201_405);
    }
}
