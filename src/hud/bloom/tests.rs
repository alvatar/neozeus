use std::collections::BTreeSet;

use super::super::{
    modules::{AGENT_LIST_BORDER_ORANGE_B, AGENT_LIST_BORDER_ORANGE_G, AGENT_LIST_BORDER_ORANGE_R},
    state::{default_hud_module_instance, HudState},
    view_models::{
        AgentListActivity, AgentListRowKey, AgentListRowKind, AgentListRowView, AgentListView,
        OwnedTmuxOwnerBinding,
    },
    widgets::{HudWidgetKey, HUD_WIDGET_DEFINITIONS},
};
use super::*;
use crate::tests::insert_test_hud_state;
use bevy::{
    camera::{visibility::RenderLayers, RenderTarget},
    ecs::system::RunSystemOnce,
    prelude::{
        default, Assets, Image, Mesh, Transform, Vec3, Vec4, Visibility, Window, With, World,
    },
    render::render_resource::TextureFormat,
    sprite_render::{AlphaMode2d, Material2d},
    window::{PrimaryWindow, WindowResolution},
};

fn insert_mock_group_canvas(world: &mut World, group: HudBloomGroupId, size: UVec2) {
    let image = {
        let mut images = world.resource_mut::<Assets<Image>>();
        let handle = images.add(Image::default());
        let image = images.get_mut(&handle).expect("group image should exist");
        image.resize(bevy::render::render_resource::Extent3d {
            width: size.x.max(1),
            height: size.y.max(1),
            depth_or_array_layers: 1,
        });
        handle
    };
    let material = world
        .resource_mut::<Assets<bevy_vello::render::VelloCanvasMaterial>>()
        .add(bevy_vello::render::VelloCanvasMaterial { texture: image });
    world.spawn((
        HudBloomGroupMarker { group },
        bevy::sprite_render::MeshMaterial2d::<bevy_vello::render::VelloCanvasMaterial>(material),
        Visibility::Visible,
    ));
}

fn set_active_bloom_groups(world: &mut World, groups: impl IntoIterator<Item = HudBloomGroupId>) {
    let active_groups = groups.into_iter().collect();
    world
        .resource_mut::<crate::hud::HudBloomGroupRenderState>()
        .set_active_groups(active_groups);
}

fn visible_group_composite_count(world: &mut World, group: HudBloomGroupId) -> usize {
    world
        .query::<(
            &HudBloomGroupMarker,
            &Visibility,
            &AgentListBloomCompositeMarker,
        )>()
        .iter(world)
        .filter(|(marker, visibility, _)| {
            marker.group == group && **visibility == Visibility::Visible
        })
        .count()
}

/// Locks down the exact reference RGB constants used by the agent-list border and bloom styling.
///
/// These values matter because the visual verification scripts compare rendered output against known
/// color targets; accidental changes here would silently change the whole visual signature.
#[test]
fn agent_list_reference_colors_match_requested_values() {
    assert_eq!(
        (
            AGENT_LIST_BORDER_ORANGE_R,
            AGENT_LIST_BORDER_ORANGE_G,
            AGENT_LIST_BORDER_ORANGE_B,
        ),
        (225, 129, 10)
    );
    assert_eq!(
        (
            AGENT_LIST_BLOOM_RED_R,
            AGENT_LIST_BLOOM_RED_G,
            AGENT_LIST_BLOOM_RED_B,
        ),
        (143, 37, 15)
    );
}

#[test]
fn selected_idle_rows_use_red_bloom_contract() {
    let main = bloom_source_color(AgentListBloomSourceKind::Main, false, false);
    let marker = bloom_source_color(AgentListBloomSourceKind::Marker, false, false);

    let main_linear = main.to_linear();
    let marker_linear = marker.to_linear();
    assert!(main_linear.red > main_linear.green);
    assert!(main_linear.red > main_linear.blue);
    assert!(marker_linear.red >= main_linear.red);
}

