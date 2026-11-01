//! 用户动作：键盘与按钮共用的操作入口。
//!
//! [`UiAction`] 枚举全部可触发操作，[`run_action`] 是唯一执行点；
//! 键盘系统与按钮系统各自负责把输入事件映射为动作。

use std::time::Duration;

use bevy::window::{MonitorSelection, Window, WindowMode};

use crate::app::{LoadWorker, begin_load};
use crate::audio::AudioBackend;
use crate::chart::{LoadOptions, LoadedChart, ModeChoice, ModeKind, random_seed};
use crate::config::ConfigStore;
use crate::picker::Picker;
use crate::state::{Playback, Session, UiState};

/// 可由键盘或按钮触发的用户动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiAction {
    /// 打开谱面文件。
    OpenFile,
    /// 暂停或继续。
    TogglePause,
    /// 从头重播。
    Restart,
    /// 后退 5 秒。
    SeekBack,
    /// 前进 5 秒。
    SeekForward,
    /// 跳到开头。
    SeekStart,
    /// 跳到末尾。
    SeekEnd,
    /// 切换窗口化与无边框全屏。
    ToggleFullscreen,
    /// 退出程序。
    Quit,
    /// 切换静音。
    ToggleMute,
    /// 提升播放倍速。
    SpeedUp,
    /// 降低播放倍速。
    SpeedDown,
    /// 提升主音量。
    VolumeUp,
    /// 降低主音量。
    VolumeDown,
    /// 音频偏移加 5 毫秒。
    OffsetUp,
    /// 音频偏移减 5 毫秒。
    OffsetDown,
    /// 以新 seed 重载当前谱面。
    RerollSeed,
    /// 切换 BME/PMS 布局。
    ToggleLayout,
    /// 开关最近文件菜单。
    ToggleRecent,
    /// 开关警告面板。
    ToggleWarnings,
    /// 开关调试信息。
    ToggleDebug,
    /// 开关"更多"操作面板。
    ToggleMore,
}

/// 执行动作所需的全部可变上下文。
pub struct ActionContext<'a> {
    /// 会话状态。
    pub session: &'a Session,
    /// 播放状态。
    pub playback: &'a mut Playback,
    /// 界面状态。
    pub ui: &'a mut UiState,
    /// 持久化配置。
    pub config: &'a mut ConfigStore,
    /// 文件选择器。
    pub picker: &'a mut Picker,
    /// 加载 worker。
    pub worker: &'a mut LoadWorker,
    /// 音频后端。
    pub audio: &'a mut AudioBackend,
    /// 采样率。
    pub sample_rate: u32,
}

