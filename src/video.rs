//! video-rs 解码 worker 与视频精灵同步。

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, SyncSender};
use std::time::Duration;

use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::image::Image;
use bevy::prelude::{Entity, Query, Res, ResMut, Sprite, Transform, Visibility};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bmsrs::chart::EventKind;
use bmsrs::player::Player;
use video_rs::{Decoder, Error as VideoError};

use crate::chart::{PreviewEvent, VideoSource};
use crate::layout::{VIEW_HEIGHT, VIEW_WIDTH};
use crate::state::{Playback, Session};

/// 视频解码命令。
#[derive(Clone, Copy, Debug)]
pub enum VideoCommand {
    /// 跳转到指定秒数。
    Seek {
        /// 代号；主线程丢弃旧帧。
        generation: u64,
        /// 目标秒数。
        seconds: f64,
    },
    /// 停止 worker。
    Stop,
}

/// 视频帧事件。
pub enum VideoEvent {
    /// 解码出一帧 RGBA8。
    Frame {
        /// 代号。
        generation: u64,
        /// 源时间戳（秒）。
        pts: f64,
        /// 宽度。
        width: u32,
        /// 高度。
        height: u32,
        /// RGBA8 数据。
        data: Vec<u8>,
    },
    /// 到达文件末尾。
    Ended {
        /// 代号。
        generation: u64,
        /// 最后一帧时间戳（秒）。
        last_pts: f64,
    },
    /// 解码错误。
    Error {
        /// 代号。
        generation: u64,
        /// 错误文本。
        message: String,
    },
}

/// 视频帧。
pub struct VideoFrame {
    /// 源时间戳（秒）。
    pub pts: f64,
    /// 宽度。
    pub width: u32,
    /// 高度。
    pub height: u32,
    /// RGBA8 数据。
    pub data: Vec<u8>,
}

/// 视频状态。
pub struct VideoState {
    sender: std::sync::mpsc::Sender<VideoCommand>,
    receiver: Mutex<Receiver<VideoEvent>>,
    frames: VecDeque<VideoFrame>,
    current: Option<VideoFrame>,
    texture: Option<Handle<Image>>,
    generation: u64,
    delay_seconds: f64,
    seek_serial: u64,
    seek_points: Vec<(u64, u64)>,
    last_video_time: Option<f64>,
    loop_playback: bool,
    frame_serial: u64,
    uploaded_serial: u64,
    duration: Option<f64>,
}

impl VideoState {
    /// 启动视频 worker。
    #[must_use]
    pub fn start(source: &VideoSource, player: &Player<(), PreviewEvent>) -> Self {
        let (command_sender, command_receiver) = std::sync::mpsc::channel();
        let (event_sender, event_receiver) = std::sync::mpsc::sync_channel(16);
        let generation = 1;
        let loop_playback = source.loop_playback;
        let fps = source.fps.unwrap_or(60.0).max(1.0);
        let delay_seconds = source
            .delay_frames
            .map_or(0.0, |frames| f64::from(frames) / fps);
        let worker_loop = loop_playback;
        let worker_path = source.path.clone();

        let spawn = std::thread::Builder::new()
            .name("bms-preview-video".to_owned())
            .spawn(move || {
                run_worker(
                    &worker_path,
                    worker_loop,
                    generation,
                    &command_receiver,
                    &event_sender,
                );
            });
        if let Err(error) = spawn {
            bevy::log::warn!("无法启动视频解码线程: {error}");
        }

        let seek_points = player
            .chart()
            .data
            .events
            .iter()
            .filter_map(|event| match &event.kind {
                EventKind::Custom(PreviewEvent::VideoSeek { position_ms }) => {
                    Some((event.tick(), *position_ms))
                }
                _ => None,
            })
            .collect();

        Self {
            sender: command_sender,
            receiver: Mutex::new(event_receiver),
            frames: VecDeque::new(),
            current: None,
            texture: None,
            generation,
            delay_seconds,
            seek_serial: 0,
            seek_points,
            last_video_time: None,
            loop_playback,
            frame_serial: 0,
            uploaded_serial: 0,
            duration: None,
        }
    }

    /// 当前帧。
    #[must_use]
    pub fn current(&self) -> Option<&VideoFrame> {
        self.current.as_ref()
    }

    /// 是否需要用当前帧更新纹理。
    #[must_use]
    pub const fn needs_upload(&self) -> bool {
        self.uploaded_serial != self.frame_serial
    }

    /// 标记当前帧已上传。
    pub const fn mark_uploaded(&mut self) {
        self.uploaded_serial = self.frame_serial;
    }

    /// 当前纹理句柄。
    #[must_use]
    pub fn texture(&self) -> Option<&Handle<Image>> {
        self.texture.as_ref()
    }

    /// 设置纹理句柄。
    pub fn set_texture(&mut self, texture: Handle<Image>) {
        self.texture = Some(texture);
    }

