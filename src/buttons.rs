//! 常驻按钮栏与"更多"操作面板。
//!
//! 基于 `bevy_ui_widgets` 的无头 [`Button`]：按钮只负责按下行为，
//! 观察者统一把 [`Activate`] 转发给 [`run_action`]；样式在本模块提供。

use bevy::ecs::hierarchy::ChildSpawnerCommands;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::system::EntityCommands;
use bevy::picking::hover::Hovered;
use bevy::prelude::{
    AppExit, Color, Commands, Component, Entity, MessageWriter, NonSendMut, Query, Res, ResMut,
    Text, TextColor, TextFont, Visibility,
};
use bevy::ui::{
    AlignItems, BackgroundColor, BorderColor, BorderRadius, FlexDirection, JustifyContent, Node,
    PositionType, Pressed, UiRect, UiTargetCamera, Val,
};
use bevy::ui_widgets::{Activate, Button, observe};
use bevy::window::{PrimaryWindow, Window};

use crate::actions::{ActionContext, UiAction, run_action};
use crate::app::LoadWorker;
use crate::audio::AudioBackend;
use crate::config::ConfigStore;
use crate::picker::Picker;
use crate::state::{Playback, SampleRate, Session, UiState};
use crate::ui::Fonts;

/// 按钮对应的用户动作。
#[derive(Component, Clone, Copy)]
pub struct ButtonAction(pub UiAction);

/// "更多"面板根节点标记。
#[derive(Component)]
pub struct MorePanel;

/// 暂停按钮文本标记：随播放状态切换文案。
#[derive(Component)]
pub struct PauseLabel;

const COLOR_BUTTON_BG: Color = Color::srgba(0.10, 0.10, 0.12, 0.88);
const COLOR_BUTTON_BG_HOVER: Color = Color::srgba(0.18, 0.18, 0.22, 0.92);
const COLOR_BUTTON_BORDER: Color = Color::srgba(0.36, 0.36, 0.42, 0.90);
const COLOR_TEXT: Color = Color::srgb(0.85, 0.85, 0.88);
const COLOR_TEXT_DIM: Color = Color::srgb(0.60, 0.60, 0.66);
const COLOR_PANEL_BG: Color = Color::srgba(0.07, 0.07, 0.09, 0.94);
const BUTTON_HEIGHT: f32 = 26.0;
const FONT_SIZE: f32 = 12.0;

/// "更多"面板的操作清单：动作与内嵌键位标注。
const MORE_ACTIONS: &[(UiAction, &str)] = &[
    (UiAction::SeekStart, "回到开头 Home"),
    (UiAction::SeekEnd, "跳到末尾 End"),
    (UiAction::SpeedUp, "倍速加 ↑"),
    (UiAction::SpeedDown, "倍速减 ↓"),
    (UiAction::VolumeUp, "音量加 +"),
    (UiAction::VolumeDown, "音量减 -"),
    (UiAction::ToggleMute, "静音 M"),
    (UiAction::OffsetUp, "偏移加 5 毫秒 ."),
    (UiAction::OffsetDown, "偏移减 5 毫秒 ,"),
    (UiAction::RerollSeed, "重掷 seed N"),
    (UiAction::ToggleLayout, "切换布局 L"),
    (UiAction::ToggleRecent, "最近文件 Tab"),
    (UiAction::ToggleWarnings, "警告 W"),
    (UiAction::ToggleDebug, "调试 F3"),
    (UiAction::Quit, "退出 Q"),
];

