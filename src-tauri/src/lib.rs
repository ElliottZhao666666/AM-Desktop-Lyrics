macro_rules! println {
    ($($arg:tt)*) => {{
        ::std::println!($($arg)*);
        crate::logger::write_line(::std::format_args!($($arg)*));
    }};
}

macro_rules! eprintln {
    ($($arg:tt)*) => {{
        ::std::eprintln!($($arg)*);
        crate::logger::write_line(::std::format_args!($($arg)*));
    }};
}

pub mod app_paths;
pub mod config;
pub mod domain;
pub mod logger;
pub mod lyrics;
pub mod media;
pub mod playback;
pub mod settings_store;
mod window_geometry;
mod window_stack;

use base64::Engine;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::Duration;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, Wry};

static EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);
static SETTINGS_IO_LOCK: Mutex<()> = Mutex::new(());
static LYRICS_OFFSET_UPDATE_LOCK: Mutex<()> = Mutex::new(());
static LYRICS_TIME_OFFSET_MS: AtomicI64 = AtomicI64::new(0);
static LYRICS_TIME_OFFSET_REVISION: AtomicU64 = AtomicU64::new(0);
static LYRICS_OFFSET_INITIALIZED: Once = Once::new();

struct WindowLockState {
    locked: AtomicBool,
    menu_item: CheckMenuItem<Wry>,
    always_on_top: AtomicBool,
    topmost_menu_item: CheckMenuItem<Wry>,
    unlock_hovered: AtomicBool,
    bounds_save_generation: Arc<AtomicU64>,
    bounds_save_suspended: Arc<AtomicBool>,
}

const DEFAULT_THEME_NAME: &str = "AMDL 默认";
const DEFAULT_WINDOW_WIDTH: u32 = 800;
const DEFAULT_WINDOW_HEIGHT: u32 = 150;
const DEFAULT_THEME_CSS: &str = include_str!("../../src/default-theme.css");

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsWindowBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeLyricsTimeOffset {
    pub offset_ms: i64,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default = "default_true")]
    pub auto_convert_traditional_to_simplified: bool,
    #[serde(default = "default_cache_limit")]
    pub local_cache_limit: u8,
    #[serde(default = "default_theme_name")]
    pub theme_name: String,
    #[serde(default = "default_theme_css")]
    pub theme_css: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme_editor: Option<serde_json::Value>,
    #[serde(default)]
    pub lyrics_time_offset_ms: i64,
    #[serde(default)]
    pub remember_lyrics_window_bounds: bool,
    #[serde(default = "default_true")]
    pub lyrics_window_always_on_top: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lyrics_window_bounds: Option<LyricsWindowBounds>,
}

fn default_true() -> bool {
    true
}
fn default_cache_limit() -> u8 {
    10
}
fn default_theme_name() -> String {
    DEFAULT_THEME_NAME.to_string()
}
fn default_theme_css() -> String {
    DEFAULT_THEME_CSS.trim().to_string()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            auto_convert_traditional_to_simplified: true,
            local_cache_limit: 10,
            theme_name: default_theme_name(),
            theme_css: default_theme_css(),
            theme_editor: None,
            lyrics_time_offset_ms: 0,
            remember_lyrics_window_bounds: false,
            lyrics_window_always_on_top: true,
            lyrics_window_bounds: None,
        }
    }
}

fn read_settings_unlocked() -> Result<AppSettings, String> {
    serde_json::from_value(settings_store::read()?)
        .map_err(|error| format!("设置解析失败: {error}"))
}

fn normalize_settings(mut settings: AppSettings) -> AppSettings {
    settings.local_cache_limit = settings.local_cache_limit.clamp(1, 50);
    if settings.theme_name.trim().is_empty() {
        settings.theme_name = default_theme_name();
    }
    if settings.theme_css.trim().is_empty() {
        settings.theme_css = default_theme_css();
    }
    settings
}

