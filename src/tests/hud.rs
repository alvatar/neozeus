use super::{
    fake_runtime_spawner, insert_default_hud_resources, insert_terminal_manager_resources,
    insert_test_hud_state, pressed_text, snapshot_test_hud_state, test_bridge, FakeDaemonClient,
};
use crate::terminals::{
    kill_active_terminal_session_and_remove as kill_active_terminal, TerminalManager,
    TerminalNotesState, TerminalPanel, TerminalPanelFrame, TerminalPresentationStore,
    TerminalSessionPersistenceState, TerminalViewState,
};
use crate::{
    app::{
        AgentCommand as AppAgentCommand, AppCommand, ComposerCommand as AppComposerCommand,
        TaskCommand as AppTaskCommand, WidgetCommand,
    },
    hud::{
        agent_row_rect, agent_rows, debug_toolbar_buttons, dispatch_hud_pointer_click,
        dispatch_hud_scroll, handle_hud_module_shortcuts, handle_hud_pointer_input,
        hud_needs_redraw, message_box_action_buttons, message_box_rect, task_dialog_action_buttons,
        AgentListRowSection, AgentListRowView, AgentListView, ConversationListView, HudDragState,
        HudIntent, HudModuleModel, HudOffscreenCompositor, HudPersistenceState, HudRect, HudState,
        HudWidgetKey, TerminalVisibilityPolicy, TerminalVisibilityState, ThreadView,
    },
};
use bevy::{
    camera::visibility::{NoFrustumCulling, RenderLayers},
    ecs::system::RunSystemOnce,
    input::{keyboard::KeyboardInput, mouse::MouseWheel},
    mesh::VertexAttributeValues,
    prelude::*,
    sprite_render::MeshMaterial2d,
    window::{PrimaryWindow, RequestRedraw},
};
use bevy_vello::render::VelloCanvasMaterial;
use std::{sync::Arc, time::Duration};

/// Initializes the HUD/app command message resources in a test world.
fn init_hud_commands(world: &mut World) {
    world.init_resource::<Messages<HudIntent>>();
    world.init_resource::<Messages<AppCommand>>();
}

/// Drains queued HUD/app commands, projecting app commands back into the historical HUD vocabulary.
fn drain_hud_commands(world: &mut World) -> Vec<HudIntent> {
    let mut commands = world
        .run_system_once(|mut reader: bevy::prelude::MessageReader<HudIntent>| {
            reader.read().cloned().collect::<Vec<_>>()
        })
        .unwrap();

    let translated = world
        .run_system_once(
            |mut reader: bevy::prelude::MessageReader<AppCommand>,
             runtime_index: Option<Res<crate::agents::AgentRuntimeIndex>>,
             app_session: Option<Res<crate::app::AppSessionState>>| {
                reader
                    .read()
                    .flat_map(|command| match command {
                        AppCommand::Widget(WidgetCommand::Toggle(widget_id)) => {
                            vec![HudIntent::ToggleModule(*widget_id)]
                        }
                        AppCommand::Widget(WidgetCommand::Reset(widget_id)) => {
                            vec![HudIntent::ResetModule(*widget_id)]
                        }
                        AppCommand::Agent(AppAgentCommand::Focus(_)) => Vec::new(),
                        AppCommand::Agent(AppAgentCommand::Inspect(agent_id)) => runtime_index
                            .as_ref()
                            .and_then(|runtime_index| runtime_index.primary_terminal(*agent_id))
                            .map(|terminal_id| {
                                vec![
                                    HudIntent::FocusTerminal(terminal_id),
                                    HudIntent::HideAllButTerminal(terminal_id),
                                ]
                            })
                            .unwrap_or_default(),
                        AppCommand::Task(AppTaskCommand::ClearDone { agent_id }) => runtime_index
                            .as_ref()
                            .and_then(|runtime_index| runtime_index.primary_terminal(*agent_id))
                            .or_else(|| {
                                app_session.as_ref().and_then(|app_session| {
                                    app_session.composer.task_editor.target_terminal
                                })
                            })
                            .map(|terminal_id| vec![HudIntent::ClearDoneTerminalTasks(terminal_id)])
                            .unwrap_or_default(),
                        AppCommand::Task(AppTaskCommand::ConsumeNext { agent_id }) => runtime_index
                            .as_ref()
                            .and_then(|runtime_index| runtime_index.primary_terminal(*agent_id))
                            .or_else(|| {
                                app_session.as_ref().and_then(|app_session| {
                                    app_session.composer.task_editor.target_terminal
                                })
                            })
                            .map(|terminal_id| {
                                vec![HudIntent::ConsumeNextTerminalTask(terminal_id)]
                            })
                            .unwrap_or_default(),
                        AppCommand::Task(AppTaskCommand::Append { agent_id, text }) => {
                            runtime_index
                                .as_ref()
                                .and_then(|runtime_index| runtime_index.primary_terminal(*agent_id))
                                .or_else(|| {
                                    app_session.as_ref().and_then(|app_session| {
                                        app_session.composer.message_editor.target_terminal
                                    })
                                })
                                .map(|terminal_id| {
                                    vec![HudIntent::AppendTerminalTask(terminal_id, text.clone())]
                                })
                                .unwrap_or_default()
                        }
                        AppCommand::Composer(AppComposerCommand::Submit) => app_session
                            .as_ref()
                            .and_then(|app_session| app_session.composer.current_agent())
                            .and_then(|agent_id| {
                                runtime_index
                                    .as_ref()
                                    .and_then(|runtime_index| {
                                        runtime_index.primary_terminal(agent_id)
                                    })
                                    .map(|terminal_id| {
                                        if app_session.as_ref().is_some_and(|app_session| {
                                            app_session.composer.message_editor.visible
                                        }) {
                                            vec![HudIntent::SendActiveTerminalCommand(
                                                app_session
                                                    .as_ref()
                                                    .unwrap()
                                                    .composer
                                                    .message_editor
                                                    .text
                                                    .clone(),
                                            )]
                                        } else if app_session.as_ref().is_some_and(|app_session| {
                                            app_session.composer.task_editor.visible
                                        }) {
                                            vec![HudIntent::SetTerminalTaskText(
                                                terminal_id,
                                                app_session
                                                    .as_ref()
                                                    .unwrap()
                                                    .composer
                                                    .task_editor
                                                    .text
                                                    .clone(),
                                            )]
                                        } else {
                                            Vec::new()
                                        }
                                    })
                            })
                            .unwrap_or_default(),
                        _ => Vec::new(),
                    })
                    .collect::<Vec<_>>()
            },
        )
        .unwrap();

    commands.extend(translated);
    commands
}

