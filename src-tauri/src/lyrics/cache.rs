use crate::config::AppConfig;
use crate::domain::{LyricsDocument, LyricsSource, TrackIdentity};
use crate::lyrics::cache_scanner::CacheCandidate;
use crate::lyrics::ttml::parse_ttml;
use crate::lyrics::{am_api, cache_scanner, matching};
use ferrous_opencc::{config::BuiltinConfig, OpenCC};
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime};

pub struct VerifiedCache {
    pub track_id: String,
    pub document: LyricsDocument,
}

/// 缓存和官方逐字 API 共用已核验的 ID 与同一份 TTML，不再分别猜测歌曲。
pub async fn fetch_lyrics(
    track: &TrackIdentity,
    requested_at: SystemTime,
    duration_ms: u64,
    config: &AppConfig,
) -> Result<Option<VerifiedCache>, String> {
    let Some(root) = cache_scanner::cache_root() else {
        println!("[歌词][Apple 缓存] 未找到 Apple Music 缓存目录");
        return Ok(None);
    };
    let mut seen_ids = std::collections::BTreeSet::new();
    let mut metadata = BTreeMap::new();
    for attempt in 0..8 {
        let scan_root = root.clone();
        let candidates = tauri::async_runtime::spawn_blocking(move || {
            cache_scanner::scan_candidates(&scan_root, duration_ms)
        })
        .await
        .map_err(|error| error.to_string())??;
        if !candidates.is_empty() {
            let ids: Vec<_> = candidates
                .iter()
                .map(|candidate| candidate.track_id.clone())
                .filter(|id| !seen_ids.contains(id))
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            if !ids.is_empty() {
                metadata.extend(
                    am_api::fetch_song_metadata(&ids, &config.am_api_tokens)
                        .await
                        .map_err(|error| format!("核验缓存歌曲身份失败: {error}"))?,
                );
                seen_ids.extend(ids);
            }
            if let Some(candidate) =
                select_candidate(&candidates, &metadata, track, duration_ms, requested_at)
            {
                let mut document = parse_ttml(
                    track.clone(),
                    LyricsSource::AppleMusicCache,
                    &candidate.ttml,
                )
                .map_err(|error| error.to_string())?;
                document.storage.apple_music_id = Some(candidate.track_id.clone());
                if !document.lines.is_empty() {
                    println!(
                        "[Cache] 身份核验通过: id={}, track={track:?}, duration={}",
                        candidate.track_id, candidate.duration_ms
                    );
                    return Ok(Some(VerifiedCache {
                        track_id: candidate.track_id,
                        document,
                    }));
                }
            }
        }
        if attempt < 7 {
            tokio::time::sleep(Duration::from_millis(240)).await;
        }
    }
    println!(
        "[歌词][Apple 缓存] 扫描完成，核验 {} 个候选 ID，未找到当前歌曲的可信歌词",
        seen_ids.len()
    );
    Ok(None)
}