fn write_settings_unlocked(settings: &AppSettings) -> Result<(), String> {
    let value =
        serde_json::to_value(settings).map_err(|error| format!("设置序列化失败: {error}"))?;
    settings_store::update(|root| {
        root.remove("themeEditor");
        root.extend(value.as_object().unwrap().clone());
        if let Some(config) = root
            .get_mut("config")
            .and_then(serde_json::Value::as_object_mut)
        {
            config.insert(
                "autoChs".into(),
                settings.auto_convert_traditional_to_simplified.into(),
            );
        }
    })
}

fn initialize_runtime_lyrics_offset() {
    LYRICS_OFFSET_INITIALIZED.call_once(|| {
        let settings = read_settings_unlocked().unwrap_or_else(|error| {
            eprintln!("[设置] 启动读取失败: {error}");
            AppSettings::default()
        });
        LYRICS_TIME_OFFSET_MS.store(settings.lyrics_time_offset_ms, Ordering::SeqCst);
        LYRICS_TIME_OFFSET_REVISION.store(1, Ordering::SeqCst);
    });
}

#[tauri::command]
fn get_settings() -> Result<AppSettings, String> {
    let _guard = SETTINGS_IO_LOCK
        .lock()
        .map_err(|_| "设置文件锁已损坏".to_string())?;
    read_settings_unlocked()
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: AppSettings) -> Result<(), String> {
    initialize_runtime_lyrics_offset();
    let runtime_offset_guard = LYRICS_OFFSET_UPDATE_LOCK
        .lock()
        .map_err(|_| "歌词偏移状态锁已损坏".to_string())?;
    let (settings, conversion_changed) = {
        let _guard = SETTINGS_IO_LOCK
            .lock()
            .map_err(|_| "设置文件锁已损坏".to_string())?;
        let previous = read_settings_unlocked()?;
        let latest_bounds = previous.lyrics_window_bounds;
        let mut settings = normalize_settings(settings);
        settings.lyrics_time_offset_ms = LYRICS_TIME_OFFSET_MS.load(Ordering::SeqCst);
        settings.lyrics_window_bounds = if settings.remember_lyrics_window_bounds {
            app.get_webview_window("main")
                .and_then(|window| current_window_bounds(&window))
                .or(latest_bounds)
        } else {
            latest_bounds
        };
        write_settings_unlocked(&settings)?;
        let conversion_changed = previous.auto_convert_traditional_to_simplified
            != settings.auto_convert_traditional_to_simplified;
        (settings, conversion_changed)
    };
    drop(runtime_offset_guard);
    apply_always_on_top(&app, settings.lyrics_window_always_on_top)?;
    lyrics::provider::apply_cache_settings(settings.local_cache_limit as usize, conversion_changed)?;
    let _ = app.emit("lyrics-pools-changed", ());
    if conversion_changed {
        lyrics::provider::refresh_current_lyrics(app.clone());
    }
    app.emit("settings-changed", &settings)
        .map_err(|error| format!("广播设置失败: {error}"))?;
    Ok(())
}

#[tauri::command]
fn get_config() -> Result<config::AppConfig, String> {
    let _guard = SETTINGS_IO_LOCK
        .lock()
        .map_err(|_| "设置文件锁已损坏".to_string())?;
    config::read_config().map(|config| config.normalize())
}

#[tauri::command]
fn save_config(app: AppHandle, config: config::AppConfig) -> Result<(), String> {
    let _guard = SETTINGS_IO_LOCK
        .lock()
        .map_err(|_| "设置文件锁已损坏".to_string())?;
    let previous = config::read_config()?;
    let config = config.normalize();
    config::write_config(&config)?;
    drop(_guard);
    lyrics::provider::apply_cache_settings(
        get_settings()?.local_cache_limit as usize,
        previous.auto_chs != config.auto_chs,
    )?;
    app.emit("settings-changed", get_settings()?)
        .map_err(|error| error.to_string())?;
    lyrics::provider::refresh_current_lyrics(app);
    Ok(())
}

#[tauri::command]
fn get_runtime_lyrics_time_offset() -> RuntimeLyricsTimeOffset {
    initialize_runtime_lyrics_offset();
    let _guard = LYRICS_OFFSET_UPDATE_LOCK.lock().ok();
    RuntimeLyricsTimeOffset {
        offset_ms: LYRICS_TIME_OFFSET_MS.load(Ordering::SeqCst),
        revision: LYRICS_TIME_OFFSET_REVISION.load(Ordering::SeqCst),
    }
}