fn run_app_commands(world: &mut World) {
    if !world.contains_resource::<Assets<Image>>() {
        world.insert_resource(Assets::<Image>::default());
    }
    if !world.contains_resource::<TerminalPresentationStore>() {
        world.insert_resource(TerminalPresentationStore::default());
    }
    if !world.contains_resource::<crate::terminals::TerminalRuntimeSpawner>() {
        world.insert_resource(fake_runtime_spawner(Arc::new(FakeDaemonClient::default())));
    }
    if !world.contains_resource::<crate::conversations::ConversationStore>() {
        world.insert_resource(crate::conversations::ConversationStore::default());
    }
    if !world.contains_resource::<crate::conversations::AgentTaskStore>() {
        world.insert_resource(crate::conversations::AgentTaskStore::default());
    }
    if !world.contains_resource::<crate::conversations::ConversationPersistenceState>() {
        world.insert_resource(crate::conversations::ConversationPersistenceState::default());
    }
    if !world.contains_resource::<crate::conversations::MessageTransportAdapter>() {
        world.insert_resource(crate::conversations::MessageTransportAdapter);
    }
    if !world.contains_resource::<TerminalNotesState>() {
        world.insert_resource(TerminalNotesState::default());
    }
    if !world.contains_resource::<TerminalSessionPersistenceState>() {
        world.insert_resource(TerminalSessionPersistenceState::default());
    }
    if !world.contains_resource::<TerminalVisibilityState>() {
        world.insert_resource(TerminalVisibilityState::default());
    }
    if !world.contains_resource::<TerminalViewState>() {
        world.insert_resource(TerminalViewState::default());
    }
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .run_system_once(crate::app::apply_app_commands)
        .unwrap();
    world
        .run_system_once(crate::conversations::sync_task_notes_projection)
        .unwrap();
}

/// Verifies that HUD setup spawns the expected scene/compositor entities and immediately requests a
/// redraw.
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

/// Verifies that structural HUD sync forcibly docks the agent list to the left edge at full window
/// height.
#[test]
fn sync_structural_hud_layout_docks_agent_list_to_full_height_left_column() {
    let mut world = World::default();
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudWidgetKey::AgentList,
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
    let module = hud_state.get(HudWidgetKey::AgentList).unwrap();
    assert_eq!(module.shell.current_rect, expected_rect);
}

