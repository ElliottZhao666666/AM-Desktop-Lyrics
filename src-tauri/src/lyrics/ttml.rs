use crate::domain::{LyricLine, LyricWord, LyricsDocument, LyricsSource, TrackIdentity};
use quick_xml::{escape::unescape, events::Event, Reader};
use std::{collections::BTreeMap, error::Error};
type ParseResult<T> = Result<T, Box<dyn Error + Send + Sync>>;
#[derive(Default)]
struct Element {
    name: String,
    attrs: BTreeMap<String, String>,
    children: Vec<Content>,
}
enum Content {
    Text(String),
    Element(Element),
}
impl Element {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.get(name).map(String::as_str)
    }
    fn time(&self, name: &str) -> ParseResult<Option<u64>> {
        self.attr(name)
            .map(|value| {
                parse_time(value).ok_or_else(|| format!("invalid TTML {name}: {value}").into())
            })
            .transpose()
    }
}
fn local_name(name: &[u8]) -> String {
    String::from_utf8_lossy(name.rsplit(|byte| *byte == b':').next().unwrap_or(name)).into_owned()
}
fn element(
    start: &quick_xml::events::BytesStart<'_>,
    reader: &Reader<&[u8]>,
) -> ParseResult<Element> {
    let mut attrs = BTreeMap::new();
    for attr in start.attributes() {
        let attr = attr?;
        attrs.insert(
            local_name(attr.key.as_ref()),
            attr.decode_and_unescape_value(reader.decoder())?
                .into_owned(),
        );
    }
    Ok(Element {
        name: local_name(start.name().as_ref()),
        attrs,
        children: Vec::new(),
    })
}
fn parse_tree(xml: &str) -> ParseResult<Element> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut stack = vec![Element::default()];
    loop {
        match reader.read_event()? {
            Event::Start(start) => {
                if stack.len() >= 128 {
                    return Err("TTML 嵌套层数超过 128".into());
                }
                stack.push(element(&start, &reader)?);
            }
            Event::Empty(start) => {
                let child = element(&start, &reader)?;
                stack
                    .last_mut()
                    .unwrap()
                    .children
                    .push(Content::Element(child));
            }
            Event::End(_) => {
                if stack.len() < 2 {
                    return Err("invalid TTML closing element".into());
                }
                let child = stack.pop().unwrap();
                stack
                    .last_mut()
                    .unwrap()
                    .children
                    .push(Content::Element(child));
            }
            Event::Text(text) => {
                let decoded = text.decode()?;
                stack
                    .last_mut()
                    .unwrap()
                    .children
                    .push(Content::Text(unescape(&decoded)?.into_owned()));
            }
            Event::CData(text) => stack
                .last_mut()
                .unwrap()
                .children
                .push(Content::Text(text.decode()?.into_owned())),
            Event::GeneralRef(reference) => {
                let encoded = format!("&{};", reference.decode()?);
                stack
                    .last_mut()
                    .unwrap()
                    .children
                    .push(Content::Text(unescape(&encoded)?.into_owned()));
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if stack.len() != 1 {
        return Err("incomplete TTML document".into());
    }
    Ok(stack.remove(0))
}
fn find<'a>(root: &'a Element, name: &str) -> Option<&'a Element> {
    if root.name == name {
        return Some(root);
    }
    root.children.iter().find_map(|child| {
        if let Content::Element(child) = child {
            find(child, name)
        } else {
            None
        }
    })
}