/// 执行一个用户动作；`window` 单独传入，避免长借窗口查询。
/// 返回是否请求退出程序。
pub fn run_action(action: UiAction, ctx: &mut ActionContext, window: &mut Window) -> bool {
    let clock = ctx
        .audio
        .clock_seconds()
        .unwrap_or_else(|| ctx.playback.wall_seconds());
    let duration = ctx
        .session
        .loaded
        .as_ref()
        .map_or(Duration::ZERO, LoadedChart::duration);

    match action {
        UiAction::OpenFile => {
            ctx.picker.open(ctx.config.config.last_dir.clone());
            ctx.ui.recent_open = false;
            ctx.ui.more_open = false;
        }
        UiAction::TogglePause => {
            if ctx.session.loaded.is_some() && !ctx.playback.finished {
                if ctx.playback.playing {
                    ctx.playback.pause(clock);
                    ctx.audio.pause_all();
                } else {
                    ctx.playback.resume(clock);
                    ctx.audio.resume_all();
                }
            }
        }
        UiAction::Restart => {
            if ctx.session.loaded.is_some() {
                ctx.playback.request_seek(Duration::ZERO);
                ctx.playback.playing = true;
                ctx.playback.finished = false;
                ctx.audio.stop_all();
            }
        }
        UiAction::SeekBack => {
            let target = ctx
                .playback
                .chart_time
                .saturating_sub(Duration::from_secs(5));
            ctx.playback.request_seek(target);
            ctx.audio.stop_all();
        }
        UiAction::SeekForward => {
            let target = (ctx.playback.chart_time + Duration::from_secs(5)).min(duration);
            ctx.playback.request_seek(target);
            ctx.audio.stop_all();
        }
        UiAction::SeekStart => {
            ctx.playback.request_seek(Duration::ZERO);
            ctx.audio.stop_all();
        }
        UiAction::SeekEnd => {
            ctx.playback.request_seek(duration);
            ctx.audio.stop_all();
        }
        UiAction::ToggleFullscreen => {
            window.mode = if matches!(window.mode, WindowMode::Windowed) {
                WindowMode::BorderlessFullscreen(MonitorSelection::Current)
            } else {
                WindowMode::Windowed
            };
        }
        UiAction::Quit => {
            return true;
        }
        UiAction::ToggleMute => {
            ctx.playback.muted = !ctx.playback.muted;
            ctx.audio.set_volume(ctx.playback.effective_volume());
            ctx.config.mark_dirty();
        }
        UiAction::SpeedUp => adjust_speed(ctx, clock, 0.05),
        UiAction::SpeedDown => adjust_speed(ctx, clock, -0.05),
        UiAction::VolumeUp => adjust_volume(ctx, 0.05),
        UiAction::VolumeDown => adjust_volume(ctx, -0.05),
        UiAction::OffsetUp => adjust_offset(ctx, 5),
        UiAction::OffsetDown => adjust_offset(ctx, -5),
        UiAction::RerollSeed => {
            let Some(loaded) = ctx.session.loaded.as_ref() else {
                return false;
            };
            let path = loaded.path.clone();
            let mode = match loaded.mode {
                ModeKind::Bme => ModeChoice::Bme,
                ModeKind::Pms => ModeChoice::Pms,
                ModeKind::Bmson => ModeChoice::Auto,
            };
            begin_load(
                ctx.worker,
                ctx.config,
                ctx.ui,
                ctx.playback,
                ctx.audio,
                path,
                LoadOptions {
                    seed: random_seed(),
                    mode,
                },
                ctx.sample_rate,
            );
        }
        UiAction::ToggleLayout => {
            let Some(loaded) = ctx.session.loaded.as_ref() else {
                return false;
            };
            if loaded.mode == ModeKind::Bmson {
                return false;
            }
            let path = loaded.path.clone();
            let seed = loaded.seed;
            let mode = if loaded.mode == ModeKind::Pms {
                ModeChoice::Bme
            } else {
                ModeChoice::Pms
            };
            begin_load(
                ctx.worker,
                ctx.config,
                ctx.ui,
                ctx.playback,
                ctx.audio,
                path,
                LoadOptions { seed, mode },
                ctx.sample_rate,
            );
        }
        UiAction::ToggleRecent => {
            ctx.ui.more_open = false;
            ctx.ui.recent_open = !ctx.ui.recent_open;
            ctx.ui.recent_index = 0;
        }
        UiAction::ToggleWarnings => {
            ctx.ui.show_warnings = !ctx.ui.show_warnings;
        }
        UiAction::ToggleDebug => {
            ctx.ui.debug = !ctx.ui.debug;
        }
        UiAction::ToggleMore => {
            ctx.ui.recent_open = false;
            ctx.ui.more_open = !ctx.ui.more_open;
        }
    }

    false
}

fn adjust_speed(ctx: &mut ActionContext, clock: f64, delta: f64) {
    let speed = (ctx.playback.speed + delta).clamp(0.25, 2.0);
    if (speed - ctx.playback.speed).abs() > f64::EPSILON {
        ctx.playback.rebase(clock);
        ctx.playback.speed = speed;
        ctx.audio.set_speed(speed);
        ctx.config.config.speed = speed;
        ctx.config.mark_dirty();
    }
}

fn adjust_volume(ctx: &mut ActionContext, delta: f32) {
    let volume = (ctx.playback.volume + delta).clamp(0.0, 1.0);
    if (volume - ctx.playback.volume).abs() > f32::EPSILON {
        ctx.playback.volume = volume;
        ctx.audio.set_volume(ctx.playback.effective_volume());
        ctx.config.config.volume = volume;
        ctx.config.mark_dirty();
    }
}

fn adjust_offset(ctx: &mut ActionContext, delta: i32) {
    let offset = (ctx.playback.offset_ms + delta).clamp(-500, 500);
    if offset != ctx.playback.offset_ms {
        ctx.playback.offset_ms = offset;
        ctx.config.config.offset_ms = offset;
        ctx.config.mark_dirty();
    }
}