/// Verifies the fixed geometry split between the main body, marker strip, and accent strip of an
/// agent-list row.
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

/// Verifies that compositor sync hides the upstream Vello canvas and routes its texture through the
/// compositor quad instead.
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

/// Verifies that compositor sync leaves the modal Vello canvas alone instead of hiding it with the
/// main HUD canvas.
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

/// Verifies the compositor quad mesh/UV contract expected by the upstream Vello texture-present path.
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

/// Verifies the color-space roundtrip assumption behind the HUD orange byte-preservation check.
#[test]
fn upstream_vello_present_contract_preserves_target_orange_bytes() {
    /// Converts one 8-bit sRGB channel into linear space for the roundtrip color check.
    fn srgb_to_linear_channel(value: u8) -> f32 {
        let srgb = value as f32 / 255.0;
        if srgb <= 0.04045 {
            srgb / 12.92
        } else {
            ((srgb + 0.055) / 1.055).powf(2.4)
        }
    }

    /// Converts one linear-space channel back into 8-bit sRGB for the roundtrip color check.
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

/// Verifies that resetting a HUD module restores the baked-in default shell state instead of merely
/// toggling enablement.
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
    hud_state.insert(HudWidgetKey::DebugToolbar, module);

    hud_state.reset_module(HudWidgetKey::DebugToolbar);

    let module = hud_state.get(HudWidgetKey::DebugToolbar).unwrap();
    assert!(module.shell.enabled);
    assert_eq!(
        module.shell.target_rect,
        crate::hud::HUD_MODULE_DEFINITIONS[0].default_rect
    );
    assert_eq!(module.shell.current_alpha, 1.0);
    assert!(hud_state.dirty_layout);
}

/// Verifies that a plain digit key emits the expected module-toggle intent.
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
        vec![HudIntent::ToggleModule(HudWidgetKey::AgentList)]
    );
}

/// Verifies the plain `j` agent-list navigation shortcut emits focus+isolate for the next terminal.
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

/// Verifies that the down-arrow shortcut uses the same next-agent focus+isolate behavior as `j`.
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

/// Verifies the plain `k` agent-list navigation shortcut emits focus+isolate for the previous
/// terminal.
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

/// Verifies that the up-arrow shortcut uses the same previous-agent focus+isolate behavior as `k`.
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

/// Verifies that the authoritative app-command path updates focus/visibility and requests redraws.
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
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(id)
        .expect("agent should be linked");
    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::Inspect(agent_id)));
    run_app_commands(&mut world);

    assert_eq!(
        world
            .resource::<crate::terminals::TerminalFocusState>()
            .active_id(),
        Some(id)
    );
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::Isolate(id)
    );
    let redraws_after_focus = world.resource::<Messages<RequestRedraw>>().len();
    assert!(redraws_after_focus >= 1);

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::ShowAll));
    run_app_commands(&mut world);

    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::ShowAll
    );
    assert!(world.resource::<Messages<RequestRedraw>>().len() > redraws_after_focus);
}

/// Verifies that `Alt+Shift+digit` still emits reset intents rather than toggle intents.
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
        vec![HudIntent::ResetModule(HudWidgetKey::DebugToolbar)]
    );
}

/// Verifies that HUD module shortcuts are ignored while direct terminal input has keyboard capture.
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

/// Verifies that explicit agent-directory labels override the synthetic `agent-N` fallback names.
#[test]
fn agent_rows_use_derived_agent_view_labels() {
    let rows = agent_rows(
        HudRect {
            x: 24.0,
            y: 96.0,
            w: 300.0,
            h: 420.0,
        },
        0.0,
        None,
        &AgentListView {
            rows: vec![
                AgentListRowView {
                    agent_id: crate::agents::AgentId(1),
                    terminal_id: Some(crate::terminals::TerminalId(1)),
                    label: "agent-1".into(),
                    focused: false,
                    has_tasks: false,
                    interactive: true,
                },
                AgentListRowView {
                    agent_id: crate::agents::AgentId(2),
                    terminal_id: Some(crate::terminals::TerminalId(2)),
                    label: "oracle".into(),
                    focused: true,
                    has_tasks: true,
                    interactive: true,
                },
            ],
        },
    );

    assert_eq!(rows[0].label, "agent-1");
    assert_eq!(rows[1].label, "oracle");
    assert_eq!(rows[1].display_label, "ORACLE");
    assert!(rows[1].focused);
    assert!(rows[1].has_tasks);
}