#[test]
fn selected_working_rows_use_green_bloom_contract() {
    let main = bloom_source_color(AgentListBloomSourceKind::Main, false, true);
    let marker = bloom_source_color(AgentListBloomSourceKind::Marker, false, true);

    let main_linear = main.to_linear();
    let marker_linear = marker.to_linear();
    assert!(main_linear.green > main_linear.red);
    assert!(main_linear.green > main_linear.blue);
    assert!(marker_linear.green >= main_linear.green);
}

#[test]
fn selected_paused_rows_use_gray_bloom_contract() {
    let main = bloom_source_color(AgentListBloomSourceKind::Main, true, true);
    let marker = bloom_source_color(AgentListBloomSourceKind::Marker, true, false);

    let main_linear = main.to_linear();
    let marker_linear = marker.to_linear();
    assert!((main_linear.red - main_linear.green).abs() < 0.15);
    assert!((main_linear.green - main_linear.blue).abs() < 0.15);
    assert!(marker_linear.red >= main_linear.red);
}

/// Verifies the permissive parser for the bloom-intensity override.
///
/// Valid non-negative finite numbers should be accepted, while empty, negative, or malformed values
/// should fall back to the module default intensity.
#[test]
fn parses_agent_bloom_intensity_override() {
    assert_eq!(resolve_agent_list_bloom_intensity(None), 0.10);
    assert_eq!(resolve_agent_list_bloom_intensity(Some("")), 0.10);
    assert_eq!(resolve_agent_list_bloom_intensity(Some("2.0")), 2.0);
    assert_eq!(resolve_agent_list_bloom_intensity(Some(" 0.0 ")), 0.0);
    assert_eq!(resolve_agent_list_bloom_intensity(Some("-1")), 0.10);
    assert_eq!(resolve_agent_list_bloom_intensity(Some("abc")), 0.10);
}

/// Verifies the boolean parser for bloom debug previews.
///
/// The test covers the accepted truthy spellings and confirms that empty or explicit falsy values do
/// not enable the preview overlays.
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
fn selected_agent_row_emits_selected_bloom_sources_only_for_that_row() {
    let active_row_key = AgentListRowKey::Agent(crate::agents::AgentId(1));
    let specs = build_bloom_specs(
        HudRect {
            x: 0.0,
            y: 0.0,
            w: 400.0,
            h: 240.0,
        },
        0.0,
        None,
        Some(&active_row_key),
        &BTreeSet::new(),
        &AgentListView {
            rows: vec![
                AgentListRowView {
                    key: AgentListRowKey::Agent(crate::agents::AgentId(1)),
                    label: "ALPHA".into(),
                    focused: true,
                    kind: AgentListRowKind::Agent {
                        agent_id: crate::agents::AgentId(1),
                        terminal_id: Some(crate::terminals::TerminalId(11)),
                        has_tasks: false,
                        interactive: true,
                        activity: AgentListActivity::Idle,
                        paused: false,
                        aegis_enabled: false,
                        context_pct_milli: None,
                        agent_kind: crate::agents::AgentKind::Terminal,
                        session_metrics: crate::shared::daemon_wire::DaemonSessionMetrics::default(
                        ),
                    },
                },
                AgentListRowView {
                    key: AgentListRowKey::Agent(crate::agents::AgentId(2)),
                    label: "BETA".into(),
                    focused: false,
                    kind: AgentListRowKind::Agent {
                        agent_id: crate::agents::AgentId(2),
                        terminal_id: Some(crate::terminals::TerminalId(22)),
                        has_tasks: false,
                        interactive: true,
                        activity: AgentListActivity::Idle,
                        paused: false,
                        aegis_enabled: false,
                        context_pct_milli: None,
                        agent_kind: crate::agents::AgentKind::Terminal,
                        session_metrics: crate::shared::daemon_wire::DaemonSessionMetrics::default(
                        ),
                    },
                },
            ],
        },
    );

    assert_eq!(specs.len(), 8);
    assert!(specs
        .iter()
        .all(|spec| spec.key.terminal_id == crate::terminals::TerminalId(11)));
}

