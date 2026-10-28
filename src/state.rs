//! 应用状态与资源。

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::prelude::Resource;

use crate::bga::BgaState;
use crate::chart::{LoadOptions, LoadedChart};
use crate::layout::TrackLayout;
use crate::video::VideoState;

/// 应用阶段。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    /// 没有谱面。
    #[default]
    Empty,
    /// 正在加载。
    Loading,
    /// 已就绪。
    Ready,
    /// 加载失败。
    Error,
}

/// 当前谱面会话。
#[derive(Resource, Default)]
pub struct Session {
    /// 已加载谱面。
    pub loaded: Option<LoadedChart>,
    /// 轨道表。
    pub layout: TrackLayout,
    /// BGA 状态。
    pub bga: BgaState,
    /// 视频状态。
    pub video: Option<VideoState>,
}

impl Session {
    /// 清空会话。
    pub fn clear(&mut self) {
        self.loaded = None;
        self.layout = TrackLayout::default();
        self.bga.clear();
        self.video = None;
    }
}

/// 播放状态。
#[derive(Resource)]
pub struct Playback {
    /// 是否正在播放。
    pub playing: bool,
    /// 倍速。
    pub speed: f64,
    /// 主音量。
    pub volume: f32,
    /// 静音。
    pub muted: bool,
    /// 音频偏移（毫秒）。
    pub offset_ms: i32,
    /// 当前谱面时间。
    pub chart_time: Duration,
    /// 倍速基准谱面时间。
    pub base_chart: Duration,
    /// 倍速基准音频时钟。
    pub base_clock: f64,
    /// 已触发的最后一个音频脉冲。
    pub last_audio_tick: Option<u64>,
    /// 待处理的跳转。
    pub seek_request: Option<Duration>,
    /// 跳转序号；视频与 BGA 用它检测需要重建。
    pub seek_serial: u64,
    /// 是否已播放结束。
    pub finished: bool,
    /// 墙钟起点（音频不可用时使用）。
    pub wall_start: Instant,
    /// 音频时钟是否可用。
    pub audio_available: bool,
}

impl Playback {
    /// 从配置创建。
    #[must_use]
    pub fn new(volume: f32, speed: f64, offset_ms: i32, muted: bool) -> Self {
        Self {
            playing: false,
            speed,
            volume,
            muted,
            offset_ms,
            chart_time: Duration::ZERO,
            base_chart: Duration::ZERO,
            base_clock: 0.0,
            last_audio_tick: None,
            seek_request: None,
            seek_serial: 0,
            finished: false,
            wall_start: Instant::now(),
            audio_available: false,
        }
    }

    /// 墙钟秒数。
    #[must_use]
    pub fn wall_seconds(&self) -> f64 {
        self.wall_start.elapsed().as_secs_f64()
    }

    /// 推进到新的时钟值。
    pub fn advance_to(&mut self, clock: f64) {
        if !self.playing || self.finished {
            return;
        }
        let elapsed = (clock - self.base_clock).max(0.0);
        let seconds = self.base_chart.as_secs_f64() + elapsed * self.speed;
        self.chart_time = Duration::from_secs_f64(seconds.max(0.0));
    }

    /// 以当前时间重置倍速基准。
    pub fn rebase(&mut self, clock: f64) {
        self.base_chart = self.chart_time;
        self.base_clock = clock;
    }

    /// 暂停。
    pub fn pause(&mut self, clock: f64) {
        self.advance_to(clock);
        self.rebase(clock);
        self.playing = false;
    }

    /// 继续。
    pub fn resume(&mut self, clock: f64) {
        self.base_clock = clock;
        self.playing = true;
        self.finished = false;
    }

    /// 请求跳转。
    pub fn request_seek(&mut self, target: Duration) {
        self.seek_request = Some(target.max(Duration::ZERO));
    }

    /// 应用跳转，返回跳转后的谱面时间。
    pub fn take_seek(&mut self, clock: f64) -> Option<Duration> {
        let target = self.seek_request.take()?;
        self.chart_time = target;
        self.base_chart = target;
        self.base_clock = clock;
        self.last_audio_tick = None;
        self.finished = false;
        self.seek_serial = self.seek_serial.wrapping_add(1);
        Some(target)
    }

    /// 重置到开头。
    pub fn reset(&mut self, clock: f64) {
        self.chart_time = Duration::ZERO;
        self.base_chart = Duration::ZERO;
        self.base_clock = clock;
        self.last_audio_tick = None;
        self.finished = false;
        self.playing = true;
        self.seek_serial = self.seek_serial.wrapping_add(1);
    }

    /// 当前有效音量。
    #[must_use]
    pub fn effective_volume(&self) -> f32 {
        if self.muted { 0.0 } else { self.volume }
    }
}

impl Default for Playback {
    fn default() -> Self {
        Self::new(0.8, 1.0, 0, false)
    }
}

/// UI 状态。
#[derive(Resource)]
pub struct UiState {
    /// 应用阶段。
    pub phase: Phase,
    /// 加载阶段文字。
    pub loading_message: String,
    /// 加载错误。
    pub error: Option<String>,
    /// 警告列表。
    pub warnings: Vec<String>,
    /// 是否显示警告。
    pub show_warnings: bool,
    /// 最近文件菜单是否打开。
    pub recent_open: bool,
    /// 最近文件选中索引。
    pub recent_index: usize,
    /// 是否显示调试信息。
    pub debug: bool,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            phase: Phase::Empty,
            loading_message: String::new(),
            error: None,
            warnings: Vec::new(),
            show_warnings: false,
            recent_open: false,
            recent_index: 0,
            debug: false,
        }
    }
}

/// 启动参数。
#[derive(Resource, Clone, Debug)]
pub struct StartupOptions {
    /// 初始谱面。
    pub chart: Option<PathBuf>,
    /// 初始加载参数。
    pub load: LoadOptions,
    /// 初始音频偏移。
    pub offset_ms: i32,
    /// 初始音量。
    pub volume: f32,
    /// 初始倍速。
    pub speed: f64,
    /// 初始静音。
    pub muted: bool,
}

impl Default for StartupOptions {
    fn default() -> Self {
        Self {
            chart: None,
            load: LoadOptions::default(),
            offset_ms: 0,
            volume: 0.8,
            speed: 1.0,
            muted: false,
        }
    }
}

/// 音频采样率。
#[derive(Resource, Clone, Copy, Debug)]
pub struct SampleRate(pub u32);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_advances_with_speed() {
        let mut playback = Playback::new(1.0, 2.0, 0, false);
        playback.playing = true;
        playback.base_clock = 10.0;
        playback.advance_to(11.0);
        assert!((playback.chart_time.as_secs_f64() - 2.0).abs() < 1e-9);
    }

    #[test]
    fn seek_increments_serial_once() {
        let mut playback = Playback::new(1.0, 1.0, 0, false);
        playback.request_seek(Duration::from_secs(3));
        let target = playback.take_seek(5.0);
        assert_eq!(target, Some(Duration::from_secs(3)));
        assert_eq!(playback.seek_serial, 1);
        assert_eq!(playback.take_seek(5.0), None);
    }
}
