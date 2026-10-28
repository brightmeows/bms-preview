//! Text2d HUD、警告、最近文件菜单与加载遮罩。

use std::time::Duration;

use bevy::asset::Handle;
use bevy::prelude::{
    Color, Commands, Component, FontSize, Query, Res, Resource, Text2d, TextColor, TextFont,
    TextLayout, Time, Transform, Visibility,
};
use bevy::sprite::Anchor;
use bevy::text::{Font, Justify};

use crate::config::ConfigStore;
use crate::layout::{VIEW_HEIGHT, VIEW_WIDTH};
use crate::state::{Phase, Playback, Session, UiState};

/// UI 字体。
#[derive(Resource, Clone)]
pub struct Fonts {
    /// 主字体。
    pub ui: Handle<Font>,
}

/// HUD 文本类型。
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum HudKind {
    /// 左上信息。
    Info,
    /// 顶部标题。
    Title,
    /// 底部键位提示。
    Hints,
    /// 警告面板。
    Warnings,
    /// 最近文件菜单。
    Recent,
    /// 加载遮罩。
    Loading,
    /// 错误遮罩。
    Error,
    /// 调试信息。
    Debug,
}

/// 创建 HUD。
pub fn spawn_hud(commands: &mut Commands, fonts: &Fonts) {
    spawn_text(
        commands,
        fonts,
        HudKind::Info,
        Anchor::TOP_LEFT,
        (-390.0, 290.0),
        16.0,
        Color::srgb(0.92, 0.92, 0.95),
    );
    spawn_text(
        commands,
        fonts,
        HudKind::Title,
        Anchor::TOP_CENTER,
        (0.0, 286.0),
        22.0,
        Color::WHITE,
    );
    spawn_text(
        commands,
        fonts,
        HudKind::Hints,
        Anchor::BOTTOM_LEFT,
        (-390.0, -290.0),
        13.0,
        Color::srgb(0.62, 0.62, 0.68),
    );
    spawn_text(
        commands,
        fonts,
        HudKind::Warnings,
        Anchor::TOP_RIGHT,
        (390.0, 250.0),
        14.0,
        Color::srgb(0.98, 0.82, 0.28),
    );
    spawn_text(
        commands,
        fonts,
        HudKind::Recent,
        Anchor::CENTER,
        (0.0, 0.0),
        18.0,
        Color::srgb(0.95, 0.95, 0.98),
    );
    spawn_text(
        commands,
        fonts,
        HudKind::Loading,
        Anchor::CENTER,
        (0.0, 30.0),
        24.0,
        Color::WHITE,
    );
    spawn_text(
        commands,
        fonts,
        HudKind::Error,
        Anchor::CENTER,
        (0.0, 0.0),
        20.0,
        Color::srgb(0.98, 0.36, 0.36),
    );
    spawn_text(
        commands,
        fonts,
        HudKind::Debug,
        Anchor::BOTTOM_RIGHT,
        (390.0, -290.0),
        12.0,
        Color::srgb(0.55, 0.75, 0.95),
    );
}

fn spawn_text(
    commands: &mut Commands,
    fonts: &Fonts,
    kind: HudKind,
    anchor: Anchor,
    position: (f32, f32),
    size: f32,
    color: Color,
) {
    commands.spawn((
        kind,
        Text2d::new(""),
        TextFont {
            font: fonts.ui.clone().into(),
            font_size: FontSize::Px(size),
            ..Default::default()
        },
        TextColor(color),
        TextLayout::justify(Justify::Left),
        anchor,
        Transform::from_xyz(position.0, position.1, 10.0),
        Visibility::Hidden,
    ));
}