fn select_candidate(
    candidates: &[CacheCandidate],
    metadata: &BTreeMap<String, am_api::CatalogSong>,
    track: &TrackIdentity,
    duration_ms: u64,
    requested_at: SystemTime,
) -> Option<CacheCandidate> {
    candidates
        .iter()
        .filter_map(|candidate| {
            let song = metadata.get(&candidate.track_id)?;
            if song.id != candidate.track_id {
                return None;
            }
            if song.duration_ms.is_some_and(|value| {
                value.abs_diff(candidate.duration_ms) > cache_scanner::MATCH_TOLERANCE_MS
            }) {
                return None;
            }
            let score =
                matching::score_candidate(track, duration_ms, &song.track, song.duration_ms);
            if !score.acceptable {
                return None;
            }
            if duration_ms > 0
                && candidate.duration_ms.abs_diff(duration_ms) > cache_scanner::MATCH_TOLERANCE_MS
            {
                return None;
            }
            let fresh = if candidate
                .modified
                .duration_since(requested_at)
                .unwrap_or_else(|error| error.duration())
                <= Duration::from_secs(4)
            {
                1
            } else {
                0
            };
            Some((score.total * 2 + fresh, candidate.modified, candidate))
        })
        .max_by_key(|(score, modified, _)| (*score, *modified))
        .map(|(_, _, candidate)| candidate.clone())
}
pub fn convert_to_simplified(mut document: LyricsDocument, enabled: bool) -> LyricsDocument {
    if !enabled {
        return document;
    }
    let converter = match OpenCC::from_config(BuiltinConfig::T2s) {
        Ok(converter) => converter,
        Err(error) => {
            eprintln!("[Cache] 繁转简转换器初始化失败，保留原歌词: {error}");
            return document;
        }
    };
    let mut changed = false;
    for line in &mut document.lines {
        let converted = converter.convert(&line.text);
        changed |= converted != line.text;
        line.text = converted;
        for word in &mut line.words {
            let converted = converter.convert(&word.text);
            changed |= converted != word.text;
            word.text = converted;
        }
    }
    if changed {
        println!("[Cache] 检测到繁体歌词，已转换为简体");
    } else {
        println!("[Cache] 歌词无需繁转简");
    }
    document
}

#[cfg(test)]
mod tests {
    use super::convert_to_simplified;
    use crate::domain::{LyricLine, LyricWord, LyricsDocument, LyricsSource, TrackIdentity};

    #[test]
    fn newer_previous_song_with_same_duration_cannot_override_verified_current_song() {
        use super::*;
        let now = SystemTime::now();
        let target = TrackIdentity {
            title: "Current".into(),
            artist: "Artist".into(),
            album: Some("Album".into()),
        };
        let candidates = vec![
            CacheCandidate {
                track_id: "1".into(),
                ttml: String::new(),
                duration_ms: 180000,
                modified: now,
            },
            CacheCandidate {
                track_id: "2".into(),
                ttml: String::new(),
                duration_ms: 180000,
                modified: now - Duration::from_secs(300),
            },
        ];
        let metadata = BTreeMap::from([
            (
                "1".into(),
                am_api::CatalogSong {
                    id: "1".into(),
                    track: TrackIdentity {
                        title: "Previous".into(),
                        ..target.clone()
                    },
                    duration_ms: Some(180000),
                },
            ),
            (
                "2".into(),
                am_api::CatalogSong {
                    id: "2".into(),
                    track: target.clone(),
                    duration_ms: Some(180000),
                },
            ),
        ]);
        assert_eq!(
            select_candidate(&candidates, &metadata, &target, 180000, now)
                .unwrap()
                .track_id,
            "2"
        );
        assert!(select_candidate(&candidates, &BTreeMap::new(), &target, 180000, now).is_none());
        let mut corrupt = metadata;
        corrupt.get_mut("2").unwrap().duration_ms = Some(220000);
        assert!(select_candidate(&candidates, &corrupt, &target, 180000, now).is_none());
    }

    #[test]
    fn converts_cached_lyrics_to_simplified_chinese() {
        let document = LyricsDocument {
            storage: Default::default(),
            track: TrackIdentity::default(),
            source: LyricsSource::AppleMusicCache,
            duration_ms: None,
            lines: vec![LyricLine {
                begin_ms: 0,
                end_ms: Some(1_000),
                text: "這條歌陪我一直到大漢".into(),
                words: vec![LyricWord {
                    text: "這條歌".into(),
                    begin_ms: 0,
                    end_ms: 1_000,
                }],
            }],
        };

        assert_eq!(
            convert_to_simplified(document.clone(), false).lines[0].text,
            document.lines[0].text
        );
        let converted = convert_to_simplified(document, true);
        assert_eq!(converted.lines[0].text, "这条歌陪我一直到大汉");
        assert_eq!(converted.lines[0].words[0].text, "这条歌");
    }
}