pub fn parse_ttml(
    track: TrackIdentity,
    source: LyricsSource,
    xml: &str,
) -> ParseResult<LyricsDocument> {
    parse_ttml_with_translation(track, source, xml, false)
}
pub fn parse_ttml_with_translation(
    track: TrackIdentity,
    source: LyricsSource,
    xml: &str,
    prefer_translation: bool,
) -> ParseResult<LyricsDocument> {
    if xml.len() > 8 * 1024 * 1024 {
        return Err("TTML 超过 8 MiB".into());
    }
    let root = parse_tree(xml)?;
    let body = find(&root, "body").ok_or("TTML body missing")?;
    let duration_ms = body.time("dur")?;
    let mut lines = Vec::new();
    collect_lines(body, None, None, &mut lines)?;
    finalize(&mut lines, duration_ms)?;
    if prefer_translation {
        let mut translated = Vec::new();
        collect_translations(&root, &mut translated);
        for translation in translated {
            let mut replacement = Vec::new();
            if collect_translation_lines(translation, &mut replacement).is_ok()
                && finalize(&mut replacement, duration_ms).is_ok()
                && replacement.len() == lines.len()
                && !replacement.is_empty()
                && replacement.iter().all(|line| !line.words.is_empty())
            {
                // 只采用可按时间逐行关联的完整 replacement，subtitle 或局部翻译不能覆盖正文。
                if replacement
                    .iter()
                    .zip(&lines)
                    .all(|(a, b)| a.begin_ms.abs_diff(b.begin_ms) <= 2000)
                {
                    lines = replacement;
                    break;
                }
            }
        }
    }
    Ok(LyricsDocument {
        track,
        source,
        lines,
        duration_ms,
    })
}
fn collect_translations<'a>(node: &'a Element, output: &mut Vec<&'a Element>) {
    if node.name == "translation"
        && node.attr("type") == Some("replacement")
        && node
            .attr("lang")
            .is_none_or(|language| language.to_lowercase().starts_with("zh"))
    {
        output.push(node);
        return;
    }
    for child in &node.children {
        if let Content::Element(child) = child {
            collect_translations(child, output);
        }
    }
}
fn collect_translation_lines(node: &Element, output: &mut Vec<LyricLine>) -> ParseResult<()> {
    for child in &node.children {
        if let Content::Element(child) = child {
            if child.name == "text" {
                if let Some(line) = parse_line(child, None, None)? {
                    output.push(line);
                }
            } else {
                collect_translation_lines(child, output)?;
            }
        }
    }
    Ok(())
}
fn collect_lines(
    node: &Element,
    parent_begin: Option<u64>,
    parent_end: Option<u64>,
    output: &mut Vec<LyricLine>,
) -> ParseResult<()> {
    let begin = node.time("begin")?.or(parent_begin);
    let end = node.time("end")?.or(parent_end);
    for child in &node.children {
        if let Content::Element(child) = child {
            if child.name == "p" {
                if let Some(line) = parse_line(child, begin, end)? {
                    output.push(line);
                }
            } else {
                collect_lines(child, begin, end, output)?;
            }
        }
    }
    Ok(())
}
#[derive(Clone)]
struct Fragment {
    text: String,
    time: Option<(u64, u64)>,
}
fn fragments(
    node: &Element,
    inherited: Option<(u64, u64)>,
    preserve: bool,
    output: &mut Vec<Fragment>,
) -> ParseResult<()> {
    let preserve = node
        .attr("space")
        .map(|value| value == "preserve")
        .unwrap_or(preserve);
    let time = if node.name == "span" {
        match (node.time("begin")?, node.time("end")?) {
            (Some(begin), Some(end)) if end > begin => Some((begin, end)),
            (Some(begin), None) => node
                .time("dur")?
                .map(|duration| (begin, begin.saturating_add(duration)))
                .or(inherited),
            (Some(_), Some(_)) => return Err("invalid TTML word range".into()),
            _ => inherited,
        }
    } else {
        inherited
    };
    for child in &node.children {
        match child {
            Content::Text(text) => {
                if !preserve && text.contains('\n') && text.chars().all(char::is_whitespace) {
                    continue;
                }
                if let Some(last) = output.last_mut().filter(|last| last.time == time) {
                    last.text.push_str(text);
                } else {
                    output.push(Fragment {
                        text: text.clone(),
                        time,
                    });
                }
            }
            Content::Element(child) if child.name == "br" => output.push(Fragment {
                text: "\n".into(),
                time,
            }),
            Content::Element(child) => fragments(child, time, preserve, output)?,
        }
    }
    Ok(())
}
fn parse_line(
    node: &Element,
    parent_begin: Option<u64>,
    parent_end: Option<u64>,
) -> ParseResult<Option<LyricLine>> {
    let mut parts = Vec::new();
    fragments(node, None, false, &mut parts)?;
    let text = parts
        .iter()
        .map(|part| part.text.as_str())
        .collect::<String>();
    if text.trim().is_empty() {
        return Ok(None);
    }
    let begin = node
        .time("begin")?
        .or_else(|| {
            parts
                .iter()
                .filter_map(|part| part.time.map(|time| time.0))
                .min()
        })
        .or(parent_begin)
        .unwrap_or(0);
    let end = node.time("end")?.or(node
        .time("dur")?
        .map(|duration| begin.saturating_add(duration)));
    let _ = parent_end;
    if end.is_some_and(|end| end <= begin) {
        return Err("invalid TTML line range".into());
    }
    let has_words = parts.iter().any(|part| part.time.is_some());
    let mut words: Vec<LyricWord> = Vec::new();
    if has_words {
        for (index, part) in parts.iter().enumerate() {
            if part.time.is_none() && part.text.chars().all(char::is_whitespace) {
                if let Some(last) = words.last_mut() {
                    last.text.push_str(&part.text);
                    continue;
                }
            }
            let before = parts[..index].iter().rev().find_map(|part| part.time);
            let after = parts[index + 1..].iter().find_map(|part| part.time);
            let inferred_begin = before.map(|time| time.1).unwrap_or(begin);
            let inferred_end = after.map(|time| time.0).or(end).unwrap_or(inferred_begin);
            let time = part.time.unwrap_or_else(|| {
                if inferred_end > inferred_begin {
                    (inferred_begin, inferred_end)
                } else {
                    before.or(after).unwrap_or((begin, begin.saturating_add(1)))
                }
            });
            words.push(LyricWord {
                text: part.text.clone(),
                begin_ms: time.0,
                end_ms: time.1,
            });
        }
    }
    Ok(Some(LyricLine {
        begin_ms: begin,
        end_ms: end,
        words,
        text,
    }))
}
fn finalize(lines: &mut [LyricLine], duration: Option<u64>) -> ParseResult<()> {
    lines.sort_by_key(|line| line.begin_ms);
    for index in 0..lines.len() {
        if lines[index].end_ms.is_none() {
            lines[index].end_ms = lines[index]
                .words
                .iter()
                .map(|word| word.end_ms)
                .max()
                .or_else(|| {
                    lines[index + 1..]
                        .iter()
                        .find(|next| next.begin_ms > lines[index].begin_ms)
                        .map(|next| next.begin_ms)
                })
                .or(duration);
        }
        if lines[index]
            .end_ms
            .is_some_and(|end| end <= lines[index].begin_ms)
        {
            return Err("invalid TTML inferred range".into());
        }
    }
    Ok(())
}
fn parse_time(value: &str) -> Option<u64> {
    let value = value.trim();
    let (value, multiplier) = if let Some(v) = value.strip_suffix("ms") {
        (v, 1.0)
    } else if let Some(v) = value.strip_suffix('h') {
        (v, 3600000.0)
    } else if let Some(v) = value.strip_suffix('m') {
        (v, 60000.0)
    } else {
        (value.strip_suffix('s').unwrap_or(value), 1000.0)
    };
    let parts: Vec<_> = value.split(':').collect();
    let mut seconds = 0.0;
    for part in parts {
        let value = part.parse::<f64>().ok()?;
        if !value.is_finite() || value < 0.0 {
            return None;
        }
        seconds = seconds * 60.0 + value;
    }
    let milliseconds = seconds * multiplier;
    (milliseconds.is_finite() && milliseconds <= u64::MAX as f64)
        .then_some(milliseconds.round() as u64)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_spaces_mixed_text_and_child_word_timing() {
        let xml = r#"<tt><body dur="10s"><p begin="1s" end="5s">PREFIX <span><span begin="2s" end="3s">Hello</span> <span begin="3s" end="4s">world &amp; friends</span></span> SUFFIX</p></body></tt>"#;
        let doc = parse_ttml(TrackIdentity::default(), LyricsSource::AppleMusicCache, xml).unwrap();
        let line = &doc.lines[0];
        assert_eq!(line.text, "PREFIX Hello world & friends SUFFIX");
        assert_eq!(
            line.words
                .iter()
                .map(|word| word.text.as_str())
                .collect::<String>(),
            line.text
        );
        assert_eq!(line.words[1].begin_ms, 2000);
        assert_eq!(line.words[1].end_ms, 3000);
    }
    #[test]
    fn both_parser_entry_points_infer_the_same_line_ends() {
        let xml = r#"<tt><body dur="10s"><div end="10s"><p begin="1s">First</p><p begin="4s">Second</p></div></body></tt>"#;
        let cache =
            parse_ttml(TrackIdentity::default(), LyricsSource::AppleMusicCache, xml).unwrap();
        let api = crate::lyrics::ttml_parser::parse_ttml(xml).unwrap();
        assert_eq!(cache.lines[0].end_ms, Some(4000));
        assert_eq!(api[0].end, Some(4000.0));
        assert_eq!(api[1].end, Some(10000.0));
    }
    #[test]
    fn only_complete_timed_chinese_replacement_can_replace_body() {
        let xml = r#"<tt><head><metadata><translations><translation type="subtitle" xml:lang="zh-Hans"><text begin="1s">字幕</text></translation><translation type="replacement" xml:lang="zh-Hans"><text begin="1s" end="3s"><span begin="1s" end="3s">简体</span></text></translation></translations></metadata></head><body dur="5s"><p begin="1s" end="3s"><span begin="1s" end="3s">繁體</span></p></body></tt>"#;
        let original = parse_ttml_with_translation(
            TrackIdentity::default(),
            LyricsSource::AppleMusicApi,
            xml,
            false,
        )
        .unwrap();
        let translated = parse_ttml_with_translation(
            TrackIdentity::default(),
            LyricsSource::AppleMusicApi,
            xml,
            true,
        )
        .unwrap();
        assert_eq!(original.lines[0].text, "繁體");
        assert_eq!(translated.lines[0].text, "简体");
        let no_timing = xml.replace("<span begin=\"1s\" end=\"3s\">简体</span>", "简体");
        assert_eq!(
            parse_ttml_with_translation(
                TrackIdentity::default(),
                LyricsSource::AppleMusicApi,
                &no_timing,
                true
            )
            .unwrap()
            .lines[0]
                .text,
            "繁體"
        );
    }
    #[test]
    fn rejects_invalid_times_and_keeps_same_start_lines() {
        assert!(parse_time("00:NaN").is_none());
        assert!(parse_time("-1s").is_none());
        assert_eq!(parse_time("1500ms"), Some(1500));
        let xml = r#"<tt><body dur="8s"><p begin="1s">A</p><p begin="1s">B</p><p begin="4s">C</p></body></tt>"#;
        let doc = parse_ttml(TrackIdentity::default(), LyricsSource::AmllDb, xml).unwrap();
        assert_eq!(doc.lines[0].end_ms, Some(4000));
        assert_eq!(doc.lines[1].end_ms, Some(4000));
    }

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
