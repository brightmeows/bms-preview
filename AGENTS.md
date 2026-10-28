# bms-preview

## 定位

Bevy 0.20 的 BMS/BMSON 谱面预览器，`bmsrs` facade 的下游消费者。独立仓库，
bmsrs 通过 `Cargo.toml` 中的 git `rev` 引入；不持有 bmsrs 源码。

## 命令

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --quiet
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
pre-commit run --all-files
```

## 依赖

`bmsrs` 以 git `rev` 锁定。升级步骤：

1. 修改 `Cargo.toml` 中的 `rev`
2. 运行 `cargo update -p bmsrs`
3. 运行 `cargo test`

本地联调时，复制 `.cargo/config.toml.example` 为 `.cargo/config.toml`，
把 `[patch]` 指向本地 `bmsrs` checkout。`.cargo/config.toml` 不提交。

## 架构

- `main.rs`：CLI 与 Bevy App 组装。
- `chart.rs`：BMS/BMSON 加载、seed、模式，统一为 `Chart<(), PreviewEvent>`。
- `layout.rs`：从 `(NoteSide, Lane)` 推断轨道表。
- `audio.rs`：firewheel context/stream、sampler 池、音频时钟、播放控制。
- `video.rs`：video-rs 解码 worker、帧队列、seek。
- `bga.rs`：图片加载、BGA 层状态。
- `render.rs`：相机、轨道、音符、BGA、视频精灵。
- `ui.rs`：Text2d HUD、警告、最近文件菜单、加载遮罩。
- `config.rs`：JSON 配置。
- `picker.rs`：rfd 文件选择器 worker。
- `state.rs`：应用阶段、播放状态、资源。
- `timeline.rs`：暂停/seek/重播时的音频、视频、BGA 状态重建。

音频时钟是唯一时间基准：画面由 `FirewheelContext::audio_clock_corrected`
驱动；音频设备不可用时降级墙钟并在 HUD 标警告。

## 约定

- `[lints]` 与 `clippy.toml` 复制自 `bmsrs`；`missing_docs_in_private_items`
  在 app 中放行。bmsrs 调整 lint 时同步更新。
- 只通过 `bmsrs` facade 使用库 API，不直连子 crate。
- `publish = false`：本仓只维护源码，不做 Release / crates.io 分发。
- 平台 Linux-first；Wayland 优先，保留 X11 回退。文件选择器走 XDG portal。

## Always

- 修改后跑完 `cargo fmt --all --check`、clippy、test、doc、pre-commit。
- 新增公开项写文档；私有 GUI 代码不强制。
- 音频时间基准统一走 `audio_clock_corrected`；不要用帧时间推进谱面。
- BGA/视频/音频路径都走同一套回退：反斜杠、大小写、扩展名。

## Ask

- 引入新的系统库或外部进程依赖。
- 修改音频 sampler 池大小或块大小。
- 在 app 层新增 bmsrs 未提供的解析语义。

## Never

- 在 app 层做判定、计分或输入延迟校准。
- 把 Bevy 的默认音频栈接回来。
- 直接修改 bmsrs 源码后不更新 git `rev`。
