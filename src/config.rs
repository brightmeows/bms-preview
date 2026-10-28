//! 用户配置与最近文件。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const MAX_RECENT: usize = 10;
const DEFAULT_WIDTH: f32 = 1000.0;
const DEFAULT_HEIGHT: f32 = 700.0;

/// 持久化配置。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// 窗口宽度。
    pub window_width: f32,
    /// 窗口高度。
    pub window_height: f32,
    /// 主音量，`0.0` 到 `1.0`。
    pub volume: f32,
    /// 播放倍速。
    pub speed: f64,
    /// 音频偏移（毫秒）。
    pub offset_ms: i32,
    /// 上次文件选择目录。
    pub last_dir: Option<PathBuf>,
    /// 最近文件。
    pub recent: Vec<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            window_width: DEFAULT_WIDTH,
            window_height: DEFAULT_HEIGHT,
            volume: 0.8,
            speed: 1.0,
            offset_ms: 0,
            last_dir: None,
            recent: Vec::new(),
        }
    }
}

impl Config {
    /// 限制到合法范围。
    pub fn sanitize(&mut self) {
        self.window_width = self.window_width.clamp(640.0, 3840.0);
        self.window_height = self.window_height.clamp(480.0, 2160.0);
        self.volume = self.volume.clamp(0.0, 1.0);
        self.speed = self.speed.clamp(0.25, 2.0);
        self.offset_ms = self.offset_ms.clamp(-500, 500);
        self.recent.truncate(MAX_RECENT);
    }

    /// 加入最近文件。
    pub fn add_recent(&mut self, path: &Path) {
        let path = path.to_path_buf();
        self.recent.retain(|item| item != &path);
        self.recent.insert(0, path);
        self.recent.truncate(MAX_RECENT);
    }
}

/// 配置存储。
#[derive(bevy::prelude::Resource)]
pub struct ConfigStore {
    /// 配置内容。
    pub config: Config,
    /// 配置文件路径。
    pub path: PathBuf,
    /// 是否有未保存修改。
    pub dirty: bool,
    /// 自动保存倒计时。
    pub save_timer: f32,
}

impl ConfigStore {
    /// 从默认路径加载。
    #[must_use]
    pub fn load() -> Self {
        let path = config_path();
        let config = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Config>(&text).ok())
            .map(|mut config| {
                config.sanitize();
                config
            })
            .unwrap_or_default();
        Self {
            config,
            path,
            dirty: false,
            save_timer: 2.0,
        }
    }

    /// 标记需要保存。
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.save_timer = 2.0;
    }

    /// 立即保存。
    pub fn save(&mut self) {
        if !self.dirty {
            return;
        }
        self.config.sanitize();
        if let Some(parent) = self.path.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            bevy::log::warn!("无法创建配置目录 {}: {error}", parent.display());
            return;
        }
        match serde_json::to_string_pretty(&self.config) {
            Ok(text) => {
                if let Err(error) = std::fs::write(&self.path, text) {
                    bevy::log::warn!("无法写入配置 {}: {error}", self.path.display());
                } else {
                    self.dirty = false;
                }
            }
            Err(error) => bevy::log::warn!("无法序列化配置: {error}"),
        }
    }

    /// 倒计时保存。
    pub fn tick(&mut self, delta_seconds: f32) {
        if !self.dirty {
            return;
        }
        self.save_timer -= delta_seconds;
        if self.save_timer <= 0.0 {
            self.save();
        }
    }
}

/// 默认配置路径。
#[must_use]
pub fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("bmsrs").join("preview.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_files_are_deduplicated_and_bounded() {
        let mut config = Config::default();
        for index in 0..12 {
            config.add_recent(Path::new(&format!("/tmp/chart-{index}.bms")));
        }
        config.add_recent(Path::new("/tmp/chart-5.bms"));
        assert_eq!(config.recent.len(), MAX_RECENT);
        assert_eq!(config.recent[0], PathBuf::from("/tmp/chart-5.bms"));
    }

    #[test]
    fn config_roundtrips_json() {
        let config = Config {
            volume: 0.42,
            recent: vec![PathBuf::from("/tmp/a.bms")],
            ..Config::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let decoded: Config = serde_json::from_str(&json).unwrap();
        assert!((decoded.volume - 0.42).abs() < f32::EPSILON);
        assert_eq!(decoded.recent.len(), 1);
    }

    #[test]
    fn sanitize_clamps_values() {
        let mut config = Config {
            window_width: 10.0,
            window_height: 99_999.0,
            volume: 2.0,
            speed: 9.0,
            offset_ms: 9_999,
            ..Config::default()
        };
        config.sanitize();
        assert_eq!(config.volume, 1.0);
        assert_eq!(config.speed, 2.0);
        assert_eq!(config.offset_ms, 500);
    }
}
