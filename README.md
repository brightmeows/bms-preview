# bms-preview

基于 Bevy 0.20 的 BMS/BMSON 谱面预览器。独立仓库，通过 git rev 消费 `bmsrs`。

## 状态

预发布。当前只面向 Linux（Wayland 优先，保留 X11 回退），未做 Windows/macOS 验收。

## 功能

- 系统文件选择器（XDG Desktop Portal）、命令行路径、拖放、最近文件。
- BMS/BME/BML/PMS 与 BMSON v0/v1/v2。
- BMS 默认 BME 布局；PMS 自动检测并支持手动覆盖；BMSON 按 `mode_hint`。
- 5/7/9/10/14/n-key 与 P1/P2、脚踏板轨道推断。
- 普通音符、长音、地雷、不可见音符、小节线、SPEED/SCROLL。
- BGA 四层图片、裁剪、透明度。
- 视频 `#VIDEOFILE`/`#MOVIE`、`#SEEK`、帧率与延迟提示。
- firewheel 音频时钟、BGM 与 keysound、暂停/重播/跳转/倍速/音量/静音/偏移。
- `#RANDOM` seed 显示与重掷。
- JSON 配置与最近文件菜单。

## 系统依赖

Arch Linux：

```bash
sudo pacman -S --needed ffmpeg alsa-lib wayland libxkbcommon \
  xdg-desktop-portal xdg-desktop-portal-gtk dbus
```

`rfd` 通过 XDG Desktop Portal 打开文件选择器；Hyprland 下由
`xdg-desktop-portal-gtk` 提供 FileChooser 后端。`video-rs` 在构建时链接
`libavcodec`/`libavformat`/`libavutil`/`libswscale`。

## 构建与运行

```bash
cargo run --release -- path/to/chart.bms
cargo run --release -- path/to/chart.bmson
cargo run --release                 # 启动后打开文件选择器
```

调试构建也能运行，但 Bevy 与视频解码建议用 release 验收。

## 命令行

```text
bms-preview [CHART]
  --seed <U64>        #RANDOM 分支种子
  --mode <auto|bme|pms>
  --offset-ms <I32>
  --volume <0.0-1.0>
  --mute
```

## 键位

| 键 | 作用 |
|---|---|
| `O` | 打开文件选择器 |
| `Space` | 暂停/继续 |
| `R` | 从头重播 |
| `Left` / `Right` | 后退/前进 5 秒 |
| `Home` / `End` | 跳到开头/结尾 |
| `Up` / `Down` | 倍速 0.25–2.0 |
| `+` / `-` | 音量 |
| `M` | 静音 |
| `N` | 重掷 `#RANDOM` seed 并重载 |
| `L` | 在 BME/PMS 布局间切换（BMS） |
| `Tab` | 最近文件菜单 |
| `W` | 警告面板 |
| `F3` | 调试信息 |
| `F` | 全屏 |
| `Q` | 退出 |

最近文件菜单中：`Up`/`Down` 选择，`Enter` 打开，`Esc` 关闭。

## 配置

`$XDG_CONFIG_HOME/bmsrs/preview.json`，未设置时回退到
`~/.config/bmsrs/preview.json`。保存窗口尺寸、音量、倍速、offset、
上次目录与最近文件。

## 已知限制

- 不做判定、计分、输入延迟校准或练习模式。
- 不支持 BGA `#SWBGA`、ARGB/EXBMP、视频调色板与视频音轨。
- 音频全量解码进内存；大谱面内存占用高。
- 倍速会同步改变音频播放速度并产生变调。
- PMS 自动检测在无指示通道时默认 Standard，必要时用 `--mode pms`。
- 视频循环时长以解码器最后一帧时间戳近似。

## bmsrs 依赖

`Cargo.toml` 锁定 bmsrs 的 git `rev`。升级步骤：

1. 修改 `Cargo.toml` 中的 `rev`
2. `cargo update -p bmsrs`
3. `cargo test`

本地联调时复制 `.cargo/config.toml.example` 为 `.cargo/config.toml`，
把 `[patch]` 指向本地 `bmsrs` checkout；该文件不提交。

## 检查

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --quiet
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
pre-commit run --all-files
```

## 许可

Apache-2.0。