/// Verifies that agent-row generation follows terminal creation order and annotates the focused row.
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
        &AgentListView {
            rows: vec![
                AgentListRowView {
                    agent_id: crate::agents::AgentId(1),
                    terminal_id: Some(id_one),
                    label: "agent-1".into(),
                    focused: false,
                    has_tasks: false,
                    interactive: true,
                },
                AgentListRowView {
                    agent_id: crate::agents::AgentId(2),
                    terminal_id: Some(id_two),
                    label: "agent-2".into(),
                    focused: true,
                    has_tasks: false,
                    interactive: true,
                },
            ],
        },
    );
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].terminal_id, Some(id_one));
    assert_eq!(rows[0].label, "agent-1");
    assert!(rows[0].rect.y > shell_rect.y + 20.0);
    assert!(rows[0].rect.x > shell_rect.x + 20.0);
    assert_eq!(rows[1].terminal_id, Some(id_two));
    assert!(rows[1].focused);
    assert_eq!(rows[1].rect.y - rows[0].rect.y, 42.0);
}

/// Verifies that agent-row generation marks only the explicitly hovered agent as hovered.
#[test]
fn agent_rows_mark_hovered_agent() {
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
        Some(crate::agents::AgentId(1)),
        &AgentListView {
            rows: vec![
                AgentListRowView {
                    agent_id: crate::agents::AgentId(1),
                    terminal_id: Some(id_one),
                    label: "agent-1".into(),
                    focused: false,
                    has_tasks: false,
                    interactive: true,
                },
                AgentListRowView {
                    agent_id: crate::agents::AgentId(2),
                    terminal_id: Some(id_two),
                    label: "agent-2".into(),
                    focused: false,
                    has_tasks: false,
                    interactive: true,
                },
            ],
        },
    );
    assert!(
        rows.iter()
            .find(|row| row.terminal_id == Some(id_one))
            .unwrap()
            .hovered
    );
    assert!(
        !rows
            .iter()
            .find(|row| row.terminal_id == Some(id_two))
            .unwrap()
            .hovered
    );
}

/// Verifies that clicking the agent-list title region does not start drag state like ordinary HUD
/// modules do.
#[test]
fn agent_list_is_not_draggable() {
    let mut world = World::default();
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudWidgetKey::AgentList,
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
    world.insert_resource(AgentListView::default());
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

/// Verifies the fixed proportional layout of the message-box modal within the window.
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

/// Verifies that clicking the task-dialog `Clear done` button emits the clear-done intent but leaves
/// the dialog/editor state open for the subsequent persistence update.
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
    world.insert_resource(AgentListView::default());
    init_hud_commands(&mut world);
    world.spawn((window, PrimaryWindow));

    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    world.run_system_once(handle_hud_pointer_input).unwrap();

    let emitted = world
        .run_system_once(|mut reader: bevy::prelude::MessageReader<AppCommand>| {
            reader.read().cloned().collect::<Vec<_>>()
        })
        .unwrap();
    assert_eq!(
        emitted,
        vec![AppCommand::Task(AppTaskCommand::ClearDone {
            agent_id: crate::agents::AgentId(1),
        })]
    );
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
    let hud_state = snapshot_test_hud_state(&world);
    assert!(hud_state.task_dialog.visible);
    assert_eq!(hud_state.task_dialog.text, "- [x] done\n- [ ] keep");
}

/// Verifies that clearing done tasks through the app-command path refreshes the open task editor
/// from authoritative task state rather than leaving stale local text behind.
#[test]
fn clear_done_task_request_updates_open_dialog_from_persisted_state() {
    let (bridge, _, _) = super::capturing_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal_with_session(bridge, "session-a".into());

    let mut notes_state = TerminalNotesState::default();
    assert!(notes_state.set_note_text("session-a", "- [x] done\n- [ ] keep"));

    let mut world = World::default();
    world.insert_resource(Time::<()>::default());
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(notes_state);
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    let mut hud_state = HudState::default();
    hud_state.open_task_dialog(terminal_id, "stale local");
    insert_test_hud_state(&mut world, hud_state);
    {
        let mut tasks = world.resource_mut::<crate::conversations::AgentTaskStore>();
        let _ = tasks.set_text(agent_id, "- [x] done\n- [ ] keep");
    }
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Task(AppTaskCommand::ClearDone { agent_id }));

    run_app_commands(&mut world);

    {
        let notes_state = world.resource::<TerminalNotesState>();
        assert_eq!(notes_state.note_text("session-a"), Some("- [ ] keep"));
    }
    let hud_state = snapshot_test_hud_state(&world);
    assert_eq!(hud_state.task_dialog.text, "- [ ] keep");
    assert!(hud_state.task_dialog.visible);
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
}

