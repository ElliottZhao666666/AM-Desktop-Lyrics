use serde::{Deserialize, Serialize};

/// 当前播放曲目的稳定身份，用于媒体状态、歌词匹配和缓存键。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrackIdentity {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
}

/// 歌词来源，后续由设置决定优先级和回退策略。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LyricsSource {
    AppleMusicCache,
    QqMusic,
    AmllDb,
    Dummy,
}

/// 一个参与时间轴同步的词或音节。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LyricWord {
    pub text: String,
    pub begin_ms: u64,
    pub end_ms: u64,
}

/// 一行歌词，words 为空时表示只有逐行时间轴。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LyricLine {
    pub begin_ms: u64,
    pub end_ms: Option<u64>,
    pub words: Vec<LyricWord>,
    pub text: String,
}

/// 统一的歌词文档格式。内存 TTML 和 AMLL-DB 最终都转换为此模型。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LyricsDocument {
    pub track: TrackIdentity,
    pub source: LyricsSource,
    pub lines: Vec<LyricLine>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LyricsLoadState {
    Loading,
    Loaded,
    NotFound,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LyricsStatusPayload {
    pub track: TrackIdentity,
    pub state: LyricsLoadState,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MediaStatePayload {
    pub track: TrackIdentity,
    pub playback_state: PlaybackState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PlaybackState {
    Playing,
    Paused,
    Stopped,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MediaPositionPayload {
    pub position_ms: u64,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MediaSnapshotPayload {
    pub media: MediaStatePayload,
    pub position: MediaPositionPayload,
}
