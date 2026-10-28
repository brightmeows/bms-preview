//! App 组装、加载 worker、输入与时间线。

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use bevy::asset::Assets;
use bevy::image::Image;
use bevy::prelude::*;
use bevy::window::{FileDragAndDrop, MonitorSelection, PrimaryWindow, WindowMode};
use bmsrs::chart::EventKind;
use bmsrs::player::Player;

use crate::audio::{AudioBackend, SlicedSample, decode_audio_assets};
use crate::bga::{DecodedImage, decode_bga_images};
use crate::chart::{
    LoadOptions, LoadedChart, ModeChoice, ModeKind, PreviewEvent, load_chart, random_seed,
};
use crate::config::ConfigStore;
use crate::font::load_ui_font;
use crate::layout::infer_layout;
use crate::picker::Picker;
use crate::render::{ChartEntity, despawn_chart, setup_scene, spawn_chart};
use crate::state::{Phase, Playback, SampleRate, Session, StartupOptions, UiState};
use crate::ui::{Fonts, spawn_hud};
use crate::video::VideoState;

/// 加载 worker 消息。
enum LoadMessage {
    /// 进度。
    Progress {
        /// 代号。
        serial: u64,
        /// 文本。
        message: String,
    },
    /// 完成。
    Done {
        /// 代号。
        serial: u64,
        /// 结果。
        result: Box<LoadResult>,
    },
    /// 失败。
    Error {
        /// 代号。
        serial: u64,
        /// 错误文本。
        message: String,
    },
}

impl LoadMessage {
    const fn serial(&self) -> u64 {
        match self {
            Self::Progress { serial, .. }
            | Self::Done { serial, .. }
            | Self::Error { serial, .. } => *serial,
        }
    }
}

/// 加载结果。
pub struct LoadResult {
    /// 谱面。
    pub chart: LoadedChart,
    /// 音频采样。
    pub audio: Vec<Option<SlicedSample>>,
    /// BGA 图片。
    pub bga: Vec<Option<DecodedImage>>,
}

/// 后台加载 worker。
#[derive(Resource)]
pub struct LoadWorker {
    receiver: Mutex<Receiver<LoadMessage>>,
    serial: u64,
    active: bool,
}

impl LoadWorker {
    /// 创建空闲 worker。
    #[must_use]
    pub fn new() -> Self {
        let (_sender, receiver) = std::sync::mpsc::channel();
        Self {
            receiver: Mutex::new(receiver),
            serial: 0,
            active: false,
        }
    }

    /// 启动加载。
    fn start(&mut self, path: PathBuf, options: LoadOptions, sample_rate: u32) {
        self.serial = self.serial.wrapping_add(1);
        let serial = self.serial;
        self.active = true;

        let (sender, receiver) = std::sync::mpsc::channel();
        if let Ok(mut guard) = self.receiver.lock() {
            *guard = receiver;
        }

        let spawn = std::thread::Builder::new()
            .name("bms-preview-loader".to_owned())
            .spawn(move || {
                let progress = |message: &str| {
                    let _ = sender.send(LoadMessage::Progress {
                        serial,
                        message: message.to_owned(),
                    });
                };
                progress("读取谱面...");
                let mut chart = match load_chart(&path, options) {
                    Ok(chart) => chart,
                    Err(error) => {
                        let _ = sender.send(LoadMessage::Error {
                            serial,
                            message: error.to_string(),
                        });
                        return;
                    }
                };

                progress("解码 BGA 图片...");
                let (bga, bga_warnings) = decode_bga_images(&chart.bga_resources, &chart.base_dir);
                bevy::log::info!("BGA 图片解码完成: {} 张", bga.iter().flatten().count());
                chart.warnings.extend(bga_warnings);

                progress("解码音频...");
                let (audio, audio_warnings) =
                    decode_audio_assets(&chart.audio_assets, &chart.base_dir, sample_rate);
                chart.warnings.extend(audio_warnings);

                let _ = sender.send(LoadMessage::Done {
                    serial,
                    result: Box::new(LoadResult { chart, audio, bga }),
                });
            });

        if let Err(error) = spawn {
            self.active = false;
            bevy::log::warn!("无法启动加载线程: {error}");
        }
    }

