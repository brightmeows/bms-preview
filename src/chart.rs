//! 谱面加载与统一事件映射。
//!
//! BMS 的 `BmsCustomEvent` 与 BMSON 的 `BmsonNoteExt` 在这里收敛为
//! `Chart<(), PreviewEvent>`，保留渲染层真正需要的 BGA 透明度与视频定位。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use bmsrs::bms::ErrorStrategy;
use bmsrs::bms::parser::Bms;
use bmsrs::bms::processor::BmsProcessor;
use bmsrs::bms::processor::custom_event::BmsCustomEvent;
use bmsrs::bms::processor::layout::{Bme, BmsLayout, Pms, PmsBme, PmsLayout};
use bmsrs::bms::source::Document;
use bmsrs::bmson::def::{Bmson, DetectedVersion};
use bmsrs::bmson::processor::BmsonProcessor;
use bmsrs::chart::{
    AudioAsset, BgaLayer, BgaResource, Chart, CustomEvent, Event, EventKind, Lane, NoteKind,
    NoteSide,
};
use bmsrs::player::Player;
use clap::ValueEnum;
use thiserror::Error;

/// 预览器保留的自定义事件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreviewEvent {
    /// 非自定义事件的占位负载；渲染层忽略。
    Other,
    /// BGA 图层不透明度变更。
    BgaOpacity {
        /// 目标图层。
        layer: BgaLayer,
        /// 不透明度，`0` 到 `255`。
        opacity: u8,
    },
    /// 视频定位。
    VideoSeek {
        /// 目标位置（毫秒）。
        position_ms: u64,
    },
}

impl CustomEvent for PreviewEvent {}

/// BMS 布局选择。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum ModeChoice {
    /// 按扩展名选择：`.pms` 用 PMS，其余用 BME。
    #[default]
    Auto,
    /// 强制 BME 族。
    Bme,
    /// 强制 PMS。
    Pms,
}

/// 实际使用的布局族。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeKind {
    /// BME 族（5/7/10/14/n-key）。
    Bme,
    /// PMS（9-key 或 5-key 子集）。
    Pms,
    /// BMSON，按 `mode_hint`。
    Bmson,
}

impl ModeKind {
    /// 用于 HUD 的短名称。
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Bme => "BME",
            Self::Pms => "PMS",
            Self::Bmson => "BMSON",
        }
    }
}

/// 一次加载的参数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadOptions {
    /// `#RANDOM` 分支种子。
    pub seed: u64,
    /// 布局选择。
    pub mode: ModeChoice,
}

impl Default for LoadOptions {
    fn default() -> Self {
        Self {
            seed: 1,
            mode: ModeChoice::Auto,
        }
    }
}

/// 谱面统计。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChartStats {
    /// 可玩音符（普通 + 长音）。
    pub playable: usize,
    /// 普通音符。
    pub normal: usize,
    /// 长音。
    pub long: usize,
    /// 地雷。
    pub mines: usize,
    /// 不可见音符。
    pub invisible: usize,
    /// BGM 事件。
    pub bgm: usize,
    /// BGA 事件。
    pub bga: usize,
}

/// 视频源信息。
#[derive(Clone, Debug, PartialEq)]
pub struct VideoSource {
    /// 视频文件路径。
    pub path: PathBuf,
    /// 是否循环。
    pub loop_playback: bool,
    /// 帧率提示。
    pub fps: Option<f64>,
    /// 起始帧延迟。
    pub delay_frames: Option<u32>,
}

/// 已加载、已归一化的谱面。
pub struct LoadedChart {
    /// 仿真层播放器。
    pub player: Player<(), PreviewEvent>,
    /// 谱面文件路径。
    pub path: PathBuf,
    /// 谱面所在目录。
    pub base_dir: PathBuf,
    /// `#RANDOM` 种子。
    pub seed: u64,
    /// 实际布局族。
    pub mode: ModeKind,
    /// 解析与处理警告。
    pub warnings: Vec<String>,
    /// 统计。
    pub stats: ChartStats,
    /// 视频源。
    pub video: Option<VideoSource>,
    /// 音频素材表。
    pub audio_assets: Vec<AudioAsset>,
    /// BGA 资源表。
    pub bga_resources: Vec<BgaResource>,
}

