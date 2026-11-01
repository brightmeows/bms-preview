//! 轨道、音符、小节线与静态场景。

use crate::layout::{
    BASIS_HEIGHT, BASIS_WIDTH, JUDGMENT_WORLD_Y, LOOKAHEAD_MS, PIXELS_PER_BEAT, REACTION_MS,
};
use crate::state::Session;
use crate::viewport::{ScreenPos, ViewportRes};
use crate::z_order;
use bevy::camera::{OrthographicProjection, Projection, ScalingMode};
use bevy::ecs::change_detection::DetectChanges as _;
use bevy::prelude::{
    Camera2d, Color, Commands, Component, Entity, FontSize, Query, Res, Sprite, Text2d, TextColor,
    TextFont, TextLayout, Transform, Vec2, Visibility, With,
};
use bevy::sprite::Anchor;
use bevy::text::Justify;
use bmsrs::chart::{EventKind, Lane, NoteKind};

/// 谱面实体标记；换谱时统一清理。
#[derive(Component)]
pub struct ChartEntity;

/// 轨道背景精灵：记录基准带内几何，视口变化时由同步系统重排。
#[derive(Component)]
pub struct TrackBg {
    basis_center_x: f32,
    width: f32,
}

/// 轨道边界线精灵。
#[derive(Component)]
pub struct TrackEdge {
    basis_x: f32,
}

/// 轨道标签文本。
#[derive(Component)]
pub struct TrackLabelMarker {
    basis_center_x: f32,
}

/// 音符部件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotePart {
    /// 音符头。
    Head,
    /// 长音身体。
    Body,
}

/// 音符渲染数据。
#[derive(Component, Clone, Copy, Debug)]
pub struct NoteVisual {
    /// 脉冲位置。
    pub tick: u64,
    /// 轨道索引。
    pub track: usize,
    /// 长音终点。
    pub end_tick: Option<u64>,
    /// 部件。
    pub part: NotePart,
}

/// 小节线渲染数据。
#[derive(Component, Clone, Copy, Debug)]
pub struct BarVisual {
    /// 脉冲位置。
    pub tick: u64,
}

const COLOR_BG: Color = Color::srgb(0.04, 0.04, 0.05);
const COLOR_TRACK_BG: Color = Color::srgb(0.12, 0.12, 0.13);
const COLOR_TRACK_LINE: Color = Color::srgb(0.32, 0.32, 0.34);
const COLOR_BAR: Color = Color::srgba(0.55, 0.55, 0.58, 0.55);
const COLOR_NOTE_WHITE: Color = Color::srgb(0.96, 0.96, 0.98);
const COLOR_NOTE_BLUE: Color = Color::srgb(0.36, 0.62, 0.95);
const COLOR_NOTE_SCRATCH: Color = Color::srgb(0.95, 0.24, 0.28);
const COLOR_NOTE_MINE: Color = Color::srgb(0.98, 0.82, 0.18);
const COLOR_NOTE_INVISIBLE: Color = Color::srgba(0.65, 0.65, 0.68, 0.45);
const COLOR_LABEL: Color = Color::srgb(0.72, 0.72, 0.76);

/// 创建相机与背景。
pub fn setup_scene(commands: &mut Commands) -> Entity {
    let camera = commands
        .spawn((
            Camera2d,
            Projection::Orthographic(OrthographicProjection {
                scaling_mode: ScalingMode::AutoMin {
                    min_width: BASIS_WIDTH,
                    min_height: BASIS_HEIGHT,
                },
                ..OrthographicProjection::default_2d()
            }),
        ))
        .id();

    commands.spawn((
        Sprite::from_color(COLOR_BG, Vec2::new(2000.0, 1400.0)),
        Transform::from_xyz(0.0, 0.0, z_order::BACKGROUND),
    ));
    camera
}

/// 清理旧谱面实体。
pub fn despawn_chart(commands: &mut Commands, query: &Query<Entity, With<ChartEntity>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
}