    /// 轮询一条消息。
    fn poll(&mut self) -> Option<LoadMessage> {
        if !self.active {
            return None;
        }
        let receiver = self.receiver.lock().ok()?;
        match receiver.try_recv() {
            Ok(message) => {
                if message.serial() != self.serial {
                    return None;
                }
                if !matches!(message, LoadMessage::Progress { .. }) {
                    self.active = false;
                }
                Some(message)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.active = false;
                None
            }
        }
    }
}

impl Default for LoadWorker {
    fn default() -> Self {
        Self::new()
    }
}

/// 启动系统。
#[expect(
    clippy::too_many_arguments,
    reason = "Bevy startup system needs many resources"
)]
pub fn startup(
    mut commands: Commands,
    mut fonts_assets: ResMut<Assets<bevy::text::Font>>,
    mut config: ResMut<ConfigStore>,
    mut playback: ResMut<Playback>,
    mut ui: ResMut<UiState>,
    mut worker: ResMut<LoadWorker>,
    mut picker: ResMut<Picker>,
    startup: Res<StartupOptions>,
    sample_rate: Res<SampleRate>,
    mut audio: NonSendMut<AudioBackend>,
) {
    setup_scene(&mut commands);
    let font = load_ui_font(&mut fonts_assets);
    let fonts = Fonts { ui: font };
    spawn_hud(&mut commands, &fonts);
    commands.insert_resource(fonts);
    let bga_sprites = crate::bga::spawn_sprites(&mut commands);
    commands.insert_resource(bga_sprites);
    let video_sprite = crate::video::spawn_sprite(&mut commands);
    commands.insert_resource(video_sprite);

    playback.volume = startup.volume;
    playback.speed = startup.speed;
    playback.offset_ms = startup.offset_ms;
    playback.muted = startup.muted;
    playback.audio_available = audio.clock_seconds().is_some();
    bevy::log::info!(
        "音频后端: sample_rate={} clock={:?} error={:?}",
        audio.sample_rate(),
        audio.clock_seconds(),
        audio.error()
    );
    audio.set_volume(playback.effective_volume());
    audio.set_speed(playback.speed);

    if let Some(path) = startup.chart.clone() {
        begin_load(
            &mut *worker,
            &mut *config,
            &mut *ui,
            &mut *playback,
            &mut *audio,
            path,
            startup.load,
            sample_rate.0,
        );
    } else {
        picker.open(config.config.last_dir.clone());
        ui.phase = Phase::Empty;
    }
}

