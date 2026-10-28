//! firewheel 音频后端、采样池与音频时钟。

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bmsrs::chart::AudioAsset;
use firewheel::core::node::NodeID;
use firewheel::cpal::{CpalConfig, CpalOutputConfig, CpalStream};
use firewheel::nodes::sampler::{PlayFrom, PlaybackID, RepeatMode, SamplerNode, SamplerState};
use firewheel::sample_resource::{SampleResource, SampleResourceInfo};
use firewheel::{FirewheelContext, SymphoniumAudio, Volume};
use symphonium::DecodeConfig;

use crate::chart::resolve_path;

/// keysound 与 BGM 共用的采样池大小。
pub const VOICE_COUNT: usize = 64;

/// 解码后的音频片段。
#[derive(Clone)]
pub struct SlicedSample {
    inner: Arc<SymphoniumAudio>,
    start_frame: u64,
    len_frames: u64,
}

impl SlicedSample {
    /// 创建片段；起点与长度按目标采样率换算。
    #[must_use]
    pub fn new(inner: Arc<SymphoniumAudio>, start_frame: u64, len_frames: u64) -> Self {
        Self {
            inner,
            start_frame,
            len_frames,
        }
    }

    /// 片段时长（秒）。
    #[must_use]
    pub fn duration_seconds(&self) -> f64 {
        let rate = self.inner.sample_rate().get() as f64;
        self.len_frames as f64 / rate
    }
}

impl SampleResourceInfo for SlicedSample {
    fn num_channels(&self) -> std::num::NonZeroUsize {
        self.inner.num_channels()
    }

    fn len_frames(&self) -> u64 {
        self.len_frames
    }

    fn sample_rate(&self) -> Option<NonZeroU32> {
        SampleResourceInfo::sample_rate(&*self.inner)
    }
}

impl SampleResource for SlicedSample {
    fn fill_buffers(
        &self,
        out_buffer: &mut [&mut [f32]],
        out_buffer_range: Range<usize>,
        start_frame: u64,
    ) -> usize {
        if start_frame >= self.len_frames {
            return 0;
        }
        let remaining = usize::try_from(self.len_frames - start_frame).unwrap_or(usize::MAX);
        let requested = out_buffer_range.end.saturating_sub(out_buffer_range.start);
        let allowed = requested.min(remaining);
        if allowed == 0 {
            return 0;
        }
        let adjusted = out_buffer_range.start..out_buffer_range.start + allowed;
        self.inner
            .fill_buffers(out_buffer, adjusted, self.start_frame + start_frame)
    }
}

#[derive(Clone)]
struct Voice {
    id: NodeID,
    node: SamplerNode,
    playback_id: PlaybackID,
    busy: bool,
}

struct Core {
    context: FirewheelContext,
    stream: CpalStream,
    voices: Vec<Voice>,
}

/// firewheel 音频后端；初始化失败时保持 `core = None` 并降级静音。
pub struct AudioBackend {
    core: Option<Core>,
    samples: Vec<Option<SlicedSample>>,
    volume: f32,
    speed: f64,
    next_voice: usize,
    sample_rate: u32,
    error: Option<String>,
}

impl AudioBackend {
    /// 创建后端；设备打开失败不返回错误，而是进入静音降级。
    #[must_use]
    pub fn new() -> Self {
        match Self::try_new() {
            Ok(backend) => backend,
            Err(error) => {
                bevy::log::warn!("音频初始化失败，降级静音: {error}");
                Self {
                    core: None,
                    samples: Vec::new(),
                    volume: 0.8,
                    speed: 1.0,
                    next_voice: 0,
                    sample_rate: 44_100,
                    error: Some(error),
                }
            }
        }
    }