#[test]
fn selected_paused_agent_row_emits_gray_bloom_sources_only_for_that_row() {
    let active_row_key = AgentListRowKey::Agent(crate::agents::AgentId(1));
    let specs = build_bloom_specs(
        HudRect {
            x: 0.0,
            y: 0.0,
            w: 400.0,
            h: 240.0,
        },
        0.0,
        None,
        Some(&active_row_key),
        &BTreeSet::new(),
        &AgentListView {
            rows: vec![AgentListRowView {
                key: AgentListRowKey::Agent(crate::agents::AgentId(1)),
                label: "ALPHA".into(),
                focused: true,
                kind: AgentListRowKind::Agent {
                    agent_id: crate::agents::AgentId(1),
                    terminal_id: Some(crate::terminals::TerminalId(11)),
                    has_tasks: false,
                    interactive: true,
                    activity: AgentListActivity::Working,
                    paused: true,
                    aegis_enabled: false,
                    context_pct_milli: None,
                    agent_kind: crate::agents::AgentKind::Terminal,
                    session_metrics: crate::shared::daemon_wire::DaemonSessionMetrics::default(),
                },
            }],
        },
    );

    assert_eq!(specs.len(), 8);
    assert!(specs.iter().all(|spec| {
        let linear = spec.color.to_linear();
        (linear.red - linear.green).abs() < 0.2 && (linear.green - linear.blue).abs() < 0.2
    }));
}

#[test]
fn selected_working_agent_row_emits_green_bloom_sources_only_for_that_row() {
    let active_row_key = AgentListRowKey::Agent(crate::agents::AgentId(1));
    let specs = build_bloom_specs(
        HudRect {
            x: 0.0,
            y: 0.0,
            w: 400.0,
            h: 240.0,
        },
        0.0,
        None,
        Some(&active_row_key),
        &BTreeSet::new(),
        &AgentListView {
            rows: vec![
                AgentListRowView {
                    key: AgentListRowKey::Agent(crate::agents::AgentId(1)),
                    label: "ALPHA".into(),
                    focused: true,
                    kind: AgentListRowKind::Agent {
                        agent_id: crate::agents::AgentId(1),
                        terminal_id: Some(crate::terminals::TerminalId(11)),
                        has_tasks: false,
                        interactive: true,
                        activity: AgentListActivity::Working,
                        paused: false,
                        aegis_enabled: false,
                        context_pct_milli: None,
                        agent_kind: crate::agents::AgentKind::Terminal,
                        session_metrics: crate::shared::daemon_wire::DaemonSessionMetrics::default(
                        ),
                    },
                },
                AgentListRowView {
                    key: AgentListRowKey::Agent(crate::agents::AgentId(2)),
                    label: "BETA".into(),
                    focused: false,
                    kind: AgentListRowKind::Agent {
                        agent_id: crate::agents::AgentId(2),
                        terminal_id: Some(crate::terminals::TerminalId(22)),
                        has_tasks: false,
                        interactive: true,
                        activity: AgentListActivity::Idle,
                        paused: false,
                        aegis_enabled: false,
                        context_pct_milli: None,
                        agent_kind: crate::agents::AgentKind::Terminal,
                        session_metrics: crate::shared::daemon_wire::DaemonSessionMetrics::default(
                        ),
                    },
                },
            ],
        },
    );

    assert_eq!(specs.len(), 8);
    assert!(specs
        .iter()
        .all(|spec| spec.key.terminal_id == crate::terminals::TerminalId(11)));
    let linear = specs[0].color.to_linear();
    assert!(linear.green > linear.red);
    assert!(linear.green > linear.blue);
}

