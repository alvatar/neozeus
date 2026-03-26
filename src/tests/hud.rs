use super::{
    fake_runtime_spawner, insert_default_hud_resources, insert_terminal_manager_resources,
    insert_test_hud_state, pressed_text, snapshot_test_hud_state, temp_dir, test_bridge,
    FakeDaemonClient,
};
use crate::hud::{
    agent_list_bloom_layer, agent_list_bloom_z, agent_row_rect, agent_rows, apply_persisted_layout,
    apply_terminal_focus_requests, apply_terminal_lifecycle_requests, apply_terminal_task_requests,
    apply_visibility_requests, debug_toolbar_buttons, dispatch_hud_pointer_click,
    dispatch_hud_scroll, handle_hud_module_shortcuts, handle_hud_pointer_input, hud_needs_redraw,
    kill_active_terminal, message_box_action_buttons, message_box_rect, parse_persisted_hud_state,
    resolve_agent_label, resolve_agent_list_bloom_debug_previews,
    resolve_agent_list_bloom_intensity, resolve_hud_layout_path_with, save_hud_layout_if_dirty,
    serialize_persisted_hud_state, task_dialog_action_buttons, AgentDirectory,
    AgentListBloomCameraMarker, AgentListBloomCompositeMarker, AgentListBloomSourceKind,
    AgentListBloomSourceSegment, AgentListBloomSourceSprite, AgentListRowSection, HudBloomSettings,
    HudDragState, HudIntent, HudModuleId, HudModuleModel, HudOffscreenCompositor,
    HudPersistenceState, HudRect, HudState, HudWidgetBloom, PersistedHudModuleState,
    PersistedHudState, TerminalFocusRequest, TerminalLifecycleRequest, TerminalVisibilityPolicy,
    TerminalVisibilityRequest, TerminalVisibilityState, AGENT_LIST_BLOOM_RED_B,
    AGENT_LIST_BLOOM_RED_G, AGENT_LIST_BLOOM_RED_R, AGENT_LIST_BORDER_ORANGE_B,
    AGENT_LIST_BORDER_ORANGE_G, AGENT_LIST_BORDER_ORANGE_R,
};
use crate::terminals::{
    TerminalManager, TerminalNotesState, TerminalPanel, TerminalPanelFrame,
    TerminalPresentationStore, TerminalSessionPersistenceState, TerminalViewState,
};
use bevy::{
    camera::{
        visibility::{NoFrustumCulling, RenderLayers},
        RenderTarget,
    },
    ecs::system::RunSystemOnce,
    input::{keyboard::KeyboardInput, mouse::MouseWheel},
    mesh::VertexAttributeValues,
    prelude::*,
    render::render_resource::TextureFormat,
    sprite_render::{AlphaMode2d, Material2d, MeshMaterial2d},
    window::{PrimaryWindow, RequestRedraw, WindowResolution},
};
use bevy_vello::render::VelloCanvasMaterial;
use std::{fs, path::PathBuf, sync::Arc, time::Duration};

fn init_hud_commands(world: &mut World) {
    world.init_resource::<Messages<HudIntent>>();
}

fn drain_hud_commands(world: &mut World) -> Vec<HudIntent> {
    world
        .run_system_once(|mut reader: bevy::prelude::MessageReader<HudIntent>| {
            reader.read().cloned().collect::<Vec<_>>()
        })
        .unwrap()
}

#[test]
fn setup_hud_requests_initial_redraw() {
    let mut world = World::default();
    insert_default_hud_resources(&mut world);
    world.insert_resource(HudPersistenceState::default());
    world.insert_resource(HudOffscreenCompositor::default());
    world.insert_resource(Assets::<Mesh>::default());
    world.insert_resource(Assets::<VelloCanvasMaterial>::default());
    world.init_resource::<Messages<RequestRedraw>>();

    world.run_system_once(crate::hud::setup_hud).unwrap();

    let redraws = world.resource::<Messages<RequestRedraw>>();
    assert_eq!(redraws.len(), 1);
    assert_eq!(
        world
            .query::<&crate::hud::HudVectorSceneMarker>()
            .iter(&world)
            .count(),
        1
    );
    assert_eq!(
        world
            .query::<&crate::hud::HudCompositeLayerMarker>()
            .iter(&world)
            .count(),
        1
    );
    assert_eq!(
        world
            .query::<&crate::hud::HudModalVectorSceneMarker>()
            .iter(&world)
            .count(),
        1
    );
    let mut camera_query = world
        .query_filtered::<(&Camera, &RenderLayers), With<crate::hud::HudCompositeCameraMarker>>();
    let (camera, layers) = camera_query.single(&world).unwrap();
    assert_eq!(camera.order, 50);
    assert!(layers.intersects(&RenderLayers::layer(crate::hud::HUD_COMPOSITE_RENDER_LAYER,)));

    let mut modal_camera_query =
        world.query_filtered::<(&Camera, &RenderLayers), With<crate::hud::HudModalCameraMarker>>();
    let (modal_camera, modal_layers) = modal_camera_query.single(&world).unwrap();
    assert_eq!(modal_camera.order, crate::hud::HUD_MODAL_CAMERA_ORDER);
    assert!(modal_layers.intersects(&RenderLayers::layer(crate::hud::HUD_MODAL_RENDER_LAYER)));
}