/// 生成轨道、音符与小节线。
pub fn spawn_chart(
    commands: &mut Commands,
    session: &Session,
    fonts: &crate::ui::Fonts,
    viewport: ViewportRes,
) {
    let Some(loaded) = session.loaded.as_ref() else {
        return;
    };
    let layout = &session.layout;
    let (left, right) = layout.bounds();
    let total_width = right - left;
    let origin = viewport.basis_origin();

    for track in &layout.tracks {
        let center = track.x + track.width / 2.0;
        let at = |basis_x: f32, screen_y: f32, z: f32| {
            ScreenPos::new(origin.x + basis_x, screen_y).to_world(viewport, z)
        };
        commands.spawn((
            ChartEntity,
            TrackBg {
                basis_center_x: center,
                width: track.width,
            },
            Sprite::from_color(COLOR_TRACK_BG, Vec2::new(track.width, viewport.height)),
            Transform::from_translation(at(center, viewport.height / 2.0, z_order::TRACK_BG)),
        ));
        for edge in [track.x, track.x + track.width] {
            commands.spawn((
                ChartEntity,
                TrackEdge { basis_x: edge },
                Sprite::from_color(COLOR_TRACK_LINE, Vec2::new(1.0, viewport.height)),
                Transform::from_translation(at(edge, viewport.height / 2.0, z_order::TRACK_LINE)),
            ));
        }
        if layout.len() <= 16 {
            commands.spawn((
                ChartEntity,
                Text2d::new(track.label.clone()),
                TextFont {
                    font: fonts.ui.clone().into(),
                    font_size: FontSize::Px(12.0),
                    ..Default::default()
                },
                TextColor(COLOR_LABEL),
                TextLayout::justify(Justify::Center),
                Anchor::TOP_CENTER,
                TrackLabelMarker {
                    basis_center_x: center,
                },
                Transform::from_translation(at(center, 10.0, z_order::TRACK_LABEL)),
            ));
        }
    }

    commands.spawn((
        ChartEntity,
        JudgmentLineMarker,
        Sprite::from_color(Color::WHITE, Vec2::new(total_width + 24.0, 3.0)),
        Transform::from_xyz(0.0, JUDGMENT_WORLD_Y, z_order::JUDGMENT_LINE),
    ));

    for event in &loaded.player.chart().data.events {
        match &event.kind {
            EventKind::Bar => {
                commands.spawn((
                    ChartEntity,
                    BarVisual { tick: event.tick() },
                    Sprite::from_color(COLOR_BAR, Vec2::new(total_width.max(1.0), 1.0)),
                    Transform::from_xyz(0.0, 0.0, z_order::BAR),
                    Visibility::Hidden,
                ));
            }
            EventKind::Note {
                side, lane, kind, ..
            } => {
                let Some(track) = layout.index_of(*side, *lane) else {
                    continue;
                };
                spawn_note(
                    commands,
                    event.tick(),
                    track,
                    *kind,
                    layout.width(track),
                    *lane,
                );
            }
            _ => {}
        }
    }
}

/// 判定线实体标记；位置固定于世界坐标，不随视口同步。
#[derive(Component)]
struct JudgmentLineMarker;

fn spawn_note(
    commands: &mut Commands,
    tick: u64,
    track: usize,
    kind: NoteKind,
    width: f32,
    lane: Lane,
) {
    let base_color = match kind {
        NoteKind::Mine { .. } => COLOR_NOTE_MINE,
        NoteKind::Invisible => COLOR_NOTE_INVISIBLE,
        _ => note_color(lane, track),
    };
    let (size, z, rotation) = match kind {
        NoteKind::Mine { .. } => (
            Vec2::splat((width / 3.0).max(6.0)),
            z_order::NOTE_MINE,
            0.785_398_2,
        ),
        NoteKind::Invisible => (Vec2::splat(8.0), z_order::NOTE_INVISIBLE, 0.0),
        _ => (
            Vec2::new((width - 4.0).max(2.0), 8.0),
            z_order::NOTE_HEAD,
            0.0,
        ),
    };
    commands.spawn((
        ChartEntity,
        NoteVisual {
            tick,
            track,
            end_tick: match kind {
                NoteKind::Long { duration } if duration > 0 => Some(tick + duration),
                _ => None,
            },
            part: NotePart::Head,
        },
        Sprite::from_color(base_color, size),
        Transform::from_xyz(0.0, 0.0, z).with_rotation(bevy::math::Quat::from_rotation_z(rotation)),
        Visibility::Hidden,
    ));

    if let NoteKind::Long { duration } = kind
        && duration > 0
    {
        commands.spawn((
            ChartEntity,
            NoteVisual {
                tick,
                track,
                end_tick: Some(tick + duration),
                part: NotePart::Body,
            },
            Sprite::from_color(
                with_alpha(base_color, 0.35),
                Vec2::new((width - 4.0).max(2.0), 1.0),
            ),
            Transform::from_xyz(0.0, 0.0, z_order::LONG_BODY),
            Visibility::Hidden,
        ));
    }
}

