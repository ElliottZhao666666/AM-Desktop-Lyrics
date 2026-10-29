use super::{am_api, custom_pool, lrc, provider, ttml};
use crate::domain::{
    LocalizedLyricText, LyricsDocument, LyricsSource, LyricsStorage, LyricsTiming, TrackIdentity,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

static EDITOR_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PoolKind {
    Cache,
    Manual,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolEntry {
    id: String,
    revision: String,
    track: TrackIdentity,
    apple_music_id: Option<String>,
    timing: LyricsTiming,
    has_translation: bool,
    has_pronunciation: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolDetail {
    entry: PoolEntry,
    lyrics_text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolDraft {
    pool: PoolKind,
    id: Option<String>,
    revision: Option<String>,
    track: TrackIdentity,
    apple_music_id: String,
    lyrics_text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Validation {
    timing: LyricsTiming,
    line_count: usize,
}

fn fingerprint(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    format!("{hash:016x}")
}
pub(crate) fn entry_id(track: &TrackIdentity) -> String {
    fingerprint(&serde_json::to_vec(track).expect("track serialization"))
}
fn revision(document: &LyricsDocument) -> String {
    fingerprint(&serde_json::to_vec(document).expect("lyrics serialization"))
}
pub(crate) fn check_revision(
    document: &LyricsDocument,
    expected: Option<&str>,
) -> Result<(), String> {
    if expected != Some(revision(document).as_str()) {
        return Err("条目已更新，请关闭编辑窗口并刷新列表后重试，避免覆盖新内容".into());
    }
    Ok(())
}

fn timing(document: &LyricsDocument) -> LyricsTiming {
    if document.lines.iter().any(|line| !line.words.is_empty()) {
        LyricsTiming::Word
    } else {
        LyricsTiming::Line
    }
}
fn summary(id: String, document: &LyricsDocument) -> PoolEntry {
    PoolEntry {
        id,
        revision: revision(document),
        track: document.track.clone(),
        timing: timing(document),
        has_translation: document.lines.iter().any(|line| {
            line.translations
                .iter()
                .any(|text| !text.text.trim().is_empty())
        }),
        has_pronunciation: document.lines.iter().any(|line| {
            line.pronunciations
                .iter()
                .any(|text| !text.text.trim().is_empty())
        }),
        apple_music_id: document.storage.apple_music_id.clone().or_else(|| {
            am_api::cached_song(&document.track, document.duration_ms.unwrap_or(0))
                .map(|song| song.id)
        }),
    }
}
fn records(pool: PoolKind) -> Result<Vec<(String, LyricsDocument)>, String> {
    match pool {
        PoolKind::Cache => Ok(provider::pool_cache_list()?
            .into_iter()
            .map(|doc| (entry_id(&doc.track), doc))
            .collect()),
        PoolKind::Manual => custom_pool::list(),
    }
}
fn document(pool: PoolKind, id: &str) -> Result<LyricsDocument, String> {
    records(pool)?
        .into_iter()
        .find(|(key, _)| key == id)
        .map(|(_, doc)| doc)
        .ok_or("该条目已不存在或已被缓存淘汰，请刷新列表".into())
}

#[tauri::command]
pub async fn list_lyrics_pool(pool: PoolKind) -> Result<Vec<PoolEntry>, String> {
    Ok(records(pool)?
        .into_iter()
        .map(|(id, doc)| summary(id, &doc))
        .collect())
}
#[tauri::command]
pub async fn get_lyrics_pool_entry(pool: PoolKind, id: String) -> Result<PoolDetail, String> {
    let doc = document(pool, &id)?;
    Ok(PoolDetail {
        entry: summary(id, &doc),
        lyrics_text: editable_text(&doc),
    })
}

fn xml_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn append_localization_text(output: &mut String, text: &LocalizedLyricText) {
    if text.words.is_empty() {
        output.push_str(&xml_text(&text.text));
    } else {
        for word in &text.words {
            output.push_str(&format!(
                "<span xmlns=\"http://www.w3.org/ns/ttml\" begin=\"{}ms\" end=\"{}ms\">{}</span>",
                word.begin_ms,
                word.end_ms,
                xml_text(&word.text),
            ));
        }
    }
}

fn append_localizations(output: &mut String, doc: &LyricsDocument, pronunciation: bool) {
    fn items(line: &crate::domain::LyricLine, pronunciation: bool) -> &[LocalizedLyricText] {
        if pronunciation {
            &line.pronunciations
        } else {
            &line.translations
        }
    }
    let languages: std::collections::BTreeSet<_> = doc
        .lines
        .iter()
        .flat_map(|line| {
            items(line, pronunciation)
                .iter()
                .map(|entry| entry.language.as_str())
        })
        .collect();
    if languages.is_empty() {
        return;
    }
    let (container, element) = if pronunciation {
        ("transliterations", "transliteration")
    } else {
        ("translations", "translation")
    };
    output.push_str(&format!("        <{container}>\n"));
    for language in languages {
        let kind = if pronunciation {
            ""
        } else {
            " type=\"subtitle\""
        };
        output.push_str(&format!(
            "          <{element}{kind} xml:lang=\"{}\">\n",
            xml_text(language)
        ));
        for (index, line) in doc.lines.iter().enumerate() {
            if let Some(entry) = items(line, pronunciation)
                .iter()
                .find(|entry| entry.language == language)
            {
                output.push_str(&format!(
                    "            <text for=\"L{}\" xml:space=\"preserve\">",
                    index + 1
                ));
                append_localization_text(output, entry);
                output.push_str("</text>\n");
            }
        }
        output.push_str(&format!("          </{element}>\n"));
    }
    output.push_str(&format!("        </{container}>\n"));
}

/// 重建可编辑 TTML，同时保留逐字时间、重叠行、句间边界和所有语言的辅助文本。
fn editable_text(doc: &LyricsDocument) -> String {
    let duration = doc
        .duration_ms
        .map(|time| format!(" dur=\"{time}ms\""))
        .unwrap_or_default();
    let mut text = "<tt xmlns=\"http://www.w3.org/ns/ttml\" xmlns:itunes=\"http://music.apple.com/lyric-ttml-internal\">\n".to_string();
    if doc
        .lines
        .iter()
        .any(|line| !line.translations.is_empty() || !line.pronunciations.is_empty())
    {
        text.push_str("  <head>\n    <metadata>\n      <iTunesMetadata xmlns=\"http://music.apple.com/lyric-ttml-internal\">\n");
        append_localizations(&mut text, doc, false);
        append_localizations(&mut text, doc, true);
        text.push_str("      </iTunesMetadata>\n    </metadata>\n  </head>\n");
    }
    text.push_str(&format!("  <body{duration}>\n    <div>\n"));
    for (index, line) in doc.lines.iter().enumerate() {
        let end = line
            .end_ms
            .map(|time| format!(" end=\"{time}ms\""))
            .unwrap_or_default();
        text.push_str(&format!(
            "      <p begin=\"{}ms\"{end} itunes:key=\"L{}\" xml:space=\"preserve\">",
            line.begin_ms,
            index + 1,
        ));
        if line.words.is_empty() {
            text.push_str(&xml_text(&line.text));
        } else {
            for word in &line.words {
                text.push_str(&format!(
                    "<span begin=\"{}ms\" end=\"{}ms\">{}</span>",
                    word.begin_ms,
                    word.end_ms,
                    xml_text(&word.text)
                ));
            }
        }
        text.push_str("</p>\n");
    }
    text.push_str("    </div>\n  </body>\n</tt>");
    text
}

fn parse_text(
    track: TrackIdentity,
    text: &str,
    duration: Option<u64>,
) -> Result<LyricsDocument, String> {
    let text = text.trim().trim_start_matches('\u{feff}');
    if text.is_empty() || text.len() > 8 * 1024 * 1024 {
        return Err("歌词内容不能为空，且不能超过 8 MiB".into());
    }
    let mut doc = if text.starts_with('<') {
        // 不把任意 XML 或无时间轴的正文误认为 TTML 歌词。
        let mut reader = quick_xml::Reader::from_str(text);
        let mut root_seen = false;
        let mut depth = 0usize;
        let mut has_timing = false;
        loop {
            use quick_xml::events::Event;
            match reader
                .read_event()
                .map_err(|error| format!("TTML XML 格式错误：{error}"))?
            {
                event @ (Event::Start(_) | Event::Empty(_)) => {
                    let (element, opens) = match &event {
                        Event::Start(element) => (element, true),
                        Event::Empty(element) => (element, false),
                        _ => unreachable!(),
                    };
                    if depth == 0 && root_seen {
                        return Err("TTML 只能有一个根元素".into());
                    }
                    if !root_seen {
                        if element.local_name().as_ref() != b"tt" {
                            return Err("TTML 的根元素必须是 <tt>".into());
                        }
                        root_seen = true;
                    }
                    if opens {
                        depth += 1;
                    }
                    for attr in element.attributes() {
                        let attr = attr.map_err(|error| error.to_string())?;
                        if attr.key.local_name().as_ref() == b"begin" {
                            has_timing = true;
                        }
                    }
                }
                Event::End(_) => {
                    depth = depth.saturating_sub(1);
                }
                Event::Text(text)
                    if depth == 0
                        && !text
                            .decode()
                            .map_err(|error| error.to_string())?
                            .trim()
                            .is_empty() =>
                {
                    return Err("TTML 根元素外不能有正文".into())
                }
                Event::DocType(_) => return Err("TTML 歌词不支持 DOCTYPE".into()),
                Event::Eof => break,
                _ => {}
            }
        }
        if !root_seen || !has_timing {
            return Err("TTML 中缺少有效的歌词时间标记".into());
        }
        ttml::parse_ttml(track, LyricsSource::Manual, text)
            .map_err(|error| format!("TTML 解析失败：{error}"))?
    } else {
        validate_lrc(text)?;
        let lines = lrc::parse_lrc(text, duration);
        LyricsDocument {
            track,
            source: LyricsSource::Manual,
            lines,
            duration_ms: duration,
            storage: LyricsStorage {
                apple_music_id: None,
                timing: Some(LyricsTiming::Line),
                localization_version: 1,
            },
        }
    };
    if doc.lines.is_empty() {
        return Err("没有找到有效的带时间戳歌词行，请输入 TTML 逐字、TTML 逐行或 LRC 歌词".into());
    }
    doc.duration_ms = doc.duration_ms.or(duration);
    Ok(doc)
}

fn validate_lrc(text: &str) -> Result<(), String> {
    for (index, line) in text.lines().enumerate() {
        let mut rest = line.trim();
        if rest.is_empty() {
            continue;
        }
        let mut timed = false;
        while let Some((tag, tail)) = rest.strip_prefix('[').and_then(|rest| rest.split_once(']')) {
            if !tag
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b':' || byte == b'.')
                || lrc::timestamp(tag).is_none()
            {
                break;
            }
            timed = true;
            rest = tail;
        }
        if timed {
            continue;
        }
        let valid_metadata = rest
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
            .and_then(|tag| tag.split_once(':'))
            .is_some_and(|(key, value)| {
                if key.eq_ignore_ascii_case("offset") {
                    value.trim().parse::<i64>().is_ok()
                } else {
                    ["ar", "ti", "al", "by", "re", "ve", "length", "au", "id"]
                        .iter()
                        .any(|known| key.eq_ignore_ascii_case(known))
                }
            });
        if !valid_metadata {
            return Err(format!(
                "LRC 第 {} 行格式不正确，需要 [mm:ss.xxx] 时间戳或标准信息标签",
                index + 1
            ));
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn validate_pool_lyrics(lyrics_text: String) -> Result<Validation, String> {
    let doc = parse_text(TrackIdentity::default(), &lyrics_text, None)?;
    Ok(Validation {
        timing: timing(&doc),
        line_count: doc.lines.len(),
    })
}

fn normalize_track(track: &TrackIdentity) -> Result<TrackIdentity, String> {
    let track = TrackIdentity {
        title: track.title.trim().into(),
        artist: track.artist.trim().into(),
        album: track
            .album
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
    };
    if track.title.is_empty() || track.artist.is_empty() {
        return Err("请填写歌名和艺人".into());
    }
    if track.title.len() > 1024
        || track.artist.len() > 1024
        || track.album.as_ref().is_some_and(|album| album.len() > 1024)
    {
        return Err("歌曲信息过长".into());
    }
    Ok(track)
}

#[tauri::command]
pub async fn save_lyrics_pool_entry(app: AppHandle, draft: PoolDraft) -> Result<String, String> {
    ensure_editable(draft.pool)?;
    let track = normalize_track(&draft.track)?;
    let apple_id = draft.apple_music_id.trim();
    if !apple_id.is_empty()
        && (apple_id.len() > 32
            || !apple_id.bytes().all(|byte| byte.is_ascii_digit())
            || apple_id.bytes().all(|byte| byte == b'0'))
    {
        return Err("Apple Music 歌曲 ID 必须是有效的数字 ID，也可以留空".into());
    }
    let old = draft
        .id
        .as_ref()
        .map(|id| document(draft.pool, id))
        .transpose()?;
    if let Some(old) = &old {
        check_revision(old, draft.revision.as_deref())?;
    }
    let duration = old
        .as_ref()
        .filter(|old| old.track == track)
        .and_then(|old| old.duration_ms);
    // 必须先完整校验、重新解析，再进行任何池文件写入。
    let mut doc = parse_text(track.clone(), &draft.lyrics_text, duration)?;
    doc.storage.apple_music_id = (!apple_id.is_empty()).then(|| apple_id.to_string());
    let id = custom_pool::managed_save(draft.id.as_deref(), draft.revision.as_deref(), doc)?;
    if let Some(old) = old.filter(|old| old.track != track) {
        provider::invalidate_manual_cache(&old.track)?;
        refresh_if_current(&app, &old.track);
    }
    provider::invalidate_manual_cache(&track)?;
    refresh_if_current(&app, &track);
    let _ = app.emit("lyrics-pools-changed", ());
    println!(
        "[歌词池] 已保存 {:?}: {} - {}",
        draft.pool, track.title, track.artist
    );
    Ok(id)
}

fn refresh_if_current(app: &AppHandle, track: &TrackIdentity) {
    if crate::playback::snapshot()
        .is_ok_and(|snapshot| custom_pool::same_track(&snapshot.media.track, track))
    {
        provider::refresh_current_lyrics(app.clone());
    }
}

#[tauri::command]
pub async fn delete_lyrics_pool_entry(
    app: AppHandle,
    pool: PoolKind,
    id: String,
    revision: String,
) -> Result<(), String> {
    match pool {
        PoolKind::Cache => provider::pool_cache_delete(&id, &revision)?,
        PoolKind::Manual => {
            let removed = custom_pool::managed_delete(&id, &revision)?;
            provider::invalidate_manual_cache(&removed.track)?;
            refresh_if_current(&app, &removed.track);
        }
    }
    let _ = app.emit("lyrics-pools-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn promote_lyrics_pool_entry(
    app: AppHandle,
    id: String,
    revision: String,
) -> Result<(), String> {
    let mut doc = document(PoolKind::Cache, &id)?;
    check_revision(&doc, Some(&revision))?;
    if doc.storage.apple_music_id.is_none() {
        doc.storage.apple_music_id =
            am_api::cached_song(&doc.track, doc.duration_ms.unwrap_or(0)).map(|song| song.id);
    }
    let track = doc.track.clone();
    custom_pool::managed_save(None, None, doc)?;
    let _ = app.emit("lyrics-pools-changed", ());
    refresh_if_current(&app, &track);
    Ok(())
}

#[tauri::command]
pub async fn resolve_pool_apple_music_id(track: TrackIdentity) -> Result<String, String> {
    let track = normalize_track(&track)?;
    am_api::search_song_id(&track).await
}

#[tauri::command]
pub async fn open_lyrics_pool_editor(
    app: AppHandle,
    pool: PoolKind,
    id: Option<String>,
) -> Result<(), String> {
    ensure_editable(pool)?;
    if let Some(id) = &id {
        document(pool, id)?;
    }
    let label = id
        .as_ref()
        .map(|id| format!("lyrics-editor-edit-{}", fingerprint(id.as_bytes())))
        .unwrap_or_else(|| {
            format!(
                "lyrics-editor-new-{}",
                EDITOR_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            )
        });
    if let Some(window) = app.get_webview_window(&label) {
        window.unminimize().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
        return Ok(());
    }
    let pool = "manual";
    let query = format!(
        "index.html?pool={pool}{}",
        id.map(|id| format!("&id={}", urlencoding::encode(&id)))
            .unwrap_or_default()
    );
    let mut builder = WebviewWindowBuilder::new(&app, label, WebviewUrl::App(query.into()))
        .title("AMDL · 编辑歌词")
        .inner_size(760.0, 650.0)
        .min_inner_size(640.0, 520.0)
        .center()
        .maximizable(false);
    if let Some(settings) = app.get_webview_window("settings") {
        builder = builder
            .parent(&settings)
            .map_err(|error| error.to_string())?;
    }
    builder
        .build()
        .map_err(|error| format!("打开歌词编辑窗口失败：{error}"))?;
    Ok(())
}

fn ensure_editable(pool: PoolKind) -> Result<(), String> {
    if pool == PoolKind::Cache {
        return Err("缓存池不支持直接编辑，请先保存到手动匹配池".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing_and_serialization_preserve_all_localizations_and_timed_pronunciation() {
        let xml = r#"<tt xmlns:itunes="urn:i"><head><metadata>
          <translation type="subtitle" xml:lang="zh-Hans"><text for="A">你好 &amp; 再见</text><text for="B">和声</text></translation>
          <translation type="subtitle" xml:lang="en"><text for="A">Hello &amp; bye</text></translation>
          <transliteration xml:lang="zh-Latn-pinyin"><text for="A"><span begin="1s" end="2s">nǐ </span><span begin="2s" end="4s">hǎo</span></text><text for="B">hé shēng</text></transliteration>
        </metadata></head><body dur="8s"><p itunes:key="A" begin="1s" end="4s"><span begin="1s" end="2s">你</span><span begin="2s" end="4s">好</span></p><p itunes:key="B" begin="2s" end="5s">(和声)</p><p begin="6s" end="7s">end</p></body></tt>"#;
        let mut doc = parse_text(TrackIdentity::default(), xml, None).unwrap();
        doc.storage.apple_music_id = Some("123".into());
        let stored = serde_json::to_vec(&doc).unwrap();
        let loaded: LyricsDocument = serde_json::from_slice(&stored).unwrap();
        assert_eq!(loaded, doc);
        let edited = editable_text(&loaded);
        let reparsed = parse_text(loaded.track.clone(), &edited, None).unwrap();
        assert_eq!(reparsed.lines, loaded.lines);
        assert_eq!(reparsed.duration_ms, loaded.duration_ms);
        let entry = summary("test".into(), &reparsed);
        assert_eq!(entry.timing, LyricsTiming::Word);
        assert!(entry.has_translation && entry.has_pronunciation);
        let json = serde_json::to_value(entry).unwrap();
        assert_eq!(json["hasTranslation"], true);
        assert_eq!(json["hasPronunciation"], true);
        let old = parse_text(TrackIdentity::default(), "[00:01]old", None).unwrap();
        let entry = summary("old".into(), &old);
        assert!(!entry.has_translation && !entry.has_pronunciation);
        assert_eq!(
            parse_text(old.track.clone(), &editable_text(&old), None)
                .unwrap()
                .lines,
            old.lines
        );
    }
    #[test]
    #[ignore = "只读兼容性检查：需要 AMDL_POOL_COMPAT_FILE 指定真实缓存副本或原文件"]
    fn existing_pool_reconstruction_readonly() {
        let path = std::env::var_os("AMDL_POOL_COMPAT_FILE").expect("AMDL_POOL_COMPAT_FILE");
        let bytes = std::fs::read(path).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let docs: Vec<LyricsDocument> = serde_json::from_value(value["entries"].clone()).unwrap();
        for doc in &docs {
            let restored = parse_text(doc.track.clone(), &editable_text(doc), None).unwrap();
            assert_eq!(restored.lines, doc.lines, "{}", doc.track.title);
        }
        println!(
            "已只读验证 {} 首已有缓存的时间轴还原，逐字、逐行与重叠计时均保持一致",
            docs.len()
        );
    }
    #[test]
    #[ignore = "仅在隔离进程中运行：使用临时 LOCALAPPDATA 验证池文件与内存联动"]
    fn isolated_pool_storage_roundtrip_and_shrink() {
        let directory = std::env::temp_dir().join(format!("amdl-pool-test-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        std::env::set_var("LOCALAPPDATA", &directory);
        crate::settings_store::update(|root| {
            root.insert("localCacheLimit".into(), 30.into());
        })
        .unwrap();
        let docs: Vec<_> = (0..30)
            .map(|index| {
                let track = TrackIdentity {
                    title: format!("recent-{index}"),
                    artist: "Artist".into(),
                    album: Some("Album".into()),
                };
                let mut doc = parse_text(track, "[00:01.000]A\n[00:03.000]B", Some(5000)).unwrap();
                doc.source = LyricsSource::QqMusic;
                doc
            })
            .collect();
        let cache_file = crate::app_paths::lyrics_cache_file().unwrap();
        crate::settings_store::atomic_write(&cache_file, &serde_json::to_vec(&serde_json::json!({"match_version":super::super::matching::MATCH_VERSION,"auto_chs":true,"entries":docs})).unwrap()).unwrap();
        assert_eq!(provider::pool_cache_list().unwrap().len(), 30);
        let old = provider::pool_cache_list().unwrap()[0].clone();
        let id = entry_id(&old.track);
        let mut edited = parse_text(old.track.clone(), "<tt><body dur=\"5s\"><p begin=\"1s\" end=\"3s\"><span begin=\"1s\" end=\"2s\">A</span><span begin=\"2s\" end=\"3s\">B</span></p></body></tt>", None).unwrap();
        edited.storage.apple_music_id = Some("12345".into());
        assert!(ensure_editable(PoolKind::Cache).is_err());
        assert!(ensure_editable(PoolKind::Manual).is_ok());
        let key = custom_pool::managed_save(None, None, old.clone()).unwrap();
        let original_manual = custom_pool::get(&old.track).unwrap().unwrap();
        custom_pool::managed_save(
            Some(&key),
            Some(&revision(&original_manual)),
            edited.clone(),
        )
        .unwrap();
        assert!(custom_pool::managed_save(
            Some(&key),
            Some(&revision(&original_manual)),
            old.clone()
        )
        .is_err());
        assert!(custom_pool::managed_save(None, None, edited.clone()).is_err());
        let mut manual = custom_pool::get(&edited.track).unwrap().unwrap();
        assert_eq!(manual.storage.apple_music_id.as_deref(), Some("12345"));
        assert_eq!(timing(&manual), LyricsTiming::Word);
        assert_eq!(
            provider::pool_cache_list().unwrap()[0],
            old,
            "编辑手动匹配不修改缓存池"
        );
        let old_manual = manual.clone();
        manual.track.title = "Renamed".into();
        let new_key =
            custom_pool::managed_save(Some(&key), Some(&revision(&old_manual)), manual.clone())
                .unwrap();
        assert_ne!(new_key, key);
        assert!(custom_pool::get(&old_manual.track).unwrap().is_none());
        assert!(custom_pool::get(&manual.track).unwrap().is_some());
        custom_pool::managed_delete(&new_key, &revision(&manual)).unwrap();
        assert!(custom_pool::list().unwrap().is_empty());
        // 磁盘写入失败不能把内存中的条目改成未落盘的新内容。
        let backup = cache_file.with_extension("backup");
        std::fs::rename(&cache_file, &backup).unwrap();
        std::fs::create_dir(&cache_file).unwrap();
        assert!(provider::pool_cache_delete(&id, &revision(&old)).is_err());
        assert_eq!(provider::pool_cache_list().unwrap()[0], old);
        std::fs::remove_dir(&cache_file).unwrap();
        std::fs::rename(&backup, &cache_file).unwrap();
        crate::settings_store::update(|root| {
            root.insert("localCacheLimit".into(), 10.into());
        })
        .unwrap();
        provider::apply_cache_settings(10, false).unwrap();
        let retained = provider::pool_cache_list().unwrap();
        assert_eq!(retained.len(), 10);
        assert_eq!(retained[0].track.title, "recent-0");
        assert_eq!(retained[9].track.title, "recent-9");
        let stored: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&cache_file).unwrap()).unwrap();
        assert_eq!(stored["entries"].as_array().unwrap().len(), 10);
        provider::pool_cache_delete(&id, &revision(&old)).unwrap();
        assert_eq!(provider::pool_cache_list().unwrap().len(), 9);
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn validates_and_distinguishes_all_three_formats() {
        let track = TrackIdentity::default();
        assert_eq!(
            timing(&parse_text(track.clone(), "[00:01.000]Hello\n[00:03.000]World", None).unwrap()),
            LyricsTiming::Line
        );
        let line = "<tt><body><p begin=\"1s\" end=\"3s\">Hello</p></body></tt>";
        assert_eq!(
            timing(&parse_text(track.clone(), line, None).unwrap()),
            LyricsTiming::Line
        );
        let word = line.replace("Hello", "<span begin=\"1s\" end=\"3s\">Hello</span>");
        let doc = parse_text(track, &word, None).unwrap();
        assert_eq!(timing(&doc), LyricsTiming::Word);
        assert_eq!(doc.lines[0].words[0].end_ms, 3000);
        for invalid in [
            "Hello world",
            "[00:99]bad",
            "[00:01]Good\nnot a lyric",
            "<tt><body><p>Hello</p></body></tt>",
            "<foo><body><p begin=\"1s\">Hi</p></body></foo>",
            "<tt><body><p begin=\"2s\" end=\"1s\">bad</p></body></tt>",
            "<tt><body>",
        ] {
            assert!(
                parse_text(TrackIdentity::default(), invalid, None).is_err(),
                "{invalid}"
            );
        }
    }
    #[test]
    fn legacy_editing_preserves_words_overlaps_and_lrc_gaps() {
        let doc = parse_text(TrackIdentity::default(), "<tt><body dur=\"10s\"><p begin=\"1s\" end=\"5s\"><span begin=\"1s\" end=\"4s\">A &amp; </span><span begin=\"2s\" end=\"5s\">(B)</span></p></body></tt>", None).unwrap();
        assert_eq!(
            parse_text(doc.track.clone(), &editable_text(&doc), None)
                .unwrap()
                .lines,
            doc.lines
        );
        let doc = parse_text(
            TrackIdentity::default(),
            "[00:01.000]A\n[00:02.000]\n[00:05.000]B\n[00:06.000]",
            None,
        )
        .unwrap();
        assert_eq!(
            parse_text(doc.track.clone(), &editable_text(&doc), None)
                .unwrap()
                .lines,
            doc.lines
        );
    }
    #[test]
    fn revisions_reject_stale_edits_and_old_json_still_loads() {
        let old = r#"{"track":{"title":"Song","artist":"Artist","album":null},"source":"qqMusic","lines":[],"duration_ms":null}"#;
        let doc: LyricsDocument = serde_json::from_str(old).unwrap();
        assert!(doc.storage.timing.is_none());
        assert!(check_revision(&doc, Some(&revision(&doc))).is_ok());
        let mut changed = doc.clone();
        changed.track.title = "New".into();
        assert!(check_revision(&changed, Some(&revision(&doc))).is_err());
    }
}