#[test]
fn setup_hud_widget_bloom_spawns_camera_and_composite_sprite() {
    let mut world = World::default();
    world.insert_resource(HudBloomSettings::default());
    world.insert_resource(HudWidgetBloom::default());
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(Assets::<Mesh>::default());
    world.insert_resource(Assets::<crate::hud::AgentListBloomBlurMaterial>::default());
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..default()
        },
        PrimaryWindow,
    ));

    world
        .run_system_once(crate::hud::setup_hud_widget_bloom)
        .unwrap();

    assert_eq!(
        world
            .query::<&AgentListBloomCameraMarker>()
            .iter(&world)
            .count(),
        1
    );

    let layer = agent_list_bloom_layer();
    let mut camera_query =
        world.query_filtered::<(&RenderLayers, &RenderTarget), With<AgentListBloomCameraMarker>>();
    let (layers, target) = camera_query.single(&world).unwrap();
    assert!(layers.intersects(&RenderLayers::layer(layer)));

    let RenderTarget::Image(handle) = target else {
        panic!("bloom camera must render to image target");
    };
    let bloom_image_handle = handle.handle.clone();
    let image_format = {
        let images = world.resource::<Assets<Image>>();
        images
            .get(bloom_image_handle.id())
            .expect("bloom image exists")
            .texture_descriptor
            .format
    };
    assert_eq!(image_format, TextureFormat::Rgba16Float);

    let mut composite_query = world.query::<(
        &Transform,
        &Visibility,
        &Sprite,
        &AgentListBloomCompositeMarker,
    )>();
    let (transform, visibility, sprite, _) = composite_query.single(&world).unwrap();
    assert_eq!(transform.translation.z, agent_list_bloom_z());
    assert_eq!(visibility, &Visibility::Hidden);
    let composite_image_format = {
        let images = world.resource::<Assets<Image>>();
        images
            .get(sprite.image.id())
            .expect("bloom composite image exists")
            .texture_descriptor
            .format
    };
    assert_eq!(composite_image_format, TextureFormat::Rgba16Float);
}
#[test]
fn setup_hud_widget_bloom_uses_logical_window_size_for_targets() {
    let mut world = World::default();
    world.insert_resource(HudBloomSettings::default());
    world.insert_resource(HudWidgetBloom::default());
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(Assets::<Mesh>::default());
    world.insert_resource(Assets::<crate::hud::AgentListBloomBlurMaterial>::default());
    world.spawn((
        Window {
            resolution: WindowResolution::new(1400, 900).with_scale_factor_override(2.0),
            ..default()
        },
        PrimaryWindow,
    ));

    world
        .run_system_once(crate::hud::setup_hud_widget_bloom)
        .unwrap();

    let target_handles = {
        let mut target_query = world.query::<&RenderTarget>();
        target_query
            .iter(&world)
            .filter_map(|target| match target {
                RenderTarget::Image(handle) => Some(handle.handle.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let images = world.resource::<Assets<Image>>();
    let target_images = target_handles
        .iter()
        .filter_map(|handle| images.get(handle.id()))
        .collect::<Vec<_>>();
    let target_sizes = target_images
        .iter()
        .map(|image| image.texture_descriptor.size)
        .collect::<Vec<_>>();
    assert!(target_sizes.iter().all(|size| size.width == 700));
    assert!(target_sizes.iter().all(|size| size.height == 450));
    assert!(target_images
        .iter()
        .all(|image| image.texture_descriptor.format == TextureFormat::Rgba16Float));
}

#[test]
fn sync_structural_hud_layout_docks_agent_list_to_full_height_left_column() {
    let mut world = World::default();
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudModuleId::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    insert_test_hud_state(&mut world, hud_state);
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..default()
        },
        PrimaryWindow,
    ));

    world
        .run_system_once(crate::hud::sync_structural_hud_layout)
        .unwrap();

    let expected_rect = {
        let mut query = world.query_filtered::<&Window, With<PrimaryWindow>>();
        crate::hud::docked_agent_list_rect(query.single(&world).unwrap())
    };
    let hud_state = snapshot_test_hud_state(&world);
    let module = hud_state.get(HudModuleId::AgentList).unwrap();
    assert_eq!(module.shell.current_rect, expected_rect);
}

#[test]
fn agent_list_reference_colors_match_requested_values() {
    assert_eq!(
        (
            AGENT_LIST_BORDER_ORANGE_R,
            AGENT_LIST_BORDER_ORANGE_G,
            AGENT_LIST_BORDER_ORANGE_B
        ),
        (225, 129, 10)
    );
    assert_eq!(
        (
            AGENT_LIST_BLOOM_RED_R,
            AGENT_LIST_BLOOM_RED_G,
            AGENT_LIST_BLOOM_RED_B
        ),
        (143, 37, 15)
    );
}

#[test]
fn parses_agent_bloom_intensity_override() {
    assert_eq!(resolve_agent_list_bloom_intensity(None), 0.10);
    assert_eq!(resolve_agent_list_bloom_intensity(Some("")), 0.10);
    assert_eq!(resolve_agent_list_bloom_intensity(Some("2.0")), 2.0);
    assert_eq!(resolve_agent_list_bloom_intensity(Some(" 0.0 ")), 0.0);
    assert_eq!(resolve_agent_list_bloom_intensity(Some("-1")), 0.10);
    assert_eq!(resolve_agent_list_bloom_intensity(Some("abc")), 0.10);
}

#[test]
fn parses_agent_bloom_debug_previews_override() {
    assert!(!resolve_agent_list_bloom_debug_previews(None));
    assert!(!resolve_agent_list_bloom_debug_previews(Some("")));
    assert!(resolve_agent_list_bloom_debug_previews(Some("1")));
    assert!(resolve_agent_list_bloom_debug_previews(Some(" true ")));
    assert!(resolve_agent_list_bloom_debug_previews(Some("on")));
    assert!(!resolve_agent_list_bloom_debug_previews(Some("0")));
    assert!(!resolve_agent_list_bloom_debug_previews(Some("false")));
}

#[test]
fn bloom_blur_material_writes_offscreen_passes_opaquely() {
    let material = crate::hud::AgentListBloomBlurMaterial {
        image: default(),
        uniform: crate::hud::AgentListBloomBlurUniform {
            texel_step_gain: Vec4::ZERO,
        },
    };
    assert_eq!(Material2d::alpha_mode(&material), AlphaMode2d::Opaque);
}

#[test]
fn agent_row_rect_splits_main_and_marker_geometry() {
    let row = HudRect {
        x: 40.0,
        y: 120.0,
        w: 220.0,
        h: 28.0,
    };
    let main = agent_row_rect(row, AgentListRowSection::Main);
    let marker = agent_row_rect(row, AgentListRowSection::Marker);
    let accent = agent_row_rect(row, AgentListRowSection::Accent);
    assert!(main.w > marker.w);
    assert!(main.x < marker.x);
    assert_eq!(main.y, row.y + 2.0);
    assert_eq!(marker.y, row.y + 2.0);
    assert_eq!(main.h, marker.h);
    assert_eq!(accent.x, row.x + 3.0);
    assert_eq!(accent.y, row.y + 3.0);
    assert_eq!(accent.w, 8.0);
    assert_eq!(accent.h, row.h - 6.0);
}

#[test]
fn sync_hud_offscreen_compositor_hides_vello_canvas_and_binds_texture() {
    let mut world = World::default();
    world.insert_resource(HudOffscreenCompositor::default());
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(Assets::<VelloCanvasMaterial>::default());
    world.insert_resource(Assets::<Mesh>::default());
    let texture = world.resource_mut::<Assets<Image>>().add(Image::default());
    {
        let mut images = world.resource_mut::<Assets<Image>>();
        let image = images.get_mut(&texture).unwrap();
        image.resize(bevy::render::render_resource::Extent3d {
            width: 1400,
            height: 900,
            depth_or_array_layers: 1,
        });
    }
    let material = world
        .resource_mut::<Assets<VelloCanvasMaterial>>()
        .add(VelloCanvasMaterial {
            texture: texture.clone(),
        });
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..default()
        },
        PrimaryWindow,
    ));
    let source_canvas = world
        .spawn((
            MeshMaterial2d::<VelloCanvasMaterial>(material),
            Visibility::Visible,
        ))
        .id();
    world
        .run_system_once(
            |mut commands: Commands,
             mut compositor: ResMut<HudOffscreenCompositor>,
             mut meshes: ResMut<Assets<Mesh>>,
             mut composite_materials: ResMut<Assets<VelloCanvasMaterial>>| {
                crate::hud::setup_hud_offscreen_compositor(
                    &mut commands,
                    &mut compositor,
                    &mut meshes,
                    &mut composite_materials,
                );
            },
        )
        .unwrap();

    world
        .run_system_once(crate::hud::sync_hud_offscreen_compositor)
        .unwrap();

    assert_eq!(
        world.get::<Visibility>(source_canvas),
        Some(&Visibility::Hidden)
    );

    let mut camera_query = world
        .query_filtered::<(&Camera, &RenderLayers), With<crate::hud::HudCompositeCameraMarker>>();
    let (camera, layers) = camera_query.single(&world).unwrap();
    assert_eq!(camera.order, 50);
    assert!(layers.intersects(&RenderLayers::layer(crate::hud::HUD_COMPOSITE_RENDER_LAYER,)));

    let mut quad_query = world.query::<(
        &crate::hud::HudCompositeLayerMarker,
        &MeshMaterial2d<VelloCanvasMaterial>,
        &Transform,
        &Visibility,
        &RenderLayers,
        &NoFrustumCulling,
    )>();
    let (marker, composite_material, transform, visibility, quad_layers, _) =
        quad_query.single(&world).unwrap();
    assert_eq!(marker.id, crate::hud::HudCompositeLayerId::MainHud);
    let composite_texture = {
        let materials = world.resource::<Assets<VelloCanvasMaterial>>();
        materials
            .get(composite_material.id())
            .expect("composite material exists")
            .texture
            .clone()
    };
    assert_eq!(composite_texture, texture);
    assert_eq!(transform.scale, Vec3::ONE);
    assert_eq!(transform.translation, Vec3::ZERO);
    assert_eq!(visibility, &Visibility::Visible);
    assert!(quad_layers.intersects(&RenderLayers::layer(crate::hud::HUD_COMPOSITE_RENDER_LAYER,)));
}

#[test]
fn sync_hud_offscreen_compositor_leaves_modal_vello_canvas_visible() {
    let mut world = World::default();
    world.insert_resource(HudOffscreenCompositor::default());
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(Assets::<VelloCanvasMaterial>::default());
    world.insert_resource(Assets::<Mesh>::default());
    let texture = world.resource_mut::<Assets<Image>>().add(Image::default());
    {
        let mut images = world.resource_mut::<Assets<Image>>();
        let image = images.get_mut(&texture).unwrap();
        image.resize(bevy::render::render_resource::Extent3d {
            width: 1400,
            height: 900,
            depth_or_array_layers: 1,
        });
    }
    let material = world
        .resource_mut::<Assets<VelloCanvasMaterial>>()
        .add(VelloCanvasMaterial {
            texture: texture.clone(),
        });
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..default()
        },
        PrimaryWindow,
    ));
    let modal_canvas = world
        .spawn((
            MeshMaterial2d::<VelloCanvasMaterial>(material),
            Visibility::Visible,
            crate::hud::HudModalVectorSceneMarker,
        ))
        .id();
    world
        .run_system_once(
            |mut commands: Commands,
             mut compositor: ResMut<HudOffscreenCompositor>,
             mut meshes: ResMut<Assets<Mesh>>,
             mut composite_materials: ResMut<Assets<VelloCanvasMaterial>>| {
                crate::hud::setup_hud_offscreen_compositor(
                    &mut commands,
                    &mut compositor,
                    &mut meshes,
                    &mut composite_materials,
                );
            },
        )
        .unwrap();

    world
        .run_system_once(crate::hud::sync_hud_offscreen_compositor)
        .unwrap();

    assert_eq!(
        world.get::<Visibility>(modal_canvas),
        Some(&Visibility::Visible)
    );
}