#[test]
fn selected_tmux_row_does_not_emit_parent_agent_bloom() {
    let active_row_key = AgentListRowKey::OwnedTmux("tmux-1".into());
    let specs = build_bloom_specs(
        HudRect {
            x: 0.0,
            y: 0.0,
            w: 400.0,
            h: 240.0,
        },
        0.0,
        None,
        Some(&active_row_key),
        &BTreeSet::new(),
        &AgentListView {
            rows: vec![
                AgentListRowView {
                    key: AgentListRowKey::Agent(crate::agents::AgentId(1)),
                    label: "ALPHA".into(),
                    focused: false,
                    kind: AgentListRowKind::Agent {
                        agent_id: crate::agents::AgentId(1),
                        terminal_id: Some(crate::terminals::TerminalId(11)),
                        has_tasks: false,
                        interactive: true,
                        activity: AgentListActivity::Idle,
                        paused: false,
                        aegis_enabled: false,
                        context_pct_milli: None,
                        agent_kind: crate::agents::AgentKind::Terminal,
                        session_metrics: crate::shared::daemon_wire::DaemonSessionMetrics::default(
                        ),
                    },
                },
                AgentListRowView {
                    key: AgentListRowKey::OwnedTmux("tmux-1".into()),
                    label: "BUILD".into(),
                    focused: true,
                    kind: AgentListRowKind::OwnedTmux {
                        session_uid: "tmux-1".into(),
                        owner: OwnedTmuxOwnerBinding::Bound(crate::agents::AgentId(1)),
                        tmux_name: "neozeus-tmux-1".into(),
                        cwd: "/tmp/work".into(),
                        attached: false,
                    },
                },
            ],
        },
    );

    assert_eq!(specs.len(), 4);
    assert!(specs
        .iter()
        .all(|spec| spec.key.kind == AgentListBloomSourceKind::Main));
    assert!(specs
        .iter()
        .all(|spec| spec.key.terminal_id == crate::terminals::TerminalId(11)));
}

#[test]
fn aegis_enabled_rows_emit_pink_outer_bloom_when_unselected() {
    let mut aegis_rows = BTreeSet::new();
    aegis_rows.insert(AgentListRowKey::Agent(crate::agents::AgentId(1)));
    let specs = build_bloom_specs(
        HudRect {
            x: 0.0,
            y: 0.0,
            w: 400.0,
            h: 240.0,
        },
        0.0,
        None,
        None,
        &aegis_rows,
        &AgentListView {
            rows: vec![AgentListRowView {
                key: AgentListRowKey::Agent(crate::agents::AgentId(1)),
                label: "ALPHA".into(),
                focused: false,
                kind: AgentListRowKind::Agent {
                    agent_id: crate::agents::AgentId(1),
                    terminal_id: Some(crate::terminals::TerminalId(11)),
                    has_tasks: false,
                    interactive: true,
                    activity: AgentListActivity::Idle,
                    paused: false,
                    aegis_enabled: false,
                    context_pct_milli: None,
                    agent_kind: crate::agents::AgentKind::Terminal,
                    session_metrics: crate::shared::daemon_wire::DaemonSessionMetrics::default(),
                },
            }],
        },
    );

    assert_eq!(specs.len(), 4);
    assert!(specs
        .iter()
        .all(|spec| spec.key.kind == AgentListBloomSourceKind::Aegis));
    let linear = specs[0].color.to_linear();
    assert!(linear.red > linear.green);
    assert!(linear.blue > linear.green);
}