fn update_runtime_lyrics_time_offset(
    app: &AppHandle,
    offset_ms: i64,
    expected_revision: Option<u64>,
) -> Result<RuntimeLyricsTimeOffset, String> {
    initialize_runtime_lyrics_offset();
    let _guard = LYRICS_OFFSET_UPDATE_LOCK
        .lock()
        .map_err(|_| "歌词偏移状态锁已损坏".to_string())?;
    let current_revision = LYRICS_TIME_OFFSET_REVISION.load(Ordering::SeqCst);
    if expected_revision.is_some_and(|revision| revision != current_revision) {
        return Ok(RuntimeLyricsTimeOffset {
            offset_ms: LYRICS_TIME_OFFSET_MS.load(Ordering::SeqCst),
            revision: current_revision,
        });
    }
    LYRICS_TIME_OFFSET_MS.store(offset_ms, Ordering::SeqCst);
    let payload = RuntimeLyricsTimeOffset {
        offset_ms,
        revision: LYRICS_TIME_OFFSET_REVISION.fetch_add(1, Ordering::SeqCst) + 1,
    };
    app.emit("lyrics-time-offset-changed", payload)
        .map_err(|error| format!("广播歌词偏移失败: {error}"))?;
    Ok(payload)
}

#[tauri::command]
fn set_runtime_lyrics_time_offset(
    app: AppHandle,
    offset_ms: i64,
    expected_revision: Option<u64>,
) -> Result<RuntimeLyricsTimeOffset, String> {
    update_runtime_lyrics_time_offset(&app, offset_ms, expected_revision)
}

#[tauri::command]
fn adjust_runtime_lyrics_time_offset(
    app: AppHandle,
    delta_ms: i64,
) -> Result<RuntimeLyricsTimeOffset, String> {
    initialize_runtime_lyrics_offset();
    let _guard = LYRICS_OFFSET_UPDATE_LOCK
        .lock()
        .map_err(|_| "歌词偏移状态锁已损坏".to_string())?;
    let offset_ms = LYRICS_TIME_OFFSET_MS
        .load(Ordering::SeqCst)
        .saturating_add(delta_ms);
    LYRICS_TIME_OFFSET_MS.store(offset_ms, Ordering::SeqCst);
    let payload = RuntimeLyricsTimeOffset {
        offset_ms,
        revision: LYRICS_TIME_OFFSET_REVISION.fetch_add(1, Ordering::SeqCst) + 1,
    };
    app.emit("lyrics-time-offset-changed", payload)
        .map_err(|error| format!("广播歌词偏移失败: {error}"))?;
    Ok(payload)
}

#[derive(Debug, Serialize, Deserialize)]
struct ThemeFile {
    name: String,
    css: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    editor: Option<serde_json::Value>,
}

fn validate_theme(theme: ThemeFile) -> Result<ThemeFile, String> {
    if theme.name.trim().is_empty() {
        return Err("主题名称不能为空".to_string());
    }
    if theme.css.trim().is_empty() || theme.css.len() > 128 * 1024 {
        return Err("主题 CSS 为空或超过 128 KiB".to_string());
    }
    if theme
        .editor
        .as_ref()
        .is_some_and(|editor| editor.to_string().len() > 32 * 1024)
    {
        return Err("主题编辑参数超过 32 KiB".into());
    }
    Ok(theme)
}

#[tauri::command]
fn import_theme_file(path: String) -> Result<ThemeFile, String> {
    if std::fs::metadata(&path)
        .map_err(|error| error.to_string())?
        .len()
        > 256 * 1024
    {
        return Err("主题文件超过 256 KiB".into());
    }
    let encoded =
        std::fs::read_to_string(path).map_err(|error| format!("读取主题失败: {error}"))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|error| format!("主题 Base64 解码失败: {error}"))?;
    let theme = serde_json::from_slice::<ThemeFile>(&bytes)
        .map_err(|error| format!("主题内容解析失败: {error}"))?;
    validate_theme(theme)
}