    /// 推进视频状态。
    pub fn update(
        &mut self,
        chart_time: Duration,
        player: &Player<(), PreviewEvent>,
        seek_serial: u64,
    ) {
        self.poll_events();

        if seek_serial != self.seek_serial {
            self.seek_serial = seek_serial;
            self.request_seek(self.video_time(chart_time, player));
        }

        let mut video_time = self.video_time(chart_time, player) - self.delay_seconds;
        if self.loop_playback
            && let Some(duration) = self.duration
            && duration > 0.0
        {
            video_time = video_time.rem_euclid(duration);
        }
        if let Some(last) = self.last_video_time
            && (video_time - last).abs() > 0.5
        {
            self.request_seek(video_time);
        }
        self.last_video_time = Some(video_time);

        while let Some(front) = self.frames.front() {
            if front.pts <= video_time + 0.10 {
                self.current = self.frames.pop_front();
                self.frame_serial = self.frame_serial.wrapping_add(1);
            } else {
                break;
            }
        }
    }

    fn poll_events(&mut self) {
        let events: Vec<VideoEvent> = {
            let Ok(receiver) = self.receiver.lock() else {
                return;
            };
            receiver.try_iter().collect()
        };
        let mut loop_requested = false;
        for event in events {
            match event {
                VideoEvent::Frame {
                    generation,
                    pts,
                    width,
                    height,
                    data,
                } => {
                    if generation == self.generation {
                        self.frames.push_back(VideoFrame {
                            pts,
                            width,
                            height,
                            data,
                        });
                        if self.frames.len() > 32 {
                            self.frames.pop_front();
                        }
                    }
                }
                VideoEvent::Ended {
                    generation,
                    last_pts,
                } => {
                    if generation == self.generation {
                        self.duration = Some(last_pts.max(0.01));
                        if self.loop_playback {
                            loop_requested = true;
                        }
                    }
                }
                VideoEvent::Error {
                    generation,
                    message,
                } => {
                    if generation == self.generation {
                        bevy::log::warn!("视频解码失败: {message}");
                    }
                }
            }
        }
        if loop_requested {
            self.request_seek(0.0);
        }
    }

    fn request_seek(&mut self, seconds: f64) {
        self.frames.clear();
        self.current = None;
        self.last_video_time = None;
        self.generation = self.generation.wrapping_add(1);
        let _ = self.sender.send(VideoCommand::Seek {
            generation: self.generation,
            seconds: seconds.max(0.0),
        });
    }

    fn video_time(&self, chart_time: Duration, player: &Player<(), PreviewEvent>) -> f64 {
        let tick = player.duration_to_tick(chart_time);
        let index = self
            .seek_points
            .partition_point(|(tick_at, _)| *tick_at <= tick);
        if index == 0 {
            return chart_time.as_secs_f64();
        }
        let (seek_tick, position_ms) = self.seek_points[index - 1];
        let seek_time = player.tick_to_duration(seek_tick).as_secs_f64();
        position_ms as f64 / 1000.0 + (chart_time.as_secs_f64() - seek_time)
    }
}

impl Drop for VideoState {
    fn drop(&mut self) {
        let _ = self.sender.send(VideoCommand::Stop);
    }
}

fn run_worker(
    path: &std::path::Path,
    loop_playback: bool,
    initial_generation: u64,
    commands: &Receiver<VideoCommand>,
    events: &SyncSender<VideoEvent>,
) {
    let mut decoder = match Decoder::new(path.to_path_buf()) {
        Ok(decoder) => decoder,
        Err(error) => {
            let _ = events.send(VideoEvent::Error {
                generation: initial_generation,
                message: format!("打开视频失败: {error}"),
            });
            return;
        }
    };
    let mut generation = initial_generation;
    let mut last_pts: f64 = 0.0;

    loop {
        while let Ok(command) = commands.try_recv() {
            match command {
                VideoCommand::Seek {
                    generation: new_generation,
                    seconds,
                } => {
                    generation = new_generation;
                    last_pts = 0.0;
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "video seek position in milliseconds fits i64"
                    )]
                    let millis = (seconds * 1000.0).round() as i64;
                    if let Err(error) = decoder.seek(millis) {
                        let _ = events.send(VideoEvent::Error {
                            generation,
                            message: format!("视频 seek 失败: {error}"),
                        });
                    }
                }
                VideoCommand::Stop => return,
            }
        }

        match decoder.decode() {
            Ok((timestamp, frame)) => {
                let pts = timestamp.as_secs_f64();
                last_pts = last_pts.max(pts);
                let (height, width, channels) = frame.dim();
                if channels < 3 || width == 0 || height == 0 {
                    continue;
                }
                let mut data = Vec::with_capacity(width * height * 4);
                for y in 0..height {
                    for x in 0..width {
                        data.push(frame[[y, x, 0]]);
                        data.push(frame[[y, x, 1]]);
                        data.push(frame[[y, x, 2]]);
                        data.push(255);
                    }
                }
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "video dimensions and timestamps fit the render types"
                )]
                let event = VideoEvent::Frame {
                    generation,
                    pts,
                    width: width as u32,
                    height: height as u32,
                    data,
                };
                if events.send(event).is_err() {
                    return;
                }
            }
            Err(VideoError::DecodeExhausted) => {
                let _ = events.send(VideoEvent::Ended {
                    generation,
                    last_pts,
                });
                if loop_playback {
                    // 等待主线程的 seek 命令；避免在 EOF 后空转。
                    match commands.recv() {
                        Ok(VideoCommand::Seek {
                            generation: new_generation,
                            seconds,
                        }) => {
                            generation = new_generation;
                            last_pts = 0.0;
                            #[expect(
                                clippy::cast_possible_truncation,
                                reason = "video seek position in milliseconds fits i64"
                            )]
                            let millis = (seconds * 1000.0).round() as i64;
                            if let Err(error) = decoder.seek(millis) {
                                let _ = events.send(VideoEvent::Error {
                                    generation,
                                    message: format!("视频重播失败: {error}"),
                                });
                                return;
                            }
                        }
                        Ok(VideoCommand::Stop) | Err(_) => return,
                    }
                } else {
                    return;
                }
            }
            Err(error) => {
                let _ = events.send(VideoEvent::Error {
                    generation,
                    message: format!("视频解码失败: {error}"),
                });
                return;
            }
        }
    }
}

