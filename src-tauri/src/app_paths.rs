use std::path::PathBuf;

const APP_DATA_DIRECTORY: &str = "AM-Desktop-Lyrics";

pub fn data_dir() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|path| path.join(APP_DATA_DIRECTORY))
        .ok_or_else(|| "LOCALAPPDATA 不可用".to_string())
}

pub fn ensure_data_dir() -> Result<PathBuf, String> {
    let directory = data_dir()?;
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("创建应用数据目录失败: {error}"))?;
    Ok(directory)
}

pub fn settings_file() -> Result<PathBuf, String> {
    Ok(data_dir()?.join("settings.json"))
}

pub fn lyrics_cache_file() -> Result<PathBuf, String> {
    Ok(data_dir()?.join("lyrics-cache.json"))
}

pub fn custom_lyrics_file() -> Result<PathBuf, String> {
    Ok(data_dir()?.join("custom_lyrics.json"))
}

pub fn lyrics_dir() -> Result<PathBuf, String> {
    Ok(data_dir()?.join("lyrics"))
}

pub fn logs_dir() -> Result<PathBuf, String> {
    Ok(data_dir()?.join("logs"))
}