/// Verifies that submitting an empty task editor clears persisted note state instead of storing an
/// empty note blob.
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
    assert!(world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .is_some());
    let mut hud_state = HudState::default();
    hud_state.open_task_dialog(terminal_id, "");
    insert_test_hud_state(&mut world, hud_state);
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Composer(AppComposerCommand::Submit));

    run_app_commands(&mut world);

    let notes_state = world.resource::<TerminalNotesState>();
    assert_eq!(notes_state.note_text("session-a"), None);
    assert!(!notes_state.has_note_text("session-a"));
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
    let hud_state = snapshot_test_hud_state(&world);
    assert!(!hud_state.task_dialog.visible);
}

/// Verifies that consuming the next task through the app-command path sends the task payload to
/// the terminal and marks that task done in persisted notes.
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
    let agent_id = world
        .resource::<crate::agents::AgentRuntimeIndex>()
        .agent_for_terminal(terminal_id)
        .expect("agent should be linked");
    {
        let mut tasks = world.resource_mut::<crate::conversations::AgentTaskStore>();
        let _ = tasks.set_text(agent_id, "- [ ] first\n  detail\n- [ ] second");
    }
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();
    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Task(AppTaskCommand::ConsumeNext { agent_id }));

    run_app_commands(&mut world);

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

/// Verifies that clicking the message-box append-task button turns the current draft into an
/// `AppendTerminalTask` intent and closes the modal.
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
    world.insert_resource(AgentListView::default());
    init_hud_commands(&mut world);
    world.spawn((window, PrimaryWindow));

    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    world.run_system_once(handle_hud_pointer_input).unwrap();

    let emitted = world
        .run_system_once(|mut reader: bevy::prelude::MessageReader<AppCommand>| {
            reader.read().cloned().collect::<Vec<_>>()
        })
        .unwrap();
    assert_eq!(
        emitted,
        vec![AppCommand::Task(AppTaskCommand::Append {
            agent_id: crate::agents::AgentId(1),
            text: "follow up".into(),
        })]
    );
    assert_eq!(world.resource::<Messages<RequestRedraw>>().len(), 1);
    assert!(!snapshot_test_hud_state(&world).message_box.visible);
}

/// Verifies that one animation tick moves both HUD rect position and alpha toward their targets.
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
    hud_state.insert(HudWidgetKey::AgentList, module);
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    world.insert_resource(time);
    insert_test_hud_state(&mut world, hud_state);

    world
        .run_system_once(crate::hud::animate_hud_modules)
        .unwrap();

    let hud_state = snapshot_test_hud_state(&world);
    let module = hud_state.get(HudWidgetKey::AgentList).unwrap();
    assert!(module.shell.current_rect.x > 24.0);
    assert!(module.shell.current_rect.x < 124.0);
    assert!(module.shell.current_alpha > 0.2);
    assert!(module.shell.current_alpha < 1.0);
}

/// Verifies that clicking the debug-toolbar `new terminal` button emits the spawn-terminal intent.
#[test]
fn clicking_debug_toolbar_button_emits_spawn_terminal_command() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    manager.create_terminal(bridge);
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudWidgetKey::DebugToolbar,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[0]),
    );
    hud_state.insert(
        HudWidgetKey::AgentList,
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
        HudWidgetKey::DebugToolbar,
        hud_state
            .get(HudWidgetKey::DebugToolbar)
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
        &AgentListView::default(),
        &ConversationListView::default(),
        &ThreadView::default(),
        &hud_state.layout_state(),
        &mut emitted_commands,
    );

    assert_eq!(emitted_commands, vec![crate::hud::HudIntent::SpawnTerminal]);
}