/// 每帧更新 HUD。
pub fn update_hud(
    session: Res<Session>,
    playback: Res<Playback>,
    ui: Res<UiState>,
    config: Res<ConfigStore>,
    time: Res<Time>,
    mut query: Query<(&HudKind, &mut Text2d, &mut Visibility)>,
) {
    for (kind, mut text, mut visibility) in &mut query {
        match kind {
            HudKind::Info => {
                if let Some(loaded) = session.loaded.as_ref() {
                    let duration = loaded.duration().as_secs_f64().max(0.001);
                    let elapsed = playback.chart_time.as_secs_f64().min(duration);
                    let progress = (elapsed / duration * 100.0).clamp(0.0, 100.0);
                    let audio = if playback.audio_available {
                        "audio clock"
                    } else {
                        "wall clock"
                    };
                    text.0 = format!(
                        "BPM {:.1}   {}\nTime {} / {}\nProgress {:.0}%\nNotes {}  LN {}  Mine {}  BGM {}\nSpeed {:.2}x  Vol {:.0}%  Offset {} ms\nSeed {}  Mode {}\n{}",
                        loaded.player.current_bpm(),
                        loaded.mode.label(),
                        format_time(playback.chart_time),
                        format_time(loaded.duration()),
                        progress,
                        loaded.stats.playable,
                        loaded.stats.long,
                        loaded.stats.mines,
                        loaded.stats.bgm,
                        playback.speed,
                        playback.effective_volume() * 100.0,
                        playback.offset_ms,
                        loaded.seed,
                        loaded.mode.label(),
                        audio,
                    );
                    *visibility = Visibility::Visible;
                } else {
                    *visibility = Visibility::Hidden;
                }
            }
            HudKind::Title => {
                if let Some(loaded) = session.loaded.as_ref() {
                    let title = loaded.title();
                    let artist = loaded.artist();
                    if title.is_empty() && artist.is_empty() {
                        *visibility = Visibility::Hidden;
                    } else {
                        let mut line = if artist.is_empty() {
                            title.to_owned()
                        } else {
                            format!("{title} — {artist}")
                        };
                        let chart_name = loaded.chart_name();
                        if !chart_name.is_empty() {
                            line.push_str("  [");
                            line.push_str(chart_name);
                            line.push(']');
                        }
                        text.0 = line;
                        *visibility = Visibility::Visible;
                    }
                } else {
                    *visibility = Visibility::Hidden;
                }
            }
            HudKind::Hints => {
                text.0 = "O 打开   Space 暂停   R 重播   ←/→ 5s   Home/End   ↑/↓ 倍速   +/- 音量   M 静音\nN 重掷 seed   L 布局   Tab 最近文件   W 警告   F3 调试   F 全屏   Q 退出".to_owned();
                *visibility = if session.loaded.is_some() {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                };
            }
            HudKind::Warnings => {
                if ui.show_warnings && !ui.warnings.is_empty() {
                    let mut body = format!("警告 (W) 共 {} 条\n", ui.warnings.len());
                    for warning in ui.warnings.iter().take(8) {
                        body.push_str(warning);
                        body.push('\n');
                    }
                    if ui.warnings.len() > 8 {
                        body.push_str("...\n");
                    }
                    text.0 = body;
                    *visibility = Visibility::Visible;
                } else {
                    *visibility = Visibility::Hidden;
                }
            }
            HudKind::Recent => {
                if ui.recent_open {
                    let mut body = String::from("最近文件 (Up/Down 选择, Enter 打开, Esc 关闭)\n");
                    if config.config.recent.is_empty() {
                        body.push_str("(空)");
                    } else {
                        for (index, path) in config.config.recent.iter().enumerate() {
                            let marker = if index == ui.recent_index { "> " } else { "  " };
                            body.push_str(marker);
                            body.push_str(&truncate_path(path));
                            body.push('\n');
                        }
                    }
                    text.0 = body;
                    *visibility = Visibility::Visible;
                } else {
                    *visibility = Visibility::Hidden;
                }
            }
            HudKind::Loading => {
                if ui.phase == Phase::Loading {
                    text.0 = if ui.loading_message.is_empty() {
                        "加载中...".to_owned()
                    } else {
                        ui.loading_message.clone()
                    };
                    *visibility = Visibility::Visible;
                } else if ui.phase == Phase::Empty && session.loaded.is_none() {
                    text.0 = if ui.loading_message.is_empty() {
                        "按 O 打开谱面".to_owned()
                    } else {
                        ui.loading_message.clone()
                    };
                    *visibility = Visibility::Visible;
                } else {
                    *visibility = Visibility::Hidden;
                }
            }
            HudKind::Error => {
                if ui.phase == Phase::Error {
                    text.0 = format!(
                        "{}\n\nO 打开其他谱面",
                        ui.error.as_deref().unwrap_or("加载失败")
                    );
                    *visibility = Visibility::Visible;
                } else {
                    *visibility = Visibility::Hidden;
                }
            }
            HudKind::Debug => {
                if ui.debug {
                    let fps = 1.0 / time.delta_secs().max(0.000_001);
                    text.0 = format!(
                        "FPS {fps:.0}  Δ {:.2} ms\nclock {:.2} s  voices n/a\nchart {:.2} s  seek #{}\nspeed {:.2}x  offset {} ms",
                        time.delta_secs_f64() * 1000.0,
                        playback.chart_time.as_secs_f64(),
                        playback.chart_time.as_secs_f64(),
                        playback.seek_serial,
                        playback.speed,
                        playback.offset_ms,
                    );
                    *visibility = Visibility::Visible;
                } else {
                    *visibility = Visibility::Hidden;
                }
            }
        }
    }
}

fn format_time(duration: Duration) -> String {
    let total = duration.as_secs();
    format!("{:02}:{:02}", total / 60, total % 60)
}

fn truncate_path(path: &std::path::Path) -> String {
    let text = path.display().to_string();
    if text.chars().count() <= 72 {
        text
    } else {
        let mut truncated: String = text.chars().take(69).collect();
        truncated.push_str("...");
        truncated
    }
}

// 让 VIEW_WIDTH/VIEW_HEIGHT 在布局调整时保持显式依赖。
const _: (f32, f32) = (VIEW_WIDTH, VIEW_HEIGHT);