#[test]
fn hud_composite_quad_matches_upstream_vello_canvas_contract() {
    assert_eq!(crate::hud::HUD_COMPOSITE_FOREGROUND_Z, 0.0);

    let mut world = World::default();
    world.insert_resource(HudOffscreenCompositor::default());
    world.insert_resource(Assets::<VelloCanvasMaterial>::default());
    world.insert_resource(Assets::<Mesh>::default());
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..default()
        },
        PrimaryWindow,
    ));

    world
        .run_system_once(
            |mut commands: Commands,
             mut compositor: ResMut<HudOffscreenCompositor>,
             mut meshes: ResMut<Assets<Mesh>>,
             mut composite_materials: ResMut<Assets<VelloCanvasMaterial>>| {
                crate::hud::setup_hud_offscreen_compositor(
                    &mut commands,
                    &mut compositor,
                    &mut meshes,
                    &mut composite_materials,
                );
            },
        )
        .unwrap();

    let mesh_handle = world
        .query::<&Mesh2d>()
        .single(&world)
        .expect("composite mesh exists")
        .0
        .clone();
    let meshes = world.resource::<Assets<Mesh>>();
    let mesh = meshes.get(&mesh_handle).expect("mesh asset exists");
    let positions = mesh
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .expect("positions present")
        .as_float3()
        .expect("positions are float3");
    let uvs = match mesh.attribute(Mesh::ATTRIBUTE_UV_0).expect("uvs present") {
        VertexAttributeValues::Float32x2(values) => values.as_slice(),
        other => panic!("unexpected uv attribute format: {other:?}"),
    };
    assert_eq!(
        positions,
        &[
            [-1.0, -1.0, 0.0],
            [1.0, -1.0, 0.0],
            [1.0, 1.0, 0.0],
            [-1.0, 1.0, 0.0],
        ]
    );
    assert_eq!(uvs, &[[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [1.0, 1.0]]);
}

#[test]
fn upstream_vello_present_contract_preserves_target_orange_bytes() {
    fn srgb_to_linear_channel(value: u8) -> f32 {
        let srgb = value as f32 / 255.0;
        if srgb <= 0.04045 {
            srgb / 12.92
        } else {
            ((srgb + 0.055) / 1.055).powf(2.4)
        }
    }

    fn linear_to_srgb_channel(value: f32) -> u8 {
        let srgb = if value <= 0.0031308 {
            value * 12.92
        } else {
            1.055 * value.powf(1.0 / 2.4) - 0.055
        };
        (srgb.clamp(0.0, 1.0) * 255.0).round() as u8
    }

    let target = (225u8, 129u8, 10u8);
    let visible = (
        linear_to_srgb_channel(srgb_to_linear_channel(target.0)),
        linear_to_srgb_channel(srgb_to_linear_channel(target.1)),
        linear_to_srgb_channel(srgb_to_linear_channel(target.2)),
    );
    let wrong = (255u8, 177u8, 18u8);

    assert_eq!(visible, target);
    let target_dist = ((visible.0 as f32 - target.0 as f32).powi(2)
        + (visible.1 as f32 - target.1 as f32).powi(2)
        + (visible.2 as f32 - target.2 as f32).powi(2))
    .sqrt();
    let wrong_dist = ((visible.0 as f32 - wrong.0 as f32).powi(2)
        + (visible.1 as f32 - wrong.1 as f32).powi(2)
        + (visible.2 as f32 - wrong.2 as f32).powi(2))
    .sqrt();
    assert!(target_dist < wrong_dist);
}

#[test]
fn hud_layout_path_prefers_xdg_then_home() {
    assert_eq!(
        resolve_hud_layout_path_with(Some("/tmp/xdg"), Some("/tmp/home")),
        Some(PathBuf::from("/tmp/xdg/neozeus/hud-layout.v1"))
    );
    assert_eq!(
        resolve_hud_layout_path_with(None, Some("/tmp/home")),
        Some(PathBuf::from("/tmp/home/.config/neozeus/hud-layout.v1"))
    );
    assert_eq!(resolve_hud_layout_path_with(None, None), None);
}

#[test]
fn hud_layout_parse_and_serialize_roundtrip() {
    let mut persisted = PersistedHudState::default();
    persisted.modules.insert(
        HudModuleId::AgentList,
        PersistedHudModuleState {
            enabled: true,
            rect: HudRect {
                x: 24.0,
                y: 96.0,
                w: 300.0,
                h: 420.0,
            },
        },
    );
    let text = serialize_persisted_hud_state(&persisted);
    assert_eq!(parse_persisted_hud_state(&text), persisted);
}

#[test]
fn hud_layout_v1_parser_remains_backward_compatible() {
    let persisted =
        parse_persisted_hud_state("version 1\nAgentList enabled=1 x=24 y=96 w=300 h=420\n");
    let module = persisted.modules.get(&HudModuleId::AgentList).unwrap();
    assert!(module.enabled);
    assert_eq!(module.rect.w, 300.0);
}

#[test]
fn apply_persisted_layout_overrides_defaults() {
    let mut persisted = PersistedHudState::default();
    persisted.modules.insert(
        HudModuleId::AgentList,
        PersistedHudModuleState {
            enabled: false,
            rect: HudRect {
                x: 11.0,
                y: 22.0,
                w: 333.0,
                h: 444.0,
            },
        },
    );
    let hud_state =
        apply_persisted_layout(crate::hud::HUD_MODULE_DEFINITIONS.as_slice(), &persisted);
    let module = hud_state.get(HudModuleId::AgentList).unwrap();
    assert!(!module.shell.enabled);
    assert_eq!(module.shell.target_rect.x, 11.0);
    assert_eq!(module.shell.target_rect.w, 333.0);
}

#[test]
fn reset_module_restores_default_toolbar_state() {
    let mut hud_state = HudState::default();
    let mut module =
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[0]);
    module.shell.enabled = false;
    module.shell.target_alpha = 0.0;
    module.shell.current_alpha = 0.0;
    module.shell.target_rect = HudRect {
        x: 1800.0,
        y: 1200.0,
        w: 10.0,
        h: 10.0,
    };
    module.shell.current_rect = module.shell.target_rect;
    hud_state.insert(HudModuleId::DebugToolbar, module);

    hud_state.reset_module(HudModuleId::DebugToolbar);

    let module = hud_state.get(HudModuleId::DebugToolbar).unwrap();
    assert!(module.shell.enabled);
    assert_eq!(
        module.shell.target_rect,
        crate::hud::HUD_MODULE_DEFINITIONS[0].default_rect
    );
    assert_eq!(module.shell.current_alpha, 1.0);
    assert!(hud_state.dirty_layout);
}

#[test]
fn plain_digit_module_shortcut_toggles_module() {
    let mut world = World::default();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(TerminalManager::default());
    insert_default_hud_resources(&mut world);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::Digit1, Some("1")));

    world.run_system_once(handle_hud_module_shortcuts).unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![HudIntent::ToggleModule(HudModuleId::AgentList)]
    );
}

#[test]
fn plain_j_navigates_to_next_agent_and_isolates_it() {
    let mut world = World::default();
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);
    manager.focus_terminal(id_one);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(ButtonInput::<KeyCode>::default());
    insert_default_hud_resources(&mut world);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyJ, Some("j")));

    world.run_system_once(handle_hud_module_shortcuts).unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![
            HudIntent::FocusTerminal(id_two),
            HudIntent::HideAllButTerminal(id_two)
        ]
    );
}

#[test]
fn down_arrow_navigates_to_next_agent_and_isolates_it() {
    let mut world = World::default();
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);
    manager.focus_terminal(id_one);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(ButtonInput::<KeyCode>::default());
    insert_default_hud_resources(&mut world);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::ArrowDown, None));

    world.run_system_once(handle_hud_module_shortcuts).unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![
            HudIntent::FocusTerminal(id_two),
            HudIntent::HideAllButTerminal(id_two)
        ]
    );
}

#[test]
fn plain_k_navigates_to_previous_agent_and_isolates_it() {
    let mut world = World::default();
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);
    manager.focus_terminal(id_two);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(ButtonInput::<KeyCode>::default());
    insert_default_hud_resources(&mut world);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::KeyK, Some("k")));

    world.run_system_once(handle_hud_module_shortcuts).unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![
            HudIntent::FocusTerminal(id_one),
            HudIntent::HideAllButTerminal(id_one)
        ]
    );
}

#[test]
fn up_arrow_navigates_to_previous_agent_and_isolates_it() {
    let mut world = World::default();
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);
    manager.focus_terminal(id_two);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(ButtonInput::<KeyCode>::default());
    insert_default_hud_resources(&mut world);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::ArrowUp, None));

    world.run_system_once(handle_hud_module_shortcuts).unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![
            HudIntent::FocusTerminal(id_one),
            HudIntent::HideAllButTerminal(id_one)
        ]
    );
}

#[test]
fn focus_and_visibility_requests_request_redraw_immediately() {
    let mut world = World::default();
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal_without_focus(bridge);

    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    insert_default_hud_resources(&mut world);
    world.insert_resource(TerminalSessionPersistenceState::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.init_resource::<Messages<TerminalFocusRequest>>();
    world.init_resource::<Messages<TerminalVisibilityRequest>>();
    world.init_resource::<Messages<RequestRedraw>>();

    world
        .resource_mut::<Messages<TerminalFocusRequest>>()
        .write(TerminalFocusRequest { terminal_id: id });
    world
        .run_system_once(apply_terminal_focus_requests)
        .unwrap();

    assert_eq!(
        world
            .resource::<crate::terminals::TerminalFocusState>()
            .active_id(),
        Some(id)
    );
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);

    world
        .resource_mut::<Messages<TerminalVisibilityRequest>>()
        .write(TerminalVisibilityRequest::Isolate(id));
    world.run_system_once(apply_visibility_requests).unwrap();

    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::Isolate(id)
    );
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 2);
}