impl LoadedChart {
    /// 谱面标题。
    #[must_use]
    pub fn title(&self) -> &str {
        &self.player.chart().song.title
    }

    /// 艺术家。
    #[must_use]
    pub fn artist(&self) -> &str {
        &self.player.chart().song.artist
    }

    /// 谱面名称。
    #[must_use]
    pub fn chart_name(&self) -> &str {
        &self.player.chart().chart.chart_name
    }

    /// 总时长。
    #[must_use]
    pub fn duration(&self) -> Duration {
        self.player.duration()
    }
}

/// 加载错误。
#[derive(Debug, Error)]
pub enum LoadError {
    /// 文件读取失败。
    #[error("读取文件失败: {path}: {source}")]
    Io {
        /// 文件路径。
        path: PathBuf,
        /// 底层错误。
        #[source]
        source: std::io::Error,
    },
    /// 扩展名无法识别。
    #[error("不支持的谱面扩展名: {0}")]
    UnsupportedExtension(String),
    /// BMS 语法解析失败。
    #[error("BMS 解析失败: {0}")]
    BmsParse(String),
    /// BMS 处理失败。
    #[error("BMS 处理失败: {0}")]
    BmsProcess(String),
    /// BMSON 解析失败。
    #[error("BMSON 解析失败: {0}")]
    BmsonParse(String),
    /// BMSON 处理失败。
    #[error("BMSON 处理失败: {0}")]
    BmsonProcess(String),
    /// 播放器校验失败。
    #[error("谱面校验失败: {0}")]
    Player(String),
}

/// 确定性 `#RANDOM` 随机源。
#[derive(Clone, Debug)]
pub struct SeedRng(u64);

impl SeedRng {
    /// 由种子创建；零种子替换为非零值。
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }

    /// xorshift64*，返回 `1..=max`。
    pub fn next_bounded(&mut self, max: u64) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        let value = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        if max == 0 { 1 } else { value % max + 1 }
    }
}

/// 加载谱面。
///
/// # Errors
///
/// 文件不可读、扩展名不支持、解析/处理/校验失败时返回 [`LoadError`]。
pub fn load_chart(path: &Path, options: LoadOptions) -> Result<LoadedChart, LoadError> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| LoadError::UnsupportedExtension(path.display().to_string()))?;

    let base_dir = path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);

    let (chart, mode, mut warnings) = match extension.as_str() {
        "bms" | "bme" | "bml" | "pms" => load_bms(path, options, &extension)?,
        "bmson" => load_bmson(path)?,
        other => return Err(LoadError::UnsupportedExtension(other.to_owned())),
    };

    let stats = chart_stats(&chart);
    let video_asset = chart.chart.video.clone();
    let bga_resources = chart.chart.bga_resources.clone();
    let audio_assets = chart.data.audio_assets.clone();
    let player = Player::new(chart).map_err(|e| LoadError::Player(e.to_string()))?;

    let video = video_asset.and_then(|asset| {
        match resolve_path(
            &base_dir,
            &asset.path,
            &["mp4", "avi", "mpg", "mpeg", "webm", "mkv", "wmv"],
        ) {
            Some(path) => Some(VideoSource {
                path,
                loop_playback: asset.loop_playback,
                fps: asset.fps,
                delay_frames: asset.delay_frames,
            }),
            None => {
                warnings.push(format!("未找到视频文件: {}", asset.path.display()));
                None
            }
        }
    });

    Ok(LoadedChart {
        player,
        path: path.to_path_buf(),
        base_dir,
        seed: options.seed,
        mode,
        warnings,
        stats,
        video,
        audio_assets,
        bga_resources,
    })
}

