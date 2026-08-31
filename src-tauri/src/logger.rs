use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use tauri::AppHandle;

const CURRENT_LOG_NAME: &str = "amdl_current.log";
const PREVIOUS_LOG_NAME: &str = "amdl_previous.log";

struct LoggerState {
    directory: PathBuf,
    current_path: PathBuf,
    file: File,
}

static LOGGER: OnceLock<Mutex<LoggerState>> = OnceLock::new();

pub fn initialize(_app: &AppHandle) -> Result<(), String> {
    let directory = crate::app_paths::logs_dir()?;
    fs::create_dir_all(&directory).map_err(|error| format!("创建日志目录失败: {error}"))?;
    migrate_legacy_logs(&directory);

    let current_path = directory.join(CURRENT_LOG_NAME);
    let previous_path = directory.join(PREVIOUS_LOG_NAME);
    if previous_path.exists() {
        fs::remove_file(&previous_path).map_err(|error| format!("删除上一份日志失败: {error}"))?;
    }
    if current_path.exists() {
        fs::rename(&current_path, &previous_path)
            .map_err(|error| format!("轮转当前日志失败: {error}"))?;
    }

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&current_path)
        .map_err(|error| format!("创建当前日志失败: {error}"))?;
    LOGGER
        .set(Mutex::new(LoggerState {
            directory,
            current_path,
            file,
        }))
        .map_err(|_| "日志系统已初始化".to_string())?;
    Ok(())
}

fn migrate_legacy_logs(directory: &Path) {
    let Some(roaming) = std::env::var_os("APPDATA") else {
        return;
    };
    let legacy = PathBuf::from(roaming)
        .join("com.crh38.tauri-app")
        .join("logs");
    if legacy == directory || !legacy.is_dir() {
        return;
    }
    for name in [CURRENT_LOG_NAME, PREVIOUS_LOG_NAME] {
        let source = legacy.join(name);
        let destination = directory.join(name);
        if source.is_file() && !destination.exists() {
            let _ = fs::rename(&source, &destination).or_else(|_| {
                fs::copy(&source, &destination)?;
                fs::remove_file(&source)
            });
        }
    }
    let _ = fs::remove_dir(&legacy);
    if let Some(parent) = legacy.parent() {
        let _ = fs::remove_dir(parent);
    }
}

pub fn write_line(arguments: std::fmt::Arguments<'_>) {
    let Some(logger) = LOGGER.get() else {
        return;
    };
    let Ok(mut state) = logger.lock() else {
        ::std::eprintln!("[AMDL][日志] 日志文件锁已损坏");
        return;
    };
    if let Err(error) = writeln!(state.file, "{arguments}").and_then(|_| state.file.flush()) {
        ::std::eprintln!("[AMDL][日志] 写入日志失败: {error}");
    }
}

fn with_state<T>(operation: impl FnOnce(&LoggerState) -> io::Result<T>) -> Result<T, String> {
    let logger = LOGGER
        .get()
        .ok_or_else(|| "日志系统尚未初始化".to_string())?;
    let state = logger.lock().map_err(|_| "日志文件锁已损坏".to_string())?;
    operation(&state).map_err(|error| format!("日志操作失败: {error}"))
}

#[tauri::command]
pub fn read_current_log() -> Result<String, String> {
    with_state(|state| fs::read_to_string(&state.current_path))
}

#[tauri::command]
pub fn open_log_dir() -> Result<(), String> {
    let directory = with_state(|state| Ok(state.directory.clone()))?;
    open_directory(&directory)
}

#[cfg(target_os = "windows")]
fn open_directory(directory: &Path) -> Result<(), String> {
    Command::new("explorer.exe")
        .arg(directory)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("打开日志目录失败: {error}"))
}

#[cfg(target_os = "macos")]
fn open_directory(directory: &Path) -> Result<(), String> {
    Command::new("open")
        .arg(directory)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("打开日志目录失败: {error}"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_directory(directory: &Path) -> Result<(), String> {
    Command::new("xdg-open")
        .arg(directory)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("打开日志目录失败: {error}"))
}