#[test]
fn alt_shift_module_shortcut_still_resets_module() {
    let mut world = World::default();
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::AltLeft);
    keys.press(KeyCode::ShiftLeft);
    world.insert_resource(keys);
    world.insert_resource(TerminalManager::default());
    insert_default_hud_resources(&mut world);
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::Digit0, Some("0")));

    world.run_system_once(handle_hud_module_shortcuts).unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![HudIntent::ResetModule(HudModuleId::DebugToolbar)]
    );
}

#[test]
fn module_shortcuts_are_suppressed_while_direct_input_is_open() {
    let mut world = World::default();
    let mut hud_state = HudState::default();
    hud_state.open_direct_terminal_input(crate::terminals::TerminalId(1));
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(TerminalManager::default());
    world.insert_resource(ButtonInput::<KeyCode>::default());
    init_hud_commands(&mut world);
    world.init_resource::<Messages<KeyboardInput>>();
    world
        .resource_mut::<Messages<KeyboardInput>>()
        .write(pressed_text(KeyCode::Digit1, Some("1")));

    world.run_system_once(handle_hud_module_shortcuts).unwrap();

    assert!(drain_hud_commands(&mut world).is_empty());
}

#[test]
fn resolve_agent_label_prefers_directory_over_fallback() {
    let terminal_ids = [
        crate::terminals::TerminalId(1),
        crate::terminals::TerminalId(2),
    ];
    let mut directory = AgentDirectory::default();
    directory
        .labels
        .insert(crate::terminals::TerminalId(2), "oracle".into());

    assert_eq!(
        resolve_agent_label(&terminal_ids, &directory, crate::terminals::TerminalId(1)),
        "agent-1"
    );
    assert_eq!(
        resolve_agent_label(&terminal_ids, &directory, crate::terminals::TerminalId(2)),
        "oracle"
    );
}

#[test]
fn agent_rows_follow_terminal_order_and_focus() {
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);
    manager.focus_terminal(id_two);

    let shell_rect = HudRect {
        x: 24.0,
        y: 96.0,
        w: 300.0,
        h: 420.0,
    };
    let rows = agent_rows(
        shell_rect,
        0.0,
        None,
        &manager,
        &manager.clone_focus_state(),
        &AgentDirectory::default(),
    );
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].terminal_id, id_one);
    assert_eq!(rows[0].label, "agent-1");
    assert!(rows[0].rect.y > shell_rect.y + 20.0);
    assert!(rows[0].rect.x > shell_rect.x + 20.0);
    assert_eq!(rows[1].terminal_id, id_two);
    assert!(rows[1].focused);
    assert_eq!(rows[1].rect.y - rows[0].rect.y, 42.0);
}

#[test]
fn agent_rows_mark_hovered_terminal() {
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);

    let rows = agent_rows(
        HudRect {
            x: 24.0,
            y: 96.0,
            w: 300.0,
            h: 420.0,
        },
        0.0,
        Some(id_one),
        &manager,
        &manager.clone_focus_state(),
        &AgentDirectory::default(),
    );
    assert!(
        rows.iter()
            .find(|row| row.terminal_id == id_one)
            .unwrap()
            .hovered
    );
    assert!(
        !rows
            .iter()
            .find(|row| row.terminal_id == id_two)
            .unwrap()
            .hovered
    );
}

#[test]
fn sync_hud_widget_bloom_spawns_agent_list_source_sprites() {
    let mut world = World::default();
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    manager.create_terminal(bridge);
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudModuleId::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    insert_terminal_manager_resources(&mut world, manager);
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(AgentDirectory::default());
    world.insert_resource(HudBloomSettings::default());
    world.insert_resource(HudWidgetBloom::default());
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(Assets::<Mesh>::default());
    world.insert_resource(Assets::<crate::hud::AgentListBloomBlurMaterial>::default());
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..default()
        },
        PrimaryWindow,
    ));

    world
        .run_system_once(crate::hud::setup_hud_widget_bloom)
        .unwrap();
    world
        .run_system_once(crate::hud::sync_structural_hud_layout)
        .unwrap();
    world
        .run_system_once(crate::hud::sync_hud_widget_bloom)
        .unwrap();

    let source_sprites = world
        .query::<(&AgentListBloomSourceSprite, &Sprite)>()
        .iter(&world)
        .map(|(marker, sprite)| (*marker, sprite.clone()))
        .collect::<Vec<_>>();
    assert_eq!(source_sprites.len(), 8);
    assert_eq!(
        source_sprites
            .iter()
            .filter(|(sprite, _)| sprite.kind == AgentListBloomSourceKind::Main)
            .count(),
        4
    );
    assert_eq!(
        source_sprites
            .iter()
            .filter(|(sprite, _)| sprite.kind == AgentListBloomSourceKind::Marker)
            .count(),
        4
    );
    for segment in [
        AgentListBloomSourceSegment::Top,
        AgentListBloomSourceSegment::Right,
        AgentListBloomSourceSegment::Bottom,
        AgentListBloomSourceSegment::Left,
    ] {
        assert!(source_sprites.iter().any(|(sprite, _)| {
            sprite.kind == AgentListBloomSourceKind::Main && sprite.segment == segment
        }));
        assert!(source_sprites.iter().any(|(sprite, _)| {
            sprite.kind == AgentListBloomSourceKind::Marker && sprite.segment == segment
        }));
    }

    let expected_sizes = {
        let manager = world.resource::<TerminalManager>();
        let hud_state = snapshot_test_hud_state(&world);
        let directory = world.resource::<AgentDirectory>();
        let module = hud_state.get(HudModuleId::AgentList).unwrap();
        let crate::hud::HudModuleModel::AgentList(state) = &module.model else {
            panic!("agent list module model missing")
        };
        let row = agent_rows(
            module.shell.current_rect,
            state.scroll_offset,
            state.hovered_terminal,
            manager,
            &manager.clone_focus_state(),
            directory,
        )
        .into_iter()
        .next()
        .expect("agent row exists");
        let main = agent_row_rect(row.rect, AgentListRowSection::Main);
        let marker = agent_row_rect(row.rect, AgentListRowSection::Marker);
        let target_size = {
            let mut camera_query =
                world.query_filtered::<&RenderTarget, With<AgentListBloomCameraMarker>>();
            let RenderTarget::Image(handle) = camera_query.single(&world).unwrap() else {
                panic!("bloom target missing")
            };
            let images = world.resource::<Assets<Image>>();
            images
                .get(handle.handle.id())
                .expect("bloom target image exists")
                .texture_descriptor
                .size
        };
        let scale_x = target_size.width as f32 / 1400.0;
        let scale_y = target_size.height as f32 / 900.0;
        [
            Vec2::new(main.w * scale_x, 3.0 * scale_y),
            Vec2::new(3.0 * scale_x, main.h * scale_y),
            Vec2::new(marker.w * scale_x, 2.5 * scale_y),
            Vec2::new(2.5 * scale_x, marker.h * scale_y),
        ]
    };
    let actual_sizes = source_sprites
        .iter()
        .map(|(_, sprite)| sprite.custom_size.expect("source size exists"))
        .collect::<Vec<_>>();
    assert!(actual_sizes
        .iter()
        .all(|size| expected_sizes.contains(size)));

    let mut composite_query = world.query::<(
        &Visibility,
        &Transform,
        &Sprite,
        &AgentListBloomCompositeMarker,
    )>();
    let (visibility, transform, sprite, _) = composite_query.single(&world).unwrap();
    assert_eq!(visibility, &Visibility::Visible);
    assert_eq!(transform.translation.z, agent_list_bloom_z());
    assert_eq!(sprite.custom_size, Some(Vec2::new(1400.0, 900.0)));
}

#[test]
fn sync_hud_widget_bloom_hides_sources_and_composite_while_modal_is_visible() {
    let mut world = World::default();
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    manager.create_terminal(bridge);
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudModuleId::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    hud_state.message_box.visible = true;
    insert_terminal_manager_resources(&mut world, manager);
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(AgentDirectory::default());
    world.insert_resource(HudBloomSettings::default());
    world.insert_resource(HudWidgetBloom::default());
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(Assets::<Mesh>::default());
    world.insert_resource(Assets::<crate::hud::AgentListBloomBlurMaterial>::default());
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..default()
        },
        PrimaryWindow,
    ));

    world
        .run_system_once(crate::hud::setup_hud_widget_bloom)
        .unwrap();
    world
        .run_system_once(crate::hud::sync_structural_hud_layout)
        .unwrap();
    world
        .run_system_once(crate::hud::sync_hud_widget_bloom)
        .unwrap();

    assert_eq!(
        world
            .query::<&AgentListBloomSourceSprite>()
            .iter(&world)
            .count(),
        0
    );
    let mut composite_query = world.query::<(&Visibility, &AgentListBloomCompositeMarker)>();
    let (visibility, _) = composite_query.single(&world).unwrap();
    assert_eq!(visibility, &Visibility::Hidden);
}

