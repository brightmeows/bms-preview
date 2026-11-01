//! 场景绘制层次：所有精灵 Z 常量的单一权威。
//!
//! 数值越小越靠下层；新增绘制元素时在此登记，禁止在使用处写字面量 Z。

/// 背景（覆盖全窗口的最底层色块）。
pub const BACKGROUND: f32 = -100.0;
/// 视频（BGA 之下）。
pub const VIDEO: f32 = -20.0;
/// BGA Base 层。
pub const BGA_BASE: f32 = -10.0;
/// BGA Layer 层。
pub const BGA_LAYER: f32 = -9.0;
/// BGA Layer2 层。
pub const BGA_LAYER2: f32 = -8.0;
/// BGA Poor 层。
pub const BGA_POOR: f32 = -7.0;
/// 轨道背景。
pub const TRACK_BG: f32 = -1.0;
/// 轨道边界线。
pub const TRACK_LINE: f32 = -0.5;
/// 小节线。
pub const BAR: f32 = -0.4;
/// 长音身体。
pub const LONG_BODY: f32 = 0.9;
/// 普通音符头。
pub const NOTE_HEAD: f32 = 1.0;
/// 不可见音符。
pub const NOTE_INVISIBLE: f32 = 1.2;
/// 地雷音符。
pub const NOTE_MINE: f32 = 1.4;
/// 判定线。
pub const JUDGMENT_LINE: f32 = 2.0;
/// 轨道标签。
pub const TRACK_LABEL: f32 = 2.5;
/// HUD 文本。
pub const HUD: f32 = 10.0;

/// BGA 图层索引对应的 Z。
#[must_use]
pub const fn bga_layer(layer: usize) -> f32 {
    match layer {
        0 => BGA_BASE,
        1 => BGA_LAYER,
        2 => BGA_LAYER2,
        _ => BGA_POOR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bga_layer_order_matches_base_to_poor() {
        assert!(bga_layer(0) < bga_layer(1));
        assert!(bga_layer(1) < bga_layer(2));
        assert!(bga_layer(2) < bga_layer(3));
        assert!((bga_layer(3) - BGA_POOR).abs() < 1e-6);
    }
}