/// Verifies that clicking a debug-toolbar command button emits the corresponding active-terminal
/// command intent.
#[test]
fn clicking_debug_toolbar_command_button_emits_terminal_command() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    manager.create_terminal(bridge);
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudWidgetKey::DebugToolbar,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[0]),
    );
    hud_state.insert(
        HudWidgetKey::AgentList,
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
        HudWidgetKey::DebugToolbar,
        hud_state
            .get(HudWidgetKey::DebugToolbar)
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
        &AgentListView::default(),
        &ConversationListView::default(),
        &ThreadView::default(),
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

/// Verifies that clicking an agent-list row emits the standard focus-plus-isolate command pair for
/// that terminal.
#[test]
fn clicking_agent_list_row_emits_focus_and_isolate_commands() {
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudWidgetKey::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    let mut emitted_commands = Vec::new();
    let agent_list_view = AgentListView {
        rows: vec![
            AgentListRowView {
                agent_id: crate::agents::AgentId(1),
                terminal_id: Some(crate::terminals::TerminalId(1)),
                label: "agent-1".into(),
                focused: false,
                has_tasks: false,
                interactive: true,
            },
            AgentListRowView {
                agent_id: crate::agents::AgentId(2),
                terminal_id: Some(id_two),
                label: "agent-2".into(),
                focused: false,
                has_tasks: false,
                interactive: true,
            },
        ],
    };
    let rows = agent_rows(
        HudRect {
            x: 24.0,
            y: 132.0,
            w: 300.0,
            h: 392.0,
        },
        0.0,
        None,
        &agent_list_view,
    );
    let target_row = rows
        .iter()
        .find(|row| row.terminal_id == Some(id_two))
        .expect("agent row for second terminal missing");
    let click_point = Vec2::new(
        target_row.rect.x + target_row.rect.w * 0.5,
        target_row.rect.y + target_row.rect.h * 0.5,
    );

    dispatch_hud_pointer_click(
        HudWidgetKey::AgentList,
        hud_state
            .get(HudWidgetKey::AgentList)
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
        &agent_list_view,
        &ConversationListView::default(),
        &ThreadView::default(),
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

/// Verifies that clicking a conversation-list row selects the linked terminal through the standard
/// focus+isolate command pair.
#[test]
fn clicking_conversation_list_row_emits_focus_and_isolate_commands() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal(bridge);
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudWidgetKey::ConversationList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[2]),
    );
    let conversation_list_view = ConversationListView {
        rows: vec![crate::hud::ConversationListRowView {
            agent_id: crate::agents::AgentId(1),
            terminal_id: Some(terminal_id),
            conversation_id: crate::conversations::ConversationId(1),
            label: "alpha".into(),
            message_count: 2,
            selected: true,
        }],
    };
    let mut emitted_commands = Vec::new();

    dispatch_hud_pointer_click(
        HudWidgetKey::ConversationList,
        hud_state
            .get(HudWidgetKey::ConversationList)
            .map(|module| &module.model)
            .expect("conversation list module missing"),
        HudRect {
            x: 332.0,
            y: 140.0,
            w: 320.0,
            h: 280.0,
        },
        Vec2::new(360.0, 154.0),
        &manager,
        &manager.clone_focus_state(),
        &Default::default(),
        &TerminalViewState::default(),
        &AgentListView::default(),
        &conversation_list_view,
        &ThreadView::default(),
        &hud_state.layout_state(),
        &mut emitted_commands,
    );

    assert_eq!(
        emitted_commands,
        vec![
            crate::hud::HudIntent::FocusTerminal(terminal_id),
            crate::hud::HudIntent::HideAllButTerminal(terminal_id),
        ]
    );
}

/// Verifies that agent-list wheel scrolling clamps at the maximum content offset rather than running
/// past the last row.
#[test]
fn agent_list_scroll_clamps_to_content_height() {
    let mut model = HudModuleModel::AgentList(Default::default());
    let mut manager = TerminalManager::default();
    for _ in 0..5 {
        let (bridge, _) = test_bridge();
        manager.create_terminal(bridge);
    }

    dispatch_hud_scroll(
        HudWidgetKey::AgentList,
        &mut model,
        -500.0,
        &manager,
        HudRect {
            x: 24.0,
            y: 132.0,
            w: 300.0,
            h: 112.0,
        },
        &AgentListView {
            rows: (0..5)
                .map(|index| AgentListRowView {
                    agent_id: crate::agents::AgentId(index + 1),
                    terminal_id: Some(crate::terminals::TerminalId(index + 1)),
                    label: format!("agent-{}", index + 1),
                    focused: false,
                    has_tasks: false,
                    interactive: true,
                })
                .collect(),
        },
        &ConversationListView::default(),
    );

    let HudModuleModel::AgentList(state) = model else {
        panic!("expected agent list model");
    };
    assert_eq!(state.scroll_offset, 84.0);
}

