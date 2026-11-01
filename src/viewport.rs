//! 视口资源与屏幕坐标：全项目唯一的定位参照。
//!
//! [`ScreenPos`] 以视口左上角为原点、Y 向下，所有布局与放置只允许用它
//! 表达；落世界坐标必须经 [`ScreenPos::to_world`] 收口。视口尺寸由相机
//! 投影反算进 [`ViewportRes`]，窗口宽窄变化时布局跟随。

use bevy::camera::Projection;
use bevy::prelude::{Camera2d, Query, ResMut, Resource, Vec2, Vec3, With};
use bevy::window::Window;

use crate::layout::{BASIS_HEIGHT, BASIS_WIDTH};

/// 视口尺寸（世界单位），每帧从相机投影反算。
#[derive(Resource, Clone, Copy, Debug)]
pub struct ViewportRes {
    /// 视口宽度。
    pub width: f32,
    /// 视口高度。
    pub height: f32,
}

impl Default for ViewportRes {
    fn default() -> Self {
        Self {
            width: BASIS_WIDTH,
            height: BASIS_HEIGHT,
        }
    }
}

impl ViewportRes {
    /// 基准带在视口内的左上角（基准带始终居中）。
    #[must_use]
    pub fn basis_origin(self) -> ScreenPos {
        ScreenPos::new(
            (self.width - BASIS_WIDTH) / 2.0,
            (self.height - BASIS_HEIGHT) / 2.0,
        )
    }
}

/// 屏幕坐标：视口左上角为原点，Y 向下，单位为世界单位。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenPos {
    /// 横坐标。
    pub x: f32,
    /// 纵坐标。
    pub y: f32,
}

impl ScreenPos {
    /// 构造屏幕坐标。
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// 落世界坐标：唯一的屏幕到世界收口。
    #[must_use]
    pub fn to_world(self, viewport: ViewportRes, z: f32) -> Vec3 {
        Vec3::new(
            self.x - viewport.width / 2.0,
            viewport.height / 2.0 - self.y,
            z,
        )
    }
}

/// 从相机投影反算视口尺寸。
pub fn update_viewport(
    mut viewport: ResMut<ViewportRes>,
    cameras: Query<&Projection, With<Camera2d>>,
) {
    let Ok(Projection::Orthographic(ortho)) = cameras.single() else {
        return;
    };
    let size = ortho.area.size();
    if (viewport.width - size.x).abs() < f32::EPSILON
        && (viewport.height - size.y).abs() < f32::EPSILON
    {
        return;
    }
    viewport.width = size.x;
    viewport.height = size.y;
}

/// 逻辑像素光标转屏幕坐标的纯换算。
#[must_use]
fn screen_from_cursor(logical: Vec2, window_logical: Vec2, viewport: ViewportRes) -> ScreenPos {
    let scale_x = viewport.width / window_logical.x;
    let scale_y = viewport.height / window_logical.y;
    ScreenPos::new(
        (logical.x - window_logical.x / 2.0).mul_add(scale_x, viewport.width / 2.0),
        (logical.y - window_logical.y / 2.0).mul_add(scale_y, viewport.height / 2.0),
    )
}

/// 窗口光标（物理像素）转屏幕坐标。
///
/// 相机视野与窗口同心，逻辑坐标按视口与窗口的比值缩放。
#[must_use]
pub fn cursor_to_screen(cursor: Vec2, window: &Window, viewport: ViewportRes) -> ScreenPos {
    let logical = cursor / window.scale_factor();
    let window_logical = Vec2::new(window.width(), window.height());
    screen_from_cursor(logical, window_logical, viewport)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_pos_maps_to_world_with_y_flip() {
        let viewport = ViewportRes {
            width: 800.0,
            height: 600.0,
        };
        let top_left = ScreenPos::new(0.0, 0.0).to_world(viewport, 1.0);
        assert_eq!(top_left, Vec3::new(-400.0, 300.0, 1.0));
        let bottom_right = ScreenPos::new(800.0, 600.0).to_world(viewport, 1.0);
        assert_eq!(bottom_right, Vec3::new(400.0, -300.0, 1.0));
        let center = ScreenPos::new(400.0, 300.0).to_world(viewport, 0.0);
        assert_eq!(center, Vec3::ZERO);
    }

    #[test]
    fn basis_origin_centers_basis_band() {
        let wide = ViewportRes {
            width: 1066.0,
            height: 600.0,
        };
        assert_eq!(wide.basis_origin(), ScreenPos::new(133.0, 0.0));
        let tall = ViewportRes {
            width: 800.0,
            height: 1066.0,
        };
        assert_eq!(tall.basis_origin(), ScreenPos::new(0.0, 233.0));
    }

    #[test]
    fn cursor_conversions_round_trip() {
        let viewport = ViewportRes::default();
        let window = Vec2::new(800.0, 600.0);
        let center = screen_from_cursor(Vec2::new(400.0, 300.0), window, viewport);
        assert_eq!(center, ScreenPos::new(400.0, 300.0));
        let corner = screen_from_cursor(Vec2::new(800.0, 600.0), window, viewport);
        assert_eq!(corner, ScreenPos::new(800.0, 600.0));
    }

    #[test]
    fn wide_window_cursor_maps_into_viewport() {
        let viewport = ViewportRes {
            width: 1600.0,
            height: 600.0,
        };
        let window = Vec2::new(1600.0, 600.0);
        let right_edge = screen_from_cursor(Vec2::new(1590.0, 300.0), window, viewport);
        assert!((right_edge.x - 1590.0).abs() < 1e-4);
        assert!((right_edge.y - 300.0).abs() < 1e-4);
    }
}