/// 输入系统。
#[expect(
    clippy::too_many_arguments,
    reason = "Bevy input system touches many resources"
)]
pub fn input_system(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    session: Res<Session>,
    mut playback: ResMut<Playback>,
    mut ui: ResMut<UiState>,
    mut config: ResMut<ConfigStore>,
    mut picker: ResMut<Picker>,
    mut worker: ResMut<LoadWorker>,
    sample_rate: Res<SampleRate>,
    mut audio: NonSendMut<AudioBackend>,
    mut drag: MessageReader<FileDragAndDrop>,
    mut exit: MessageWriter<AppExit>,
) {
    let clock = audio
        .clock_seconds()
        .unwrap_or_else(|| playback.wall_seconds());

    for event in drag.read() {
        if let FileDragAndDrop::DroppedFile { path_buf, .. } = event
            && supported_chart(path_buf)
        {
            begin_load(
                &mut *worker,
                &mut *config,
                &mut *ui,
                &mut *playback,
                &mut *audio,
                path_buf.clone(),
                LoadOptions {
                    seed: random_seed(),
                    mode: ModeChoice::Auto,
                },
                sample_rate.0,
            );
        }
    }

    if ui.recent_open {
        let count = config.config.recent.len();
        if count > 0 {
            if keys.just_pressed(KeyCode::ArrowDown) {
                ui.recent_index = (ui.recent_index + 1) % count;
            }
            if keys.just_pressed(KeyCode::ArrowUp) {
                ui.recent_index = ui.recent_index.saturating_sub(1).min(count - 1);
            }
            if keys.just_pressed(KeyCode::Enter)
                && let Some(path) = config.config.recent.get(ui.recent_index).cloned()
            {
                begin_load(
                    &mut *worker,
                    &mut *config,
                    &mut *ui,
                    &mut *playback,
                    &mut *audio,
                    path,
                    LoadOptions {
                        seed: random_seed(),
                        mode: ModeChoice::Auto,
                    },
                    sample_rate.0,
                );
            }
        }
        if keys.just_pressed(KeyCode::Escape) {
            ui.recent_open = false;
        }
        return;
    }

    if keys.just_pressed(KeyCode::KeyO) {
        picker.open(config.config.last_dir.clone());
        ui.recent_open = false;
    }
    if keys.just_pressed(KeyCode::Tab) {
        ui.recent_open = true;
        ui.recent_index = 0;
        ui.recent_index = ui
            .recent_index
            .min(config.config.recent.len().saturating_sub(1));
    }
    if keys.just_pressed(KeyCode::KeyW) {
        ui.show_warnings = !ui.show_warnings;
    }
    if keys.just_pressed(KeyCode::F3) {
        ui.debug = !ui.debug;
    }
    if keys.just_pressed(KeyCode::KeyF)
        && let Ok(mut window) = windows.single_mut()
    {
        window.mode = if matches!(window.mode, WindowMode::Windowed) {
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        } else {
            WindowMode::Windowed
        };
    }
    if keys.just_pressed(KeyCode::KeyQ) {
        exit.write(AppExit::Success);
    }
    if keys.just_pressed(KeyCode::Escape) {
        ui.show_warnings = false;
    }

    let duration = session
        .loaded
        .as_ref()
        .map_or(Duration::ZERO, LoadedChart::duration);

    if keys.just_pressed(KeyCode::Space) && session.loaded.is_some() && !playback.finished {
        if playback.playing {
            playback.pause(clock);
            audio.pause_all();
        } else {
            playback.resume(clock);
            audio.resume_all();
        }
    }
    if keys.just_pressed(KeyCode::KeyR) && session.loaded.is_some() {
        playback.request_seek(Duration::ZERO);
        playback.playing = true;
        playback.finished = false;
        audio.stop_all();
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        let target = playback.chart_time.saturating_sub(Duration::from_secs(5));
        playback.request_seek(target);
        audio.stop_all();
    }
    if keys.just_pressed(KeyCode::ArrowRight) {
        let target = (playback.chart_time + Duration::from_secs(5)).min(duration);
        playback.request_seek(target);
        audio.stop_all();
    }
    if keys.just_pressed(KeyCode::Home) {
        playback.request_seek(Duration::ZERO);
        audio.stop_all();
    }
    if keys.just_pressed(KeyCode::End) {
        playback.request_seek(duration);
        audio.stop_all();
    }

    let mut speed = playback.speed;
    if keys.just_pressed(KeyCode::ArrowUp) {
        speed += 0.05;
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        speed -= 0.05;
    }
    speed = speed.clamp(0.25, 2.0);
    if (speed - playback.speed).abs() > f64::EPSILON {
        playback.rebase(clock);
        playback.speed = speed;
        audio.set_speed(speed);
        config.config.speed = speed;
        config.mark_dirty();
    }

    let mut volume = playback.volume;
    if keys.just_pressed(KeyCode::Equal) || keys.just_pressed(KeyCode::NumpadAdd) {
        volume += 0.05;
    }
    if keys.just_pressed(KeyCode::Minus) || keys.just_pressed(KeyCode::NumpadSubtract) {
        volume -= 0.05;
    }
    volume = volume.clamp(0.0, 1.0);
    if (volume - playback.volume).abs() > f32::EPSILON {
        playback.volume = volume;
        audio.set_volume(playback.effective_volume());
        config.config.volume = volume;
        config.mark_dirty();
    }
    if keys.just_pressed(KeyCode::KeyM) {
        playback.muted = !playback.muted;
        audio.set_volume(playback.effective_volume());
        config.mark_dirty();
    }
    let mut offset = playback.offset_ms;
    if keys.just_pressed(KeyCode::Period) {
        offset += 5;
    }
    if keys.just_pressed(KeyCode::Comma) {
        offset -= 5;
    }
    offset = offset.clamp(-500, 500);
    if offset != playback.offset_ms {
        playback.offset_ms = offset;
        config.config.offset_ms = offset;
        config.mark_dirty();
    }

    if keys.just_pressed(KeyCode::KeyN)
        && let Some(loaded) = session.loaded.as_ref()
    {
        let path = loaded.path.clone();
        let mode = match loaded.mode {
            ModeKind::Bme => ModeChoice::Bme,
            ModeKind::Pms => ModeChoice::Pms,
            ModeKind::Bmson => ModeChoice::Auto,
        };
        begin_load(
            &mut *worker,
            &mut *config,
            &mut *ui,
            &mut *playback,
            &mut *audio,
            path,
            LoadOptions {
                seed: random_seed(),
                mode,
            },
            sample_rate.0,
        );
    }
    if keys.just_pressed(KeyCode::KeyL)
        && let Some(loaded) = session.loaded.as_ref()
        && loaded.mode != ModeKind::Bmson
    {
        let path = loaded.path.clone();
        let mode = if loaded.mode == ModeKind::Pms {
            ModeChoice::Bme
        } else {
            ModeChoice::Pms
        };
        begin_load(
            &mut *worker,
            &mut *config,
            &mut *ui,
            &mut *playback,
            &mut *audio,
            path,
            LoadOptions {
                seed: loaded.seed,
                mode,
            },
            sample_rate.0,
        );
    }

    if mouse.just_pressed(MouseButton::Left)
        && !ui.recent_open
        && session.loaded.is_some()
        && let Ok(window) = windows.single()
        && let Some(cursor) = window.cursor_position()
        && cursor.y > window.height() - 24.0
    {
        let fraction = f64::from((cursor.x / window.width()).clamp(0.0, 1.0));
        let target = duration.mul_f64(fraction);
        playback.request_seek(target);
        audio.stop_all();
    }
}

