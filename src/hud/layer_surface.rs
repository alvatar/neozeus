use bevy::{
    app::{Plugin, PostUpdate},
    asset::RenderAssetUsages,
    prelude::*,
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderSystems, render_asset::RenderAssets,
        render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
        renderer::{RenderDevice, RenderQueue},
        texture::GpuImage,
    },
    window::PrimaryWindow,
};
use bevy_vello::{
    integrations::scene::VelloScene2d,
    render::{VelloRenderSettings, VelloRenderer},
    vello,
    vello::kurbo::Affine,
};

use super::HudLayerRegistry;

const HUD_LAYER_SURFACE_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;

#[derive(Clone)]
struct ExtractedHudLayerSurface {
    scene: VelloScene2d,
    target_image: Handle<Image>,
}

#[derive(Resource, Clone, Default)]
struct ExtractedHudLayerSurfaces {
    layers: Vec<ExtractedHudLayerSurface>,
}

pub(crate) struct HudLayerSurfacePlugin;

impl Plugin for HudLayerSurfacePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, sync_hud_layer_surface_images);

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<ExtractedHudLayerSurfaces>()
            .add_systems(ExtractSchedule, extract_hud_layer_surfaces)
            .add_systems(
                Render,
                render_hud_layer_surfaces
                    .in_set(RenderSystems::Render)
                    .run_if(resource_exists::<RenderDevice>),
            );
    }
}

fn create_hud_layer_surface_image(size: UVec2) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: size.x.max(1),
            height: size.y.max(1),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        HUD_LAYER_SURFACE_FORMAT,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
        | TextureUsages::COPY_DST
        | TextureUsages::COPY_SRC
        | TextureUsages::STORAGE_BINDING
        | TextureUsages::RENDER_ATTACHMENT;
    image
}

pub(crate) fn sync_hud_layer_surface_images(
    primary_window: Single<&Window, With<PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    mut registry: ResMut<HudLayerRegistry>,
) {
    let expected_size = UVec2::new(
        primary_window.physical_width().max(1),
        primary_window.physical_height().max(1),
    );

    for spec in registry.ordered_specs() {
        let Some(runtime) = registry.layer_mut(spec.id) else {
            continue;
        };

        let needs_new_image = runtime
            .surface_image
            .as_ref()
            .and_then(|handle| images.get(handle.id()))
            .is_none_or(|image| {
                image.texture_descriptor.size.width != expected_size.x
                    || image.texture_descriptor.size.height != expected_size.y
                    || image.texture_descriptor.format != HUD_LAYER_SURFACE_FORMAT
            });

        if needs_new_image {
            let image = images.add(create_hud_layer_surface_image(expected_size));
            registry.set_surface_image(spec.id, image);
        }
    }
}

fn extract_hud_layer_surfaces(
    mut extracted: ResMut<ExtractedHudLayerSurfaces>,
    registry: Extract<Res<HudLayerRegistry>>,
    scenes: Extract<Query<&VelloScene2d>>,
) {
    extracted.layers.clear();
    extracted.layers.reserve(registry.ordered_specs().len());

    for spec in registry.ordered_specs() {
        let Some(runtime) = registry.layer(spec.id) else {
            continue;
        };
        let (Some(scene_entity), Some(target_image)) =
            (runtime.scene_entity, runtime.surface_image.clone())
        else {
            continue;
        };
        let Ok(scene) = scenes.get(scene_entity) else {
            continue;
        };
        extracted.layers.push(ExtractedHudLayerSurface {
            scene: scene.clone(),
            target_image,
        });
    }
}

fn render_hud_layer_surfaces(
    extracted: Res<ExtractedHudLayerSurfaces>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    renderer: Res<VelloRenderer>,
    render_settings: Res<VelloRenderSettings>,
) {
    for layer in &extracted.layers {
        let Some(gpu_image) = gpu_images.get(layer.target_image.id()) else {
            continue;
        };
        let mut scene_buffer = vello::Scene::new();
        scene_buffer.append(
            &layer.scene,
            Some(Affine::translate((
                f64::from(gpu_image.size.width) * 0.5,
                f64::from(gpu_image.size.height) * 0.5,
            ))),
        );
        renderer
            .lock()
            .expect("vello renderer lock should not be poisoned")
            .render_to_texture(
                device.wgpu_device(),
                &queue,
                &scene_buffer,
                &gpu_image.texture_view,
                &vello::RenderParams {
                    base_color: vello::peniko::Color::TRANSPARENT,
                    width: gpu_image.size.width,
                    height: gpu_image.size.height,
                    antialiasing_method: render_settings.antialiasing,
                },
            )
            .expect("HUD layer render_to_texture should succeed");
    }
}