#[tauri::command]
fn export_theme_file(
    path: String,
    name: String,
    css: String,
    editor: Option<serde_json::Value>,
) -> Result<(), String> {
    let theme = validate_theme(ThemeFile { name, css, editor })?;
    let bytes = serde_json::to_vec(&theme).map_err(|error| format!("主题序列化失败: {error}"))?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    std::fs::write(path, encoded).map_err(|error| format!("写入主题失败: {error}"))
}

fn keep_settings_window_reusable(window: &tauri::WebviewWindow) {
    let reusable_window = window.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = reusable_window.emit("settings-closed", ());
            let _ = reusable_window.hide();
        }
    });
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn get_system_fonts() -> Result<Vec<String>, String> {
    use std::collections::BTreeSet;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;
    let mut fonts = BTreeSet::new();
    for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        let Ok(key) = RegKey::predef(root)
            .open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Fonts")
        else {
            continue;
        };
        for item in key.enum_values() {
            let Ok((name, _)) = item else { continue };
            let cleaned = name
                .trim()
                .trim_end_matches("(TrueType)")
                .trim_end_matches("(OpenType)")
                .trim();
            fonts.extend(
                cleaned
                    .split(" & ")
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned),
            );
        }
    }
    Ok(fonts.into_iter().collect())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
fn get_system_fonts() -> Result<Vec<String>, String> {
    Ok(Vec::new())
}

#[tauri::command]
fn open_settings(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("settings") {
        if !window.is_visible().unwrap_or(false) {
            let _ = window.emit("settings-opened", ());
        }
        window.show().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
        return Ok(());
    }
    let window = WebviewWindowBuilder::new(&app, "settings", WebviewUrl::App("index.html".into()))
        .title("AMDL 设置")
        .inner_size(1160.0, 680.0)
        .min_inner_size(1040.0, 580.0)
        .resizable(false)
        .maximizable(false)
        .minimizable(true)
        .closable(true)
        .decorations(true)
        .center()
        .build()
        .map_err(|error| error.to_string())?;
    keep_settings_window_reusable(&window);
    window.set_focus().map_err(|error| error.to_string())
}

fn position_unlock_window(app: &AppHandle) -> Result<(), String> {
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| "找不到歌词主窗口".to_string())?;
    let unlock = app
        .get_webview_window("unlock")
        .ok_or_else(|| "找不到解锁窗口".to_string())?;
    let main_position = main.outer_position().map_err(|error| error.to_string())?;
    let main_size = main.outer_size().map_err(|error| error.to_string())?;
    let scale = main.scale_factor().map_err(|error| error.to_string())?;
    let width = (44.0 * scale).round() as u32;
    unlock
        .set_size(tauri::PhysicalSize::new(
            width,
            (40.0 * scale).round() as u32,
        ))
        .map_err(|error| error.to_string())?;
    let x = main_position.x + (main_size.width as i32 - width as i32) / 2;
    unlock
        .set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
            x,
            main_position.y,
        )))
        .map_err(|error| format!("定位解锁窗口失败: {error}"))
}

fn apply_window_lock(
    app: &AppHandle,
    state: &WindowLockState,
    locked: bool,
) -> Result<bool, String> {
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| "找不到歌词主窗口".to_string())?;
    if locked {
        position_unlock_window(app)?;
        main.set_ignore_cursor_events(true)
            .map_err(|error| format!("启用鼠标穿透失败: {error}"))?;
        if let Err(error) = window_stack::sync(app) {
            let _ = main.set_ignore_cursor_events(false);
            return Err(format!("同步解锁窗口层级失败: {error}"));
        }
    } else {
        window_stack::hide_unlock(app)?;
        main.set_ignore_cursor_events(false)
            .map_err(|error| format!("关闭鼠标穿透失败: {error}"))?;
    }

    state.locked.store(locked, Ordering::SeqCst);
    state
        .menu_item
        .set_checked(locked)
        .map_err(|error| format!("同步托盘锁定状态失败: {error}"))?;
    app.emit("lock-state-changed", locked)
        .map_err(|error| format!("广播锁定状态失败: {error}"))?;
    println!("[AMDL][窗口] 锁定状态已更新: locked={locked}");
    Ok(locked)
}