#[test]
fn selected_aegis_agent_emits_both_outer_aegis_and_inner_selection_glow() {
    let active_row_key = AgentListRowKey::Agent(crate::agents::AgentId(1));
    let mut aegis_rows = BTreeSet::new();
    aegis_rows.insert(active_row_key.clone());
    let specs = build_bloom_specs(
        HudRect {
            x: 0.0,
            y: 0.0,
            w: 400.0,
            h: 240.0,
        },
        0.0,
        None,
        Some(&active_row_key),
        &aegis_rows,
        &AgentListView {
            rows: vec![AgentListRowView {
                key: active_row_key.clone(),
                label: "ALPHA".into(),
                focused: true,
                kind: AgentListRowKind::Agent {
                    agent_id: crate::agents::AgentId(1),
                    terminal_id: Some(crate::terminals::TerminalId(11)),
                    has_tasks: false,
                    interactive: true,
                    activity: AgentListActivity::Idle,
                    paused: false,
                    aegis_enabled: false,
                    context_pct_milli: None,
                    agent_kind: crate::agents::AgentKind::Terminal,
                    session_metrics: crate::shared::daemon_wire::DaemonSessionMetrics::default(),
                },
            }],
        },
    );

    assert_eq!(
        specs
            .iter()
            .filter(|spec| spec.key.kind == AgentListBloomSourceKind::Aegis)
            .count(),
        4
    );
    assert_eq!(
        specs
            .iter()
            .filter(|spec| spec.key.kind == AgentListBloomSourceKind::Main)
            .count(),
        4
    );
    assert_eq!(
        specs
            .iter()
            .filter(|spec| spec.key.kind == AgentListBloomSourceKind::Marker)
            .count(),
        4
    );
    let outer_top = specs
        .iter()
        .find(|spec| {
            spec.key.kind == AgentListBloomSourceKind::Aegis
                && spec.key.segment == AgentListBloomSourceSegment::Top
        })
        .expect("outer aegis top border should exist");
    let inner_top = specs
        .iter()
        .find(|spec| {
            spec.key.kind == AgentListBloomSourceKind::Main
                && spec.key.segment == AgentListBloomSourceSegment::Top
        })
        .expect("inner selection top border should exist");
    assert!(outer_top.rect.x < inner_top.rect.x);
    assert!(outer_top.rect.w > inner_top.rect.w);
}

#[test]
fn unselected_rows_do_not_emit_selected_bloom_sources() {
    let active_row_key = AgentListRowKey::Agent(crate::agents::AgentId(2));
    let specs = build_bloom_specs(
        HudRect {
            x: 0.0,
            y: 0.0,
            w: 400.0,
            h: 240.0,
        },
        0.0,
        None,
        Some(&active_row_key),
        &BTreeSet::new(),
        &AgentListView {
            rows: vec![AgentListRowView {
                key: AgentListRowKey::Agent(crate::agents::AgentId(1)),
                label: "ALPHA".into(),
                focused: false,
                kind: AgentListRowKind::Agent {
                    agent_id: crate::agents::AgentId(1),
                    terminal_id: Some(crate::terminals::TerminalId(11)),
                    has_tasks: false,
                    interactive: true,
                    activity: AgentListActivity::Idle,
                    paused: false,
                    aegis_enabled: false,
                    context_pct_milli: None,
                    agent_kind: crate::agents::AgentKind::Terminal,
                    session_metrics: crate::shared::daemon_wire::DaemonSessionMetrics::default(),
                },
            }],
        },
    );

    assert!(specs.is_empty());
}

/// Verifies that the bloom blur material renders its offscreen passes opaquely.
///
/// The blur passes already operate on isolated float targets, so alpha blending would only pollute
/// the intermediate images. This test locks the material policy down.
#[test]
fn bloom_blur_material_writes_offscreen_passes_opaquely() {
    let material = AgentListBloomBlurMaterial {
        image: default(),
        uniform: AgentListBloomBlurUniform {
            texel_step_gain: Vec4::ZERO,
        },
    };
    assert_eq!(Material2d::alpha_mode(&material), AlphaMode2d::Opaque);
}

