use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::domain::{LyricsDocument, TrackIdentity};
use crate::lyrics::qq_music::QqSongCandidate;

static CUSTOM_POOL_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CustomLyricsRecord {
    track: TrackIdentity,
    selected_song: QqSongCandidate,
    document: LyricsDocument,
}

type CustomLyricsPool = BTreeMap<String, CustomLyricsRecord>;

pub fn get(track: &TrackIdentity) -> Result<Option<LyricsDocument>, String> {
    let _guard = CUSTOM_POOL_LOCK
        .lock()
        .map_err(|_| "自定义歌词池文件锁已损坏".to_string())?;
    let pool = load_pool()?;
    Ok(pool.get(&track_key(track)).map(|record| {
        let mut document = record.document.clone();
        document.track = track.clone();
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
    let mut pool = load_pool()?;
    pool.insert(
        track_key(track),
        CustomLyricsRecord {
            track: track.clone(),
            selected_song,
            document,
        },
    );
    let path = crate::app_paths::custom_lyrics_file()?;
    let parent = path
        .parent()
        .ok_or_else(|| "自定义歌词池目录无效".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("创建自定义歌词池目录失败: {error}"))?;
    let text = serde_json::to_string_pretty(&pool)
        .map_err(|error| format!("序列化自定义歌词池失败: {error}"))?;
    std::fs::write(path, text).map_err(|error| format!("写入自定义歌词池失败: {error}"))
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
    let normalized = format!("{}\0{}", normalize(&track.title), normalize(&track.artist));
    let hash = normalized
        .bytes()
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        });
    format!("{hash:016x}")
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
        assert_eq!(track_key(&first), track_key(&second));
    }
}