/// Verifies that the debug toolbar exposes explicit toggle buttons for the known HUD modules.
#[test]
fn debug_toolbar_buttons_include_module_toggle_entries() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    manager.create_terminal(bridge);
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudWidgetKey::DebugToolbar,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[0]),
    );
    hud_state.insert(
        HudWidgetKey::AgentList,
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
    assert!(buttons.iter().any(|button| button.label == "2 convs"));
    assert!(buttons.iter().any(|button| button.label == "3 thread"));
}

/// Verifies that debug-toolbar module toggle buttons mirror each module's current enabled state.
#[test]
fn debug_toolbar_module_toggle_buttons_reflect_enabled_state() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    manager.create_terminal(bridge);
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudWidgetKey::DebugToolbar,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[0]),
    );
    hud_state.insert(
        HudWidgetKey::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    hud_state.set_module_enabled(HudWidgetKey::AgentList, false);

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
    let conversations = buttons
        .iter()
        .find(|button| button.label == "2 convs")
        .expect("conversation toggle button missing");
    let thread = buttons
        .iter()
        .find(|button| button.label == "3 thread")
        .expect("thread toggle button missing");
    assert!(toolbar.active);
    assert!(!agents.active);
    assert!(!conversations.active);
    assert!(!thread.active);
}

/// Verifies that HUD hit-testing returns the frontmost enabled module when rects overlap.
#[test]
fn hud_state_topmost_enabled_at_prefers_frontmost_module() {
    let mut state = HudState::default();
    state.insert(
        HudWidgetKey::DebugToolbar,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[0]),
    );
    state.insert(
        HudWidgetKey::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    state.raise_to_front(HudWidgetKey::AgentList);

    assert_eq!(
        state.topmost_enabled_at(Vec2::new(40.0, 110.0)),
        Some(HudWidgetKey::AgentList)
    );
}

/// Verifies that the HUD redraw predicate turns on for either drag state or in-flight shell
/// animation.
#[test]
fn hud_needs_redraw_when_drag_or_animation_is_active() {
    let mut state = HudState::default();
    state.insert(
        HudWidgetKey::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    assert!(!hud_needs_redraw(&state.layout_state()));
    state.drag = Some(HudDragState {
        module_id: HudWidgetKey::AgentList,
        grab_offset: Vec2::ZERO,
    });
    assert!(hud_needs_redraw(&state.layout_state()));
    state.drag = None;
    let module = state.get_mut(HudWidgetKey::AgentList).unwrap();
    module.shell.current_rect.x = 0.0;
    module.shell.target_rect.x = 10.0;
    assert!(hud_needs_redraw(&state.layout_state()));
}

/// Verifies that disabling a HUD module does not suppress redraw while its fade-out animation is
/// still active.
#[test]
fn disabled_hud_module_still_requests_redraw_while_fading_out() {
    let mut state = HudState::default();
    state.insert(
        HudWidgetKey::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );

    state.set_module_enabled(HudWidgetKey::AgentList, false);

    let module = state.get(HudWidgetKey::AgentList).unwrap();
    assert!(!module.shell.enabled);
    assert!(module.shell.is_animating());
    assert!(hud_needs_redraw(&state.layout_state()));
}

/// Verifies that removing a middle active terminal promotes the previous surviving terminal in
/// creation order to active/isolate state.
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

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(store);
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
             mut session_persistence: ResMut<TerminalSessionPersistenceState>,
             _visibility_state: ResMut<TerminalVisibilityState>,
             _view_state: ResMut<TerminalViewState>| {
                let _ = kill_active_terminal(
                    &mut commands,
                    &time,
                    &mut terminal_manager,
                    &mut focus_state,
                    &mut presentation_store,
                    &runtime_spawner,
                    &mut session_persistence,
                );
            },
        )
        .unwrap();

    let manager = world.resource::<TerminalManager>();
    assert_eq!(manager.terminal_ids(), &[id_one, id_three]);
    assert_eq!(manager.active_id(), None);
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::Isolate(id_two)
    );
}