/// Verifies the initial bloom setup graph created at startup.
///
/// The setup system should create the offscreen source camera, its float render target, and the
/// hidden composite sprite that will later bring the bloom result back into the main HUD composition.
#[test]
fn setup_hud_widget_bloom_spawns_camera_and_composite_sprite() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut world = World::default();
    world.insert_resource(HudBloomSettings::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(HudWidgetBloom::default());
    world.insert_resource(HudBloomOcclusionState::default());
    world.insert_resource(crate::hud::HudBloomGroupRenderState::default());
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(Assets::<Mesh>::default());
    world.insert_resource(Assets::<AgentListBloomBlurMaterial>::default());
    world.insert_resource(Assets::<AgentListBloomCompositeMaterial>::default());
    world.insert_resource(Assets::<bevy_vello::render::VelloCanvasMaterial>::default());
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..default()
        },
        PrimaryWindow,
    ));

    world.run_system_once(setup_hud_widget_bloom).unwrap();

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

    let mut composite_query = world.query_filtered::<(
        &Transform,
        &Visibility,
        &MeshMaterial2d<AgentListBloomCompositeMaterial>,
        &AgentListBloomCompositeMarker,
    ), Without<HudBloomGroupMarker>>();
    let (transform, visibility, material_handle, _) = composite_query.single(&world).unwrap();
    assert_eq!(transform.translation.z, agent_list_bloom_z());
    assert_eq!(visibility, &Visibility::Hidden);
    let composite_image_format = {
        let materials = world.resource::<Assets<AgentListBloomCompositeMaterial>>();
        let material = materials
            .get(material_handle.id())
            .expect("bloom composite material exists");
        let images = world.resource::<Assets<Image>>();
        images
            .get(material.image.id())
            .expect("bloom composite image exists")
            .texture_descriptor
            .format
    };
    assert_eq!(composite_image_format, TextureFormat::Rgba16Float);
}

