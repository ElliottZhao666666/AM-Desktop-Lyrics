use quick_xml::{events::Event, Reader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
const MAX_CACHE_FILE_SIZE: u64 = 8 * 1024 * 1024;
pub const MATCH_TOLERANCE_MS: u64 = 4_000;
#[derive(Debug, Clone)]
pub struct CacheCandidate {
    pub track_id: String,
    pub ttml: String,
    pub duration_ms: u64,
    pub modified: SystemTime,
}
pub fn cache_root() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|local| {
        PathBuf::from(local).join("Packages/AppleInc.AppleMusicWin_nzyj5cx40ttqa/AC/INetCache")
    })
}
pub fn scan_candidates(
    root: &Path,
    target_duration_ms: u64,
) -> Result<Vec<CacheCandidate>, String> {
    let mut files = Vec::new();
    collect_files(root, &mut files)?;
    files.sort_by_key(|path| {
        std::fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
    });
    files.reverse();
    let mut candidates = Vec::new();
    for path in files {
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        if metadata.len() > MAX_CACHE_FILE_SIZE {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(json) =
            serde_json::from_str::<serde_json::Value>(content.trim_start_matches('\u{feff}'))
        else {
            continue;
        };
        for (track_id, ttml) in cached_lyrics(&json) {
            let Some(duration_ms) = body_duration_ms(ttml) else {
                continue;
            };
            if !timeline_fits_track(duration_ms, target_duration_ms) {
                continue;
            }
            candidates.push(CacheCandidate {
                track_id: track_id.into(),
                ttml: ttml.into(),
                duration_ms,
                modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            });
        }
        if candidates.len() >= 64 {
            break;
        }
    }
    Ok(candidates)
}
fn collect_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if !root.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if kind.is_dir() {
            collect_files(&path, files)?;
        } else if kind.is_file()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(is_cache_name)
        {
            files.push(path);
        }
    }
    Ok(())
}
fn is_cache_name(name: &str) -> bool {
    name.strip_prefix("ttmlLyrics[")
        .or_else(|| name.strip_prefix("syllable-lyrics["))
        .and_then(|value| value.strip_suffix("].json"))
        .is_some_and(|number| {
            !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
        })
}

/// 旧版扁平响应与 1.7 的 JSON:API 响应共存；仅取歌词资源正文，避免误取翻译片段。
fn cached_lyrics(json: &serde_json::Value) -> Vec<(&str, &str)> {
    if json
        .get("status")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|s| s != "success")
    {
        return Vec::new();
    }
    let mut result = Vec::new();
    if let (Some(id), Some(ttml)) = (
        json.get("lyricsId")
            .and_then(serde_json::Value::as_str)
            .and_then(catalog_id),
        json.get("ttml").and_then(serde_json::Value::as_str),
    ) {
        result.push((id, ttml));
    }
    for resource in json
        .get("data")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
    {
        if !matches!(
            resource.get("type").and_then(serde_json::Value::as_str),
            Some("syllable-lyrics" | "lyrics")
        ) {
            continue;
        }
        let Some(id) = resource
            .get("id")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
        else {
            continue;
        };
        let Some(attributes) = resource.get("attributes") else {
            continue;
        };
        if attributes
            .pointer("/playParams/catalogId")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|other| other != id)
        {
            continue;
        }
        // 同一资源可能同时含正文与带翻译/注音的完整版。两者都保留为同 ID 候选：
        // 完整版优先参与解析，若它损坏仍可回退正文；不能由字段顺序丢掉辅助数据。
        for field in ["ttmlLocalizations", "ttml"] {
            if let Some(ttml) = attributes.get(field).and_then(serde_json::Value::as_str) {
                if !result.iter().any(|(existing_id, existing_ttml)| {
                    *existing_id == id && *existing_ttml == ttml
                }) {
                    result.push((id, ttml));
                }
            }
        }
    }
    result
}

fn catalog_id(lyrics_id: &str) -> Option<&str> {
    let value = lyrics_id.strip_prefix("AP_")?;
    let (id, language) = value
        .split_once('-')
        .map(|(id, language)| (id, Some(language)))
        .unwrap_or((value, None));
    if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if language.is_some_and(|language| {
        !language
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
            || !language
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    }) {
        return None;
    }
    Some(id)
}
/// TTML body 的时长是歌词时间轴，尾奏可使其明显短于歌曲；只拒绝明显超出歌曲的时间轴。
/// 这只是粗筛，歌曲身份及真实曲目时长仍须通过缓存 ID 的元数据核验。
pub fn timeline_fits_track(lyrics_duration_ms: u64, track_duration_ms: u64) -> bool {
    track_duration_ms == 0
        || lyrics_duration_ms <= track_duration_ms.saturating_add(MATCH_TOLERANCE_MS)
}