fn with_alpha(color: Color, alpha: f32) -> Color {
    let color = color.to_srgba();
    Color::srgba(color.red, color.green, color.blue, alpha)
}

const fn note_color(lane: Lane, track: usize) -> Color {
    match lane {
        Lane::Scratch(_) => COLOR_NOTE_SCRATCH,
        _ if track % 2 == 1 => COLOR_NOTE_WHITE,
        _ => COLOR_NOTE_BLUE,
    }
}

/// 滚动位置差换算为屏幕像素：SCROLL 积分差乘 SPEED 事件端倍率。
const fn scroll_offset_px(scroll_diff: f64, spacing: f64) -> f32 {
    (scroll_diff * spacing) as f32 * PIXELS_PER_BEAT
}

/// 判定线的屏幕 Y：固定世界坐标，视口居中扩展时上下均匀。
const fn judgment_screen_y(viewport_height: f32) -> f32 {
    viewport_height / 2.0 - JUDGMENT_WORLD_Y
}

/// 事件的屏幕 Y：判定线减去滚动偏移；`spacing` 必须取该事件自身 tick 的倍率。
const fn event_screen_y(viewport_height: f32, scroll_diff: f64, spacing: f64) -> f32 {
    judgment_screen_y(viewport_height) - scroll_offset_px(scroll_diff, spacing)
}

/// 长音身体的屏幕几何：中心 Y 与高度。
const fn body_geometry(top: f32, bottom: f32) -> (f32, f32) {
    (f32::midpoint(top, bottom), (bottom - top).max(1.0))
}

/// 屏幕 Y 是否落在可见窗口（含上下余量）。
const fn y_visible(y: f32, viewport_height: f32, margin: f32) -> bool {
    y >= -margin && y <= viewport_height + margin
}

/// 视口尺寸变化时重排轨道背景、边界线与标签。
pub fn sync_static_visuals(
    viewport: Res<ViewportRes>,
    mut backgrounds: Query<(&TrackBg, &mut Sprite, &mut Transform)>,
    mut edges: Query<(&TrackEdge, &mut Sprite, &mut Transform)>,
    mut labels: Query<(&TrackLabelMarker, &mut Transform)>,
) {
    if !viewport.is_changed() {
        return;
    }
    let origin = viewport.basis_origin();
    for (bg, mut sprite, mut transform) in &mut backgrounds {
        sprite.custom_size = Some(Vec2::new(bg.width, viewport.height));
        transform.translation = ScreenPos::new(origin.x + bg.basis_center_x, viewport.height / 2.0)
            .to_world(*viewport, z_order::TRACK_BG);
    }
    for (edge, mut sprite, mut transform) in &mut edges {
        sprite.custom_size = Some(Vec2::new(1.0, viewport.height));
        transform.translation = ScreenPos::new(origin.x + edge.basis_x, viewport.height / 2.0)
            .to_world(*viewport, z_order::TRACK_LINE);
    }
    for (label, mut transform) in &mut labels {
        transform.translation = ScreenPos::new(origin.x + label.basis_center_x, 10.0)
            .to_world(*viewport, z_order::TRACK_LABEL);
    }
}

