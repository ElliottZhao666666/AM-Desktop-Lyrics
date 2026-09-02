use serde::{Deserialize, Serialize};

/// 逐字歌词词元，时间单位统一为毫秒。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LyricWord {
    pub text: String,
    pub begin: f64,
    pub end: f64,
}

/// 面向 Apple Music Web API 的逐行歌词模型。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LyricLine {
    pub begin: f64,
    pub end: Option<f64>,
    pub text: String,
    pub words: Option<Vec<LyricWord>>,
}