pub fn body_duration_ms(ttml: &str) -> Option<u64> {
    let mut reader = Reader::from_str(ttml);
    loop {
        match reader.read_event().ok()? {
            Event::Start(element) | Event::Empty(element)
                if element.local_name().as_ref() == b"body" =>
            {
                for attribute in element.attributes() {
                    let attribute = attribute.ok()?;
                    if attribute.key.as_ref() == b"dur" {
                        let value = attribute.decode_and_unescape_value(reader.decoder()).ok()?;
                        return parse_time(&value);
                    }
                }
                return None;
            }
            Event::Eof => return None,
            _ => {}
        }
    }
}
fn parse_time(value: &str) -> Option<u64> {
    let value = value.trim();
    if let Some(milliseconds) = value.strip_suffix("ms") {
        return milliseconds
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && *v >= 0.0)
            .map(|v| v.round() as u64);
    }
    let value = value.strip_suffix('s').unwrap_or(value);
    let parts: Vec<_> = value.split(':').collect();
    let seconds = match parts.as_slice() {
        [s] => s.parse::<f64>().ok()?,
        [m, s] => m.parse::<f64>().ok()? * 60.0 + s.parse::<f64>().ok()?,
        [h, m, s] => {
            h.parse::<f64>().ok()? * 3600.0
                + m.parse::<f64>().ok()? * 60.0
                + s.parse::<f64>().ok()?
        }
        _ => return None,
    };
    (seconds.is_finite() && seconds >= 0.0).then_some((seconds * 1000.0).round() as u64)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_and_new_envelopes_preserve_timing_and_reject_conflicting_ids() {
        use crate::domain::{LyricsSource, LyricsTiming, TrackIdentity};
        let line = r#"<tt><body dur="180s"><p begin="1s" end="3s">one line</p></body></tt>"#;
        let word = r#"<tt xmlns:itunes="urn:itunes"><head><translations><translation type="subtitle"><text for="L1">translation</text></translation></translations></head><body dur="180s"><p begin="1s" end="3s"><span begin="1s" end="2s">one </span><span begin="2s" end="3s">line</span></p></body></tt>"#;
        let legacy = serde_json::json!({"status":"success","lyricsId":"AP_123-en","ttml":line});
        assert_eq!(cached_lyrics(&legacy), vec![("123", line)]);
        let mut modern = serde_json::json!({"data":[{"id":"123","type":"syllable-lyrics","attributes":{"playParams":{"catalogId":"123"},"ttmlLocalizations":word}}]});
        let items = cached_lyrics(&modern);
        assert_eq!(items, vec![("123", word)]);
        let doc = crate::lyrics::ttml::parse_ttml(
            TrackIdentity::default(),
            LyricsSource::AppleMusicCache,
            items[0].1,
        )
        .unwrap();
        assert_eq!(doc.storage.timing, Some(LyricsTiming::Word));
        assert_eq!(doc.lines[0].words.len(), 2);
        assert_eq!(doc.lines[0].text, "one line");
        modern["data"][0]["attributes"]["playParams"]["catalogId"] = "456".into();
        assert!(cached_lyrics(&modern).is_empty());
        for name in ["ttmlLyrics[1].json", "syllable-lyrics[12].json"] {
            assert!(is_cache_name(name));
        }
        for name in ["profile[1].json", "syllable-lyrics[bad].json"] {
            assert!(!is_cache_name(name));
        }
    }

    #[test]
    #[ignore = "只读实际 Apple Music 缓存；需指定 AMDL_TEST_CACHE_ROOT"]
    fn inspect_live_apple_cache() {
        let root = PathBuf::from(std::env::var_os("AMDL_TEST_CACHE_ROOT").expect("cache root"));
        let candidates = scan_candidates(&root, 0).unwrap();
        assert!(!candidates.is_empty());
        for candidate in candidates {
            let doc = crate::lyrics::ttml::parse_ttml(
                crate::domain::TrackIdentity::default(),
                crate::domain::LyricsSource::AppleMusicCache,
                &candidate.ttml,
            )
            .unwrap();
            println!(
                "CACHE_SAMPLE id={} duration={} lines={} words={} timing={:?}",
                candidate.track_id,
                candidate.duration_ms,
                doc.lines.len(),
                doc.lines.iter().map(|line| line.words.len()).sum::<usize>(),
                doc.storage.timing
            );
            if candidate.track_id == "1833328845" {
                assert_eq!(doc.storage.timing, Some(crate::domain::LyricsTiming::Word));
            }
        }
    }
    #[test]
    fn lyric_timeline_allows_outro_without_accepting_an_overlong_track() {
        assert!(timeline_fits_track(242242, 260440));
        assert!(timeline_fits_track(267732, 275320));
        assert!(timeline_fits_track(180000, 180000));
        assert!(timeline_fits_track(184000, 180000));
        assert!(!timeline_fits_track(184001, 180000));
        assert!(timeline_fits_track(242242, 0));
    }
    #[test]
    fn parses_real_cache_duration_and_namespaced_body() {
        assert_eq!(
            body_duration_ms(r#"<tt><body dur="3:05.041"></body></tt>"#),
            Some(185041)
        );
        assert_eq!(
            body_duration_ms(r#"<tt:tt xmlns:tt="urn:test"><tt:body dur = '185041ms'/></tt:tt>"#),
            Some(185041)
        );
        assert!(body_duration_ms("<tt><body dur='NaNs'/></tt>").is_none());
    }
    #[test]
    fn localized_lyrics_id_keeps_catalog_id_without_locale_suffix() {
        assert_eq!(super::catalog_id("AP_6820341520-en"), Some("6820341520"));
        assert_eq!(super::catalog_id("AP_123-zh-Hans"), Some("123"));
        assert_eq!(super::catalog_id("AP_6779309947"), Some("6779309947"));
        assert!(super::catalog_id("AP_123-../path").is_none());
        assert!(super::catalog_id("AP_bad-en").is_none());
    }
}
