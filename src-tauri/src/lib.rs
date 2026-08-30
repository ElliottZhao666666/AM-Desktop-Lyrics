pub mod domain;
pub mod lyrics;
pub mod media;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // 在应用启动时创建并启动 SMTC 管理器，
            // 它会在独立线程中监听系统媒体控制事件
            let handle = app.handle().clone();
            media::smtc::SmtcManager::new(handle).start();

            // 窗口定位逻辑：将窗口附着在主显示器工作区底部居中
            use tauri::Manager;
            if let Some(window) = app.get_webview_window("main") {
                if let Ok(Some(monitor)) = window.current_monitor() {
                    let work_area = monitor.work_area();
                    let window_size = window
                        .outer_size()
                        .unwrap_or_else(|_| tauri::PhysicalSize::new(800, 150));

                    let x = work_area.position.x
                        + (work_area.size.width as i32 - window_size.width as i32) / 2;
                    let y = work_area.position.y + work_area.size.height as i32
                        - window_size.height as i32;

                    let _ = window.set_position(tauri::Position::Physical(
                        tauri::PhysicalPosition::new(x, y),
                    ));
                }

                let bounded_window = window.clone();
                window.on_window_event(move |event| {
                    if !matches!(event, tauri::WindowEvent::Moved(_)) {
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
                    let left = monitors
                        .iter()
                        .map(|monitor| monitor.position().x)
                        .min()
                        .unwrap();
                    let top = monitors
                        .iter()
                        .map(|monitor| monitor.position().y)
                        .min()
                        .unwrap();
                    let right = monitors
                        .iter()
                        .map(|monitor| monitor.position().x + monitor.size().width as i32)
                        .max()
                        .unwrap();
                    let bottom = monitors
                        .iter()
                        .map(|monitor| monitor.position().y + monitor.size().height as i32)
                        .max()
                        .unwrap();
                    let max_x = (right - size.width as i32).max(left);
                    let max_y = (bottom - size.height as i32).max(top);
                    let x = position.x.clamp(left, max_x);
                    let y = position.y.clamp(top, max_y);
                    if x != position.x || y != position.y {
                        let _ = bounded_window.set_position(tauri::Position::Physical(
                            tauri::PhysicalPosition::new(x, y),
                        ));
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            media::smtc::get_media_snapshot,
            media::smtc::play_pause,
            media::smtc::next_track,
            media::smtc::prev_track,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