#[tauri::command]
fn get_lock_state(state: State<'_, WindowLockState>) -> bool {
    state.locked.load(Ordering::SeqCst)
}

#[tauri::command]
fn get_unlock_hover_state(app: AppHandle) -> bool {
    app.try_state::<WindowLockState>()
        .is_some_and(|state| state.unlock_hovered.load(Ordering::SeqCst))
}

#[cfg(test)]
mod window_preferences_tests {
    use super::*;

    #[test]
    fn legacy_settings_default_to_topmost_and_explicit_false_survives_roundtrip() {
        let legacy: AppSettings = serde_json::from_value(serde_json::json!({
            "lyricsTimeOffsetMs": -1500,
            "rememberLyricsWindowBounds": true,
            "lyricsWindowBounds": {"x": -1200, "y": 400, "width": 800, "height": 150}
        }))
        .unwrap();
        assert!(legacy.lyrics_window_always_on_top);
        let mut settings = legacy;
        settings.lyrics_window_always_on_top = false;
        let restored: AppSettings =
            serde_json::from_value(serde_json::to_value(settings).unwrap()).unwrap();
        assert!(!restored.lyrics_window_always_on_top);
        assert!(restored.remember_lyrics_window_bounds);
        assert_eq!(restored.lyrics_window_bounds.unwrap().x, -1200);
        assert_eq!(restored.lyrics_time_offset_ms, -1500);
    }
}

fn apply_always_on_top(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let state = app.state::<WindowLockState>();
    let previous = state.always_on_top.swap(enabled, Ordering::SeqCst);
    for label in ["main", "unlock"] {
        app.get_webview_window(label)
            .ok_or("找不到歌词浮窗")?
            .set_always_on_top(enabled)
            .map_err(|error| error.to_string())?;
    }
    if let Err(error) = window_stack::sync(app) {
        state.always_on_top.store(previous, Ordering::SeqCst);
        let _ = window_stack::sync(app);
        return Err(error);
    }
    state
        .topmost_menu_item
        .set_checked(enabled)
        .map_err(|error| error.to_string())?;
    app.emit("lyrics-window-topmost-changed", enabled)
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn set_always_on_top(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let previous = app
        .state::<WindowLockState>()
        .always_on_top
        .load(Ordering::SeqCst);
    apply_always_on_top(app, enabled)?;
    let persisted = (|| {
        let _guard = SETTINGS_IO_LOCK.lock().map_err(|_| "设置文件锁已损坏")?;
        let mut settings = read_settings_unlocked()?;
        settings.lyrics_window_always_on_top = enabled;
        write_settings_unlocked(&settings)
    })();
    if let Err(error) = persisted {
        let _ = apply_always_on_top(app, previous);
        return Err(error);
    }
    println!("[AMDL][窗口] 保持歌词面板置顶: {enabled}");
    Ok(())
}

fn reset_lyrics_window_bounds(app: &AppHandle) -> Result<(), String> {
    let window = app.get_webview_window("main").ok_or("找不到歌词窗口")?;
    let state = app.state::<WindowLockState>();
    state.bounds_save_suspended.store(true, Ordering::SeqCst);
    let generation = state.bounds_save_generation.fetch_add(1, Ordering::SeqCst) + 1;
    let result: Result<(), String> = (|| {
        let bounds = default_window_bounds(&window);
        apply_window_bounds(&window, bounds)?;
        // 跨 DPI 移动可能触发一次自动缩放，再按目标屏幕的物理尺寸校正。
        apply_window_bounds(&window, bounds)?;
        position_unlock_window(app)?;
        let _guard = SETTINGS_IO_LOCK.lock().map_err(|_| "设置文件锁已损坏")?;
        let mut settings = read_settings_unlocked()?;
        settings.lyrics_window_bounds = Some(bounds);
        write_settings_unlocked(&settings)?;
        println!("[AMDL][窗口] 已重置到主显示器默认位置和大小: {bounds:?}");
        Ok(())
    })();
    resume_window_bounds_saving(
        state.bounds_save_suspended.clone(),
        state.bounds_save_generation.clone(),
        generation,
    );
    result?;
    show_main_window(app);
    Ok(())
}

#[tauri::command]
fn set_window_locked(
    app: AppHandle,
    state: State<'_, WindowLockState>,
    locked: bool,
) -> Result<bool, String> {
    apply_window_lock(&app, &state, locked)
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
    if app.try_state::<WindowLockState>().is_some() {
        let _ = position_unlock_window(app);
        let _ = window_stack::sync(app);
    }
}

fn monitor_screen(monitor: &tauri::Monitor) -> window_geometry::Screen {
    let area = monitor.work_area();
    window_geometry::Screen {
        bounds: LyricsWindowBounds {
            x: area.position.x,
            y: area.position.y,
            width: area.size.width,
            height: area.size.height,
        },
        scale: monitor.scale_factor(),
    }
}
fn default_window_bounds(window: &tauri::WebviewWindow) -> LyricsWindowBounds {
    window
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| window.available_monitors().ok()?.into_iter().next())
        .map(|monitor| window_geometry::default_bounds(monitor_screen(&monitor)))
        .unwrap_or(LyricsWindowBounds {
            x: 0,
            y: 0,
            width: DEFAULT_WINDOW_WIDTH,
            height: DEFAULT_WINDOW_HEIGHT,
        })
}
fn normalize_window_bounds(
    window: &tauri::WebviewWindow,
    bounds: LyricsWindowBounds,
) -> LyricsWindowBounds {
    let screens = window
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(monitor_screen)
        .collect::<Vec<_>>();
    window_geometry::restore(bounds, &screens).unwrap_or_else(|| default_window_bounds(window))
}
fn apply_window_bounds(
    window: &tauri::WebviewWindow,
    bounds: LyricsWindowBounds,
) -> Result<(), String> {
    window
        .set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
            bounds.x, bounds.y,
        )))
        .map_err(|error| error.to_string())?;
    window
        .set_size(tauri::Size::Physical(tauri::PhysicalSize::new(
            bounds.width,
            bounds.height,
        )))
        .map_err(|error| error.to_string())
}

