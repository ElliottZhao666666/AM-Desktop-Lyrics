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
    pub song: am_api::CatalogSong,
    pub document: LyricsDocument,
}

pub struct ResolvedApple {
    pub song: am_api::CatalogSong,
    pub cached: Option<VerifiedCache>,
    pub used_ambiguous_fallback: bool,
}

pub async fn resolve_identity(
    track: &TrackIdentity,
    requested_at: SystemTime,
    duration_ms: u64,
    config: &AppConfig,
) -> Result<Option<ResolvedApple>, String> {
    let (cached, searched) = futures_util::future::join(
        fetch_lyrics(track, requested_at, duration_ms, config),
        am_api::search_songs(track, duration_ms, &config.am_api_tokens),
    )
    .await;
    let mut errors = Vec::new();
    let mut cached = cached.unwrap_or_else(|error| {
        eprintln!("[歌词][歌曲 ID] 缓存核验失败，尝试搜索旁路: {error}");
        errors.push(error);
        None
    });
    let mut songs = searched.unwrap_or_else(|error| {
        eprintln!("[歌词][歌曲 ID] 搜索失败，尝试缓存旁路: {error}");
        errors.push(error);
        BTreeMap::new()
    });
    let online_ids = songs
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    if let Some(cached) = &cached {
        songs
            .entry(cached.track_id.clone())
            .or_insert_with(|| cached.song.clone());
    }
    // 两条并行路径都返回后，才能判断搜索歧义。缓存独立元数据核验失败时，
    // 搜索结果本身仍可为原始缓存 ID 提供身份信息，不能先因多个搜索结果退出。
    if cached.is_none() && !songs.is_empty() {
        cached = cache_for_metadata(track, &songs, duration_ms, requested_at)
            .await
            .unwrap_or_else(|error| {
                eprintln!("[歌词][歌曲 ID] 合并搜索结果核验原始缓存失败: {error}");
                None
            });
    }
    let mut selection = am_api::select_song_with_evidence(
        track,
        duration_ms,
        &songs,
        cached.as_ref().map(|c| c.track_id.as_str()),
    );
    let mut used_fallback = false;
    if selection.is_err() {
        println!("[歌词][歌曲 ID] 在线候选同等匹配，等待 500ms 后复查 Apple Music 缓存");
        tokio::time::sleep(Duration::from_millis(500)).await;
        if let Some(rechecked) = cache_for_metadata(track, &songs, duration_ms, SystemTime::now())
            .await
            .unwrap_or_else(|error| {
                eprintln!("[歌词][歌曲 ID] 延迟复查缓存失败，继续评分兜底: {error}");
                None
            })
        {
            cached = Some(rechecked);
        }
        selection = am_api::select_song_with_evidence(
            track,
            duration_ms,
            &songs,
            cached.as_ref().map(|c| c.track_id.as_str()),
        );
        if selection.is_err() {
            used_fallback = true;
            selection = Ok(am_api::select_song_best_effort(track, duration_ms, &songs));
        }
    }
    let Some(song) = selection? else {
        return if errors.is_empty() {
            Ok(None)
        } else {
            Err(errors.join("；"))
        };
    };
    let mut cached = cached.filter(|c| c.track_id == song.id);
    if cached.is_none() && !used_fallback {
        // 搜索已拿到身份时，可直接核验同 ID 的原始缓存，避免缓存元数据接口暂时失败阻断此渠道。
        cached = cache_for_song(track, &song, duration_ms, requested_at)
            .await
            .unwrap_or_else(|error| {
                eprintln!(
                    "[歌词][歌曲 ID] 搜索结果已核验，读取对应缓存失败，仍保留 API 旁路: {error}"
                );
                None
            });
    }
    println!(
        "[歌词][歌曲 ID] 匹配成功: id={}，依据={}",
        song.id,
        if used_fallback {
            "缓存复查未消歧，按在线最高评分兜底（同分取接口首项）"
        } else if cached.is_some() && online_ids.contains(&song.id) {
            "缓存与在线搜索交叉印证"
        } else if cached.is_some() {
            "已核验缓存旁路"
        } else {
            "在线搜索，歌名/艺人/专辑/时长评分"
        }
    );
    Ok(Some(ResolvedApple {
        song,
        cached,
        used_ambiguous_fallback: used_fallback,
    }))
}

