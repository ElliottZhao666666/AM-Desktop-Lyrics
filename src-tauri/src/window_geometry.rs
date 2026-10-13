use crate::LyricsWindowBounds;

#[derive(Clone, Copy)]
pub struct Screen {
    pub bounds: LyricsWindowBounds,
    pub scale: f64,
}

fn overlap(a: LyricsWindowBounds, b: LyricsWindowBounds) -> (i64, i64) {
    let width = (i64::from(a.x) + i64::from(a.width)).min(i64::from(b.x) + i64::from(b.width))
        - i64::from(a.x.max(b.x));
    let height = (i64::from(a.y) + i64::from(a.height)).min(i64::from(b.y) + i64::from(b.height))
        - i64::from(a.y.max(b.y));
    (width.max(0), height.max(0))
}

fn nearest(bounds: LyricsWindowBounds, screens: &[Screen]) -> Option<Screen> {
    screens.iter().copied().max_by_key(|screen| {
        let (width, height) = overlap(bounds, screen.bounds);
        let dx = i64::from(bounds.x) - i64::from(screen.bounds.x);
        let dy = i64::from(bounds.y) - i64::from(screen.bounds.y);
        (
            width * height,
            -(i128::from(dx).pow(2) + i128::from(dy).pow(2)),
        )
    })
}

pub fn default_bounds(screen: Screen) -> LyricsWindowBounds {
    let area = screen.bounds;
    let width = (800.0 * screen.scale).round() as u32;
    let height = (150.0 * screen.scale).round() as u32;
    fit_to_screen(
        LyricsWindowBounds {
            x: area
                .x
                .saturating_add((i64::from(area.width) - i64::from(width)).max(0) as i32 / 2),
            y: area.y.saturating_add(
                (i64::from(area.height) - i64::from(height) - (80.0 * screen.scale) as i64).max(0)
                    as i32,
            ),
            width,
            height,
        },
        screen,
    )
}

fn fit_to_screen(bounds: LyricsWindowBounds, screen: Screen) -> LyricsWindowBounds {
    let area = screen.bounds;
    let width = bounds
        .width
        .max((420.0 * screen.scale) as u32)
        .min(area.width.max(1));
    let height = bounds
        .height
        .max((100.0 * screen.scale) as u32)
        .min(area.height.max(1));
    LyricsWindowBounds {
        x: bounds.x.clamp(
            area.x,
            area.x
                .saturating_add(area.width.saturating_sub(width) as i32),
        ),
        y: bounds.y.clamp(
            area.y,
            area.y
                .saturating_add(area.height.saturating_sub(height) as i32),
        ),
        width,
        height,
    }
}

pub fn restore(bounds: LyricsWindowBounds, screens: &[Screen]) -> Option<LyricsWindowBounds> {
    nearest(bounds, screens).map(|screen| fit_to_screen(bounds, screen))
}

// 拖动时允许跨屏，只有实际工作区都看不到窗口时才拉回，避免在屏幕接缝卡住。
pub fn recover_if_hidden(bounds: LyricsWindowBounds, screens: &[Screen]) -> LyricsWindowBounds {
    if screens.iter().any(|screen| {
        let (w, h) = overlap(bounds, screen.bounds);
        w >= 32 && h >= 32
    }) {
        bounds
    } else {
        restore(bounds, screens).unwrap_or(bounds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn screen(x: i32, y: i32, w: u32, h: u32, scale: f64) -> Screen {
        Screen {
            bounds: LyricsWindowBounds {
                x,
                y,
                width: w,
                height: h,
            },
            scale,
        }
    }
    #[test]
    fn default_size_and_margin_follow_dpi() {
        let bounds = default_bounds(screen(0, 0, 2560, 1400, 1.5));
        assert_eq!(
            (bounds.width, bounds.height, bounds.x, bounds.y),
            (1200, 225, 680, 1055)
        );
    }
    #[test]
    fn l_shaped_desktop_gap_is_not_a_monitor() {
        let screens = [
            screen(-1920, 0, 1920, 1040, 1.0),
            screen(0, -1440, 2560, 1400, 1.0),
        ];
        let hidden = LyricsWindowBounds {
            x: 400,
            y: 400,
            width: 800,
            height: 150,
        };
        let restored = restore(hidden, &screens).unwrap();
        assert!(screens
            .iter()
            .any(|screen| overlap(restored, screen.bounds) == (800, 150)));
        let crossing = LyricsWindowBounds {
            x: -400,
            y: 0,
            ..hidden
        };
        assert_eq!(recover_if_hidden(crossing, &screens).x, -400);
        let huge = restore(
            LyricsWindowBounds {
                width: u32::MAX,
                height: u32::MAX,
                ..hidden
            },
            &screens,
        )
        .unwrap();
        assert!(huge.width <= 2560 && huge.height <= 1400);
    }
}
