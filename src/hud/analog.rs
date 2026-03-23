use crate::hud::{HudModuleId, HudState};
use bevy::{
    prelude::*,
    reflect::TypePath,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d, MeshMaterial2d},
    window::PrimaryWindow,
};

use super::compositor::HUD_COMPOSITE_FOREGROUND_Z;

const AGENT_LIST_ANALOG_SHADER_PATH: &str = "shaders/hud_agent_list_analog.wgsl";
const AGENT_LIST_ANALOG_Z: f32 = HUD_COMPOSITE_FOREGROUND_Z + 0.2;

#[derive(Clone, Copy, Debug, ShaderType)]
pub(crate) struct AgentListAnalogUniform {
    pub(crate) tint: Vec4,
    pub(crate) settings: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(crate) struct AgentListAnalogMaterial {
    #[uniform(0)]
    pub(crate) uniform: AgentListAnalogUniform,
}

impl Default for AgentListAnalogMaterial {
    fn default() -> Self {
        Self {
            uniform: AgentListAnalogUniform {
                tint: Vec4::new(0.82, 0.18, 0.18, 1.0),
                settings: Vec4::new(0.018, 0.014, 0.010, 0.020),
            },
        }
    }
}

impl Material2d for AgentListAnalogMaterial {
    fn fragment_shader() -> ShaderRef {
        AGENT_LIST_ANALOG_SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

#[derive(Component)]
pub(crate) struct AgentListAnalogOverlayMarker;

fn overlay_transform(window: &Window, rect: crate::hud::HudRect) -> Transform {
    Transform::from_xyz(
        rect.x + rect.w * 0.5 - window.width() * 0.5,
        window.height() * 0.5 - (rect.y + rect.h * 0.5),
        AGENT_LIST_ANALOG_Z,
    )
    .with_scale(Vec3::new(rect.w.max(1.0), rect.h.max(1.0), 1.0))
}

pub(crate) fn setup_agent_list_analog_overlay(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<AgentListAnalogMaterial>>,
) {
    commands.spawn((
        Mesh2d(meshes.add(Rectangle::default())),
        MeshMaterial2d(materials.add(AgentListAnalogMaterial::default())),
        Transform::from_xyz(0.0, 0.0, AGENT_LIST_ANALOG_Z),
        Visibility::Hidden,
        AgentListAnalogOverlayMarker,
    ));
}

pub(crate) fn sync_agent_list_analog_overlay(
    primary_window: Single<&Window, With<PrimaryWindow>>,
    hud_state: Res<HudState>,
    mut overlays: Query<
        (
            &MeshMaterial2d<AgentListAnalogMaterial>,
            &mut Transform,
            &mut Visibility,
        ),
        With<AgentListAnalogOverlayMarker>,
    >,
    mut materials: ResMut<Assets<AgentListAnalogMaterial>>,
) {
    let Some(module) = hud_state.get(HudModuleId::AgentList) else {
        return;
    };
    let enabled = module.shell.enabled && module.shell.current_alpha > 0.01;

    for (material_handle, mut transform, mut visibility) in &mut overlays {
        *transform = overlay_transform(&primary_window, module.shell.current_rect);
        *visibility = if enabled {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };

        if let Some(material) = materials.get_mut(material_handle.id()) {
            material.uniform.settings = Vec4::new(0.018, 0.014, 0.010, 0.020);
        }
    }
}

#[cfg(test)]
pub(crate) fn agent_list_analog_z() -> f32 {
    AGENT_LIST_ANALOG_Z
}