/// Verifies that removing the first active terminal promotes the next surviving terminal to
/// active/isolate state.
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

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(store);
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
             mut session_persistence: ResMut<TerminalSessionPersistenceState>,
             _visibility_state: ResMut<TerminalVisibilityState>,
             _view_state: ResMut<TerminalViewState>| {
                let _ = kill_active_terminal(
                    &mut commands,
                    &time,
                    &mut terminal_manager,
                    &mut focus_state,
                    &mut presentation_store,
                    &runtime_spawner,
                    &mut session_persistence,
                );
            },
        )
        .unwrap();

    let manager = world.resource::<TerminalManager>();
    assert_eq!(manager.terminal_ids(), &[id_two]);
    assert_eq!(manager.active_id(), None);
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::Isolate(id_one)
    );
}

/// Verifies that a successful active-terminal kill removes terminal-manager state, presentation
/// state, labels, spawned panel entities, and resets visibility/persistence bookkeeping.
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

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(store);
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
             mut session_persistence: ResMut<TerminalSessionPersistenceState>,
             _visibility_state: ResMut<TerminalVisibilityState>,
             _view_state: ResMut<TerminalViewState>| {
                let _ = kill_active_terminal(
                    &mut commands,
                    &time,
                    &mut terminal_manager,
                    &mut focus_state,
                    &mut presentation_store,
                    &runtime_spawner,
                    &mut session_persistence,
                );
            },
        )
        .unwrap();
    world.insert_resource(Assets::<Image>::default());
    world
        .run_system_once(crate::terminals::sync_terminal_projection_entities)
        .unwrap();

    assert!(world
        .resource::<TerminalManager>()
        .terminal_ids()
        .is_empty());
    assert!(world
        .resource::<TerminalPresentationStore>()
        .get(id)
        .is_none());
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::Isolate(id)
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

/// Verifies that the shell-spawn app command creates a session without injecting any bootstrap
/// command payload.
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
    world.insert_resource(AgentListView::default());
    world.insert_resource(fake_runtime_spawner(client.clone()));
    world.insert_resource(TerminalSessionPersistenceState::default());
    world.insert_resource(TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    world.init_resource::<Messages<AppCommand>>();
    world.init_resource::<Messages<RequestRedraw>>();

    world
        .resource_mut::<Messages<AppCommand>>()
        .write(AppCommand::Agent(AppAgentCommand::SpawnShellTerminal));

    run_app_commands(&mut world);

    assert_eq!(world.resource::<TerminalManager>().terminal_ids().len(), 1);
    assert!(client.sent_commands.lock().unwrap().is_empty());
}

/// Verifies the special-case cleanup path for disconnected terminals: local state is removed even if
/// daemon-side kill returns an error.
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

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(store);
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
             mut session_persistence: ResMut<TerminalSessionPersistenceState>,
             _visibility_state: ResMut<TerminalVisibilityState>,
             _view_state: ResMut<TerminalViewState>| {
                let _ = kill_active_terminal(
                    &mut commands,
                    &time,
                    &mut terminal_manager,
                    &mut focus_state,
                    &mut presentation_store,
                    &runtime_spawner,
                    &mut session_persistence,
                );
            },
        )
        .unwrap();
    world.insert_resource(Assets::<Image>::default());
    world
        .run_system_once(crate::terminals::sync_terminal_projection_entities)
        .unwrap();

    assert!(world
        .resource::<TerminalManager>()
        .terminal_ids()
        .is_empty());
    assert!(world
        .resource::<TerminalPresentationStore>()
        .get(id)
        .is_none());
    assert_eq!(
        world.resource::<TerminalVisibilityState>().policy,
        TerminalVisibilityPolicy::Isolate(id)
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

/// Verifies that a kill failure for an otherwise live terminal preserves all local state instead of
/// tearing presentation/labels down prematurely.
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

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(store);
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
             mut session_persistence: ResMut<TerminalSessionPersistenceState>,
             _visibility_state: ResMut<TerminalVisibilityState>,
             _view_state: ResMut<TerminalViewState>| {
                let _ = kill_active_terminal(
                    &mut commands,
                    &time,
                    &mut terminal_manager,
                    &mut focus_state,
                    &mut presentation_store,
                    &runtime_spawner,
                    &mut session_persistence,
                );
            },
        )
        .unwrap();

    assert_eq!(world.resource::<TerminalManager>().terminal_ids(), &[id]);
    assert!(world
        .resource::<TerminalPresentationStore>()
        .get(id)
        .is_some());
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

/// Verifies the enum default for terminal visibility policy is the non-isolating `ShowAll` mode.
#[test]
fn terminal_visibility_policy_defaults_to_show_all() {
    assert_eq!(
        TerminalVisibilityPolicy::default(),
        TerminalVisibilityPolicy::ShowAll
    );
}