    fn try_new() -> Result<Self, String> {
        let mut context = FirewheelContext::new(Default::default());
        let config = CpalConfig {
            output: CpalOutputConfig {
                desired_block_frames: Some(256),
                ..Default::default()
            },
            ..Default::default()
        };
        let stream = CpalStream::new(&mut context, config).map_err(|error| error.to_string())?;
        let sample_rate = context
            .stream_info()
            .map_or(44_100, |info| info.sample_rate.get());
        let graph_out = context.graph_out_node_id();
        let mut voices = Vec::with_capacity(VOICE_COUNT);
        for _ in 0..VOICE_COUNT {
            let node = SamplerNode::default();
            let id = context
                .add_node(node, None)
                .map_err(|error| format!("{error:?}"))?;
            context
                .connect(id, graph_out, &[(0, 0), (1, 1)], false)
                .map_err(|error| format!("{error:?}"))?;
            voices.push(Voice {
                id,
                node,
                playback_id: PlaybackID::DANGLING,
                busy: false,
            });
        }

        bevy::log::info!("firewheel 初始化完成: sample_rate={sample_rate}, voices={VOICE_COUNT}");
        Ok(Self {
            core: Some(Core {
                context,
                stream,
                voices,
            }),
            samples: Vec::new(),
            volume: 0.8,
            speed: 1.0,
            next_voice: 0,
            sample_rate,
            error: None,
        })
    }

    /// 目标采样率。
    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// 初始化/运行错误。
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// 音频时钟（秒）；不可用时返回 `None`。
    #[must_use]
    pub fn clock_seconds(&self) -> Option<f64> {
        self.core
            .as_ref()
            .map(|core| core.context.audio_clock_corrected().seconds.0)
    }

    /// 替换采样表并停止所有发声。
    pub fn set_samples(&mut self, samples: Vec<Option<SlicedSample>>) {
        self.stop_all();
        self.samples = samples;
    }

    /// 设置主音量。
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        let Some(core) = self.core.as_mut() else {
            return;
        };
        let Core {
            context, voices, ..
        } = core;
        for voice in voices {
            voice.node.volume = Volume::Linear(self.volume);
            context.queue_event_for(voice.id, voice.node.sync_volume_event());
        }
    }

    /// 设置播放倍速。
    pub fn set_speed(&mut self, speed: f64) {
        self.speed = speed.clamp(0.25, 2.0);
        let Some(core) = self.core.as_mut() else {
            return;
        };
        let Core {
            context, voices, ..
        } = core;
        for voice in voices {
            voice.node.speed = self.speed;
            context.queue_event_for(voice.id, voice.node.sync_speed_event());
        }
    }

    /// 播放一个素材。
    ///
    /// `from_seconds` 是片段内部的起点；返回是否成功入队。
    pub fn play(&mut self, asset_index: u32, from_seconds: f64, loop_playback: bool) -> bool {
        let Some(sample) = self
            .samples
            .get(asset_index as usize)
            .and_then(Option::as_ref)
            .cloned()
        else {
            return false;
        };
        let Some(core) = self.core.as_mut() else {
            return false;
        };
        let index = if let Some(index) = core.voices.iter().position(|voice| !voice.busy) {
            index
        } else {
            let index = self.next_voice % core.voices.len();
            self.next_voice = (index + 1) % core.voices.len();
            index
        };
        let speed = self.speed;
        let volume = self.volume;
        let Core {
            context, voices, ..
        } = core;
        let voice = &mut voices[index];
        voice.node.volume = Volume::Linear(volume);
        voice.node.speed = speed;
        voice.node.repeat_mode = if loop_playback {
            RepeatMode::RepeatEndlessly
        } else {
            RepeatMode::PlayOnce
        };
        voice
            .node
            .start_from(PlayFrom::Seconds(from_seconds.max(0.0)));
        voice.playback_id = voice.node.playback_id();
        voice.busy = true;
        context.queue_event_for(voice.id, SamplerNode::set_sample_event(sample));
        context.queue_event_for(voice.id, voice.node.sync_volume_event());
        context.queue_event_for(voice.id, voice.node.sync_speed_event());
        context.queue_event_for(voice.id, voice.node.sync_repeat_mode_event());
        context.queue_event_for(voice.id, voice.node.sync_play_from_event());
        context.queue_event_for(voice.id, voice.node.sync_play_event());
        true
    }

    /// 暂停所有发声（保留播放位置）。
    pub fn pause_all(&mut self) {
        let Some(core) = self.core.as_mut() else {
            return;
        };
        let Core {
            context, voices, ..
        } = core;
        for voice in voices {
            voice.node.pause();
            context.queue_event_for(voice.id, voice.node.sync_play_event());
        }
    }

    /// 恢复所有发声。
    pub fn resume_all(&mut self) {
        let Some(core) = self.core.as_mut() else {
            return;
        };
        let Core {
            context, voices, ..
        } = core;
        for voice in voices {
            voice.node.resume();
            context.queue_event_for(voice.id, voice.node.sync_play_event());
        }
    }

    /// 停止所有发声并回到片段开头。
    pub fn stop_all(&mut self) {
        let Some(core) = self.core.as_mut() else {
            return;
        };
        let Core {
            context, voices, ..
        } = core;
        for voice in voices {
            voice.node.stop();
            voice.busy = false;
            context.queue_event_for(voice.id, voice.node.sync_play_event());
        }
    }

    /// 每帧推进 firewheel 并回收已结束的 voice。
    pub fn update(&mut self) {
        let Some(core) = self.core.as_mut() else {
            return;
        };
        core.stream.log_status();
        if let Err(error) = core.context.update() {
            bevy::log::warn!("firewheel update 失败: {error:?}");
        }
        let context = &core.context;
        for voice in &mut core.voices {
            if voice.busy
                && context
                    .node_state::<SamplerState>(voice.id)
                    .is_some_and(|state| state.playback_finished(voice.playback_id))
            {
                voice.busy = false;
            }
        }
    }

    /// 片段时长（秒）。
    #[must_use]
    pub fn sample_duration(&self, asset_index: u32) -> Option<f64> {
        self.samples
            .get(asset_index as usize)
            .and_then(Option::as_ref)
            .map(SlicedSample::duration_seconds)
    }
}

