use serde::{Deserialize, Serialize};
use base64::Engine;

const DEFAULT_APPLE_MUSIC_AUTH_TOKEN: &str = "Bearer eyJ0eXAiOiJKV1QiLCJhbGciOiJFUzI1NiIsImtpZCI6IldlYlBsYXlLaWQifQ.eyJpc3MiOiJBTVBXZWJQbGF5IiwiaWF0IjoxNzg2NjMyOTI0LCJleHAiOjE3OTI2ODA5MjQsInJvb3RfaHR0cHNfb3JpZ2luIjpbImFwcGxlLmNvbSJdfQ.hBgj61sZf-y7bmuvT-joXAUAcf7TVJ51732xnH5vFkLHOmsQHxVqGMYUuI4h8c0-RX3fRY3moylhLW8fewFJyw";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsSource {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub priority: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AmApiTokens {
    #[serde(default = "default_auth_token")]
    pub auth_token: String,
    #[serde(default)]
    pub media_user_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    #[serde(default = "default_lyrics_sources")]
    pub lyrics_sources: Vec<LyricsSource>,
    #[serde(default)]
    pub am_api_tokens: AmApiTokens,
    #[serde(default = "default_auto_chs")]
    pub auto_chs: bool,
}

fn default_auto_chs() -> bool {
    true
}

fn default_auth_token() -> String {
    DEFAULT_APPLE_MUSIC_AUTH_TOKEN.to_string()
}

fn default_lyrics_sources() -> Vec<LyricsSource> {
    vec![
        LyricsSource { id: "manual".into(), name: "手动匹配结果".into(), enabled: true, priority: 1 },
        LyricsSource { id: "appleMusicApi".into(), name: "Apple Music 逐字歌词".into(), enabled: true, priority: 2 },
        LyricsSource { id: "appleMusicCache".into(), name: "Apple Music Windows 缓存歌词".into(), enabled: true, priority: 3 },
        LyricsSource { id: "qqMusic".into(), name: "QQ 音乐".into(), enabled: true, priority: 4 },
        LyricsSource { id: "neteaseMusic".into(), name: "网易云音乐".into(), enabled: false, priority: 5 },
    ]
}

impl Default for AppConfig {
    fn default() -> Self {
        Self { lyrics_sources: default_lyrics_sources(), am_api_tokens: AmApiTokens::default(), auto_chs: true }
    }
}

impl AppConfig {
    pub fn normalize(mut self) -> Self {
        let defaults = default_lyrics_sources();
        for default_source in defaults {
            if !self.lyrics_sources.iter().any(|source| source.id == default_source.id) {
                self.lyrics_sources.push(default_source);
            }
        }
        self.lyrics_sources.sort_by_key(|source| source.priority);
        if !self.lyrics_sources.iter().any(|source| source.enabled) {
            if let Some(source) = self.lyrics_sources.iter_mut().find(|source| source.id == "manual") {
                source.enabled = true;
            } else if let Some(source) = self.lyrics_sources.first_mut() {
                source.enabled = true;
            }
        }
        self
    }

    pub fn source_enabled(&self, id: &str) -> bool {
        self.lyrics_sources.iter().find(|source| source.id == id).map(|source| source.enabled).unwrap_or(false)
    }
}

pub fn read_config() -> Result<AppConfig, String> {
    let path = crate::app_paths::settings_file()?;
    let encoded = match std::fs::read_to_string(path) {
        Ok(value) => value,
        Err(_) => return Ok(AppConfig::default()),
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|error| format!("配置 Base64 解码失败: {error}"))?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("配置解析失败: {error}"))?;
    match value.get("config").cloned() {
        Some(value) => serde_json::from_value(value)
            .map(|config: AppConfig| config.normalize())
            .map_err(|error| format!("配置结构解析失败: {error}")),
        None => Ok(AppConfig::default()),
    }
}

pub fn write_config(config: &AppConfig) -> Result<(), String> {
    let path = crate::app_paths::settings_file()?;
    let mut value = if let Ok(encoded) = std::fs::read_to_string(&path) {
        base64::engine::general_purpose::STANDARD
            .decode(encoded.trim())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .unwrap_or_else(|| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };
    value["config"] = serde_json::to_value(config).map_err(|error| format!("配置序列化失败: {error}"))?;
    let bytes = serde_json::to_vec(&value).map_err(|error| format!("设置序列化失败: {error}"))?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    let parent = path.parent().ok_or_else(|| "设置目录无效".to_string())?;
    std::fs::create_dir_all(parent).map_err(|error| format!("创建设置目录失败: {error}"))?;
    std::fs::write(path, encoded).map_err(|error| format!("写入配置失败: {error}"))
}