#[test]
fn sync_hud_widget_bloom_only_uses_active_agent_source() {
    let mut world = World::default();
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);
    manager.focus_terminal(id_two);

    let mut hud_state = HudState::default();
    hud_state.insert(
        HudModuleId::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    insert_terminal_manager_resources(&mut world, manager);
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(AgentDirectory::default());
    world.insert_resource(HudBloomSettings::default());
    world.insert_resource(HudWidgetBloom::default());
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(Assets::<Mesh>::default());
    world.insert_resource(Assets::<crate::hud::AgentListBloomBlurMaterial>::default());
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..default()
        },
        PrimaryWindow,
    ));

    world
        .run_system_once(crate::hud::setup_hud_widget_bloom)
        .unwrap();
    world
        .run_system_once(crate::hud::sync_structural_hud_layout)
        .unwrap();
    world
        .run_system_once(crate::hud::sync_hud_widget_bloom)
        .unwrap();

    let source_sprites = world
        .query::<&AgentListBloomSourceSprite>()
        .iter(&world)
        .copied()
        .collect::<Vec<_>>();
    assert_eq!(source_sprites.len(), 8);
    assert!(source_sprites
        .iter()
        .all(|sprite| sprite.terminal_id == id_two));
    assert!(source_sprites
        .iter()
        .all(|sprite| sprite.terminal_id != id_one));
}

#[test]
fn agent_list_is_not_draggable() {
    let mut world = World::default();
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudModuleId::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    let mut window = Window {
        focused: true,
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    window.set_cursor_position(Some(Vec2::new(120.0, 16.0)));

    world.insert_resource(ButtonInput::<MouseButton>::default());
    world.insert_resource(Messages::<MouseWheel>::default());
    world.insert_resource(Messages::<RequestRedraw>::default());
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(TerminalManager::default());
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(AgentDirectory::default());
    init_hud_commands(&mut world);
    world.spawn((window, PrimaryWindow));
    world
        .run_system_once(crate::hud::sync_structural_hud_layout)
        .unwrap();

    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    world.run_system_once(handle_hud_pointer_input).unwrap();

    let hud_state = snapshot_test_hud_state(&world);
    assert!(hud_state.drag.is_none());
    assert!(!hud_state.dirty_layout);
}

#[test]
fn message_box_rect_is_top_aligned_and_shorter() {
    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };

    let rect = message_box_rect(&window);
    assert!((rect.w - 1176.0).abs() < 0.01);
    assert!((rect.h - 468.0).abs() < 0.01);
    assert!((rect.x - 112.0).abs() < 0.01);
    assert!((rect.y - 8.0).abs() < 0.01);
}

#[test]
fn clicking_task_dialog_clear_done_button_persists_updated_text() {
    let mut world = World::default();
    let terminal_id = crate::terminals::TerminalId(7);
    let mut hud_state = HudState::default();
    hud_state.open_task_dialog(terminal_id, "- [x] done\n- [ ] keep");

    let mut window = Window {
        focused: true,
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let clear_done_button = task_dialog_action_buttons(&window)[0];
    window.set_cursor_position(Some(Vec2::new(
        clear_done_button.rect.x + 4.0,
        clear_done_button.rect.y + 4.0,
    )));

    world.insert_resource(ButtonInput::<MouseButton>::default());
    world.insert_resource(Messages::<MouseWheel>::default());
    world.insert_resource(Messages::<RequestRedraw>::default());
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(TerminalManager::default());
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(AgentDirectory::default());
    init_hud_commands(&mut world);
    world.spawn((window, PrimaryWindow));

    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    world.run_system_once(handle_hud_pointer_input).unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![HudIntent::ClearDoneTerminalTasks(terminal_id)]
    );
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
    let hud_state = snapshot_test_hud_state(&world);
    assert!(hud_state.task_dialog.visible);
    assert_eq!(hud_state.task_dialog.text, "- [x] done\n- [ ] keep");
}

#[test]
fn clear_done_task_request_updates_open_dialog_from_persisted_state() {
    let (bridge, _, _) = super::capturing_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal_with_session(bridge, "session-a".into());

    let mut notes_state = TerminalNotesState::default();
    assert!(notes_state.set_note_text("session-a", "- [x] done\n- [ ] keep"));

    let mut hud_state = HudState::default();
    hud_state.open_task_dialog(terminal_id, "stale local");

    let mut world = World::default();
    world.insert_resource(Time::<()>::default());
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(notes_state);
    insert_test_hud_state(&mut world, hud_state);
    world.init_resource::<Messages<crate::hud::TerminalTaskRequest>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<crate::hud::TerminalTaskRequest>>()
        .write(crate::hud::TerminalTaskRequest::ClearDone { terminal_id });

    world.run_system_once(apply_terminal_task_requests).unwrap();

    {
        let notes_state = world.resource::<TerminalNotesState>();
        assert_eq!(notes_state.note_text("session-a"), Some("- [ ] keep"));
    }
    let hud_state = snapshot_test_hud_state(&world);
    assert_eq!(hud_state.task_dialog.text, "- [ ] keep");
    assert!(hud_state.task_dialog.visible);
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}

#[test]
fn set_task_text_request_clears_persisted_task_presence_when_empty() {
    let (bridge, _, _) = super::capturing_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal_with_session(bridge, "session-a".into());

    let mut notes_state = TerminalNotesState::default();
    assert!(notes_state.set_note_text("session-a", "- [x] done"));
    assert!(notes_state.has_note_text("session-a"));

    let mut world = World::default();
    world.insert_resource(Time::<()>::default());
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(notes_state);
    insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<crate::hud::TerminalTaskRequest>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<crate::hud::TerminalTaskRequest>>()
        .write(crate::hud::TerminalTaskRequest::SetText {
            terminal_id,
            text: String::new(),
        });

    world.run_system_once(apply_terminal_task_requests).unwrap();

    let notes_state = world.resource::<TerminalNotesState>();
    assert_eq!(notes_state.note_text("session-a"), None);
    assert!(!notes_state.has_note_text("session-a"));
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}

#[test]
fn consume_next_task_request_sends_message_and_marks_task_done() {
    let (bridge, input_rx, _) = super::capturing_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal_with_session(bridge, "session-a".into());

    let mut notes_state = TerminalNotesState::default();
    assert!(notes_state.set_note_text("session-a", "- [ ] first\n  detail\n- [ ] second"));

    let mut world = World::default();
    world.insert_resource(Time::<()>::default());
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(notes_state);
    insert_default_hud_resources(&mut world);
    world.init_resource::<Messages<crate::hud::TerminalTaskRequest>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<crate::hud::TerminalTaskRequest>>()
        .write(crate::hud::TerminalTaskRequest::ConsumeNext { terminal_id });

    world.run_system_once(apply_terminal_task_requests).unwrap();

    assert_eq!(
        input_rx.try_recv().unwrap(),
        crate::terminals::TerminalCommand::SendCommand("first\n  detail".into())
    );
    assert_eq!(
        world
            .resource::<TerminalNotesState>()
            .note_text("session-a"),
        Some("- [x] first\n  detail\n- [ ] second")
    );
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}

#[test]
fn clicking_message_box_task_button_emits_append_task_intent() {
    let mut world = World::default();
    let terminal_id = crate::terminals::TerminalId(7);
    let mut hud_state = HudState::default();
    hud_state.open_message_box(terminal_id);
    hud_state.message_box.insert_text("follow up");

    let mut window = Window {
        focused: true,
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let append_button = message_box_action_buttons(&window)[0];
    window.set_cursor_position(Some(Vec2::new(
        append_button.rect.x + 4.0,
        append_button.rect.y + 4.0,
    )));

    world.insert_resource(ButtonInput::<MouseButton>::default());
    world.insert_resource(Messages::<MouseWheel>::default());
    world.insert_resource(Messages::<RequestRedraw>::default());
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(TerminalManager::default());
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(AgentDirectory::default());
    init_hud_commands(&mut world);
    world.spawn((window, PrimaryWindow));

    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    world.run_system_once(handle_hud_pointer_input).unwrap();

    assert_eq!(
        drain_hud_commands(&mut world),
        vec![HudIntent::AppendTerminalTask(
            terminal_id,
            "follow up".into()
        )]
    );
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
    assert!(!snapshot_test_hud_state(&world).message_box.visible);
}

#[test]
fn animate_hud_modules_moves_current_rect_and_alpha_toward_target() {
    let mut world = World::default();
    let mut hud_state = HudState::default();
    let mut module =
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]);
    module.shell.current_rect.x = 24.0;
    module.shell.target_rect.x = 124.0;
    module.shell.current_alpha = 0.2;
    module.shell.target_alpha = 1.0;
    hud_state.insert(HudModuleId::AgentList, module);
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    world.insert_resource(time);
    insert_test_hud_state(&mut world, hud_state);

    world
        .run_system_once(crate::hud::animate_hud_modules)
        .unwrap();

    let hud_state = snapshot_test_hud_state(&world);
    let module = hud_state.get(HudModuleId::AgentList).unwrap();
    assert!(module.shell.current_rect.x > 24.0);
    assert!(module.shell.current_rect.x < 124.0);
    assert!(module.shell.current_alpha > 0.2);
    assert!(module.shell.current_alpha < 1.0);
}