/// Verifies that bloom targets are sized from logical window dimensions, not physical pixels.
///
/// This matters when a scale-factor override is active: the bloom pipeline is intentionally tied to
/// logical HUD layout, so its render targets should track the logical size.
#[test]
fn setup_hud_widget_bloom_uses_logical_window_size_for_targets() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut world = World::default();
    world.insert_resource(HudBloomSettings::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(HudWidgetBloom::default());
    world.insert_resource(HudBloomOcclusionState::default());
    world.insert_resource(crate::hud::HudBloomGroupRenderState::default());
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(Assets::<Mesh>::default());
    world.insert_resource(Assets::<AgentListBloomBlurMaterial>::default());
    world.insert_resource(Assets::<AgentListBloomCompositeMaterial>::default());
    world.insert_resource(Assets::<bevy_vello::render::VelloCanvasMaterial>::default());
    world.spawn((
        Window {
            resolution: WindowResolution::new(1400, 900).with_scale_factor_override(2.0),
            ..default()
        },
        PrimaryWindow,
    ));

    world.run_system_once(setup_hud_widget_bloom).unwrap();

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
fn composite_occlusion_rects_uv_handles_empty_single_and_multiple_rects() {
    let window = Window {
        resolution: (200, 100).into(),
        ..default()
    };
    let (empty_rects, empty_count) = composite_occlusion_rects_uv(&window, &[]);
    assert_eq!(empty_count, Vec4::new(0.0, 0.0, 0.0, 0.0));
    assert_eq!(empty_rects[0], Vec4::new(2.0, 2.0, -1.0, -1.0));

    let (single_rects, single_count) = composite_occlusion_rects_uv(
        &window,
        &[HudRect {
            x: 20.0,
            y: 10.0,
            w: 40.0,
            h: 20.0,
        }],
    );
    assert_eq!(single_count, Vec4::new(1.0, 0.0, 0.0, 0.0));
    assert_eq!(single_rects[0], Vec4::new(0.1, 0.1, 0.3, 0.3));

    let (multi_rects, multi_count) = composite_occlusion_rects_uv(
        &window,
        &[
            HudRect {
                x: 20.0,
                y: 10.0,
                w: 40.0,
                h: 20.0,
            },
            HudRect {
                x: 100.0,
                y: 20.0,
                w: 20.0,
                h: 40.0,
            },
        ],
    );
    assert_eq!(multi_count, Vec4::new(2.0, 0.0, 0.0, 0.0));
    assert_eq!(multi_rects[1], Vec4::new(0.5, 0.2, 0.6, 0.6));
}

#[test]
fn ensure_bloom_target_images_reuses_matching_handles_and_replaces_mismatched_ones() {
    let mut images = Assets::<Image>::default();
    let expected_size = UVec2::new(640, 360);
    let wrong_size = UVec2::new(320, 180);
    let source_image = images.add(bloom_target_image(expected_size));
    let blur_small_image = images.add(bloom_target_image(wrong_size));

    let mut pass = AgentListBloomPass {
        source_image: source_image.clone(),
        blur_small_image: blur_small_image.clone(),
        ..AgentListBloomPass::default()
    };

    ensure_bloom_target_images(&mut images, &mut pass, expected_size);

    assert_eq!(pass.source_image, source_image);
    assert_ne!(pass.blur_small_image, blur_small_image);
    for handle in [
        &pass.source_image,
        &pass.blur_small_image,
        &pass.blur_wide_image,
    ] {
        let image = images.get(handle).expect("bloom target image exists");
        assert_eq!(image.texture_descriptor.size.width, expected_size.x);
        assert_eq!(image.texture_descriptor.size.height, expected_size.y);
        assert_eq!(image.texture_descriptor.format, TextureFormat::Rgba16Float);
    }
}

/// Verifies that bloom sync creates the expected set of source border sprites for the active agent
/// row.
///
/// The test checks both the count and the per-segment breakdown so the bloom source generation keeps
/// producing the four border strips for both the main cell and the marker cell.
#[test]
fn sync_hud_widget_bloom_shows_group_backed_selection_composite_for_active_group_canvas() {
    let mut world = World::default();
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudWidgetKey::AgentList,
        default_hud_module_instance(&HUD_WIDGET_DEFINITIONS[1]),
    );
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(HudBloomSettings::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(HudWidgetBloom::default());
    world.insert_resource(HudBloomOcclusionState::default());
    world.insert_resource(crate::hud::HudBloomGroupRenderState::default());
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(Assets::<Mesh>::default());
    world.insert_resource(Assets::<AgentListBloomBlurMaterial>::default());
    world.insert_resource(Assets::<AgentListBloomCompositeMaterial>::default());
    world.insert_resource(Assets::<bevy_vello::render::VelloCanvasMaterial>::default());
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..default()
        },
        PrimaryWindow,
    ));

    world.run_system_once(setup_hud_widget_bloom).unwrap();
    insert_mock_group_canvas(
        &mut world,
        HudBloomGroupId::AgentListSelection,
        UVec2::new(1400, 900),
    );
    set_active_bloom_groups(&mut world, [HudBloomGroupId::AgentListSelection]);
    world.run_system_once(sync_hud_widget_bloom).unwrap();

    assert_eq!(
        visible_group_composite_count(&mut world, HudBloomGroupId::AgentListSelection),
        1
    );
    let mut composite_query = world.query::<(
        &HudBloomGroupMarker,
        &Visibility,
        &Transform,
        &AgentListBloomCompositeMarker,
    )>();
    let (_, visibility, transform, _) = composite_query
        .iter(&world)
        .find(|(marker, _, _, _)| marker.group == HudBloomGroupId::AgentListSelection)
        .expect("selection composite exists");
    assert_eq!(visibility, &Visibility::Visible);
    assert_eq!(transform.translation.z, agent_list_bloom_z());
    assert_eq!(transform.scale, Vec3::new(1400.0, 900.0, 1.0));
}