/// 创建常驻按钮栏与"更多"面板。
pub fn spawn_buttons(commands: &mut Commands, camera: Entity, fonts: &Fonts) {
    commands
        .spawn((
            UiTargetCamera(camera),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(8.0),
                bottom: Val::Px(8.0),
                column_gap: Val::Px(6.0),
                flex_direction: FlexDirection::Row,
                ..Node::default()
            },
        ))
        .with_children(|bar| {
            spawn_button(bar, camera, fonts, UiAction::OpenFile, "打开 O");
            spawn_button(bar, camera, fonts, UiAction::TogglePause, "暂停 Space")
                .insert(PauseLabel);
            spawn_button(bar, camera, fonts, UiAction::Restart, "重播 R");
            spawn_button(bar, camera, fonts, UiAction::SeekBack, "后退 5 秒 ←");
            spawn_button(bar, camera, fonts, UiAction::SeekForward, "前进 5 秒 →");
            spawn_button(bar, camera, fonts, UiAction::ToggleFullscreen, "全屏 F");
            spawn_button(bar, camera, fonts, UiAction::ToggleMore, "更多");
        });

    commands
        .spawn((
            UiTargetCamera(camera),
            MorePanel,
            Visibility::Hidden,
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(8.0),
                bottom: Val::Px(BUTTON_HEIGHT + 16.0),
                width: Val::Px(240.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                padding: UiRect::all(Val::Px(8.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                ..Node::default()
            },
            BackgroundColor(COLOR_PANEL_BG),
            BorderColor::all(COLOR_BUTTON_BORDER),
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new("更多操作"),
                TextFont {
                    font: fonts.ui.clone().into(),
                    font_size: bevy::text::FontSize::Px(FONT_SIZE),
                    ..TextFont::default()
                },
                TextColor(COLOR_TEXT_DIM),
            ));
            for (action, label) in MORE_ACTIONS {
                spawn_button(panel, camera, fonts, *action, label);
            }
        });
}

/// 生成一枚按钮：无头 [`Button`] 加样式与文本子节点。
fn spawn_button<'a>(
    parent: &'a mut ChildSpawnerCommands,
    camera: Entity,
    fonts: &Fonts,
    action: UiAction,
    label: &str,
) -> EntityCommands<'a> {
    let mut entity = parent.spawn((
        UiTargetCamera(camera),
        observe(run_button),
        ButtonAction(action),
        Button,
        Hovered::default(),
        Node {
            height: Val::Px(BUTTON_HEIGHT),
            padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(4.0)),
            ..Node::default()
        },
        BackgroundColor(COLOR_BUTTON_BG),
        BorderColor::all(COLOR_BUTTON_BORDER),
    ));
    entity.with_children(|button| {
        button.spawn((
            UiTargetCamera(camera),
            Text::new(label),
            TextFont {
                font: fonts.ui.clone().into(),
                font_size: bevy::text::FontSize::Px(FONT_SIZE),
                ..TextFont::default()
            },
            TextColor(COLOR_TEXT),
        ));
    });
    entity
}

/// 统一的按钮观察者：把激活事件转成用户动作执行。
#[expect(
    clippy::too_many_arguments,
    reason = "observer needs the full action context"
)]
fn run_button(
    activate: On<Activate>,
    actions: Query<&ButtonAction>,
    session: Res<Session>,
    mut playback: ResMut<Playback>,
    mut ui: ResMut<UiState>,
    mut config: ResMut<ConfigStore>,
    mut picker: ResMut<Picker>,
    mut worker: ResMut<LoadWorker>,
    sample_rate: Res<SampleRate>,
    mut audio: NonSendMut<AudioBackend>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(action) = actions.get(activate.entity) else {
        return;
    };
    let action = action.0;
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    let window: &mut Window = &mut window;
    let mut ctx = ActionContext {
        session: &session,
        playback: &mut playback,
        ui: &mut ui,
        config: &mut config,
        picker: &mut picker,
        worker: &mut worker,
        audio: &mut audio,
        sample_rate: sample_rate.0,
    };
    if run_action(action, &mut ctx, window) {
        exit.write(AppExit::Success);
    }
}

/// 每帧维护：悬停标记（与 seek 热区互斥）、面板显隐、暂停按钮文案。
pub fn maintain_buttons(
    mut ui: ResMut<UiState>,
    playback: Res<Playback>,
    mut panels: Query<&mut Visibility, With<MorePanel>>,
    mut pause_labels: Query<&mut Text, With<PauseLabel>>,
    mut states: Query<(&Hovered, Option<&Pressed>, &mut BackgroundColor), With<Button>>,
) {
    ui.bar_hovered = false;
    for (hovered, pressed, mut background) in &mut states {
        let active = hovered.0 || pressed.is_some();
        ui.bar_hovered |= active;
        let target = if active {
            COLOR_BUTTON_BG_HOVER
        } else {
            COLOR_BUTTON_BG
        };
        if background.0 != target {
            background.0 = target;
        }
    }
    for mut visibility in &mut panels {
        *visibility = if ui.more_open {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    for mut text in &mut pause_labels {
        let label = if playback.playing {
            "继续 Space"
        } else {
            "暂停 Space"
        };
        if text.0 != label {
            text.0 = label.to_owned();
        }
    }
}