/// 轮询文件选择器与加载 worker。
#[expect(
    clippy::too_many_arguments,
    reason = "Bevy system needs loader, session, UI, audio and assets"
)]
pub fn worker_system(
    mut commands: Commands,
    mut session: ResMut<Session>,
    mut ui: ResMut<UiState>,
    mut playback: ResMut<Playback>,
    mut config: ResMut<ConfigStore>,
    mut picker: ResMut<Picker>,
    mut worker: ResMut<LoadWorker>,
    mut audio: NonSendMut<AudioBackend>,
    mut images: ResMut<Assets<Image>>,
    fonts: Res<Fonts>,
    chart_entities: Query<Entity, With<ChartEntity>>,
    sample_rate: Res<SampleRate>,
) {
    if let Some(result) = picker.poll() {
        match result {
            Some(path) => begin_load(
                &mut *worker,
                &mut *config,
                &mut *ui,
                &mut *playback,
                &mut *audio,
                path,
                LoadOptions {
                    seed: random_seed(),
                    mode: ModeChoice::Auto,
                },
                sample_rate.0,
            ),
            None if ui.phase == Phase::Empty => {
                ui.loading_message = "未选择谱面，按 O 打开".to_owned();
            }
            None => {}
        }
    }

    if let Some(message) = worker.poll() {
        match message {
            LoadMessage::Progress { message, .. } => {
                ui.loading_message = message;
            }
            LoadMessage::Error { message, .. } => {
                bevy::log::warn!("谱面加载失败: {message}");
                ui.phase = Phase::Error;
                ui.error = Some(message);
                ui.loading_message.clear();
                playback.playing = false;
                audio.stop_all();
            }
            LoadMessage::Done { result, .. } => {
                apply_load(
                    &mut commands,
                    &mut *session,
                    &mut *ui,
                    &mut *playback,
                    &mut *config,
                    &mut *audio,
                    &mut *images,
                    &*fonts,
                    &chart_entities,
                    *result,
                );
            }
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "load application needs all session resources"
)]
fn apply_load(
    commands: &mut Commands,
    session: &mut Session,
    ui: &mut UiState,
    playback: &mut Playback,
    config: &mut ConfigStore,
    audio: &mut AudioBackend,
    images: &mut Assets<Image>,
    fonts: &Fonts,
    chart_entities: &Query<Entity, With<ChartEntity>>,
    result: LoadResult,
) {
    let LoadResult {
        mut chart,
        audio: samples,
        bga,
    } = result;
    let path = chart.path.clone();
    let layout = infer_layout(chart.player.chart());
    let video = chart
        .video
        .as_ref()
        .map(|source| VideoState::start(source, &chart.player));

    bevy::log::info!(
        "谱面加载完成: {} / {} | notes={} bga={} audio={} video={}",
        chart.title(),
        chart.artist(),
        chart.stats.playable,
        chart.bga_resources.len(),
        chart.audio_assets.len(),
        chart.video.is_some(),
    );
    audio.set_samples(samples);
    audio.set_volume(playback.effective_volume());
    audio.set_speed(playback.speed);

    session.clear();
    session.bga.install(images, bga, &chart.bga_resources);
    session.layout = layout;
    ui.warnings = std::mem::take(&mut chart.warnings);
    for warning in &ui.warnings {
        bevy::log::warn!("{warning}");
    }
    ui.show_warnings = !ui.warnings.is_empty();
    session.loaded = Some(chart);
    session.video = video;

    despawn_chart(commands, chart_entities);
    spawn_chart(commands, session, fonts);

    let clock = audio
        .clock_seconds()
        .unwrap_or_else(|| playback.wall_seconds());
    playback.reset(clock);
    playback.audio_available = audio.clock_seconds().is_some();
    audio.stop_all();

    ui.phase = Phase::Ready;
    ui.loading_message.clear();
    ui.error = None;
    ui.recent_open = false;
    ui.recent_index = 0;

    config.config.add_recent(&path);
    if let Some(parent) = path.parent() {
        config.config.last_dir = Some(parent.to_path_buf());
    }
    config.mark_dirty();
}

fn begin_load(
    worker: &mut LoadWorker,
    config: &mut ConfigStore,
    ui: &mut UiState,
    playback: &mut Playback,
    audio: &mut AudioBackend,
    path: PathBuf,
    options: LoadOptions,
    sample_rate: u32,
) {
    if let Some(parent) = path.parent() {
        config.config.last_dir = Some(parent.to_path_buf());
        config.mark_dirty();
    }
    bevy::log::info!("开始加载谱面: {}", path.display());
    ui.phase = Phase::Loading;
    ui.loading_message = "读取谱面...".to_owned();
    ui.error = None;
    ui.recent_open = false;
    playback.playing = false;
    audio.stop_all();
    worker.start(path, options, sample_rate);
}

/// 时间线系统。
pub fn clock_system(
    mut session: ResMut<Session>,
    mut playback: ResMut<Playback>,
    mut audio: NonSendMut<AudioBackend>,
    mut ui: ResMut<UiState>,
) {
    audio.update();
    let audio_clock = audio.clock_seconds();
    playback.audio_available = audio_clock.is_some();
    if let Some(error) = audio.error()
        && !ui.warnings.iter().any(|warning| warning.contains("音频"))
    {
        ui.warnings.push(format!("音频不可用: {error}"));
        ui.show_warnings = true;
    }
    let clock = audio_clock.unwrap_or_else(|| playback.wall_seconds());

    let Session { loaded, bga, .. } = &mut *session;
    let Some(loaded) = loaded.as_mut() else {
        return;
    };
    let player = &mut loaded.player;
    let duration = player.duration();
    let offset_ms = playback.offset_ms;

    if let Some(target) = playback.take_seek(clock) {
        let target = target.min(duration);
        player.seek(apply_offset(target, offset_ms));
        audio.stop_all();
        replay_bgm(&mut audio, player, target);
        bga.rebuild(player, player.duration_to_tick(target));
        playback.last_audio_tick = Some(player.duration_to_tick(target).saturating_sub(1));
        if !playback.playing {
            audio.pause_all();
        }
    }

    if playback.finished {
        player.seek(apply_offset(duration, offset_ms));
        return;
    }

    if !playback.playing {
        player.seek(apply_offset(playback.chart_time, offset_ms));
        return;
    }

    playback.advance_to(clock);
    if playback.chart_time >= duration {
        playback.chart_time = duration;
        playback.finished = true;
        playback.playing = false;
        audio.stop_all();
        player.seek(apply_offset(duration, offset_ms));
        return;
    }

    player.seek(apply_offset(playback.chart_time, offset_ms));
    let current_tick = player.duration_to_tick(playback.chart_time);
    let start_tick = playback
        .last_audio_tick
        .map_or(0, |tick| tick.saturating_add(1));
    if current_tick >= start_tick {
        for event in player.events_in_range(start_tick..=current_tick) {
            if let Some(index) = audio_index(&event.kind) {
                let _ = audio.play(index, 0.0, false);
            }
        }
        bga.apply_range(player, start_tick, current_tick);
    }
    playback.last_audio_tick = Some(current_tick);
}

fn replay_bgm(audio: &mut AudioBackend, player: &Player<(), PreviewEvent>, target: Duration) {
    let target_tick = player.duration_to_tick(target);
    for event in player.events_in_range(0..=target_tick) {
        let EventKind::Bgm { audio_index } = &event.kind else {
            continue;
        };
        let event_time = player.tick_to_duration(event.tick());
        let Some(sample_duration) = audio.sample_duration(*audio_index) else {
            continue;
        };
        let event_end = event_time.as_secs_f64() + sample_duration;
        if event_end > target.as_secs_f64() && target >= event_time {
            let offset = target.as_secs_f64() - event_time.as_secs_f64();
            let _ = audio.play(*audio_index, offset, false);
        }
    }
}

const fn audio_index(kind: &EventKind<(), PreviewEvent>) -> Option<u32> {
    match kind {
        EventKind::Note {
            audio_index: Some(index),
            ..
        } => Some(*index),
        EventKind::Bgm { audio_index } => Some(*audio_index),
        _ => None,
    }
}

fn apply_offset(time: Duration, offset_ms: i32) -> Duration {
    if offset_ms >= 0 {
        time.saturating_add(Duration::from_millis(u64::from(offset_ms.unsigned_abs())))
    } else {
        time.saturating_sub(Duration::from_millis(u64::from(offset_ms.unsigned_abs())))
    }
}

/// 持久化系统。
pub fn persist_system(
    mut config: ResMut<ConfigStore>,
    time: Res<Time>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut exit: MessageReader<AppExit>,
) {
    if exit.read().next().is_some() {
        if let Ok(window) = windows.single() {
            config.config.window_width = window.width();
            config.config.window_height = window.height();
        }
        config.dirty = true;
        config.save();
    }
    config.tick(time.delta_secs());
}

fn supported_chart(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "bms" | "bme" | "bml" | "pms" | "bmson"
            )
        })
}
