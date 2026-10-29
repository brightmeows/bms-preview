//! BGA 图片加载、图层状态与精灵同步。

use std::path::Path;

use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::image::Image;
use bevy::prelude::{Entity, Query, Res, Sprite, Transform, Visibility};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bmsrs::chart::{BgaLayer, BgaResource, CropRect, EventKind};
use bmsrs::player::Player;

use crate::chart::{PreviewEvent, resolve_path};
use crate::layout::{VIEW_HEIGHT, VIEW_WIDTH, world_x, world_y};
use crate::state::Session;

/// BGA 画布宽度。
pub const BGA_CANVAS_WIDTH: f32 = 640.0;
/// BGA 画布高度。
pub const BGA_CANVAS_HEIGHT: f32 = 480.0;
/// BGA 画布到虚拟画布的缩放。
pub const BGA_SCALE: f32 = VIEW_WIDTH / BGA_CANVAS_WIDTH;

/// 解码后的 BGA 图片。
pub struct DecodedImage {
    /// 宽度。
    pub width: u32,
    /// 高度。
    pub height: u32,
    /// RGBA8 数据。
    pub data: Vec<u8>,
}

/// 已上传的 BGA 图片与资源元数据。
pub struct BgaRenderInfo {
    /// 图片句柄。
    pub handle: Handle<Image>,
    /// 图片宽度。
    pub width: u32,
    /// 图片高度。
    pub height: u32,
    /// 裁剪信息。
    pub crop: Option<CropRect>,
}

/// BGA 图层状态。
#[derive(Default)]
pub struct BgaState {
    /// 每个资源索引对应的图片。
    pub images: Vec<Option<BgaRenderInfo>>,
    /// 每个图层当前资源索引。
    pub current: [Option<usize>; 4],
    /// 每个图层当前透明度。
    pub opacity: [f32; 4],
}

impl BgaState {
    /// 清空状态。
    pub fn clear(&mut self) {
        self.images.clear();
        self.current = [None; 4];
        self.opacity = [1.0; 4];
    }

    /// 安装解码后的图片。
    pub fn install(
        &mut self,
        assets: &mut Assets<Image>,
        decoded: Vec<Option<DecodedImage>>,
        resources: &[BgaResource],
    ) {
        self.images = decoded
            .into_iter()
            .enumerate()
            .map(|(index, image)| {
                let image = image?;
                let width = image.width;
                let height = image.height;
                let bevy_image = Image::new(
                    Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    image.data,
                    TextureFormat::Rgba8UnormSrgb,
                    RenderAssetUsages::default(),
                );
                let handle = assets.add(bevy_image);
                Some(BgaRenderInfo {
                    handle,
                    width,
                    height,
                    crop: resources.get(index).and_then(|resource| resource.crop),
                })
            })
            .collect();
        self.current = [None; 4];
        self.opacity = [1.0; 4];
    }

    /// 应用 BGA 事件。
    pub fn apply_event(&mut self, layer: BgaLayer, resource_id: u32) {
        let index = layer_index(layer);
        self.current[index] = Some(resource_id as usize);
    }

    /// 应用透明度事件。
    pub fn apply_opacity(&mut self, layer: BgaLayer, opacity: u8) {
        let index = layer_index(layer);
        self.opacity[index] = f32::from(opacity) / 255.0;
    }

    /// 重建到目标脉冲。
    pub fn rebuild(&mut self, player: &Player<(), PreviewEvent>, tick: u64) {
        self.current = [None; 4];
        self.opacity = [1.0; 4];
        for event in player.events_in_range(0..=tick) {
            self.apply_chart_event(&event.kind);
        }
    }

    /// 应用一段事件。
    pub fn apply_range(&mut self, player: &Player<(), PreviewEvent>, start: u64, end: u64) {
        for event in player.events_in_range(start..=end) {
            self.apply_chart_event(&event.kind);
        }
    }

    fn apply_chart_event(&mut self, kind: &EventKind<(), PreviewEvent>) {
        match kind {
            EventKind::Bga { layer, resource_id } => {
                self.apply_event(*layer, resource_id.get());
            }
            EventKind::Custom(PreviewEvent::BgaOpacity { layer, opacity }) => {
                self.apply_opacity(*layer, *opacity);
            }
            _ => {}
        }
    }
}

/// BGA 图层精灵实体。
#[derive(bevy::prelude::Resource, Clone, Copy)]
pub struct BgaSprites {
    /// Base、Layer、Layer2、Poor。
    pub layers: [Entity; 4],
}

/// 创建 BGA 图层精灵与视频精灵。
pub fn spawn_sprites(commands: &mut bevy::prelude::Commands) -> BgaSprites {
    let mut layers = [Entity::PLACEHOLDER; 4];
    for (index, entity) in layers.iter_mut().enumerate() {
        *entity = commands
            .spawn((
                Sprite::default(),
                Transform::from_xyz(0.0, 0.0, layer_z(index)),
                Visibility::Hidden,
            ))
            .id();
    }
    BgaSprites { layers }
}