/// 视频精灵实体。
#[derive(bevy::prelude::Resource, Clone, Copy)]
pub struct VideoSprite {
    /// 精灵实体。
    pub entity: Entity,
}

/// 创建视频精灵。
pub fn spawn_sprite(commands: &mut bevy::prelude::Commands) -> VideoSprite {
    let entity = commands
        .spawn((
            Sprite::default(),
            Transform::from_xyz(0.0, 0.0, crate::bga::VIDEO_Z),
            Visibility::Hidden,
        ))
        .id();
    VideoSprite { entity }
}

/// 同步视频纹理与精灵。
pub fn update_video_sprite(
    mut session: ResMut<Session>,
    playback: Res<Playback>,
    video_sprite: Res<VideoSprite>,
    mut images: ResMut<Assets<Image>>,
    mut sprite_query: Query<&mut Sprite>,
    mut transform_query: Query<&mut Transform>,
    mut visibility_query: Query<&mut Visibility>,
) {
    let session = &mut *session;
    let Session { loaded, video, .. } = session;
    let Some(loaded) = loaded.as_ref() else {
        hide_video(video_sprite.entity, &mut visibility_query);
        return;
    };
    let Some(video) = video.as_mut() else {
        hide_video(video_sprite.entity, &mut visibility_query);
        return;
    };
    video.update(playback.chart_time, &loaded.player, playback.seek_serial);

    let Some(_) = video.current() else {
        hide_video(video_sprite.entity, &mut visibility_query);
        return;
    };
    if !video.needs_upload() {
        return;
    }
    let (frame_width, frame_height, frame_data) = {
        let Some(frame) = video.current() else {
            hide_video(video_sprite.entity, &mut visibility_query);
            return;
        };
        (frame.width, frame.height, frame.data.clone())
    };

    let needs_new = video
        .texture()
        .and_then(|handle| images.get(handle))
        .is_none_or(|image| image.width() != frame_width || image.height() != frame_height);
    if needs_new {
        let image = Image::new(
            Extent3d {
                width: frame_width,
                height: frame_height,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            frame_data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        let handle = images.add(image);
        video.set_texture(handle);
        video.mark_uploaded();
    } else if let Some(handle) = video.texture().cloned()
        && let Some(mut image) = images.get_mut(&handle)
    {
        if let Some(data) = &mut image.data {
            data.clone_from(&frame_data);
        } else {
            image.data = Some(frame_data);
        }
        video.mark_uploaded();
    }

    let Some(handle) = video.texture().cloned() else {
        hide_video(video_sprite.entity, &mut visibility_query);
        return;
    };
    let Some(mut sprite) = sprite_query.get_mut(video_sprite.entity).ok() else {
        return;
    };
    let Some(mut transform) = transform_query.get_mut(video_sprite.entity).ok() else {
        return;
    };
    let Some(mut visibility) = visibility_query.get_mut(video_sprite.entity).ok() else {
        return;
    };
    let width = f32::from(u16::try_from(frame_width).unwrap_or(u16::MAX));
    let height = f32::from(u16::try_from(frame_height).unwrap_or(u16::MAX));
    let scale = (VIEW_WIDTH / width).min(VIEW_HEIGHT / height);
    sprite.image = handle;
    sprite.custom_size = Some(bevy::math::Vec2::new(width * scale, height * scale));
    sprite.rect = None;
    transform.translation = bevy::math::Vec3::new(0.0, 0.0, crate::bga::VIDEO_Z);
    *visibility = Visibility::Visible;
}

fn hide_video(entity: Entity, query: &mut Query<&mut Visibility>) {
    if let Ok(mut visibility) = query.get_mut(entity) {
        *visibility = Visibility::Hidden;
    }
}
