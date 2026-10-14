use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::domain::{LyricsDocument, LyricsSource, TrackIdentity};
use crate::lyrics::qq_music::QqSongCandidate;

static CUSTOM_POOL_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CustomLyricsRecord {
    track: TrackIdentity,
    #[serde(default)]
    selected_song: Option<QqSongCandidate>,
    document: LyricsDocument,
}

type CustomLyricsPool = BTreeMap<String, CustomLyricsRecord>;

pub fn get(track: &TrackIdentity) -> Result<Option<LyricsDocument>, String> {
    let _guard = CUSTOM_POOL_LOCK
        .lock()
        .map_err(|_| "自定义歌词池文件锁已损坏".to_string())?;
    let pool = load_pool()?;
    // 旧键没有专辑字段，迁移时仍核对记录中的专辑，避免跨版本覆盖。
    Ok(pool
        .get(&track_key(track))
        .or_else(|| {
            pool.values()
                .find(|record| same_track(&record.track, track))
        })
        .map(|record| {
            let mut document = record.document.clone();
            document.track = track.clone();
            document.source = LyricsSource::Manual;
            document
        }))
}

pub fn save(
    track: &TrackIdentity,
    selected_song: QqSongCandidate,
    mut document: LyricsDocument,
) -> Result<(), String> {
    let _guard = CUSTOM_POOL_LOCK
        .lock()
        .map_err(|_| "自定义歌词池文件锁已损坏".to_string())?;
    document.track = track.clone();
    document.source = LyricsSource::Manual;
    document.storage.timing = Some(
        if document.lines.iter().any(|line| !line.words.is_empty()) {
            crate::domain::LyricsTiming::Word
        } else {
            crate::domain::LyricsTiming::Line
        },
    );
    if document.storage.apple_music_id.is_none() {
        document.storage.apple_music_id =
            super::am_api::cached_song(track, document.duration_ms.unwrap_or(0))
                .map(|song| song.id);
    }
    let mut pool = load_pool()?;
    pool.insert(
        track_key(track),
        CustomLyricsRecord {
            track: track.clone(),
            selected_song: Some(selected_song),
            document,
        },
    );
    write_pool(&pool)
}

fn write_pool(pool: &CustomLyricsPool) -> Result<(), String> {
    let path = crate::app_paths::custom_lyrics_file()?;
    let parent = path
        .parent()
        .ok_or_else(|| "自定义歌词池目录无效".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("创建自定义歌词池目录失败: {error}"))?;
    let text = serde_json::to_string_pretty(&pool)
        .map_err(|error| format!("序列化自定义歌词池失败: {error}"))?;
    crate::settings_store::atomic_write(&path, text.as_bytes())
}

pub(crate) fn list() -> Result<Vec<(String, LyricsDocument)>, String> {
    let _guard = CUSTOM_POOL_LOCK.lock().map_err(|_| "手动匹配池锁已损坏")?;
    Ok(load_pool()?
        .into_iter()
        .map(|(id, record)| (id, record.document))
        .collect())
}

pub(crate) fn managed_save(
    id: Option<&str>,
    revision: Option<&str>,
    mut document: LyricsDocument,
) -> Result<String, String> {
    let _guard = CUSTOM_POOL_LOCK.lock().map_err(|_| "手动匹配池锁已损坏")?;
    let mut pool = load_pool()?;
    let original = if let Some(id) = id {
        let record = pool.get(id).ok_or("该条目已不存在，请刷新列表")?;
        super::pool_management::check_revision(&record.document, revision)?;
        Some(record.clone())
    } else {
        None
    };
    let key = track_key(&document.track);
    if pool.iter().any(|(other_id, record)| {
        Some(other_id.as_str()) != id && same_track(&record.track, &document.track)
    }) {
        return Err("手动匹配池已有这首歌曲，请选择已有条目编辑".into());
    }
    if let Some(id) = id {
        pool.remove(id);
    }
    document.source = LyricsSource::Manual;
    document.storage.timing = Some(
        if document.lines.iter().any(|line| !line.words.is_empty()) {
            crate::domain::LyricsTiming::Word
        } else {
            crate::domain::LyricsTiming::Line
        },
    );
    pool.insert(
        key.clone(),
        CustomLyricsRecord {
            track: document.track.clone(),
            selected_song: original.and_then(|record| record.selected_song),
            document,
        },
    );
    write_pool(&pool)?;
    Ok(key)
}

pub(crate) fn managed_delete(id: &str, revision: &str) -> Result<LyricsDocument, String> {
    let _guard = CUSTOM_POOL_LOCK.lock().map_err(|_| "手动匹配池锁已损坏")?;
    let mut pool = load_pool()?;
    let record = pool.get(id).ok_or("该条目已不存在，请刷新列表")?;
    super::pool_management::check_revision(&record.document, Some(revision))?;
    let document = pool.remove(id).unwrap().document;
    write_pool(&pool)?;
    Ok(document)
}

fn load_pool() -> Result<CustomLyricsPool, String> {
    let path = crate::app_paths::custom_lyrics_file()?;
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(format!("读取自定义歌词池失败: {error}")),
    };
    serde_json::from_str(&text).map_err(|error| format!("解析自定义歌词池失败: {error}"))
}

fn track_key(track: &TrackIdentity) -> String {
    let normalized = format!(
        "{}\0{}\0{}",
        normalize(&track.title),
        normalize(&track.artist),
        normalize(track.album.as_deref().unwrap_or(""))
    );
    let hash = normalized
        .bytes()
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        });
    format!("{hash:016x}")
}

pub(crate) fn same_track(first: &TrackIdentity, second: &TrackIdentity) -> bool {
    normalize(&first.title) == normalize(&second.title)
        && normalize(&first.artist) == normalize(&second.artist)
        && normalize(first.album.as_deref().unwrap_or(""))
            == normalize(second.album.as_deref().unwrap_or(""))
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::track_key;
    use crate::domain::TrackIdentity;

    #[test]
    fn key_ignores_title_and_artist_spacing_and_case() {
        let first = TrackIdentity {
            title: "Test Song".into(),
            artist: "Some Artist".into(),
            album: Some("Album A".into()),
        };
        let second = TrackIdentity {
            title: "testsong".into(),
            artist: "someartist".into(),
            album: Some("Album B".into()),
        };
        assert_ne!(track_key(&first), track_key(&second));
        let same_album = TrackIdentity {
            album: first.album.clone(),
            ..second
        };
        assert_eq!(track_key(&first), track_key(&same_album));
    }
}
