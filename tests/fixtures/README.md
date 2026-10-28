# 合成夹具

这些文件用于单元测试与手动验收，不依赖真实谱面。

| 文件 | 用途 |
|---|---|
| `minimal.bms` | 最小 BMS：两条轨道、BGM、一个 keysound |
| `minimal.bmson` | 最小 BMSON v2：单音符 |
| `minimal.pms` | PMS 9-key：P1 11-15 与 P2 22-25 |
| `visual.bms` | BGA 图片、`#VIDEOFILE`、透明度通道 |
| `bga.png` | 640x480 BGA 图片 |
| `video.mp4` | 2 秒 320x240 测试视频 |
| `kick.wav` | 0.12 秒单声道测试音 |

重新生成：

```bash
ffmpeg -hide_banner -loglevel error \
  -f lavfi -i color=c=0x203060:s=640x480 -frames:v 1 bga.png -y
ffmpeg -hide_banner -loglevel error \
  -f lavfi -i testsrc=size=320x240:rate=30 -t 2 -pix_fmt yuv420p video.mp4 -y
```
