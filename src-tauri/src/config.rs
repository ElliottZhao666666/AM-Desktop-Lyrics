use serde::{Deserialize, Serialize};

const DEFAULT_APPLE_MUSIC_AUTH_TOKEN: &str = "Bearer eyJ0eXAiOiJKV1QiLCJhbGciOiJFUzI1NiIsImtpZCI6IldlYlBsYXlLaWQifQ.eyJpc3MiOiJBTVBXZWJQbGF5IiwiaWF0IjoxNzg2NjMyOTI0LCJleHAiOjE3OTI2ODA5MjQsInJvb3RfaHR0cHNfb3JpZ2luIjpbImFwcGxlLmNvbSJdfQ.hBgj61sZf-y7bmuvT-joXAUAcf7TVJ51732xnH5vFkLHOmsQHxVqGMYUuI4h8c0-RX3fRY3moylhLW8fewFJyw";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsSource {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub priority: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AmApiTokens {
    #[serde(default = "default_auth_token")]
    pub auth_token: String,
    #[serde(default)]
    pub media_user_token: String,
}

impl Default for AmApiTokens {
    fn default() -> Self {
        Self {
            auth_token: default_auth_token(),
            media_user_token: String::new(),
        }
    }
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
        LyricsSource {
            id: "manual".into(),
            name: "手动匹配池".into(),
            enabled: true,
            priority: 1,
        },
        LyricsSource {
            id: "appleMusicApi".into(),
            name: "Apple Music API".into(),
            enabled: true,
            priority: 2,
        },
        LyricsSource {
            id: "appleMusicCache".into(),
            name: "Apple Music 本地缓存".into(),
            enabled: true,
            priority: 3,
        },
        LyricsSource {
            id: "qqMusic".into(),
            name: "QQ 音乐".into(),
            enabled: true,
            priority: 4,
        },
        LyricsSource {
            id: "neteaseMusic".into(),
            name: "网易云音乐".into(),
            enabled: false,
            priority: 5,
        },
    ]
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            lyrics_sources: default_lyrics_sources(),
            am_api_tokens: AmApiTokens::default(),
            auto_chs: true,
        }
    }
}

impl AppConfig {
    pub fn normalize(mut self) -> Self {
        let defaults = default_lyrics_sources();
        for default_source in defaults {
            if let Some(source) = self
                .lyrics_sources
                .iter_mut()
                .find(|source| source.id == default_source.id)
            {
                source.name = default_source.name;
            } else {
                self.lyrics_sources.push(default_source);
            }
        }
        self.lyrics_sources.sort_by_key(|source| source.priority);
        self
    }

    pub fn source_enabled(&self, id: &str) -> bool {
        self.lyrics_sources
            .iter()
            .find(|source| source.id == id)
            .map(|source| source.enabled)
            .unwrap_or(false)
    }
}

pub fn read_config() -> Result<AppConfig, String> {
    let value = crate::settings_store::read()?;
    let mut config = match value.get("config").cloned() {
        Some(value) => serde_json::from_value(value)
            .map(|config: AppConfig| config.normalize())
            .map_err(|error| format!("配置结构解析失败: {error}")),
        None => Ok(AppConfig::default()),
    }?;
    if let Some(enabled) = value
        .get("autoConvertTraditionalToSimplified")
        .and_then(serde_json::Value::as_bool)
    {
        config.auto_chs = enabled;
    }
    Ok(config)
}

pub fn write_config(config: &AppConfig) -> Result<(), String> {
    let value = serde_json::to_value(config).map_err(|error| format!("配置序列化失败: {error}"))?;
    crate::settings_store::update(|root| {
        root.insert("config".into(), value);
        root.insert(
            "autoConvertTraditionalToSimplified".into(),
            config.auto_chs.into(),
        );
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalization_respects_disabled_sources() {
        let mut config = AppConfig::default();
        for source in &mut config.lyrics_sources {
            source.enabled = false;
        }
        assert!(config
            .normalize()
            .lyrics_sources
            .iter()
            .all(|source| !source.enabled));
    }
    #[test]
    fn normalization_updates_legacy_names_without_changing_source_preferences() {
        let mut config = AppConfig::default();
        config.lyrics_sources[0].name = "手动匹配结果".into();
        config.lyrics_sources[0].enabled = false;
        config.lyrics_sources[0].priority = 7;
        let config = config.normalize();
        let manual = config
            .lyrics_sources
            .iter()
            .find(|source| source.id == "manual")
            .unwrap();
        assert_eq!(manual.name, "手动匹配池");
        assert!(!manual.enabled);
        assert_eq!(manual.priority, 7);
    }
    #[test]
    fn token_defaults_are_identical_for_new_and_legacy_configuration() {
        let deserialized: AmApiTokens = serde_json::from_str("{}").unwrap();
        assert_eq!(AmApiTokens::default().auth_token, deserialized.auth_token);
        assert_eq!(
            AppConfig::default().am_api_tokens.auth_token,
            deserialized.auth_token
        );
    }
}
