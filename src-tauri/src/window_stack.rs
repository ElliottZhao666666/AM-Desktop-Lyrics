//! 解锁浮窗与歌词窗口共用置顶层级；透明区域的悬停由原生鼠标坐标检测。
use crate::{LyricsWindowBounds, WindowLockState, EXIT_REQUESTED};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

pub fn in_unlock_region(bounds: LyricsWindowBounds, scale: f64, x: f64, y: f64) -> bool {
    x >= f64::from(bounds.x)
        && x < f64::from(bounds.x) + f64::from(bounds.width)
        && y >= f64::from(bounds.y)
        && y < f64::from(bounds.y) + (48.0 * scale).min(f64::from(bounds.height))
}

#[cfg(windows)]
mod native {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::UI::WindowsAndMessaging::*;

    pub fn attach(main: HWND, unlock: HWND) -> Result<(), String> {
        // GWLP_HWNDPARENT 对顶层窗口设置 owner，不会把 WebView 变为子窗口。
        unsafe {
            SetWindowLongPtrW(unlock, GWLP_HWNDPARENT, main.0 as isize);
            if GetWindow(unlock, GW_OWNER).ok() != Some(main) {
                return Err("建立解锁浮窗所属关系失败".into());
            }
        }
        Ok(())
    }

    pub fn sync(main: HWND, unlock: HWND, topmost: bool) -> Result<(), String> {
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
        unsafe {
            // 同一批次升降两个窗口，避免全屏切换时只有其中一个留在置顶组。
            let band = if topmost {
                HWND_TOPMOST
            } else {
                HWND_NOTOPMOST
            };
            let is_topmost =
                |window| GetWindowLongPtrW(window, GWL_EXSTYLE) & WS_EX_TOPMOST.0 as isize != 0;
            if topmost || is_topmost(main) || is_topmost(unlock) {
                let batch = BeginDeferWindowPos(2).map_err(|error| error.to_string())?;
                let batch = DeferWindowPos(batch, main, Some(band), 0, 0, 0, 0, flags)
                    .map_err(|error| error.to_string())?;
                let batch = DeferWindowPos(batch, unlock, Some(band), 0, 0, 0, 0, flags)
                    .map_err(|error| error.to_string())?;
                EndDeferWindowPos(batch).map_err(|error| error.to_string())?;
            }
            if topmost {
                SetWindowPos(main, Some(unlock), 0, 0, 0, 0, flags | SWP_NOOWNERZORDER)
                    .map_err(|error| error.to_string())?;
            } else {
                // 关闭置顶时只将按钮插在歌词前面，不能把整组窗口抬到其它应用前面。
                let mut preceding = GetWindow(main, GW_HWNDPREV).ok();
                if preceding == Some(unlock) {
                    return Ok(());
                }
                if preceding.is_some_and(|window| {
                    GetWindowLongPtrW(window, GWL_EXSTYLE) & WS_EX_TOPMOST.0 as isize != 0
                }) {
                    preceding = None;
                }
                SetWindowPos(
                    unlock,
                    Some(preceding.unwrap_or(HWND_TOP)),
                    0,
                    0,
                    0,
                    0,
                    flags | SWP_NOOWNERZORDER,
                )
                .map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }

    pub fn visible(main: HWND) -> bool {
        unsafe { IsWindowVisible(main).as_bool() && !IsIconic(main).as_bool() }
    }

    pub fn needs_raise(main: HWND, unlock: HWND) -> bool {
        unsafe {
            if GetWindowLongPtrW(main, GWL_EXSTYLE) & WS_EX_TOPMOST.0 as isize == 0
                || GetWindowLongPtrW(unlock, GWL_EXSTYLE) & WS_EX_TOPMOST.0 as isize == 0
            {
                return true;
            }
            let mut panel = RECT::default();
            if GetWindowRect(main, &mut panel).is_err() {
                return false;
            }
            let mut above = GetWindow(main, GW_HWNDPREV).ok();
            // 窗口会在枚举时销毁或改变顺序，限制遍历次数，防止原生窗口链异常。
            for _ in 0..512 {
                let Some(window) = above else {
                    break;
                };
                let mut other = RECT::default();
                // 允许系统菜单、提示气泡临时显示，避免置顶维护挡住托盘菜单。
                let mut class_name = [0u16; 64];
                let len = GetClassNameW(window, &mut class_name);
                let class_name = String::from_utf16_lossy(&class_name[..len.max(0) as usize]);
                if window != unlock
                    && class_name != "#32768"
                    && class_name != "tooltips_class32"
                    && visible(window)
                    && GetWindowRect(window, &mut other).is_ok()
                    && other.left < panel.right
                    && other.right > panel.left
                    && other.top < panel.bottom
                    && other.bottom > panel.top
                {
                    return true;
                }
                above = GetWindow(window, GW_HWNDPREV).ok();
            }
            false
        }
    }

    pub fn adjacent(main: HWND, unlock: HWND) -> bool {
        unsafe { GetWindow(main, GW_HWNDPREV).ok() == Some(unlock) }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use windows::core::w;

        #[test]
        fn owned_windows_share_topmost_band_and_remain_adjacent_without_focus() {
            // 使用测试自建的隐藏窗口验证真实 Windows 层级，不触碰正在运行的应用。
            unsafe {
                struct TestWindow(HWND);
                impl Drop for TestWindow {
                    fn drop(&mut self) {
                        unsafe {
                            let _ = DestroyWindow(self.0);
                        }
                    }
                }
                let create = || {
                    TestWindow(
                        CreateWindowExW(
                            WS_EX_TOOLWINDOW,
                            w!("STATIC"),
                            w!("AMDL window stack regression"),
                            WS_POPUP,
                            -30000,
                            -30000,
                            10,
                            10,
                            None,
                            None,
                            None,
                            None,
                        )
                        .unwrap(),
                    )
                };
                let main = create();
                let unlock = create();
                let other = create();
                let foreground = GetForegroundWindow();
                attach(main.0, unlock.0).unwrap();
                for topmost in [true, false, true, false] {
                    sync(main.0, unlock.0, topmost).unwrap();
                    assert_eq!(GetWindow(unlock.0, GW_OWNER).unwrap(), main.0);
                    assert_eq!(
                        GetWindow(main.0, GW_HWNDPREV).unwrap(),
                        unlock.0,
                        "topmost={topmost}"
                    );
                    for window in [main.0, unlock.0] {
                        assert_eq!(
                            GetWindowLongPtrW(window, GWL_EXSTYLE) & WS_EX_TOPMOST.0 as isize != 0,
                            topmost
                        );
                    }
                    assert_eq!(GetForegroundWindow(), foreground);
                }
                // 普通模式插入其它应用后修复按钮顺序，不能把歌词抬到其它应用前。
                SetWindowPos(
                    other.0,
                    Some(HWND_TOP),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                )
                .unwrap();
                sync(main.0, unlock.0, false).unwrap();
                assert_eq!(GetWindow(main.0, GW_HWNDPREV).unwrap(), unlock.0);
                let mut above = GetWindow(unlock.0, GW_HWNDPREV).ok();
                let mut found_other = false;
                for _ in 0..512 {
                    let Some(window) = above else {
                        break;
                    };
                    if window == other.0 {
                        found_other = true;
                        break;
                    }
                    above = GetWindow(window, GW_HWNDPREV).ok();
                }
                assert!(found_other, "关闭置顶时应保留其它应用在歌词前面的相对顺序");
                // 模拟覆盖歌词区域的其它置顶窗口；整组重排后两个窗口一起恢复。
                for window in [main.0, unlock.0, other.0] {
                    let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
                }
                sync(main.0, unlock.0, true).unwrap();
                SetWindowPos(
                    other.0,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                )
                .unwrap();
                assert!(needs_raise(main.0, unlock.0));
                sync(main.0, unlock.0, true).unwrap();
                assert!(!needs_raise(main.0, unlock.0));
                assert!(adjacent(main.0, unlock.0));
                assert_eq!(GetForegroundWindow(), foreground);
            }
        }
    }
}

pub fn initialize(app: &AppHandle) -> Result<(), String> {
    let topmost = app
        .state::<WindowLockState>()
        .always_on_top
        .load(Ordering::SeqCst);
    for label in ["main", "unlock"] {
        app.get_webview_window(label)
            .ok_or("找不到歌词浮窗")?
            .set_always_on_top(topmost)
            .map_err(|error| error.to_string())?;
    }
    hide_unlock(app)?;
    #[cfg(windows)]
    {
        let main = app.get_webview_window("main").ok_or("找不到歌词窗口")?;
        let unlock = app.get_webview_window("unlock").ok_or("找不到解锁窗口")?;
        native::attach(
            main.hwnd().map_err(|e| e.to_string())?,
            unlock.hwnd().map_err(|e| e.to_string())?,
        )?;
    }
    sync(app)?;
    let handle = app.clone();
    let queued = Arc::new(AtomicBool::new(false));
    std::thread::spawn(move || {
        let mut last_inside = Instant::now() - Duration::from_secs(1);
        let mut last_stack_check = Instant::now();
        let mut reported_error = false;
        while !EXIT_REQUESTED.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(80));
            if queued.swap(true, Ordering::SeqCst) {
                continue;
            }
            let app = handle.clone();
            let pending = queued.clone();
            let check_stack = last_stack_check.elapsed() >= Duration::from_millis(400);
            if check_stack {
                last_stack_check = Instant::now();
            }
            // 只在 UI 线程操作窗口；最多保留一个排队任务，不积压鼠标消息。
            let inside_at = last_inside;
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            if handle
                .run_on_main_thread(move || {
                    let result = tick(&app, inside_at, check_stack);
                    let _ = sender.send(result);
                    pending.store(false, Ordering::SeqCst);
                })
                .is_err()
            {
                break;
            }
            match receiver.recv_timeout(Duration::from_secs(1)) {
                Ok(Ok(inside)) => {
                    last_inside = inside;
                    reported_error = false;
                }
                Ok(Err(error)) if !reported_error => {
                    eprintln!("[AMDL][窗口] 同步浮窗失败: {error}");
                    reported_error = true;
                }
                _ => {}
            }
        }
    });
    Ok(())
}