/// Verifies that bloom is suppressed while a modal HUD surface is visible.
///
/// The bloom effect should not leak behind message/task dialogs, so the sync system must remove any
/// source sprites and hide the composite sprite when a modal is active.
#[test]
fn sync_hud_widget_bloom_hides_group_backed_composite_while_modal_is_visible() {
    let mut world = World::default();
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudWidgetKey::AgentList,
        default_hud_module_instance(&HUD_WIDGET_DEFINITIONS[1]),
    );
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(HudBloomSettings::default());
    let mut app_session = crate::app::AppSessionState::default();
    app_session.composer.message_editor.visible = true;
    world.insert_resource(app_session);
    world.insert_resource(HudWidgetBloom::default());
    world.insert_resource(HudBloomOcclusionState::default());
    world.insert_resource(crate::hud::HudBloomGroupRenderState::default());
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(Assets::<Mesh>::default());
    world.insert_resource(Assets::<AgentListBloomBlurMaterial>::default());
    world.insert_resource(Assets::<AgentListBloomCompositeMaterial>::default());
    world.insert_resource(Assets::<bevy_vello::render::VelloCanvasMaterial>::default());
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..default()
        },
        PrimaryWindow,
    ));

    world.run_system_once(setup_hud_widget_bloom).unwrap();
    insert_mock_group_canvas(
        &mut world,
        HudBloomGroupId::AgentListSelection,
        UVec2::new(1400, 900),
    );
    set_active_bloom_groups(&mut world, [HudBloomGroupId::AgentListSelection]);
    world.run_system_once(sync_hud_widget_bloom).unwrap();

    assert_eq!(
        visible_group_composite_count(&mut world, HudBloomGroupId::AgentListSelection),
        0
    );
}

/// Verifies that bloom sources are generated only for the active agent row.
///
/// Even if multiple rows exist, the bloom effect should track the currently focused/active terminal so
/// the visual emphasis stays singular.
#[test]
fn sync_hud_widget_bloom_only_shows_groups_marked_active_this_frame() {
    let mut world = World::default();
    world.insert_resource(HudBloomSettings::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(HudWidgetBloom::default());
    world.insert_resource(HudBloomOcclusionState::default());
    world.insert_resource(crate::hud::HudBloomGroupRenderState::default());
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(Assets::<Mesh>::default());
    world.insert_resource(Assets::<AgentListBloomBlurMaterial>::default());
    world.insert_resource(Assets::<AgentListBloomCompositeMaterial>::default());
    world.insert_resource(Assets::<bevy_vello::render::VelloCanvasMaterial>::default());
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..default()
        },
        PrimaryWindow,
    ));

    world.run_system_once(setup_hud_widget_bloom).unwrap();
    insert_mock_group_canvas(
        &mut world,
        HudBloomGroupId::AgentListSelection,
        UVec2::new(1400, 900),
    );
    insert_mock_group_canvas(
        &mut world,
        HudBloomGroupId::AgentListAegis,
        UVec2::new(1400, 900),
    );
    set_active_bloom_groups(&mut world, [HudBloomGroupId::AgentListSelection]);
    world.run_system_once(sync_hud_widget_bloom).unwrap();

    assert_eq!(
        visible_group_composite_count(&mut world, HudBloomGroupId::AgentListSelection),
        1
    );
    assert_eq!(
        visible_group_composite_count(&mut world, HudBloomGroupId::AgentListAegis),
        0
    );
}

#[test]
fn sync_hud_widget_bloom_includes_selected_tmux_row_in_selection_group_path() {
    let mut world = World::default();
    world.insert_resource(HudBloomSettings::default());
    world.insert_resource(crate::app::AppSessionState::default());
    world.insert_resource(HudWidgetBloom::default());
    world.insert_resource(HudBloomOcclusionState::default());
    world.insert_resource(crate::hud::HudBloomGroupRenderState::default());
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(Assets::<Mesh>::default());
    world.insert_resource(Assets::<AgentListBloomBlurMaterial>::default());
    world.insert_resource(Assets::<AgentListBloomCompositeMaterial>::default());
    world.insert_resource(Assets::<bevy_vello::render::VelloCanvasMaterial>::default());
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..default()
        },
        PrimaryWindow,
    ));

    world.run_system_once(setup_hud_widget_bloom).unwrap();
    insert_mock_group_canvas(
        &mut world,
        HudBloomGroupId::AgentListSelection,
        UVec2::new(1400, 900),
    );
    set_active_bloom_groups(&mut world, [HudBloomGroupId::AgentListSelection]);
    world.run_system_once(sync_hud_widget_bloom).unwrap();

    assert_eq!(
        visible_group_composite_count(&mut world, HudBloomGroupId::AgentListSelection),
        1
    );
}