fn load_bms(
    path: &Path,
    options: LoadOptions,
    extension: &str,
) -> Result<(Chart<(), PreviewEvent>, ModeKind, Vec<String>), LoadError> {
    let bytes = std::fs::read(path).map_err(|source| LoadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let document = Document::from_bytes(bytes);
    let mut rng = SeedRng::new(options.seed);
    let parsed = Bms::from_document(
        &document,
        |max| rng.next_bounded(max),
        ErrorStrategy::CollectAll,
    )
    .map_err(|warning| LoadError::BmsParse(format!("{:?}", warning.kind)))?;

    let mut warnings: Vec<String> = parsed
        .warnings
        .iter()
        .map(|warning| format!("解析警告: {:?}", warning.kind))
        .collect();
    let bms = parsed.bms;

    let mode = match options.mode {
        ModeChoice::Auto if extension == "pms" => ModeKind::Pms,
        ModeChoice::Auto | ModeChoice::Bme => ModeKind::Bme,
        ModeChoice::Pms => ModeKind::Pms,
    };

    let (chart, process_warnings) = match mode {
        ModeKind::Bme => process_bms::<Bme>(&bms)?,
        ModeKind::Pms => match PmsLayout::detect(&bms) {
            PmsLayout::Standard => process_bms::<Pms>(&bms)?,
            PmsLayout::BmeType => process_bms::<PmsBme>(&bms)?,
        },
        ModeKind::Bmson => {
            return Err(LoadError::BmsProcess(
                "BMS 加载分支不应选择 BMSON 模式".to_owned(),
            ));
        }
    };

    warnings.extend(
        process_warnings
            .iter()
            .map(|warning| format!("处理警告: {warning:?}")),
    );

    let (chart, ignored) = map_bms_chart(chart);
    if !ignored.is_empty() {
        warnings.push(format!(
            "忽略自定义事件: {}",
            ignored.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }

    Ok((chart, mode, warnings))
}

fn process_bms<L: BmsLayout>(
    bms: &Bms,
) -> Result<
    (
        Chart<(), BmsCustomEvent>,
        Vec<bmsrs::bms::processor::ProcessWarning>,
    ),
    LoadError,
> {
    BmsProcessor::process::<L>(bms)
        .map(|processed| (processed.chart, processed.warnings))
        .map_err(|e| LoadError::BmsProcess(e.to_string()))
}

fn map_bms_chart(
    chart: Chart<(), BmsCustomEvent>,
) -> (Chart<(), PreviewEvent>, BTreeSet<&'static str>) {
    let mut ignored = BTreeSet::new();
    let chart = chart.filter_map_events(|event| {
        let tick = event.tick();
        match event.kind {
            EventKind::Custom(custom) => match &custom {
                BmsCustomEvent::BgaOpacity { layer, opacity } => Some(Event::new(
                    tick,
                    EventKind::Custom(PreviewEvent::BgaOpacity {
                        layer: *layer,
                        opacity: *opacity,
                    }),
                )),
                BmsCustomEvent::VideoSeek { position } => Some(Event::new(
                    tick,
                    EventKind::Custom(PreviewEvent::VideoSeek {
                        position_ms: *position,
                    }),
                )),
                other => {
                    ignored.insert(custom_event_name(other));
                    None
                }
            },
            kind => Some(Event::new(tick, kind.map_custom(|_| PreviewEvent::Other))),
        }
    });
    (chart, ignored)
}

const fn custom_event_name(event: &BmsCustomEvent) -> &'static str {
    match event {
        BmsCustomEvent::BgaOpacity { .. } => "BgaOpacity",
        BmsCustomEvent::BgaArgb { .. } => "BgaArgb",
        BmsCustomEvent::BgaKeyBound { .. } => "BgaKeyBound",
        BmsCustomEvent::TextDisplay { .. } => "TextDisplay",
        BmsCustomEvent::JudgeOverride { .. } => "JudgeOverride",
        BmsCustomEvent::OptionChange { .. } => "OptionChange",
        BmsCustomEvent::BgmVolume { .. } => "BgmVolume",
        BmsCustomEvent::KeyVolume { .. } => "KeyVolume",
        BmsCustomEvent::VideoSeek { .. } => "VideoSeek",
    }
}

fn load_bmson(path: &Path) -> Result<(Chart<(), PreviewEvent>, ModeKind, Vec<String>), LoadError> {
    let content = std::fs::read_to_string(path).map_err(|source| LoadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let version =
        DetectedVersion::detect(&content).map_err(|e| LoadError::BmsonParse(e.to_string()))?;

    let bmson: Bmson<'_> = match version {
        DetectedVersion::V2 => {
            serde_json::from_str(&content).map_err(|e| LoadError::BmsonParse(e.to_string()))?
        }
        DetectedVersion::V1 => {
            let v1: bmsrs::bmson::def::v1::Bmson<'_> =
                serde_json::from_str(&content).map_err(|e| LoadError::BmsonParse(e.to_string()))?;
            let root: Bmson<'_> = v1.into();
            root
        }
        DetectedVersion::V0 => {
            let v0: bmsrs::bmson::def::v0::Bmson<'_> =
                serde_json::from_str(&content).map_err(|e| LoadError::BmsonParse(e.to_string()))?;
            let root: Bmson<'_> =
                v0.try_into()
                    .map_err(|e: bmsrs::bmson::def::v0::TryFromV0Error| {
                        LoadError::BmsonParse(e.to_string())
                    })?;
            root
        }
    };

    let processed = BmsonProcessor::process_default(&bmson)
        .map_err(|e| LoadError::BmsonProcess(e.to_string()))?;
    let chart: Chart<(), PreviewEvent> = processed.chart.filter_map_events(|event| {
        let tick = event.tick();
        let kind: EventKind<(), PreviewEvent> = event
            .kind
            .map_ext(|_ext| ())
            .map_custom(|_| PreviewEvent::Other);
        Some(Event::new(tick, kind))
    });

    Ok((chart, ModeKind::Bmson, Vec::new()))
}

fn chart_stats<T: bmsrs::chart::NoteExt, C: CustomEvent>(chart: &Chart<T, C>) -> ChartStats {
    let mut stats = ChartStats::default();
    for event in &chart.data.events {
        match &event.kind {
            EventKind::Note { kind, .. } => match kind {
                NoteKind::Normal => {
                    stats.normal += 1;
                    stats.playable += 1;
                }
                NoteKind::Long { duration } if *duration > 0 => {
                    stats.long += 1;
                    stats.playable += 1;
                }
                NoteKind::Long { .. } => {
                    stats.normal += 1;
                    stats.playable += 1;
                }
                NoteKind::Mine { .. } => stats.mines += 1,
                NoteKind::Invisible => stats.invisible += 1,
            },
            EventKind::Bgm { .. } => stats.bgm += 1,
            EventKind::Bga { .. } => stats.bga += 1,
            _ => {}
        }
    }
    stats
}

/// 解析相对路径，处理反斜杠、大小写与扩展名回退。
#[must_use]
pub fn resolve_path(base: &Path, raw: &Path, extensions: &[&str]) -> Option<PathBuf> {
    let normalized = normalize_separators(raw);
    let candidates = extension_candidates(&normalized, extensions);

    for candidate in &candidates {
        let path = if candidate.is_absolute() {
            candidate.clone()
        } else {
            base.join(candidate)
        };
        if path.is_file() {
            return Some(path);
        }
    }

    for candidate in &candidates {
        if candidate.is_absolute() {
            continue;
        }
        if let Some(path) = resolve_case_insensitive(base, candidate)
            && path.is_file()
        {
            return Some(path);
        }
    }

    None
}

fn normalize_separators(path: &Path) -> PathBuf {
    let text = path.to_string_lossy().replace('\\', "/");
    PathBuf::from(text)
}

fn extension_candidates(path: &Path, extensions: &[&str]) -> Vec<PathBuf> {
    let mut candidates = vec![path.to_path_buf()];
    for extension in extensions {
        candidates.push(path.with_extension(extension));
    }
    candidates
}

fn resolve_case_insensitive(base: &Path, relative: &Path) -> Option<PathBuf> {
    let mut current = base.to_path_buf();
    for component in relative.components() {
        match component {
            std::path::Component::Normal(name) => {
                let name = name.to_string_lossy();
                let candidate = current.join(name.as_ref());
                if candidate.exists() {
                    current = candidate;
                    continue;
                }
                let entries = std::fs::read_dir(&current).ok()?;
                let mut found = None;
                for entry in entries.flatten() {
                    let file_name = entry.file_name();
                    if file_name
                        .to_string_lossy()
                        .eq_ignore_ascii_case(name.as_ref())
                    {
                        found = Some(entry.path());
                        break;
                    }
                }
                current = found?;
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !current.pop() {
                    return None;
                }
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => return None,
        }
    }
    Some(current)
}

/// 判断事件是否属于给定侧与轨道。
#[must_use]
pub fn note_position(kind: &EventKind<(), PreviewEvent>) -> Option<(NoteSide, Lane)> {
    match kind {
        EventKind::Note { side, lane, .. } => Some((*side, *lane)),
        _ => None,
    }
}

/// 生成新的随机种子。
#[must_use]
pub fn random_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let seed = u64::try_from(nanos).unwrap_or(1);
    if seed == 0 { 1 } else { seed }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[test]
    fn minimal_bms_loads_with_notes_and_audio() {
        let chart = load_chart(
            &fixture("minimal.bms"),
            LoadOptions {
                seed: 42,
                mode: ModeChoice::Auto,
            },
        )
        .unwrap();
        assert_eq!(chart.title(), "Minimal Chart");
        assert!(chart.stats.playable >= 6, "got {}", chart.stats.playable);
        assert_eq!(chart.audio_assets.len(), 1);
        assert_eq!(chart.mode, ModeKind::Bme);
    }

    #[test]
    fn minimal_bmson_v2_loads() {
        let chart = load_chart(
            &fixture("minimal.bmson"),
            LoadOptions {
                seed: 1,
                mode: ModeChoice::Auto,
            },
        )
        .unwrap();
        assert_eq!(chart.title(), "Minimal BMSON");
        assert_eq!(chart.stats.playable, 1);
        assert_eq!(chart.mode, ModeKind::Bmson);
    }

    #[test]
    fn visual_fixture_exposes_bga_and_video() {
        let chart = load_chart(
            &fixture("visual.bms"),
            LoadOptions {
                seed: 1,
                mode: ModeChoice::Auto,
            },
        )
        .unwrap();
        assert_eq!(chart.bga_resources.len(), 1);
        assert!(chart.video.is_some(), "video should resolve");
    }

    #[test]
    fn minimal_bmson_v1_converts_to_root() {
        let chart = load_chart(
            &fixture("minimal-v1.bmson"),
            LoadOptions {
                seed: 1,
                mode: ModeChoice::Auto,
            },
        )
        .unwrap();
        assert_eq!(chart.title(), "Minimal BMSON v1");
        assert_eq!(chart.stats.playable, 1);
    }

    #[test]
    fn seed_rng_is_deterministic_and_in_range() {
        let mut first = SeedRng::new(123);
        let mut second = SeedRng::new(123);
        for max in [1, 2, 7, 1000] {
            let a = first.next_bounded(max);
            let b = second.next_bounded(max);
            assert_eq!(a, b);
            assert!((1..=max).contains(&a));
        }
    }

    #[test]
    fn resolve_path_falls_back_to_extension() {
        let base = fixture("");
        let resolved = resolve_path(&base, Path::new("kick"), &["wav"]);
        assert!(resolved.is_some());
    }
}
