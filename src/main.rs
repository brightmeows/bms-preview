//! bms-preview：Bevy 谱面预览器入口。

mod app;
mod audio;
mod bga;
mod chart;
mod config;
mod font;
mod layout;
mod picker;
mod render;
mod state;
mod ui;
mod video;

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::window::WindowResolution;
use clap::Parser;

use crate::app::{LoadWorker, clock_system, input_system, persist_system, startup, worker_system};
use crate::audio::AudioBackend;
use crate::chart::{LoadOptions, ModeChoice, random_seed};
use crate::config::ConfigStore;
use crate::picker::Picker;
use crate::state::{Playback, SampleRate, Session, StartupOptions, UiState};

/// 命令行参数。
#[derive(Parser, Debug)]
#[command(name = "bms-preview")]
#[command(about = "Bevy BMS/BMSON chart previewer")]
struct Cli {
    /// 谱面文件路径。
    #[arg(value_name = "CHART")]
    chart: Option<PathBuf>,
    /// #RANDOM 种子。
    #[arg(long)]
    seed: Option<u64>,
    /// 布局模式。
    #[arg(long, value_enum, default_value_t = ModeChoice::Auto)]
    mode: ModeChoice,
    /// 音频偏移（毫秒）。
    #[arg(long)]
    offset_ms: Option<i32>,
    /// 主音量 0.0-1.0。
    #[arg(long)]
    volume: Option<f32>,
    /// 启动时静音。
    #[arg(long)]
    mute: bool,
}

fn main() {
    let cli = Cli::parse();
    let mut config = ConfigStore::load();
    config.config.sanitize();

    let seed = cli.seed.unwrap_or_else(random_seed);
    let offset_ms = cli.offset_ms.unwrap_or(config.config.offset_ms);
    let volume = cli.volume.unwrap_or(config.config.volume);
    let startup_options = StartupOptions {
        chart: cli.chart,
        load: LoadOptions {
            seed,
            mode: cli.mode,
        },
        offset_ms,
        volume,
        speed: config.config.speed,
        muted: cli.mute,
    };

    config.config.offset_ms = offset_ms;
    config.config.volume = volume;

    let audio = AudioBackend::new();
    let sample_rate = audio.sample_rate();
    let playback = Playback::new(volume, config.config.speed, offset_ms, cli.mute);

    let width = config.config.window_width.round() as u32;
    let height = config.config.window_height.round() as u32;

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "bms-preview".to_owned(),
                resolution: WindowResolution::from((width, height)),
                resizable: true,
                ..Default::default()
            }),
            ..Default::default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.04, 0.04, 0.05)))
        .insert_resource(config)
        .insert_resource(startup_options)
        .insert_resource(playback)
        .insert_resource(UiState::default())
        .insert_resource(Picker::new())
        .insert_resource(LoadWorker::new())
        .insert_resource(SampleRate(sample_rate))
        .insert_resource(Session::default())
        .insert_non_send(audio)
        .add_systems(Startup, startup)
        .add_systems(
            Update,
            (
                (input_system, worker_system, clock_system),
                (
                    bga::update_bga_sprites,
                    video::update_video_sprite,
                    render::update_notes,
                ),
                (render::update_bars, ui::update_hud, persist_system),
            )
                .chain(),
        )
        .run();
}