/// 图层索引。
#[must_use]
pub const fn layer_index(layer: BgaLayer) -> usize {
    match layer {
        BgaLayer::Base => 0,
        BgaLayer::Layer => 1,
        BgaLayer::Layer2 => 2,
        BgaLayer::Poor => 3,
    }
}

/// 解码全部 BGA 图片；失败项返回 `None` 并收集警告。
#[must_use]
pub fn decode_bga_images(
    resources: &[BgaResource],
    base_dir: &Path,
) -> (Vec<Option<DecodedImage>>, Vec<String>) {
    let mut warnings = Vec::new();
    let decoded = resources
        .iter()
        .map(|resource| {
            let Some(path) = resolve_path(
                base_dir,
                &resource.path,
                &["png", "jpg", "jpeg", "bmp", "gif"],
            ) else {
                warnings.push(format!("未找到 BGA 图片: {}", resource.path.display()));
                return None;
            };
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    warnings.push(format!("无法读取 BGA 图片 {}: {error}", path.display()));
                    return None;
                }
            };
            let image = match image::load_from_memory(&bytes) {
                Ok(image) => image,
                Err(error) => {
                    warnings.push(format!("无法解码 BGA 图片 {}: {error}", path.display()));
                    return None;
                }
            };
            let image = image.to_rgba8();
            Some(DecodedImage {
                width: image.width(),
                height: image.height(),
                data: image.into_raw(),
            })
        })
        .collect();
    (decoded, warnings)
}

/// 将图层状态同步到精灵。
pub fn update_bga_sprites(
    session: Res<Session>,
    sprites: Res<BgaSprites>,
    mut sprite_query: Query<&mut Sprite>,
    mut transform_query: Query<&mut Transform>,
    mut visibility_query: Query<&mut Visibility>,
) {
    for layer in 0..4 {
        let entity = sprites.layers[layer];
        let state = session.bga.current[layer]
            .and_then(|index| session.bga.images.get(index))
            .and_then(Option::as_ref)
            .map(|info| (info, session.bga.opacity[layer]));
        let Some((info, alpha)) = state else {
            set_hidden(entity, &mut visibility_query);
            continue;
        };
        let Some(mut sprite) = sprite_query.get_mut(entity).ok() else {
            continue;
        };
        let Some(mut transform) = transform_query.get_mut(entity).ok() else {
            continue;
        };
        let Some(mut visibility) = visibility_query.get_mut(entity).ok() else {
            continue;
        };

        sprite.image = info.handle.clone();
        sprite.color = bevy::color::Color::srgba(1.0, 1.0, 1.0, alpha);
        if let Some(crop) = info.crop {
            let width = (crop.x2 - crop.x1).max(1) as f32;
            let height = (crop.y2 - crop.y1).max(1) as f32;
            sprite.rect = Some(bevy::math::Rect::new(
                crop.x1 as f32,
                crop.y1 as f32,
                crop.x2 as f32,
                crop.y2 as f32,
            ));
            sprite.custom_size = Some(bevy::math::Vec2::new(width * BGA_SCALE, height * BGA_SCALE));
            let screen_x = (crop.dx as f32 + width / 2.0) * BGA_SCALE;
            let screen_y = (BGA_CANVAS_HEIGHT - (crop.dy as f32 + height / 2.0)) * BGA_SCALE;
            transform.translation =
                bevy::math::Vec3::new(world_x(screen_x), world_y(screen_y), layer_z(layer));
        } else {
            let width = info.width as f32;
            let height = info.height as f32;
            sprite.rect = None;
            sprite.custom_size = Some(bevy::math::Vec2::new(width * BGA_SCALE, height * BGA_SCALE));
            let screen_x = width / 2.0 * BGA_SCALE;
            let screen_y = (BGA_CANVAS_HEIGHT - height / 2.0) * BGA_SCALE;
            transform.translation =
                bevy::math::Vec3::new(world_x(screen_x), world_y(screen_y), layer_z(layer));
        }
        *visibility = Visibility::Visible;
    }
}

fn set_hidden(entity: Entity, query: &mut Query<&mut Visibility>) {
    if let Ok(mut visibility) = query.get_mut(entity) {
        *visibility = Visibility::Hidden;
    }
}

const fn layer_z(layer: usize) -> f32 {
    match layer {
        0 => -10.0,
        1 => -9.0,
        2 => -8.0,
        _ => -7.0,
    }
}

/// 视频精灵的 Z。
pub const VIDEO_Z: f32 = -20.0;

/// 虚拟画布尺寸。
pub const _CANVAS: (f32, f32) = (VIEW_WIDTH, VIEW_HEIGHT);

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn decodes_png_fixture() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let resource = BgaResource {
            path: PathBuf::from("bga.png"),
            crop: None,
        };
        let (decoded, warnings) = decode_bga_images(&[resource], &base);
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
        let image = decoded[0].as_ref().expect("image decoded");
        assert_eq!(image.width, 640);
        assert_eq!(image.height, 480);
    }
}