/// 更新音符位置。
pub fn update_notes(
    session: Res<Session>,
    viewport: Res<ViewportRes>,
    mut query: Query<(&NoteVisual, &mut Transform, &mut Sprite, &mut Visibility)>,
) {
    let Some(loaded) = session.loaded.as_ref() else {
        return;
    };
    let player = &loaded.player;
    let current_tick = player.current_tick();
    let current_scroll = player.scroll_position_at(current_tick);
    let origin_x = viewport.basis_origin().x;

    for (note, mut transform, mut sprite, mut visibility) in &mut query {
        let note_scroll = player.scroll_position_at(note.tick);
        let scroll_diff = note_scroll - current_scroll;
        let y = event_screen_y(viewport.height, scroll_diff, player.spacing_at(note.tick));
        let screen = ScreenPos::new(origin_x + session.layout.center_x(note.track), y);

        match note.part {
            NotePart::Head => {
                transform.translation = screen.to_world(*viewport, transform.translation.z);
                *visibility = if y_visible(y, viewport.height, 50.0) {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                };
            }
            NotePart::Body => {
                let Some(end_tick) = note.end_tick else {
                    *visibility = Visibility::Hidden;
                    continue;
                };
                let end_scroll = player.scroll_position_at(end_tick);
                let end_diff = end_scroll - current_scroll;
                let end_y = event_screen_y(viewport.height, end_diff, player.spacing_at(end_tick));
                let top = y.min(end_y);
                let bottom = y.max(end_y);
                let (center, height) = body_geometry(top, bottom);
                sprite.custom_size =
                    Some(Vec2::new(session.layout.width(note.track) - 4.0, height));
                transform.translation =
                    ScreenPos::new(screen.x, center).to_world(*viewport, transform.translation.z);
                *visibility = if bottom >= -50.0 && top <= viewport.height + 50.0 {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                };
            }
        }
    }
}

/// 更新小节线。
pub fn update_bars(
    session: Res<Session>,
    viewport: Res<ViewportRes>,
    mut query: Query<(&BarVisual, &mut Transform, &mut Visibility)>,
) {
    let Some(loaded) = session.loaded.as_ref() else {
        return;
    };
    let player = &loaded.player;
    let current_tick = player.current_tick();
    let current_scroll = player.scroll_position_at(current_tick);
    let (start_tick, _) = player.visible_tick_range(
        std::time::Duration::from_millis(REACTION_MS),
        std::time::Duration::from_millis(LOOKAHEAD_MS),
    );

    for (bar, mut transform, mut visibility) in &mut query {
        let diff = player.scroll_position_at(bar.tick) - current_scroll;
        let y = event_screen_y(viewport.height, diff, player.spacing_at(bar.tick));
        transform.translation.y = viewport.height / 2.0 - y;
        *visibility = if bar.tick >= start_tick && y_visible(y, viewport.height, 20.0) {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn judgment_line_sits_below_viewport_center() {
        assert!((judgment_screen_y(600.0) - 500.0).abs() < 1e-5);
        // 窄窗口扩展：判定线固定世界坐标，屏幕位置随视口居中
        assert!((judgment_screen_y(1066.0) - 733.0).abs() < 1e-3);
    }

    #[test]
    fn future_events_render_above_judgment_line() {
        let y = event_screen_y(600.0, 1.0, 1.0);
        assert!(y < judgment_screen_y(600.0));
        assert!((y - (500.0 - PIXELS_PER_BEAT)).abs() < 1e-4);
    }

    #[test]
    fn scroll_offset_scales_with_spacing() {
        assert!((scroll_offset_px(1.0, 1.0) - PIXELS_PER_BEAT).abs() < 1e-5);
        assert!((scroll_offset_px(1.0, 2.0) - 2.0 * PIXELS_PER_BEAT).abs() < 1e-5);
        assert!((scroll_offset_px(2.0, 0.5) - PIXELS_PER_BEAT).abs() < 1e-5);
    }

    #[test]
    fn ln_tail_uses_endpoint_spacing() {
        // 回归：长音尾部必须用尾部 tick 的 SPEED 倍率，而非头部值
        let head = event_screen_y(600.0, 1.0, 2.0);
        let tail = event_screen_y(600.0, 2.0, 0.5);
        let wrong_if_reusing_head = event_screen_y(600.0, 2.0, 2.0);
        assert_ne!(tail, wrong_if_reusing_head);
        assert!(tail > head);
    }

    #[test]
    fn body_geometry_returns_center_and_min_height() {
        let (center, height) = body_geometry(100.0, 300.0);
        assert!((center - 200.0).abs() < 1e-5);
        assert!((height - 200.0).abs() < 1e-5);
        let (_, min_height) = body_geometry(300.0, 300.0);
        assert!((min_height - 1.0).abs() < 1e-5);
    }

    #[test]
    fn visibility_window_tracks_viewport_height() {
        assert!(y_visible(50.0, 600.0, 50.0));
        assert!(y_visible(-50.0, 600.0, 50.0));
        assert!(!y_visible(-51.0, 600.0, 50.0));
        assert!(y_visible(650.0, 600.0, 50.0));
        assert!(!y_visible(651.0, 600.0, 50.0));
    }
}
