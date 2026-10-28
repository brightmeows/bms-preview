//! UI 字体加载。

use std::path::PathBuf;

use bevy::asset::{Assets, Handle};
use bevy::log::warn;
use bevy::text::Font;

/// 加载支持中日文的系统字体；找不到时回退 Bevy 默认字体。
#[must_use]
pub fn load_ui_font(assets: &mut Assets<Font>) -> Handle<Font> {
    let mut candidates = vec![
        PathBuf::from("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc"),
        PathBuf::from("/usr/share/fonts/noto/NotoSansCJK-Regular.ttc"),
        PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        candidates.push(home.join(".local/share/fonts/NotoSansCJK-Regular.ttc"));
        candidates.push(home.join(".local/share/fonts/NotoSansCJKjp-Regular.otf"));
    }

    for path in candidates {
        if let Ok(bytes) = std::fs::read(&path) {
            return assets.add(Font::from_bytes(bytes));
        }
    }

    warn!("未找到 CJK 系统字体，标题可能显示为方框");
    Handle::default()
}
