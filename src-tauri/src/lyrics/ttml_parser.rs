use quick_xml::escape::unescape;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::error::Error;

use super::models::{LyricLine, LyricWord};

pub fn parse_ttml(xml: &str) -> Result<Vec<LyricLine>, Box<dyn Error + Send + Sync>> {
    parse_ttml_with_translation(xml, xml.contains("extend") || xml.contains("<translations"))
}

pub fn parse_ttml_with_translation(
    xml: &str,
    prefer_translation: bool,
) -> Result<Vec<LyricLine>, Box<dyn Error + Send + Sync>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut lines = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut current: Option<PendingLine> = None;
    let mut word: Option<PendingWord> = None;
    let mut word_depth = 0usize;
    let mut in_body = false;
    let mut in_translation = false;
    let mut translation_depth = 0usize;

    loop {
        match reader.read_event()? {
            Event::Start(element) => {
                let name = local_name(element.name().as_ref()).to_vec();
                if name == b"translations" && prefer_translation {
                    in_translation = true;
                    translation_depth = stack.len() + 1;
                }
                let is_translation_text = in_translation && name == b"text";
                if name == b"body" && (!prefer_translation || !in_translation) {
                    in_body = true;
                } else if is_translation_text || (in_body && name == b"p") {
                    if name == b"p" || is_translation_text {
                        let begin = attr_time(&element, b"begin", &reader)?.unwrap_or(0);
                        let end = attr_time(&element, b"end", &reader)?;
                        current = Some(PendingLine { begin, end, text: String::new(), words: Vec::new() });
                    }
                } else if current.is_some() && name == b"span" {
                    if word.is_none() {
                        let begin = attr_time(&element, b"begin", &reader)?.unwrap_or(current.as_ref().unwrap().begin);
                        let end = attr_time(&element, b"end", &reader)?.unwrap_or(begin);
                        word = Some(PendingWord { begin, end, text: String::new() });
                        word_depth = 1;
                    } else { word_depth += 1; }
                }
                stack.push(String::from_utf8_lossy(&name).into_owned());
            }
            Event::Empty(element) if current.is_some() && local_name(element.name().as_ref()) == b"br" => push_text(&mut current, &mut word, "\n"),
            Event::Text(text) => {
                let decoded = text.decode()?;
                push_text(&mut current, &mut word, &unescape(&decoded)?.into_owned());
            }
            Event::CData(text) => push_text(&mut current, &mut word, &text.decode()?),
            Event::End(element) => {
                let element_name = element.name();
                let name = local_name(element_name.as_ref());
                if name == b"span" && word.is_some() {
                    word_depth = word_depth.saturating_sub(1);
                    if word_depth == 0 {
                        if let Some(item) = word.take() {
                            if item.end > item.begin && !item.text.trim().is_empty() {
                                current.as_mut().unwrap().words.push(LyricWord { text: item.text, begin: item.begin as f64, end: item.end as f64 });
                            }
                        }
                    }
                }
                if (name == b"p" || name == b"text") && current.is_some() {
                    let item = current.take().unwrap();
                    let text = if item.text.trim().is_empty() { item.words.iter().map(|w| w.text.as_str()).collect() } else { item.text.clone() };
                    if !text.trim().is_empty() {
                        lines.push(LyricLine { begin: item.begin as f64, end: item.end.map(|v| v as f64), text, words: (!item.words.is_empty()).then_some(item.words) });
                    }
                }
                if name == b"body" { in_body = false; }
                if name == b"translations" && in_translation && stack.len() == translation_depth { in_translation = false; }
                stack.pop();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    lines.sort_by(|a, b| a.begin.total_cmp(&b.begin));
    Ok(lines)
}

fn push_text(line: &mut Option<PendingLine>, word: &mut Option<PendingWord>, text: &str) {
    if let Some(item) = word { item.text.push_str(text); }
    else if let Some(item) = line { item.text.push_str(text); }
}

fn local_name(name: &[u8]) -> &[u8] { name.rsplit(|byte| *byte == b':').next().unwrap_or(name) }

fn attr_time(element: &BytesStart<'_>, key: &[u8], reader: &Reader<&[u8]>) -> Result<Option<u64>, Box<dyn Error + Send + Sync>> {
    for attr in element.attributes().with_checks(false) {
        let attr = attr?;
        if local_name(attr.key.as_ref()) == key {
            let value = attr.decode_and_unescape_value(reader.decoder())?;
            return Ok(parse_time(&value));
        }
    }
    Ok(None)
}

fn parse_time(value: &str) -> Option<u64> {
    let value = value.trim().strip_suffix('s').unwrap_or(value.trim());
    let parts: Vec<_> = value.split(':').collect();
    let seconds = match parts.as_slice() {
        [seconds] => seconds.parse::<f64>().ok()?,
        [minutes, seconds] => minutes.parse::<f64>().ok()? * 60.0 + seconds.parse::<f64>().ok()?,
        [hours, minutes, seconds] => hours.parse::<f64>().ok()? * 3600.0 + minutes.parse::<f64>().ok()? * 60.0 + seconds.parse::<f64>().ok()?,
        _ => return None,
    };
    (seconds.is_finite() && seconds >= 0.0).then_some((seconds * 1000.0).round() as u64)
}

struct PendingLine { begin: u64, end: Option<u64>, text: String, words: Vec<LyricWord> }
struct PendingWord { begin: u64, end: u64, text: String }