async fn cache_for_song(
    track: &TrackIdentity,
    song: &am_api::CatalogSong,
    duration_ms: u64,
    requested_at: SystemTime,
) -> Result<Option<VerifiedCache>, String> {
    let metadata = BTreeMap::from([(song.id.clone(), song.clone())]);
    cache_for_metadata(track, &metadata, duration_ms, requested_at).await
}

async fn cache_for_metadata(
    track: &TrackIdentity,
    metadata: &BTreeMap<String, am_api::CatalogSong>,
    duration_ms: u64,
    requested_at: SystemTime,
) -> Result<Option<VerifiedCache>, String> {
    let Some(root) = cache_scanner::cache_root() else {
        return Ok(None);
    };
    let candidates = tauri::async_runtime::spawn_blocking(move || {
        cache_scanner::scan_candidates(&root, duration_ms)
    })
    .await
    .map_err(|error| error.to_string())??;
    Ok(verified_from_candidates(
        &candidates,
        metadata,
        track,
        duration_ms,
        requested_at,
    ))
}

/// AMDL 中旧的 Apple 缓存逐行条目可就地提升到新逐字缓存；不改变手动匹配池的选择。
pub async fn upgrade_cached_document(
    document: &LyricsDocument,
    duration_ms: u64,
) -> Option<LyricsDocument> {
    if document.source != LyricsSource::AppleMusicCache
        || (document.storage.localization_version > 0
            && document.lines.iter().any(|line| !line.words.is_empty()))
    {
        return None;
    }
    let song = am_api::cached_song(&document.track, duration_ms)?;
    if document.storage.apple_music_id.as_deref() != Some(song.id.as_str()) {
        return None;
    }
    let cached = cache_for_song(&document.track, &song, duration_ms, SystemTime::now())
        .await
        .ok()??;
    let old_word = document.lines.iter().any(|line| !line.words.is_empty());
    let new_word = cached
        .document
        .lines
        .iter()
        .any(|line| !line.words.is_empty());
    ((!old_word || new_word) && (document.storage.localization_version == 0 || new_word))
        .then_some(cached.document)
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
            if let Some(cached) =
                verified_from_candidates(&candidates, &metadata, track, duration_ms, requested_at)
            {
                println!(
                    "[Cache] 身份核验通过: id={}, track={track:?}, duration={}",
                    cached.track_id,
                    cached.document.duration_ms.unwrap_or(0)
                );
                return Ok(Some(cached));
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
    let mut ranked: Vec<_> = candidates
        .iter()
        .filter_map(|candidate| {
            let song = metadata.get(&candidate.track_id)?;
            if song.id != candidate.track_id {
                return None;
            }
            if song.duration_ms.is_some_and(|value| {
                !cache_scanner::timeline_fits_track(candidate.duration_ms, value)
            }) {
                return None;
            }
            let score =
                matching::score_candidate(track, duration_ms, &song.track, song.duration_ms);
            if !score.acceptable {
                return None;
            }
            if !cache_scanner::timeline_fits_track(candidate.duration_ms, duration_ms) {
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
            let document = usable_document(track, candidate)?;
            let word_timed = document.lines.iter().any(|line| !line.words.is_empty());
            let auxiliary_count: usize = document
                .lines
                .iter()
                .map(|line| line.translations.len() + line.pronunciations.len())
                .sum();
            // 只让可完整解析的候选参与排序，损坏逐字必须回退到有效逐行。
            Some((
                score.total,
                word_timed,
                auxiliary_count,
                fresh,
                candidate.modified,
                candidate,
            ))
        })
        .collect();
    ranked.sort_by_key(|(score, word_timed, auxiliary_count, fresh, modified, _)| {
        std::cmp::Reverse((*score, *word_timed, *auxiliary_count, *fresh, *modified))
    });
    let best_score = ranked.first()?.0;
    let top_ids: std::collections::BTreeSet<_> = ranked
        .iter()
        .filter(|entry| entry.0 == best_score)
        .map(|entry| entry.5.track_id.as_str())
        .collect();
    let chosen_id = if top_ids.len() == 1 {
        *top_ids.first()?
    } else {
        // 不同发行版即使正文/时长相同也不能只凭词数或mtime大小随意猜 ID。
        // 只允许唯一的本次播放附近新生成缓存提供额外时间证据。
        let fresh_ids: std::collections::BTreeSet<_> = ranked
            .iter()
            .filter(|entry| entry.0 == best_score && entry.3 == 1)
            .map(|entry| entry.5.track_id.as_str())
            .collect();
        if fresh_ids.len() != 1 {
            return None;
        }
        *fresh_ids.first()?
    };
    ranked
        .into_iter()
        .find(|entry| entry.5.track_id == chosen_id)
        .map(|entry| entry.5.clone())
}

fn usable_document(track: &TrackIdentity, candidate: &CacheCandidate) -> Option<LyricsDocument> {
    let document = parse_ttml(
        track.clone(),
        LyricsSource::AppleMusicCache,
        &candidate.ttml,
    )
    .ok()?;
    if document.lines.is_empty()
        || document
            .lines
            .iter()
            .all(|line| line.text.trim().is_empty())
    {
        return None;
    }
    // Word 声明但无可用词时间不能悄悄当成逐行，否则损坏逐字会遮住有效逐行缓存。
    let mut reader = quick_xml::Reader::from_str(&candidate.ttml);
    let mut declared_word = false;
    loop {
        match reader.read_event().ok()? {
            quick_xml::events::Event::Start(element) | quick_xml::events::Event::Empty(element)
                if element.local_name().as_ref() == b"tt" =>
            {
                for attr in element.attributes().flatten() {
                    if attr.key.local_name().as_ref() == b"timing" {
                        declared_word = attr
                            .decode_and_unescape_value(reader.decoder())
                            .ok()?
                            .eq_ignore_ascii_case("word");
                    }
                }
                break;
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
    }
    if declared_word && !document.lines.iter().any(|line| !line.words.is_empty()) {
        return None;
    }
    Some(document)
}

fn verified_from_candidates(
    candidates: &[CacheCandidate],
    metadata: &BTreeMap<String, am_api::CatalogSong>,
    track: &TrackIdentity,
    duration_ms: u64,
    requested_at: SystemTime,
) -> Option<VerifiedCache> {
    let candidate = select_candidate(candidates, metadata, track, duration_ms, requested_at)?;
    let mut document = usable_document(track, &candidate)?;
    let song = metadata.get(&candidate.track_id)?.clone();
    document.storage.apple_music_id = Some(candidate.track_id.clone());
    document.duration_ms = song
        .duration_ms
        .filter(|duration| *duration > 0)
        .or(document.duration_ms);
    Some(VerifiedCache {
        song,
        track_id: candidate.track_id,
        document,
    })
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
        for auxiliary in line.translations.iter_mut().chain(&mut line.pronunciations) {
            let converted = converter.convert(&auxiliary.text);
            changed |= converted != auxiliary.text;
            auxiliary.text = converted;
            for word in &mut auxiliary.words {
                let converted = converter.convert(&word.text);
                changed |= converted != word.text;
                word.text = converted;
            }
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
    fn verified_cache_keeps_shorter_lyric_timeline_and_uses_real_track_duration() {
        use super::*;
        let now = SystemTime::now();
        let track = TrackIdentity {
            title: "Song".into(),
            artist: "Artist".into(),
            album: Some("Album".into()),
        };
        let metadata = BTreeMap::from([(
            "1".into(),
            am_api::CatalogSong {
                id: "1".into(),
                track: track.clone(),
                duration_ms: Some(260440),
                search_rank: None,
            },
        )]);
        let mut candidate = CacheCandidate { track_id: "1".into(),
            ttml: "<tt><body dur=\"242.242s\"><p begin=\"1s\" end=\"242.242s\">outro follows</p></body></tt>".into(),
            duration_ms: 242242, modified: now,
        };
        let cached =
            verified_from_candidates(&[candidate.clone()], &metadata, &track, 260000, now).unwrap();
        assert_eq!(cached.document.duration_ms, Some(260440));
        assert_eq!(cached.document.storage.apple_music_id.as_deref(), Some("1"));
        candidate.duration_ms = 280000;
        assert!(verified_from_candidates(&[candidate], &metadata, &track, 260000, now).is_none());
        let unrelated = TrackIdentity {
            title: "Other Song".into(),
            ..track
        };
        assert!(verified_from_candidates(
            &[CacheCandidate {
                track_id: "1".into(),
                ttml: "<tt><body dur=\"242s\"><p begin=\"1s\" end=\"2s\">other</p></body></tt>"
                    .into(),
                duration_ms: 242000,
                modified: now,
            }],
            &metadata,
            &unrelated,
            260000,
            now
        )
        .is_none());
    }
    #[test]
    fn broken_word_cache_falls_back_and_search_metadata_corroborates_raw_ids() {
        use super::*;
        let now = SystemTime::now();
        let track = TrackIdentity {
            title: "Same Song".into(),
            artist: "Artist".into(),
            album: Some("Album".into()),
        };
        let metadata = BTreeMap::from([
            (
                "1".into(),
                am_api::CatalogSong {
                    id: "1".into(),
                    track: track.clone(),
                    duration_ms: Some(180000),
                    search_rank: None,
                },
            ),
            (
                "2".into(),
                am_api::CatalogSong {
                    id: "2".into(),
                    track: track.clone(),
                    duration_ms: Some(180000),
                    search_rank: None,
                },
            ),
        ]);
        assert!(am_api::select_song_with_evidence(&track, 180000, &metadata, None).is_err());
        let valid = CacheCandidate {
            track_id: "2".into(),
            ttml: r#"<tt><body dur="180s"><p begin="1s" end="3s">valid line</p></body></tt>"#
                .into(),
            duration_ms: 180000,
            modified: now - Duration::from_secs(60),
        };
        for bad in [
            r#"<tt xmlns:itunes="urn:test" itunes:timing="Word"><body dur="180s"><p begin="1s" end="3s"><span begin="4s" end="2s">bad word</span></p></body></tt>"#,
            r#"<tt xmlns:itunes="urn:test" itunes:timing="Word"><body dur="180s"><p begin="1s" end="3s">missing word times</p></body></tt>"#,
            r#"<tt><body dur="180s"><p begin="1s">"#,
        ] {
            let corrupted = CacheCandidate {
                ttml: bad.into(),
                modified: now,
                ..valid.clone()
            };
            let cached = verified_from_candidates(
                &[corrupted, valid.clone()],
                &metadata,
                &track,
                180000,
                now,
            )
            .expect("损坏逐字应回退有效逐行");
            assert_eq!(cached.document.lines[0].text, "valid line");
            let selected = am_api::select_song_with_evidence(
                &track,
                180000,
                &metadata,
                Some(&cached.track_id),
            )
            .unwrap()
            .unwrap();
            assert_eq!(
                selected.id, "2",
                "搜索的元数据应帮助核验原始缓存，然后才能判断多 ID 歧义"
            );
        }
        let duplicate = CacheCandidate {
            track_id: "1".into(),
            ..valid.clone()
        };
        assert!(
            verified_from_candidates(&[duplicate, valid], &metadata, &track, 180000, now).is_none(),
            "两个同样陈旧缓存不能凭时间先后随机选 ID"
        );
    }

    #[test]
    fn verified_word_cache_wins_over_newer_line_cache_for_same_song() {
        use super::*;
        let track = TrackIdentity {
            title: "Song".into(),
            artist: "Artist".into(),
            album: None,
        };
        let now = SystemTime::now();
        let word = CacheCandidate { track_id: "1".into(), ttml: r#"<tt><body dur="180s"><p begin="1s" end="3s"><span begin="1s" end="2s">one</span><span begin="2s" end="3s">two</span></p></body></tt>"#.into(), duration_ms: 180000, modified: now - Duration::from_secs(60) };
        let line = CacheCandidate {
            ttml: r#"<tt><body dur="180s"><p begin="1s" end="3s">line</p></body></tt>"#.into(),
            modified: now,
            ..word.clone()
        };
        let metadata = BTreeMap::from([(
            "1".into(),
            am_api::CatalogSong {
                id: "1".into(),
                track: track.clone(),
                duration_ms: Some(180000),
                search_rank: None,
            },
        )]);
        let selected =
            select_candidate(&[line, word.clone()], &metadata, &track, 180000, now).unwrap();
        assert_eq!(selected.ttml, word.ttml);
    }

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
                ttml: r#"<tt><body dur="180s"><p begin="1s" end="3s">previous</p></body></tt>"#
                    .into(),
                duration_ms: 180000,
                modified: now,
            },
            CacheCandidate {
                track_id: "2".into(),
                ttml: r#"<tt><body dur="180s"><p begin="1s" end="3s">current</p></body></tt>"#
                    .into(),
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
                    search_rank: None,
                },
            ),
            (
                "2".into(),
                am_api::CatalogSong {
                    id: "2".into(),
                    track: target.clone(),
                    duration_ms: Some(180000),
                    search_rank: None,
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
                translations: Vec::new(),
                pronunciations: Vec::new(),
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
