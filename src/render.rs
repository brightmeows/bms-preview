//! 轨道、音符、小节线与静态场景。

use crate::layout::{
    JUDGMENT_LINE_Y, LOOKAHEAD_MS, PIXELS_PER_BEAT, REACTION_MS, VIEW_HEIGHT, judgment_world_y,
    world_x, world_y,
};
use crate::state::Session;
use bevy::camera::{OrthographicProjection, Projection, ScalingMode};
use bevy::prelude::{
    Camera2d, Color, Commands, Component, Entity, FontSize, Query, Res, Sprite, Text2d, TextColor,
    TextFont, TextLayout, Transform, Vec2, Vec3, Visibility, With,
};
use bevy::sprite::Anchor;
use bevy::text::Justify;
use bmsrs::chart::{EventKind, Lane, NoteKind};

/// 谱面实体标记；换谱时统一清理。
#[derive(Component)]
pub struct ChartEntity;

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
pub fn setup_scene(commands: &mut Commands) {
    commands.spawn((
        Camera2d,
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: VIEW_HEIGHT,
            },
            ..OrthographicProjection::default_2d()
        }),
    ));

    commands.spawn((
        Sprite::from_color(COLOR_BG, Vec2::new(2000.0, 1400.0)),
        Transform::from_xyz(0.0, 0.0, -100.0),
    ));
}

/// 清理旧谱面实体。
pub fn despawn_chart(commands: &mut Commands, query: &Query<Entity, With<ChartEntity>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
}

/// 生成轨道、音符与小节线。
pub fn spawn_chart(commands: &mut Commands, session: &Session, fonts: &crate::ui::Fonts) {
    let Some(loaded) = session.loaded.as_ref() else {
        return;
    };
    let layout = &session.layout;
    let (left, right) = layout.bounds();
    let total_width = right - left;

    for track in &layout.tracks {
        let center = track.x + track.width / 2.0;
        commands.spawn((
            ChartEntity,
            Sprite::from_color(COLOR_TRACK_BG, Vec2::new(track.width, VIEW_HEIGHT)),
            Transform::from_xyz(world_x(center), 0.0, -1.0),
        ));
        for edge in [track.x, track.x + track.width] {
            commands.spawn((
                ChartEntity,
                Sprite::from_color(COLOR_TRACK_LINE, Vec2::new(1.0, VIEW_HEIGHT)),
                Transform::from_xyz(world_x(edge), 0.0, -0.5),
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
                Transform::from_xyz(world_x(center), world_y(VIEW_HEIGHT - 10.0), 2.5),
            ));
        }
    }

    commands.spawn((
        ChartEntity,
        Sprite::from_color(Color::WHITE, Vec2::new(total_width + 24.0, 3.0)),
        Transform::from_xyz(0.0, judgment_world_y(), 2.0),
    ));

    for event in &loaded.player.chart().data.events {
        match &event.kind {
            EventKind::Bar => {
                commands.spawn((
                    ChartEntity,
                    BarVisual { tick: event.tick() },
                    Sprite::from_color(COLOR_BAR, Vec2::new(total_width.max(1.0), 1.0)),
                    Transform::from_xyz(0.0, 0.0, -0.4),
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
        NoteKind::Mine { .. } => (Vec2::splat((width / 3.0).max(6.0)), 1.4, 0.785_398_2),
        NoteKind::Invisible => (Vec2::splat(8.0), 1.2, 0.0),
        _ => (Vec2::new((width - 4.0).max(2.0), 8.0), 1.0, 0.0),
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
            Transform::from_xyz(0.0, 0.0, 0.9),
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

/// 更新音符位置。
pub fn update_notes(
    session: Res<Session>,
    mut query: Query<(&NoteVisual, &mut Transform, &mut Sprite, &mut Visibility)>,
) {
    let Some(loaded) = session.loaded.as_ref() else {
        return;
    };
    let player = &loaded.player;
    let current_tick = player.current_tick();
    let current_scroll = player.scroll_position_at(current_tick);

    for (note, mut transform, mut sprite, mut visibility) in &mut query {
        let note_scroll = player.scroll_position_at(note.tick);
        let scroll_diff = note_scroll - current_scroll;
        let spacing = player.spacing_at(note.tick);
        let y = JUDGMENT_LINE_Y - scroll_diff as f32 * PIXELS_PER_BEAT * spacing as f32;
        let x = session.layout.center_x(note.track);
        let world_x_value = world_x(x);

        match note.part {
            NotePart::Head => {
                transform.translation =
                    Vec3::new(world_x_value, world_y(y), transform.translation.z);
                *visibility = if (-50.0..=VIEW_HEIGHT + 50.0).contains(&y) {
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
                let end_y = JUDGMENT_LINE_Y - end_diff as f32 * PIXELS_PER_BEAT * spacing as f32;
                let top = y.min(end_y);
                let bottom = y.max(end_y);
                let height = (bottom - top).max(1.0);
                sprite.custom_size =
                    Some(Vec2::new(session.layout.width(note.track) - 4.0, height));
                transform.translation = Vec3::new(
                    world_x_value,
                    world_y(f32::midpoint(top, bottom)),
                    transform.translation.z,
                );
                *visibility = if bottom >= -50.0 && top <= VIEW_HEIGHT + 50.0 {
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
        let y = JUDGMENT_LINE_Y
            - (player.scroll_position_at(bar.tick) - current_scroll) as f32 * PIXELS_PER_BEAT;
        transform.translation.y = world_y(y);
        *visibility = if bar.tick >= start_tick && (-20.0..=VIEW_HEIGHT + 20.0).contains(&y) {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}