impl Default for AudioBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// 解码全部音频素材；失败项返回 `None` 并收集警告。
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "asset start/duration are finite non-negative seconds converted to frame offsets"
)]
#[must_use]
pub fn decode_audio_assets(
    assets: &[AudioAsset],
    base_dir: &Path,
    sample_rate: u32,
) -> (Vec<Option<SlicedSample>>, Vec<String>) {
    let mut decoded_cache: HashMap<PathBuf, Arc<SymphoniumAudio>> = HashMap::new();
    let mut warnings = Vec::new();
    let target_rate = NonZeroU32::new(sample_rate).unwrap_or(NonZeroU32::MIN);
    let mut samples = Vec::with_capacity(assets.len());

    for (index, asset) in assets.iter().enumerate() {
        let Some(path) = resolve_path(base_dir, &asset.path, &["ogg", "flac", "wav", "mp3"]) else {
            warnings.push(format!("未找到音频文件: {}", asset.path.display()));
            samples.push(None);
            continue;
        };

        let decoded = if let Some(cached) = decoded_cache.get(&path) {
            Some(Arc::clone(cached))
        } else {
            match decode_file(&path, target_rate) {
                Ok(decoded) => {
                    let decoded = Arc::new(decoded);
                    decoded_cache.insert(path.clone(), Arc::clone(&decoded));
                    Some(decoded)
                }
                Err(error) => {
                    warnings.push(format!("无法解码音频 {}: {error}", path.display()));
                    None
                }
            }
        };

        let Some(inner) = decoded else {
            samples.push(None);
            continue;
        };

        let rate = f64::from(sample_rate);
        let start_frame = (asset.start.as_secs_f64() * rate).round().max(0.0) as u64;
        let total_frames = inner.len_frames();
        let duration_frames = asset
            .duration
            .map(|duration| (duration.as_secs_f64() * rate).round().max(0.0) as u64);
        if start_frame >= total_frames {
            warnings.push(format!(
                "音频切片起点越界: {} (asset {index})",
                path.display()
            ));
            samples.push(None);
            continue;
        }
        let len_frames = duration_frames.map_or(total_frames - start_frame, |frames| {
            frames.min(total_frames - start_frame)
        });
        if len_frames == 0 {
            warnings.push(format!(
                "音频切片长度为零: {} (asset {index})",
                path.display()
            ));
            samples.push(None);
            continue;
        }
        samples.push(Some(SlicedSample::new(inner, start_frame, len_frames)));
    }

    (samples, warnings)
}

fn decode_file(path: &Path, target_rate: NonZeroU32) -> Result<SymphoniumAudio, String> {
    let probed =
        symphonium::probe_from_file(path, None).map_err(|error| format!("probe: {error}"))?;
    let decoded = symphonium::decode(
        probed,
        &DecodeConfig::default(),
        Some(target_rate),
        None,
        None,
    )
    .map_err(|error| format!("decode: {error}"))?;
    Ok(SymphoniumAudio(decoded))
}