#[test]
fn saving_hud_layout_persists_target_rect() {
    let dir = temp_dir("neozeus-hud-layout-save");
    let path = dir.join("hud-layout.v1");
    let mut world = World::default();
    let mut hud_state = HudState::default();
    let mut module =
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]);
    module.shell.target_rect = HudRect {
        x: 321.0,
        y: 222.0,
        w: 333.0,
        h: 444.0,
    };
    hud_state.insert(HudModuleId::AgentList, module);
    hud_state.dirty_layout = true;
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(HudPersistenceState {
        path: Some(path.clone()),
        dirty_since_secs: None,
    });

    world.run_system_once(save_hud_layout_if_dirty).unwrap();
    world
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs(1));
    world.run_system_once(save_hud_layout_if_dirty).unwrap();

    let serialized = fs::read_to_string(&path).expect("hud layout file missing");
    assert!(serialized.contains("version 2"));
    assert!(serialized.contains("[module]"));
    assert!(serialized.contains("id=\"AgentList\""));
    assert!(serialized.contains("enabled=1"));
    assert!(serialized.contains("x=321"));
    assert!(serialized.contains("y=222"));
    assert!(serialized.contains("w=333"));
    assert!(serialized.contains("h=444"));
}

#[test]
fn clicking_debug_toolbar_button_emits_spawn_terminal_command() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    manager.create_terminal(bridge);
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudModuleId::DebugToolbar,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[0]),
    );
    hud_state.insert(
        HudModuleId::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    let mut emitted_commands = Vec::new();
    let buttons = debug_toolbar_buttons(
        HudRect {
            x: 24.0,
            y: 52.0,
            w: 920.0,
            h: 36.0,
        },
        &manager,
        &manager.clone_focus_state(),
        &Default::default(),
        &TerminalViewState::default(),
        &hud_state.layout_state(),
    );
    let new_terminal = buttons
        .iter()
        .find(|button| button.label == "new terminal")
        .expect("new terminal button missing");
    let click_point = Vec2::new(
        new_terminal.rect.x + new_terminal.rect.w * 0.5,
        new_terminal.rect.y + new_terminal.rect.h * 0.5,
    );

    dispatch_hud_pointer_click(
        HudModuleId::DebugToolbar,
        hud_state
            .get(HudModuleId::DebugToolbar)
            .map(|module| &module.model)
            .expect("toolbar module missing"),
        HudRect {
            x: 24.0,
            y: 52.0,
            w: 920.0,
            h: 36.0,
        },
        click_point,
        &manager,
        &manager.clone_focus_state(),
        &Default::default(),
        &TerminalViewState::default(),
        &AgentDirectory::default(),
        &hud_state.layout_state(),
        &mut emitted_commands,
    );

    assert_eq!(emitted_commands, vec![crate::hud::HudIntent::SpawnTerminal]);
}

#[test]
fn clicking_debug_toolbar_command_button_emits_terminal_command() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    manager.create_terminal(bridge);
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudModuleId::DebugToolbar,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[0]),
    );
    hud_state.insert(
        HudModuleId::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    let mut emitted_commands = Vec::new();
    let buttons = debug_toolbar_buttons(
        HudRect {
            x: 24.0,
            y: 52.0,
            w: 920.0,
            h: 36.0,
        },
        &manager,
        &manager.clone_focus_state(),
        &Default::default(),
        &TerminalViewState::default(),
        &hud_state.layout_state(),
    );
    let pwd = buttons
        .iter()
        .find(|button| button.label == "pwd")
        .expect("pwd button missing");
    let click_point = Vec2::new(pwd.rect.x + pwd.rect.w * 0.5, pwd.rect.y + pwd.rect.h * 0.5);

    dispatch_hud_pointer_click(
        HudModuleId::DebugToolbar,
        hud_state
            .get(HudModuleId::DebugToolbar)
            .map(|module| &module.model)
            .expect("toolbar module missing"),
        HudRect {
            x: 24.0,
            y: 52.0,
            w: 920.0,
            h: 36.0,
        },
        click_point,
        &manager,
        &manager.clone_focus_state(),
        &Default::default(),
        &TerminalViewState::default(),
        &AgentDirectory::default(),
        &hud_state.layout_state(),
        &mut emitted_commands,
    );

    assert_eq!(
        emitted_commands,
        vec![crate::hud::HudIntent::SendActiveTerminalCommand(
            "pwd".into()
        )]
    );
}

#[test]
fn clicking_agent_list_row_emits_focus_and_isolate_commands() {
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudModuleId::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    let mut emitted_commands = Vec::new();
    let rows = agent_rows(
        HudRect {
            x: 24.0,
            y: 132.0,
            w: 300.0,
            h: 392.0,
        },
        0.0,
        None,
        &manager,
        &manager.clone_focus_state(),
        &AgentDirectory::default(),
    );
    let target_row = rows
        .iter()
        .find(|row| row.terminal_id == id_two)
        .expect("agent row for second terminal missing");
    let click_point = Vec2::new(
        target_row.rect.x + target_row.rect.w * 0.5,
        target_row.rect.y + target_row.rect.h * 0.5,
    );

    dispatch_hud_pointer_click(
        HudModuleId::AgentList,
        hud_state
            .get(HudModuleId::AgentList)
            .map(|module| &module.model)
            .expect("agent list module missing"),
        HudRect {
            x: 24.0,
            y: 132.0,
            w: 300.0,
            h: 392.0,
        },
        click_point,
        &manager,
        &manager.clone_focus_state(),
        &Default::default(),
        &TerminalViewState::default(),
        &AgentDirectory::default(),
        &hud_state.layout_state(),
        &mut emitted_commands,
    );

    assert_eq!(emitted_commands.len(), 2);
    assert_eq!(
        emitted_commands[0],
        crate::hud::HudIntent::FocusTerminal(id_two)
    );
    assert_eq!(
        emitted_commands[1],
        crate::hud::HudIntent::HideAllButTerminal(id_two)
    );
}

#[test]
fn agent_list_scroll_clamps_to_content_height() {
    let mut model = HudModuleModel::AgentList(Default::default());
    let mut manager = TerminalManager::default();
    for _ in 0..5 {
        let (bridge, _) = test_bridge();
        manager.create_terminal(bridge);
    }

    dispatch_hud_scroll(
        HudModuleId::AgentList,
        &mut model,
        -500.0,
        &manager,
        HudRect {
            x: 24.0,
            y: 132.0,
            w: 300.0,
            h: 112.0,
        },
    );

    let HudModuleModel::AgentList(state) = model else {
        panic!("expected agent list model");
    };
    assert_eq!(state.scroll_offset, 84.0);
}

#[test]
fn debug_toolbar_buttons_include_module_toggle_entries() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    manager.create_terminal(bridge);
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudModuleId::DebugToolbar,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[0]),
    );
    hud_state.insert(
        HudModuleId::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    let buttons = debug_toolbar_buttons(
        HudRect {
            x: 24.0,
            y: 24.0,
            w: 920.0,
            h: 64.0,
        },
        &manager,
        &manager.clone_focus_state(),
        &Default::default(),
        &TerminalViewState::default(),
        &hud_state.layout_state(),
    );
    assert!(buttons.iter().any(|button| button.label == "0 toolbar"));
    assert!(buttons.iter().any(|button| button.label == "1 agents"));
}

#[test]
fn debug_toolbar_module_toggle_buttons_reflect_enabled_state() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    manager.create_terminal(bridge);
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudModuleId::DebugToolbar,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[0]),
    );
    hud_state.insert(
        HudModuleId::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    hud_state.set_module_enabled(HudModuleId::AgentList, false);

    let buttons = debug_toolbar_buttons(
        HudRect {
            x: 24.0,
            y: 24.0,
            w: 920.0,
            h: 64.0,
        },
        &manager,
        &manager.clone_focus_state(),
        &Default::default(),
        &TerminalViewState::default(),
        &hud_state.layout_state(),
    );

    let toolbar = buttons
        .iter()
        .find(|button| button.label == "0 toolbar")
        .expect("toolbar toggle button missing");
    let agents = buttons
        .iter()
        .find(|button| button.label == "1 agents")
        .expect("agent toggle button missing");
    assert!(toolbar.active);
    assert!(!agents.active);
}

#[test]
fn hud_state_topmost_enabled_at_prefers_frontmost_module() {
    let mut state = HudState::default();
    state.insert(
        HudModuleId::DebugToolbar,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[0]),
    );
    state.insert(
        HudModuleId::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    state.raise_to_front(HudModuleId::AgentList);

    assert_eq!(
        state.topmost_enabled_at(Vec2::new(40.0, 110.0)),
        Some(HudModuleId::AgentList)
    );
}

