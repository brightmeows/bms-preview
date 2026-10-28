# 手动验收清单

在 Omarchy/Hyprland 上执行。合成夹具位于 `tests/fixtures/`。

## 启动与加载

- [ ] 无参数启动打开 XDG portal 文件选择器；取消后显示空态提示，不退出。
- [ ] `cargo run -- tests/fixtures/minimal.bms` 直接加载并开始播放。
- [ ] 拖放 `tests/fixtures/minimal.bms` 到窗口可切换谱面。
- [ ] 加载 `tests/fixtures/minimal.bmson` 显示 BMSON 模式与单音符。
- [ ] 加载 `tests/fixtures/minimal.pms` 显示 9 条轨道与 PMS 模式。
- [ ] `L` 在 BME/PMS 之间重载 `minimal.bms`，轨道数变化。
- [ ] `N` 重掷 seed 并重载，HUD seed 变化。
- [ ] 损坏的谱面显示错误覆盖层，按 `O` 可重新选择。

## 播放

- [ ] `Space` 暂停/继续，音频与谱面时间同时停住。
- [ ] `R` 从头重播。
- [ ] `Left`/`Right` 跳转 5 秒，BGM 与视频从偏移处重启。
- [ ] `Home`/`End` 跳到开头/结尾。
- [ ] `Up`/`Down` 改变倍速，音频变调且画面同步。
- [ ] `+`/`-` 改变音量，`M` 静音/恢复。
- [ ] `,`/`.` 调整音频偏移，HUD 显示变化。
- [ ] 点击底部进度条区域可跳转。

## BGA 与视频

- [ ] `tests/fixtures/visual.bms` 显示蓝色 BGA 图片。
- [ ] BGA 透明度通道生效，图片不是全不透明。
- [ ] 视频在 BGA 下方播放；`#SEEK`/跳转后视频位置正确。
- [ ] 视频循环时画面回到开头，不黑屏。
- [ ] 缺失 BGA/视频时进入警告面板（`W`），不崩溃。

## UI 与持久化

- [ ] HUD 显示标题、作者、BPM、时间、进度、音符数、seed、模式、倍速、音量。
- [ ] `Tab` 打开最近文件菜单；方向键选择，Enter 打开，Esc 关闭。
- [ ] `W` 切换警告面板；`F3` 切换调试信息；`F` 全屏；`Q` 退出。
- [ ] 退出后重新启动，窗口尺寸、音量、倍速、offset、上次目录与最近文件保留。
- [ ] 删除/移动最近文件后，菜单仍可打开且不崩溃。

## 回归

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --quiet
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
```