fn current_window_bounds(window: &tauri::WebviewWindow) -> Option<LyricsWindowBounds> {
    let position = window.outer_position().ok()?;
    let size = window.outer_size().ok()?;
    Some(LyricsWindowBounds {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    })
}

fn start_window_bounds_writer(
    generation: Arc<AtomicU64>,
    suspended: Arc<AtomicBool>,
) -> std::sync::mpsc::Sender<(LyricsWindowBounds, u64)> {
    let (sender, receiver) = std::sync::mpsc::channel::<(LyricsWindowBounds, u64)>();
    std::thread::spawn(move || {
        while let Ok(mut pending) = receiver.recv() {
            loop {
                match receiver.recv_timeout(Duration::from_millis(300)) {
                    Ok(latest) => pending = latest,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
            if suspended.load(Ordering::SeqCst) || generation.load(Ordering::SeqCst) != pending.1 {
                continue;
            }
            let Ok(_guard) = SETTINGS_IO_LOCK.lock() else {
                continue;
            };
            if suspended.load(Ordering::SeqCst) || generation.load(Ordering::SeqCst) != pending.1 {
                continue;
            }
            let Ok(mut settings) = read_settings_unlocked() else {
                continue;
            };
            if !settings.remember_lyrics_window_bounds {
                continue;
            }
            settings.lyrics_window_bounds = Some(pending.0);
            if let Err(error) = write_settings_unlocked(&settings) {
                eprintln!("[窗口] 保存位置失败: {error}");
            }
        }
    });
    sender
}
fn schedule_window_bounds_save(
    bounds: LyricsWindowBounds,
    generation: &AtomicU64,
    sender: &std::sync::mpsc::Sender<(LyricsWindowBounds, u64)>,
) {
    let current_generation = generation.fetch_add(1, Ordering::SeqCst) + 1;
    let _ = sender.send((bounds, current_generation));
}
fn resume_window_bounds_saving(
    suspended: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    expected: u64,
) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        if generation.load(Ordering::SeqCst) == expected {
            suspended.store(false, Ordering::SeqCst);
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // WebView 会在 setup 完成前开始加载，首次 IPC 也必须看到持久化偏移。
    initialize_runtime_lyrics_offset();
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            show_main_window(app)
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            logger::initialize(app.handle())?;
            println!("[AMDL][启动] 日志系统已初始化");
            // 在应用启动时创建并启动 SMTC 管理器，
            // 它会在独立线程中监听系统媒体控制事件
            let handle = app.handle().clone();
            media::smtc::SmtcManager::new(handle).start();
            let initial_settings = get_settings().unwrap_or_default();
            let bounds_save_suspended = Arc::new(AtomicBool::new(true));
            let bounds_save_generation = Arc::new(AtomicU64::new(0));
            let bounds_sender = start_window_bounds_writer(
                bounds_save_generation.clone(),
                bounds_save_suspended.clone(),
            );

            let lock_item =
                CheckMenuItem::with_id(app, "lock", "锁定窗口", true, false, None::<&str>)?;
            let topmost_item = CheckMenuItem::with_id(
                app,
                "always-on-top",
                "保持歌词面板置顶",
                true,
                initial_settings.lyrics_window_always_on_top,
                None::<&str>,
            )?;
            app.manage(WindowLockState {
                locked: AtomicBool::new(false),
                menu_item: lock_item.clone(),
                always_on_top: AtomicBool::new(initial_settings.lyrics_window_always_on_top),
                topmost_menu_item: topmost_item.clone(),
                unlock_hovered: AtomicBool::new(false),
                bounds_save_generation: bounds_save_generation.clone(),
                bounds_save_suspended: bounds_save_suspended.clone(),
            });
            let show_main_interface_item =
                MenuItem::with_id(app, "show-main-interface", "显示主界面", true, None::<&str>)?;
            let settings_item = MenuItem::with_id(app, "settings", "设置", true, None::<&str>)?;
            let reset_position_item = MenuItem::with_id(
                app,
                "reset-window-bounds",
                "重置歌词面板位置",
                true,
                None::<&str>,
            )?;
            let quit_item = MenuItem::with_id(app, "quit", "退出 AMDL", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[
                    &lock_item,
                    &topmost_item,
                    &reset_position_item,
                    &show_main_interface_item,
                    &settings_item,
                    &PredefinedMenuItem::separator(app)?,
                    &quit_item,
                ],
            )?;
            let tray_app = app.handle().clone();
            let mut tray_builder = TrayIconBuilder::with_id("main-tray")
                .menu(&menu)
                .tooltip("AMDL");
            if let Some(icon) = app.default_window_icon() {
                tray_builder = tray_builder.icon(icon.clone());
            }
            tray_builder
                .on_tray_icon_event(move |_tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(&tray_app);
                    }
                })
                .build(app)?;

            let menu_app = app.handle().clone();
            let menu_lock_item = lock_item.clone();
            app.on_menu_event(move |_app, event| match event.id().as_ref() {
                "lock" => {
                    let checked = menu_lock_item.is_checked().unwrap_or(false);
                    let state = menu_app.state::<WindowLockState>();
                    if let Err(error) = apply_window_lock(&menu_app, &state, checked) {
                        let previous = state.locked.load(Ordering::SeqCst);
                        let _ = menu_lock_item.set_checked(previous);
                        eprintln!("[AMDL][托盘] 切换窗口锁定失败: {error}");
                    }
                }
                "show-main-interface" => {
                    println!("[AMDL][托盘] 显示主界面");
                    show_main_window(&menu_app);
                }
                "always-on-top" => {
                    let state = menu_app.state::<WindowLockState>();
                    let checked = state.topmost_menu_item.is_checked().unwrap_or(true);
                    if let Err(error) = set_always_on_top(&menu_app, checked) {
                        let _ = state
                            .topmost_menu_item
                            .set_checked(state.always_on_top.load(Ordering::SeqCst));
                        eprintln!("[AMDL][托盘] 切换置顶失败: {error}");
                    }
                }
                "reset-window-bounds" => {
                    if let Err(error) = reset_lyrics_window_bounds(&menu_app) {
                        eprintln!("[AMDL][托盘] 重置歌词面板位置失败: {error}");
                    }
                }
                "settings" => {
                    println!("[AMDL][托盘] 打开设置");
                    if let Err(error) = open_settings(menu_app.clone()) {
                        eprintln!("[AMDL][托盘] 打开设置失败: {error}");
                    }
                }
                "quit" => {
                    println!("[AMDL][托盘] 退出 AMDL");
                    EXIT_REQUESTED.store(true, Ordering::SeqCst);
                    menu_app.exit(0);
                }
                _ => {}
            });

            if let Some(window) = app.get_webview_window("main") {
                let initial_bounds = if initial_settings.remember_lyrics_window_bounds {
                    initial_settings.lyrics_window_bounds
                } else {
                    None
                }
                .unwrap_or_else(|| default_window_bounds(&window));
                apply_window_bounds(&window, normalize_window_bounds(&window, initial_bounds))?;

                let bounded_window = window.clone();
                let event_save_suspended = bounds_save_suspended.clone();
                let event_save_generation = bounds_save_generation.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        if !EXIT_REQUESTED.load(Ordering::SeqCst) {
                            api.prevent_close();
                            let _ = bounded_window.hide();
                            let _ = window_stack::hide_unlock(bounded_window.app_handle());
                        }
                        return;
                    }
                    if !matches!(
                        event,
                        tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_)
                    ) {
                        return;
                    }
                    let Ok(monitors) = bounded_window.available_monitors() else {
                        return;
                    };
                    if monitors.is_empty() {
                        return;
                    }
                    let Ok(position) = bounded_window.outer_position() else {
                        return;
                    };
                    let Ok(size) = bounded_window.outer_size() else {
                        return;
                    };
                    let screens = monitors.iter().map(monitor_screen).collect::<Vec<_>>();
                    let recovered = window_geometry::recover_if_hidden(
                        LyricsWindowBounds {
                            x: position.x,
                            y: position.y,
                            width: size.width,
                            height: size.height,
                        },
                        &screens,
                    );
                    let x = recovered.x;
                    let y = recovered.y;
                    if x != position.x || y != position.y {
                        let _ = apply_window_bounds(&bounded_window, recovered);
                        return;
                    }
                    let _ = position_unlock_window(bounded_window.app_handle());
                    if !event_save_suspended.load(Ordering::SeqCst) {
                        schedule_window_bounds_save(
                            LyricsWindowBounds {
                                x,
                                y,
                                width: size.width,
                                height: size.height,
                            },
                            &event_save_generation,
                            &bounds_sender,
                        );
                    }
                });
                resume_window_bounds_saving(
                    bounds_save_suspended.clone(),
                    bounds_save_generation.clone(),
                    0,
                );
            }
            if let Some(window) = app.get_webview_window("settings") {
                keep_settings_window_reusable(&window);
            }
            if let Some(window) = app.get_webview_window("unlock") {
                let _ = window.hide();
            }
            window_stack::initialize(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            media::smtc::get_media_snapshot,
            media::smtc::play_pause,
            media::smtc::next_track,
            media::smtc::prev_track,
            get_settings,
            save_settings,
            get_config,
            save_config,
            get_runtime_lyrics_time_offset,
            set_runtime_lyrics_time_offset,
            adjust_runtime_lyrics_time_offset,
            import_theme_file,
            export_theme_file,
            open_settings,
            get_system_fonts,
            get_lock_state,
            get_unlock_hover_state,
            set_window_locked,
            logger::read_current_log,
            logger::open_log_dir,
            lyrics::provider::search_qq_lyrics,
            lyrics::provider::preview_qq_lyrics,
            lyrics::provider::refetch_lyrics,
            lyrics::provider::apply_manual_lyrics,
            lyrics::provider::apply_custom_lyrics,
            lyrics::pool_management::list_lyrics_pool,
            lyrics::pool_management::get_lyrics_pool_entry,
            lyrics::pool_management::validate_pool_lyrics,
            lyrics::pool_management::save_lyrics_pool_entry,
            lyrics::pool_management::delete_lyrics_pool_entry,
            lyrics::pool_management::promote_lyrics_pool_entry,
            lyrics::pool_management::resolve_pool_apple_music_id,
            lyrics::pool_management::open_lyrics_pool_editor,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
