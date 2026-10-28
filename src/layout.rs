//! 从谱面事件推断轨道顺序与几何。

use bmsrs::chart::{Chart, Lane, NoteSide};

use crate::chart::{PreviewEvent, note_position};

/// 虚拟画布宽度。
pub const VIEW_WIDTH: f32 = 800.0;
/// 虚拟画布高度。
pub const VIEW_HEIGHT: f32 = 600.0;
/// 轨道间距。
pub const TRACK_SPACING: f32 = 4.0;
/// 单条轨道最大宽度。
pub const MAX_TRACK_WIDTH: f32 = 55.0;
/// 判定线纵坐标（屏幕坐标，顶部为 0）。
pub const JUDGMENT_LINE_Y: f32 = 500.0;
/// 每拍像素数。
pub const PIXELS_PER_BEAT: f32 = 120.0;
/// 判定线下方可见历史时长。
pub const REACTION_MS: u64 = 550;
/// 判定线上方可预见时长。
pub const LOOKAHEAD_MS: u64 = 1650;

/// 一条轨道。
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    /// 所属侧。
    pub side: NoteSide,
    /// 轨道类型。
    pub lane: Lane,
    /// 显示标签。
    pub label: String,
    /// 左边界（虚拟画布坐标）。
    pub x: f32,
    /// 宽度。
    pub width: f32,
}

/// 一张谱面的轨道表。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackLayout {
    /// 从左到右的轨道。
    pub tracks: Vec<Track>,
}

impl TrackLayout {
    /// 轨道数量。
    #[must_use]
    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    /// 查找轨道索引。
    #[must_use]
    pub fn index_of(&self, side: NoteSide, lane: Lane) -> Option<usize> {
        self.tracks
            .iter()
            .position(|track| track.side == side && track.lane == lane)
    }

    /// 轨道中心 X 坐标。
    #[must_use]
    pub fn center_x(&self, index: usize) -> f32 {
        self.tracks
            .get(index)
            .map_or(VIEW_WIDTH / 2.0, |track| track.x + track.width / 2.0)
    }

    /// 轨道宽度。
    #[must_use]
    pub fn width(&self, index: usize) -> f32 {
        self.tracks
            .get(index)
            .map_or(MAX_TRACK_WIDTH, |track| track.width)
    }

    /// 整个轨道区的左右边界。
    #[must_use]
    pub fn bounds(&self) -> (f32, f32) {
        let Some(first) = self.tracks.first() else {
            return (0.0, VIEW_WIDTH);
        };
        let last = self
            .tracks
            .last()
            .map_or(first.x + first.width, |track| track.x + track.width);
        (first.x, last)
    }
}

/// 从谱面音符事件推断轨道表。
#[must_use]
pub fn infer_layout(chart: &Chart<(), PreviewEvent>) -> TrackLayout {
    let mut positions: Vec<(NoteSide, Lane)> = Vec::new();
    for event in &chart.data.events {
        if let Some(position) = note_position(&event.kind)
            && !positions.contains(&position)
        {
            positions.push(position);
        }
    }

    positions.sort_by_key(|&(side, lane)| {
        (
            side.as_u8(),
            side_order(side, lane),
            lane_index(lane),
            lane_rank(lane),
        )
    });

    let count = positions.len();
    if count == 0 {
        return TrackLayout::default();
    }

    let spacing = TRACK_SPACING;
    let available = VIEW_WIDTH - 40.0;
    let width = ((available - spacing * (count.saturating_sub(1)) as f32) / count as f32)
        .min(MAX_TRACK_WIDTH)
        .max(12.0);
    let total = width.mul_add(count as f32, spacing * (count.saturating_sub(1)) as f32);
    let start = (VIEW_WIDTH - total) / 2.0;

    let tracks = positions
        .into_iter()
        .enumerate()
        .map(|(index, (side, lane))| Track {
            side,
            lane,
            label: track_label(side, lane),
            x: spacing.mul_add(index as f32, start),
            width,
        })
        .collect();

    TrackLayout { tracks }
}

fn side_order(side: NoteSide, lane: Lane) -> u8 {
    match side {
        NoteSide::P2 => match lane {
            Lane::Key(_) => 0,
            Lane::Scratch(_) => 1,
            Lane::FootPedal => 2,
            _ => 3,
        },
        _ => match lane {
            Lane::Scratch(_) => 0,
            Lane::Key(_) => 1,
            Lane::FootPedal => 2,
            _ => 3,
        },
    }
}

const fn lane_index(lane: Lane) -> u8 {
    match lane {
        Lane::Key(index) | Lane::Scratch(index) => index.get(),
        Lane::FootPedal => 0,
        _ => 0,
    }
}

const fn lane_rank(lane: Lane) -> u8 {
    match lane {
        Lane::Key(_) => 0,
        Lane::Scratch(_) => 1,
        Lane::FootPedal => 2,
        _ => 3,
    }
}

fn track_label(side: NoteSide, lane: Lane) -> String {
    let base = match lane {
        Lane::Key(index) => index.get().to_string(),
        Lane::Scratch(index) if index.get() == 1 => "SC".to_owned(),
        Lane::Scratch(index) => format!("SC{}", index.get()),
        Lane::FootPedal => "FP".to_owned(),
        _ => "?".to_owned(),
    };
    if side == NoteSide::P2 {
        format!("P2-{base}")
    } else {
        base
    }
}

/// 判定线上方的 world Y 坐标（Bevy 使用 Y 向上）。
#[must_use]
pub fn judgment_world_y() -> f32 {
    VIEW_HEIGHT / 2.0 - JUDGMENT_LINE_Y
}

/// 屏幕坐标转 world 坐标。
#[must_use]
pub fn world_x(x: f32) -> f32 {
    x - VIEW_WIDTH / 2.0
}

/// 屏幕坐标转 world 坐标。
#[must_use]
pub fn world_y(y: f32) -> f32 {
    VIEW_HEIGHT / 2.0 - y
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart::{LoadOptions, ModeChoice, load_chart};
    use std::path::Path;

    #[test]
    fn pms_fixture_infers_nine_tracks() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/minimal.pms");
        let chart = load_chart(
            &path,
            LoadOptions {
                seed: 1,
                mode: ModeChoice::Auto,
            },
        )
        .unwrap();
        assert_eq!(chart.mode, crate::chart::ModeKind::Pms);
        let layout = infer_layout(chart.player.chart());
        assert_eq!(layout.len(), 9);
    }

    #[test]
    fn random_seed_selects_different_branches() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/random.bms");
        let first = load_chart(
            &path,
            LoadOptions {
                seed: 1,
                mode: ModeChoice::Auto,
            },
        )
        .unwrap();
        let second = load_chart(
            &path,
            LoadOptions {
                seed: 2,
                mode: ModeChoice::Auto,
            },
        )
        .unwrap();
        let first_layout = infer_layout(first.player.chart());
        let second_layout = infer_layout(second.player.chart());
        assert_eq!(first_layout.len(), 1);
        assert_eq!(second_layout.len(), 1);
        assert_ne!(first_layout.tracks[0].label, second_layout.tracks[0].label);
    }

    #[test]
    fn infers_two_key_tracks_in_order() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/minimal.bms");
        let chart = load_chart(
            &path,
            LoadOptions {
                seed: 1,
                mode: ModeChoice::Auto,
            },
        )
        .unwrap();
        let layout = infer_layout(chart.player.chart());
        assert_eq!(layout.len(), 2);
        assert_eq!(layout.tracks[0].label, "1");
        assert_eq!(layout.tracks[1].label, "2");
    }
}