#[test]
fn hud_needs_redraw_when_drag_or_animation_is_active() {
    let mut state = HudState::default();
    state.insert(
        HudModuleId::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    assert!(!hud_needs_redraw(&state.layout_state()));
    state.drag = Some(HudDragState {
        module_id: HudModuleId::AgentList,
        grab_offset: Vec2::ZERO,
    });
    assert!(hud_needs_redraw(&state.layout_state()));
    state.drag = None;
    let module = state.get_mut(HudModuleId::AgentList).unwrap();
    module.shell.current_rect.x = 0.0;
    module.shell.target_rect.x = 10.0;
    assert!(hud_needs_redraw(&state.layout_state()));
}

#[test]
fn disabled_hud_module_still_requests_redraw_while_fading_out() {
    let mut state = HudState::default();
    state.insert(
        HudModuleId::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );

    state.set_module_enabled(HudModuleId::AgentList, false);

    let module = state.get(HudModuleId::AgentList).unwrap();
    assert!(!module.shell.enabled);
    assert!(module.shell.is_animating());
    assert!(hud_needs_redraw(&state.layout_state()));
}

#[test]
fn killing_active_terminal_selects_previous_terminal_in_creation_order() {
    let client = Arc::new(FakeDaemonClient::default());
    client.sessions.lock().unwrap().extend([
        "neozeus-session-a".to_owned(),
        "neozeus-session-b".to_owned(),
        "neozeus-session-c".to_owned(),
    ]);

    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let (bridge_three, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal_with_session(bridge_one, "neozeus-session-a".into());
    let id_two = manager.create_terminal_with_session(bridge_two, "neozeus-session-b".into());
    let id_three = manager.create_terminal_with_session(bridge_three, "neozeus-session-c".into());
    manager.focus_terminal(id_two);

    let mut store = TerminalPresentationStore::default();
    for id in [id_one, id_two, id_three] {
        store.register(
            id,
            crate::terminals::PresentedTerminal {
                image: Default::default(),
                texture_state: Default::default(),
                desired_texture_state: Default::default(),
                display_mode: Default::default(),
                uploaded_revision: 0,
                panel_entity: Entity::PLACEHOLDER,
                frame_entity: Entity::PLACEHOLDER,
            },
        );
    }

    let mut directory = AgentDirectory::default();
    directory.labels.insert(id_one, "one".into());
    directory.labels.insert(id_two, "two".into());
    directory.labels.insert(id_three, "three".into());

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(store);
    world.insert_resource(directory);
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(TerminalSessionPersistenceState::default());
    world.insert_resource(TerminalVisibilityState {
        policy: TerminalVisibilityPolicy::Isolate(id_two),
    });
    world.insert_resource(TerminalViewState::default());
    for id in [id_one, id_two, id_three] {
        let panel_entity = world.spawn((TerminalPanel { id },)).id();
        let frame_entity = world.spawn((TerminalPanelFrame { id },)).id();
        let mut store = world.resource_mut::<TerminalPresentationStore>();
        let presented = store.get_mut(id).expect("missing presented terminal");
        presented.panel_entity = panel_entity;
        presented.frame_entity = frame_entity;
    }

    world
        .run_system_once(
            |mut commands: Commands,
             time: Res<Time>,
             mut terminal_manager: ResMut<TerminalManager>,
             mut focus_state: ResMut<crate::terminals::TerminalFocusState>,
             mut presentation_store: ResMut<TerminalPresentationStore>,
             runtime_spawner: Res<crate::terminals::TerminalRuntimeSpawner>,
             mut agent_directory: ResMut<AgentDirectory>,
             mut session_persistence: ResMut<TerminalSessionPersistenceState>,
             mut visibility_state: ResMut<TerminalVisibilityState>,
             mut view_state: ResMut<TerminalViewState>| {
                let _ = kill_active_terminal(
                    &mut commands,
                    &time,
                    &mut terminal_manager,
                    &mut focus_state,
                    &mut presentation_store,
                    &runtime_spawner,
                    &mut agent_directory,
                    &mut session_persistence,
                    &mut visibility_state,
                    &mut view_state,
                );
            },
        )
        .unwrap();

    let manager = world.resource::<TerminalManager>();
    assert_eq!(manager.terminal_ids(), &[id_one, id_three]);
    assert_eq!(manager.active_id(), Some(id_one));
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::Isolate(id_one)
    );
    assert!(!world
        .resource::<AgentDirectory>()
        .labels
        .contains_key(&id_two));
}

#[test]
fn killing_first_active_terminal_selects_next_terminal() {
    let client = Arc::new(FakeDaemonClient::default());
    client.sessions.lock().unwrap().extend([
        "neozeus-session-a".to_owned(),
        "neozeus-session-b".to_owned(),
    ]);

    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal_with_session(bridge_one, "neozeus-session-a".into());
    let id_two = manager.create_terminal_with_session(bridge_two, "neozeus-session-b".into());
    manager.focus_terminal(id_one);

    let mut store = TerminalPresentationStore::default();
    for id in [id_one, id_two] {
        store.register(
            id,
            crate::terminals::PresentedTerminal {
                image: Default::default(),
                texture_state: Default::default(),
                desired_texture_state: Default::default(),
                display_mode: Default::default(),
                uploaded_revision: 0,
                panel_entity: Entity::PLACEHOLDER,
                frame_entity: Entity::PLACEHOLDER,
            },
        );
    }

    let mut directory = AgentDirectory::default();
    directory.labels.insert(id_one, "one".into());
    directory.labels.insert(id_two, "two".into());

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(store);
    world.insert_resource(directory);
    world.insert_resource(fake_runtime_spawner(client));
    world.insert_resource(TerminalSessionPersistenceState::default());
    world.insert_resource(TerminalVisibilityState {
        policy: TerminalVisibilityPolicy::Isolate(id_one),
    });
    world.insert_resource(TerminalViewState::default());
    for id in [id_one, id_two] {
        let panel_entity = world.spawn((TerminalPanel { id },)).id();
        let frame_entity = world.spawn((TerminalPanelFrame { id },)).id();
        let mut store = world.resource_mut::<TerminalPresentationStore>();
        let presented = store.get_mut(id).expect("missing presented terminal");
        presented.panel_entity = panel_entity;
        presented.frame_entity = frame_entity;
    }

    world
        .run_system_once(
            |mut commands: Commands,
             time: Res<Time>,
             mut terminal_manager: ResMut<TerminalManager>,
             mut focus_state: ResMut<crate::terminals::TerminalFocusState>,
             mut presentation_store: ResMut<TerminalPresentationStore>,
             runtime_spawner: Res<crate::terminals::TerminalRuntimeSpawner>,
             mut agent_directory: ResMut<AgentDirectory>,
             mut session_persistence: ResMut<TerminalSessionPersistenceState>,
             mut visibility_state: ResMut<TerminalVisibilityState>,
             mut view_state: ResMut<TerminalViewState>| {
                let _ = kill_active_terminal(
                    &mut commands,
                    &time,
                    &mut terminal_manager,
                    &mut focus_state,
                    &mut presentation_store,
                    &runtime_spawner,
                    &mut agent_directory,
                    &mut session_persistence,
                    &mut visibility_state,
                    &mut view_state,
                );
            },
        )
        .unwrap();

    let manager = world.resource::<TerminalManager>();
    assert_eq!(manager.terminal_ids(), &[id_two]);
    assert_eq!(manager.active_id(), Some(id_two));
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::Isolate(id_two)
    );
    assert!(!world
        .resource::<AgentDirectory>()
        .labels
        .contains_key(&id_one));
}