pub fn sync(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<WindowLockState>();
    let topmost = state.always_on_top.load(Ordering::SeqCst);
    let main = app.get_webview_window("main").ok_or("找不到歌词窗口")?;
    let unlock = app.get_webview_window("unlock").ok_or("找不到解锁窗口")?;
    #[cfg(windows)]
    native::sync(
        main.hwnd().map_err(|e| e.to_string())?,
        unlock.hwnd().map_err(|e| e.to_string())?,
        topmost,
    )?;
    #[cfg(not(windows))]
    {
        main.set_always_on_top(topmost).map_err(|e| e.to_string())?;
        unlock
            .set_always_on_top(topmost)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn hide_unlock(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<WindowLockState>();
    state.unlock_hovered.store(false, Ordering::SeqCst);
    if let Some(unlock) = app.get_webview_window("unlock") {
        unlock.hide().map_err(|e| e.to_string())?;
        unlock
            .set_ignore_cursor_events(true)
            .map_err(|e| e.to_string())?;
        unlock
            .emit("unlock-hover-changed", false)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn tick(app: &AppHandle, mut last_inside: Instant, check_stack: bool) -> Result<Instant, String> {
    let state = app.state::<WindowLockState>();
    let main = app.get_webview_window("main").ok_or("找不到歌词窗口")?;
    let unlock = app.get_webview_window("unlock").ok_or("找不到解锁窗口")?;
    #[cfg(windows)]
    let visible = native::visible(main.hwnd().map_err(|e| e.to_string())?);
    #[cfg(not(windows))]
    let visible = main.is_visible().unwrap_or(false);
    let locked = state.locked.load(Ordering::SeqCst);
    let hovered = if locked && visible {
        let bounds = crate::current_window_bounds(&main).ok_or("读取歌词窗口位置失败")?;
        let cursor = main.cursor_position().map_err(|e| e.to_string())?;
        if in_unlock_region(
            bounds,
            main.scale_factor().unwrap_or(1.0),
            cursor.x,
            cursor.y,
        ) {
            last_inside = Instant::now();
        }
        last_inside.elapsed() < Duration::from_millis(160)
    } else {
        false
    };
    if hovered != state.unlock_hovered.load(Ordering::SeqCst) {
        if hovered {
            crate::position_unlock_window(app)?;
            unlock
                .set_ignore_cursor_events(false)
                .map_err(|e| e.to_string())?;
            // unlock 配置为 focusable=false，显示时不抢占当前播放器/全屏窗口焦点。
            unlock.show().map_err(|e| e.to_string())?;
            sync(app)?;
            state.unlock_hovered.store(true, Ordering::SeqCst);
            unlock
                .emit("unlock-hover-changed", true)
                .map_err(|e| e.to_string())?;
        } else {
            hide_unlock(app)?;
        }
    }
    #[cfg(windows)]
    if check_stack
        && visible
        && if state.always_on_top.load(Ordering::SeqCst) {
            native::needs_raise(
                main.hwnd().map_err(|e| e.to_string())?,
                unlock.hwnd().map_err(|e| e.to_string())?,
            )
        } else {
            hovered
                && !native::adjacent(
                    main.hwnd().map_err(|e| e.to_string())?,
                    unlock.hwnd().map_err(|e| e.to_string())?,
                )
        }
    {
        sync(app)?;
    }
    Ok(last_inside)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hover_covers_entire_top_strip_with_negative_screen_coordinates_and_dpi() {
        let bounds = LyricsWindowBounds {
            x: -1600,
            y: 300,
            width: 1200,
            height: 225,
        };
        assert!(in_unlock_region(bounds, 1.5, -1599.0, 370.0));
        assert!(in_unlock_region(bounds, 1.5, -401.0, 300.0));
        assert!(!in_unlock_region(bounds, 1.5, -400.0, 300.0));
        assert!(!in_unlock_region(bounds, 1.5, -1000.0, 372.0));
        assert!(!in_unlock_region(bounds, 1.5, -1601.0, 350.0));
    }
}