#[test]
fn killing_active_terminal_removes_runtime_presentation_and_labels() {
    let client = Arc::new(FakeDaemonClient::default());
    client
        .sessions
        .lock()
        .unwrap()
        .insert("neozeus-session-a".into());

    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal_with_session(bridge, "neozeus-session-a".into());
    manager.focus_terminal(id);

    let mut store = TerminalPresentationStore::default();
    store.register(
        id,
        crate::terminals::PresentedTerminal {
            image: Default::default(),
            texture_state: Default::default(),
            desired_texture_state: Default::default(),
            display_mode: Default::default(),
            uploaded_revision: 0,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut directory = AgentDirectory::default();
    directory.labels.insert(id, "oracle".into());

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(store);
    world.insert_resource(directory);
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(TerminalSessionPersistenceState::default());
    world.insert_resource(TerminalVisibilityState {
        policy: TerminalVisibilityPolicy::Isolate(id),
    });
    world.insert_resource(TerminalViewState::default());
    let panel_entity = world.spawn((TerminalPanel { id },)).id();
    let frame_entity = world.spawn((TerminalPanelFrame { id },)).id();
    {
        let mut store = world.resource_mut::<TerminalPresentationStore>();
        let presented = store.get_mut(id).expect("missing presented terminal");
        presented.panel_entity = panel_entity;
        presented.frame_entity = frame_entity;
    }

    world
        .run_system_once(
            |mut commands: Commands,
             time: Res<Time>,
             mut terminal_manager: ResMut<TerminalManager>,
             mut focus_state: ResMut<crate::terminals::TerminalFocusState>,
             mut presentation_store: ResMut<TerminalPresentationStore>,
             runtime_spawner: Res<crate::terminals::TerminalRuntimeSpawner>,
             mut agent_directory: ResMut<AgentDirectory>,
             mut session_persistence: ResMut<TerminalSessionPersistenceState>,
             mut visibility_state: ResMut<TerminalVisibilityState>,
             mut view_state: ResMut<TerminalViewState>| {
                let _ = kill_active_terminal(
                    &mut commands,
                    &time,
                    &mut terminal_manager,
                    &mut focus_state,
                    &mut presentation_store,
                    &runtime_spawner,
                    &mut agent_directory,
                    &mut session_persistence,
                    &mut visibility_state,
                    &mut view_state,
                );
            },
        )
        .unwrap();

    assert!(world
        .resource::<TerminalManager>()
        .terminal_ids()
        .is_empty());
    assert!(world
        .resource::<TerminalPresentationStore>()
        .get(id)
        .is_none());
    assert!(!world.resource::<AgentDirectory>().labels.contains_key(&id));
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::ShowAll
    );
    assert!(world
        .resource::<TerminalSessionPersistenceState>()
        .dirty_since_secs
        .is_some());
    assert!(client.sessions.lock().unwrap().is_empty());
    let panel_count = world.query::<&TerminalPanel>().iter(&world).count();
    let frame_count = world.query::<&TerminalPanelFrame>().iter(&world).count();
    assert_eq!(panel_count, 0);
    assert_eq!(frame_count, 0);
}

#[test]
fn spawn_shell_lifecycle_request_does_not_send_pi_command() {
    let client = Arc::new(FakeDaemonClient::default());
    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    world.insert_resource(Assets::<Image>::default());
    insert_terminal_manager_resources(&mut world, TerminalManager::default());
    insert_default_hud_resources(&mut world);
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(AgentDirectory::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(TerminalSessionPersistenceState::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.init_resource::<Messages<TerminalLifecycleRequest>>();
    world.init_resource::<Messages<RequestRedraw>>();

    world
        .resource_mut::<Messages<TerminalLifecycleRequest>>()
        .write(TerminalLifecycleRequest::SpawnShell);

    world
        .run_system_once(apply_terminal_lifecycle_requests)
        .unwrap();

    assert_eq!(world.resource::<TerminalManager>().terminal_ids().len(), 1);
    assert!(client.sent_commands.lock().unwrap().is_empty());
}

#[test]
fn killing_disconnected_active_terminal_removes_local_state_even_if_daemon_kill_fails() {
    let client = Arc::new(FakeDaemonClient::default());
    *client.fail_kill.lock().unwrap() = true;
    client.set_session_runtime(
        "neozeus-session-a",
        crate::terminals::TerminalRuntimeState::disconnected("dead session"),
    );

    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal_with_session(bridge, "neozeus-session-a".into());
    manager
        .get_mut(id)
        .expect("missing terminal")
        .snapshot
        .runtime = crate::terminals::TerminalRuntimeState::disconnected("dead session");
    manager.focus_terminal(id);

    let mut store = TerminalPresentationStore::default();
    store.register(
        id,
        crate::terminals::PresentedTerminal {
            image: Default::default(),
            texture_state: Default::default(),
            desired_texture_state: Default::default(),
            display_mode: Default::default(),
            uploaded_revision: 0,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut directory = AgentDirectory::default();
    directory.labels.insert(id, "oracle".into());

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(store);
    world.insert_resource(directory);
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(TerminalSessionPersistenceState::default());
    world.insert_resource(TerminalVisibilityState {
        policy: TerminalVisibilityPolicy::Isolate(id),
    });
    world.insert_resource(TerminalViewState::default());
    let panel_entity = world.spawn((TerminalPanel { id },)).id();
    let frame_entity = world.spawn((TerminalPanelFrame { id },)).id();
    {
        let mut store = world.resource_mut::<TerminalPresentationStore>();
        let presented = store.get_mut(id).expect("missing presented terminal");
        presented.panel_entity = panel_entity;
        presented.frame_entity = frame_entity;
    }

    world
        .run_system_once(
            |mut commands: Commands,
             time: Res<Time>,
             mut terminal_manager: ResMut<TerminalManager>,
             mut focus_state: ResMut<crate::terminals::TerminalFocusState>,
             mut presentation_store: ResMut<TerminalPresentationStore>,
             runtime_spawner: Res<crate::terminals::TerminalRuntimeSpawner>,
             mut agent_directory: ResMut<AgentDirectory>,
             mut session_persistence: ResMut<TerminalSessionPersistenceState>,
             mut visibility_state: ResMut<TerminalVisibilityState>,
             mut view_state: ResMut<TerminalViewState>| {
                let _ = kill_active_terminal(
                    &mut commands,
                    &time,
                    &mut terminal_manager,
                    &mut focus_state,
                    &mut presentation_store,
                    &runtime_spawner,
                    &mut agent_directory,
                    &mut session_persistence,
                    &mut visibility_state,
                    &mut view_state,
                );
            },
        )
        .unwrap();

    assert!(world
        .resource::<TerminalManager>()
        .terminal_ids()
        .is_empty());
    assert!(world
        .resource::<TerminalPresentationStore>()
        .get(id)
        .is_none());
    assert!(!world.resource::<AgentDirectory>().labels.contains_key(&id));
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::ShowAll
    );
    assert!(world
        .resource::<TerminalSessionPersistenceState>()
        .dirty_since_secs
        .is_some());
    let panel_count = world.query::<&TerminalPanel>().iter(&world).count();
    let frame_count = world.query::<&TerminalPanelFrame>().iter(&world).count();
    assert_eq!(panel_count, 0);
    assert_eq!(frame_count, 0);
}

#[test]
fn killing_active_terminal_preserves_local_state_when_tmux_kill_fails() {
    let client = Arc::new(FakeDaemonClient::default());
    *client.fail_kill.lock().unwrap() = true;
    client
        .sessions
        .lock()
        .unwrap()
        .insert("neozeus-session-a".into());

    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal_with_session(bridge, "neozeus-session-a".into());
    manager.focus_terminal(id);

    let mut store = TerminalPresentationStore::default();
    store.register(
        id,
        crate::terminals::PresentedTerminal {
            image: Default::default(),
            texture_state: Default::default(),
            desired_texture_state: Default::default(),
            display_mode: Default::default(),
            uploaded_revision: 0,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut directory = AgentDirectory::default();
    directory.labels.insert(id, "oracle".into());

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(store);
    world.insert_resource(directory);
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(TerminalSessionPersistenceState::default());
    world.insert_resource(TerminalVisibilityState {
        policy: TerminalVisibilityPolicy::Isolate(id),
    });
    world.insert_resource(TerminalViewState::default());
    let panel_entity = world.spawn((TerminalPanel { id },)).id();
    let frame_entity = world.spawn((TerminalPanelFrame { id },)).id();
    {
        let mut store = world.resource_mut::<TerminalPresentationStore>();
        let presented = store.get_mut(id).expect("missing presented terminal");
        presented.panel_entity = panel_entity;
        presented.frame_entity = frame_entity;
    }

    world
        .run_system_once(
            |mut commands: Commands,
             time: Res<Time>,
             mut terminal_manager: ResMut<TerminalManager>,
             mut focus_state: ResMut<crate::terminals::TerminalFocusState>,
             mut presentation_store: ResMut<TerminalPresentationStore>,
             runtime_spawner: Res<crate::terminals::TerminalRuntimeSpawner>,
             mut agent_directory: ResMut<AgentDirectory>,
             mut session_persistence: ResMut<TerminalSessionPersistenceState>,
             mut visibility_state: ResMut<TerminalVisibilityState>,
             mut view_state: ResMut<TerminalViewState>| {
                let _ = kill_active_terminal(
                    &mut commands,
                    &time,
                    &mut terminal_manager,
                    &mut focus_state,
                    &mut presentation_store,
                    &runtime_spawner,
                    &mut agent_directory,
                    &mut session_persistence,
                    &mut visibility_state,
                    &mut view_state,
                );
            },
        )
        .unwrap();

    assert_eq!(world.resource::<TerminalManager>().terminal_ids(), &[id]);
    assert!(world
        .resource::<TerminalPresentationStore>()
        .get(id)
        .is_some());
    assert!(world.resource::<AgentDirectory>().labels.contains_key(&id));
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::Isolate(id)
    );
    assert!(world
        .resource::<TerminalSessionPersistenceState>()
        .dirty_since_secs
        .is_none());
    let panel_count = world.query::<&TerminalPanel>().iter(&world).count();
    let frame_count = world.query::<&TerminalPanelFrame>().iter(&world).count();
    assert_eq!(panel_count, 1);
    assert_eq!(frame_count, 1);
}

#[test]
fn terminal_visibility_policy_defaults_to_show_all() {
    assert_eq!(
        TerminalVisibilityPolicy::default(),
        TerminalVisibilityPolicy::ShowAll
    );
}
