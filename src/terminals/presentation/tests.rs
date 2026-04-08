use super::*;
use crate::{
    app_config::{DEFAULT_CELL_HEIGHT_PX, DEFAULT_CELL_WIDTH_PX},
    hud::{HudInputCaptureState, HudLayoutState, HudState, HudWidgetKey},
};
use bevy::ecs::system::RunSystemOnce;
use std::{
    collections::BTreeSet,
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};

use super::super::{
    bridge::TerminalBridge,
    daemon::{
        AttachedDaemonSession, DaemonSessionInfo, OwnedTmuxSessionInfo, TerminalDaemonClient,
        TerminalDaemonClientResource,
    },
    debug::TerminalDebugStats,
    mailbox::TerminalUpdateMailbox,
    presentation_state::{
        PresentedTerminal, TerminalDisplayMode, TerminalPresentationStore, TerminalTextureState,
        TerminalViewState,
    },
    registry::{TerminalFocusState, TerminalManager},
    runtime::TerminalRuntimeSpawner,
    types::{TerminalCommand, TerminalRuntimeState, TerminalSurface},
};

/// Returns the active terminal cell size.
pub(crate) fn active_terminal_cell_size(
    _window: &Window,
    _view_state: &TerminalViewState,
) -> UVec2 {
    fixed_terminal_cell_size(&TerminalFontState::default())
}

/// Returns the active terminal dimensions.
pub(crate) fn active_terminal_dimensions(
    window: &Window,
    layout_state: &HudLayoutState,
    _view_state: &TerminalViewState,
    font_state: &TerminalFontState,
) -> TerminalDimensions {
    target_active_terminal_dimensions(window, layout_state, font_state)
}

/// Returns the active terminal dimensions together with the derived texture state.
pub(crate) fn active_terminal_layout(
    window: &Window,
    layout_state: &HudLayoutState,
    view_state: &TerminalViewState,
    font_state: &TerminalFontState,
) -> (TerminalDimensions, TerminalTextureState) {
    let layout = active_terminal_layout_for_dimensions(
        window,
        layout_state,
        view_state,
        target_active_terminal_dimensions(window, layout_state, font_state),
        font_state,
    );
    (layout.dimensions, active_layout_texture_state(layout))
}

/// Computes the raster cell size chosen for pixel-perfect scaling of a fixed terminal geometry.
fn pixel_perfect_cell_size(
    cols: usize,
    rows: usize,
    window: &Window,
    layout_state: &HudLayoutState,
    font_state: &TerminalFontState,
) -> UVec2 {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let base_w = font_state.cell_metrics.cell_width;
    let base_h = font_state.cell_metrics.cell_height;
    let base_texture_width = (cols as u32).max(1) as f32 * base_w as f32;
    let base_texture_height = (rows as u32).max(1) as f32 * base_h as f32;
    let (fit_size_logical, _) = active_terminal_fit_area(window, layout_state);
    let fit_size_physical = logical_to_physical_size(fit_size_logical, window);
    let raster_scale = (fit_size_physical.x / base_texture_width)
        .min(fit_size_physical.y / base_texture_height)
        .max(1.0 / base_h as f32);

    UVec2::new(
        (base_w as f32 * raster_scale).floor().max(1.0) as u32,
        (base_h as f32 * raster_scale).floor().max(1.0) as u32,
    )
}

/// Snaps a logical position onto the physical pixel grid implied by the window scale factor.
///
/// Pixel-perfect terminal presentation uses this to avoid subpixel blur.
fn snap_to_pixel_grid(position: Vec2, window: &Window) -> Vec2 {
    let scale_factor = window_scale_factor(window);
    (position * scale_factor).round() / scale_factor
}

/// Exposes the logical on-screen size implied by a pixel-perfect texture state.
fn pixel_perfect_terminal_logical_size(
    texture_state: &TerminalTextureState,
    window: &Window,
) -> Vec2 {
    terminal_logical_size(texture_state, window)
}

/// Creates a test terminal bridge together with a mailbox tests can use for synthetic updates.
fn test_bridge() -> (TerminalBridge, Arc<TerminalUpdateMailbox>) {
    let (input_tx, _input_rx) = mpsc::channel::<TerminalCommand>();
    let mailbox = Arc::new(TerminalUpdateMailbox::default());
    let bridge = TerminalBridge::new(
        input_tx,
        mailbox.clone(),
        Arc::new(Mutex::new(TerminalDebugStats::default())),
    );
    (bridge, mailbox)
}

/// Inserts a prepared terminal manager into a world together with the mirrored focus resource.
fn insert_terminal_manager_resources(world: &mut World, terminal_manager: TerminalManager) {
    world.insert_resource(terminal_manager.clone_focus_state());
    world.insert_resource(terminal_manager);
}

/// App-level wrapper around [`insert_terminal_manager_resources`].
fn insert_terminal_manager_resources_into_app(app: &mut App, terminal_manager: TerminalManager) {
    insert_terminal_manager_resources(app.world_mut(), terminal_manager);
}

/// Inserts the default HUD resources needed by presentation systems.
fn insert_default_hud_resources(world: &mut World) {
    world.insert_resource(HudLayoutState::default());
    world.insert_resource(HudInputCaptureState::default());
    if !world.contains_resource::<TerminalFocusState>() {
        world.insert_resource(TerminalFocusState::default());
    }
    if !world.contains_resource::<crate::agents::AgentCatalog>() {
        world.insert_resource(crate::agents::AgentCatalog::default());
    }
    if !world.contains_resource::<crate::agents::AgentRuntimeIndex>() {
        world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    }
    if !world.contains_resource::<crate::agents::AgentStatusStore>() {
        world.insert_resource(crate::agents::AgentStatusStore::default());
    }
    if !world.contains_resource::<crate::visual_contract::VisualContractState>() {
        world.insert_resource(crate::visual_contract::VisualContractState::default());
    }
}

/// Restores the test HUD snapshot into the specific resources presentation tests read.
fn insert_test_hud_state(world: &mut World, hud_state: HudState) {
    let (layout_state, _modal_state, input_capture) = hud_state.into_resources();
    world.insert_resource(layout_state);
    world.insert_resource(input_capture);
    if !world.contains_resource::<TerminalFocusState>() {
        world.insert_resource(TerminalFocusState::default());
    }
    if !world.contains_resource::<crate::agents::AgentCatalog>() {
        world.insert_resource(crate::agents::AgentCatalog::default());
    }
    if !world.contains_resource::<crate::agents::AgentRuntimeIndex>() {
        world.insert_resource(crate::agents::AgentRuntimeIndex::default());
    }
    if !world.contains_resource::<crate::agents::AgentStatusStore>() {
        world.insert_resource(crate::agents::AgentStatusStore::default());
    }
    if !world.contains_resource::<crate::visual_contract::VisualContractState>() {
        world.insert_resource(crate::visual_contract::VisualContractState::default());
    }
}

/// App-level wrapper around [`insert_test_hud_state`].
fn insert_test_hud_state_into_app(app: &mut App, hud_state: HudState) {
    insert_test_hud_state(app.world_mut(), hud_state);
}

#[derive(Default)]
struct FakeDaemonClient {
    sessions: Mutex<BTreeSet<String>>,
    resize_requests: Mutex<Vec<(String, usize, usize)>>,
}

impl TerminalDaemonClient for FakeDaemonClient {
    /// Returns fake session listings from the in-memory set.
    fn list_sessions(&self) -> Result<Vec<DaemonSessionInfo>, String> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .iter()
            .cloned()
            .map(|session_id| DaemonSessionInfo {
                session_id,
                runtime: TerminalRuntimeState::running("fake daemon"),
                revision: 0,
                created_order: 0,
                metadata: crate::shared::daemon_wire::DaemonSessionMetadata::default(),
            })
            .collect())
    }

    fn update_session_metadata_label(
        &self,
        _session_id: &str,
        _agent_label: Option<&str>,
    ) -> Result<(), String> {
        Ok(())
    }

    /// Creates a fake session with a fixed suffix and inserts it into the set.
    fn create_session_with_env(
        &self,
        prefix: &str,
        _cwd: Option<&str>,
        _env_overrides: &[(String, String)],
    ) -> Result<String, String> {
        let session_id = format!("{prefix}1");
        self.sessions.lock().unwrap().insert(session_id.clone());
        Ok(session_id)
    }

    fn list_owned_tmux_sessions(&self) -> Result<Vec<OwnedTmuxSessionInfo>, String> {
        Ok(Vec::new())
    }

    fn create_owned_tmux_session(
        &self,
        _owner_agent_uid: &str,
        _display_name: &str,
        _cwd: Option<&str>,
        _command: &str,
    ) -> Result<OwnedTmuxSessionInfo, String> {
        Err("owned tmux not needed in presentation tests".into())
    }

    fn capture_owned_tmux_session(
        &self,
        _session_uid: &str,
        _lines: usize,
    ) -> Result<String, String> {
        Err("owned tmux not needed in presentation tests".into())
    }

    fn kill_owned_tmux_session(&self, _session_uid: &str) -> Result<(), String> {
        Err("owned tmux not needed in presentation tests".into())
    }

    fn kill_owned_tmux_sessions_for_agent(&self, _owner_agent_uid: &str) -> Result<(), String> {
        Err("owned tmux not needed in presentation tests".into())
    }

    /// Returns a dummy attached session with an empty snapshot and a disconnected update channel.
    fn attach_session(&self, _session_id: &str) -> Result<AttachedDaemonSession, String> {
        let (_tx, rx) = mpsc::channel();
        Ok(AttachedDaemonSession {
            snapshot: Default::default(),
            updates: rx,
        })
    }

    /// Accepts and discards the command.
    fn send_command(&self, _session_id: &str, _command: TerminalCommand) -> Result<(), String> {
        Ok(())
    }

    /// Records the resize request for later assertion.
    fn resize_session(&self, session_id: &str, cols: usize, rows: usize) -> Result<(), String> {
        self.resize_requests
            .lock()
            .unwrap()
            .push((session_id.to_owned(), cols, rows));
        Ok(())
    }

    /// Accepts and discards the kill request.
    fn kill_session(&self, _session_id: &str) -> Result<(), String> {
        Ok(())
    }
}

/// Builds a runtime spawner backed by the fake daemon client.
fn fake_runtime_spawner(client: Arc<FakeDaemonClient>) -> TerminalRuntimeSpawner {
    TerminalRuntimeSpawner::for_tests(TerminalDaemonClientResource::from_client(client))
}

fn run_panel_frame_sync(world: &mut World) {
    world
        .run_system_once(crate::visual_contract::sync_visual_contract_state)
        .unwrap();
    world.run_system_once(sync_terminal_panel_frames).unwrap();
}

/// Verifies that pixel-perfect cell sizing never collapses to zero and keeps width/height scaling
/// roughly uniform.
#[test]
fn pixel_perfect_cell_size_stays_positive_and_scales_uniformly() {
    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let hud_state = HudState::default();
    let font_state = TerminalFontState::default();
    let cell_size =
        pixel_perfect_cell_size(120, 38, &window, &hud_state.layout_state(), &font_state);
    assert!(cell_size.x >= 1);
    assert!(cell_size.y >= 1);

    let width_scale = cell_size.x as f32 / DEFAULT_CELL_WIDTH_PX as f32;
    let height_scale = cell_size.y as f32 / DEFAULT_CELL_HEIGHT_PX as f32;
    assert!((width_scale - height_scale).abs() < 0.1);
}

/// Verifies that pixel-grid snapping is performed in physical pixels and then mapped back to logical
/// coordinates via the window scale factor.
#[test]
fn snap_to_pixel_grid_respects_window_scale_factor() {
    let mut window = Window::default();
    window.resolution.set_scale_factor_override(Some(1.5));
    let snapped = snap_to_pixel_grid(Vec2::new(10.2, -3.4), &window);
    assert_eq!(snapped, Vec2::new(10.0, -10.0 / 3.0));
}

/// Verifies that active terminal target position accounts for texture parity.
#[test]
fn active_terminal_target_position_accounts_for_texture_parity() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut window = Window::default();
    window.resolution.set_scale_factor_override(Some(1.0));
    window.resolution.set(1400.0, 900.0);

    let mut hud_state = HudState::default();
    hud_state.insert_default_module(HudWidgetKey::AgentList);
    let rect = crate::hud::docked_agent_list_rect(&window);
    hud_state.set_module_shell_state(HudWidgetKey::AgentList, true, rect, rect, 1.0, 1.0);

    let even = hud_terminal_target_position(
        &window,
        &hud_state.layout_state(),
        &TerminalTextureState {
            texture_size: UVec2::new(1000, 800),
            cell_size: UVec2::new(10, 16),
        },
    );
    let odd = hud_terminal_target_position(
        &window,
        &hud_state.layout_state(),
        &TerminalTextureState {
            texture_size: UVec2::new(999, 799),
            cell_size: UVec2::new(9, 17),
        },
    );

    assert_eq!(even, Vec2::new(150.0, 0.0));
    assert_eq!(odd, Vec2::new(150.5, -0.5));
}

/// Verifies that pixel-perfect logical sizing divides physical texture size by the window scale
/// factor.
#[test]
fn pixel_perfect_terminal_logical_size_uses_scale_factor() {
    let mut window = Window::default();
    window.resolution.set_scale_factor_override(Some(2.0));
    let texture_state = TerminalTextureState {
        texture_size: UVec2::new(200, 120),
        ..Default::default()
    };
    assert_eq!(
        pixel_perfect_terminal_logical_size(&texture_state, &window),
        Vec2::new(100.0, 60.0)
    );
}

/// Verifies that the active-terminal viewport shrinks horizontally when the docked agent list is
/// enabled.
#[test]
fn active_terminal_viewport_reserves_agent_list_column() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let mut hud_state = HudState::default();
    hud_state.insert_default_module(HudWidgetKey::AgentList);
    let rect = crate::hud::docked_agent_list_rect(&window);
    hud_state.set_module_shell_state(HudWidgetKey::AgentList, true, rect, rect, 1.0, 1.0);

    assert_eq!(
        active_terminal_viewport(&window, &hud_state.layout_state()),
        (Vec2::new(1100.0, 900.0), Vec2::new(150.0, 0.0))
    );
}

/// Verifies that the active-terminal viewport also reserves the top info-bar header when enabled.
#[test]
fn active_terminal_viewport_reserves_info_bar_header() {
    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let mut hud_state = HudState::default();
    hud_state.insert_default_module(HudWidgetKey::InfoBar);
    hud_state.insert_default_module(HudWidgetKey::AgentList);
    let info_bar_rect = crate::hud::docked_info_bar_rect(&window);
    let agent_list_rect =
        crate::hud::docked_agent_list_rect_with_top_inset(&window, info_bar_rect.h);
    hud_state.set_module_shell_state(
        HudWidgetKey::InfoBar,
        true,
        info_bar_rect,
        info_bar_rect,
        1.0,
        1.0,
    );
    hud_state.set_module_shell_state(
        HudWidgetKey::AgentList,
        true,
        agent_list_rect,
        agent_list_rect,
        1.0,
        1.0,
    );

    assert_eq!(
        active_terminal_viewport(&window, &hud_state.layout_state()),
        (Vec2::new(1100.0, 840.0), Vec2::new(150.0, -30.0))
    );
}

/// Verifies that the fixed two-row info-bar header reserves the same top band on narrower windows.
#[test]
fn active_terminal_viewport_uses_fixed_two_row_info_bar_header() {
    let window = Window {
        resolution: (1000, 900).into(),
        ..Default::default()
    };
    let mut hud_state = HudState::default();
    hud_state.insert_default_module(HudWidgetKey::InfoBar);
    hud_state.insert_default_module(HudWidgetKey::AgentList);
    let info_bar_rect = crate::hud::docked_info_bar_rect(&window);
    let agent_list_rect =
        crate::hud::docked_agent_list_rect_with_top_inset(&window, info_bar_rect.h);
    hud_state.set_module_shell_state(
        HudWidgetKey::InfoBar,
        true,
        info_bar_rect,
        info_bar_rect,
        1.0,
        1.0,
    );
    hud_state.set_module_shell_state(
        HudWidgetKey::AgentList,
        true,
        agent_list_rect,
        agent_list_rect,
        1.0,
        1.0,
    );

    assert_eq!(info_bar_rect.h, 60.0);
    assert_eq!(
        active_terminal_viewport(&window, &hud_state.layout_state()),
        (Vec2::new(700.0, 840.0), Vec2::new(150.0, -30.0))
    );
}

/// Verifies that the active terminal presentation uses the texture's logical size and snaps to the
/// center of the usable viewport.
#[test]
fn active_terminal_presentation_uses_texture_logical_size_and_centers_in_viewport() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal(bridge);

    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let mut hud_state = HudState::default();
    hud_state.insert_default_module(HudWidgetKey::AgentList);
    let rect = crate::hud::docked_agent_list_rect(&window);
    hud_state.set_module_shell_state(HudWidgetKey::AgentList, true, rect, rect, 1.0, 1.0);

    let view_state = TerminalViewState::default();
    let font_state = TerminalFontState::default();
    let active_layout =
        active_terminal_layout(&window, &hud_state.layout_state(), &view_state, &font_state);
    for (_, terminal) in manager.iter_mut() {
        terminal.snapshot.surface = Some(TerminalSurface::new(
            active_layout.0.cols,
            active_layout.0.rows,
        ));
    }
    let texture_state = TerminalTextureState {
        texture_size: active_layout.1.texture_size,
        cell_size: active_layout.1.cell_size,
    };
    let expected_size = terminal_texture_screen_size(
        &texture_state,
        &view_state,
        &window,
        &hud_state.layout_state(),
        false,
    );

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id,
        PresentedTerminal {
            image: Default::default(),
            texture_state: texture_state.clone(),
            desired_texture_state: texture_state.clone(),
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 0,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(presentation_store);
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(view_state);
    insert_test_hud_state(&mut world, hud_state);
    world.spawn((window, PrimaryWindow));
    world.spawn((
        TerminalPanel { id },
        TerminalPresentation {
            home_position: Vec2::ZERO,
            current_position: Vec2::ZERO,
            target_position: Vec2::ZERO,
            current_size: Vec2::ONE,
            target_size: Vec2::ONE,
            current_alpha: 1.0,
            target_alpha: 1.0,
            current_z: 0.0,
            target_z: 0.0,
        },
        Transform::default(),
        Sprite::default(),
        Visibility::Visible,
    ));

    world.run_system_once(sync_terminal_presentations).unwrap();

    let mut query = world.query::<(&TerminalPresentation, &Transform)>();
    let (presentation, transform) = query.single(&world).unwrap();
    assert!(presentation.current_size.distance(expected_size) < 0.2);
    assert!(
        presentation
            .current_position
            .distance(Vec2::new(150.0, 0.0))
            < 0.2
    );
    assert!((transform.translation.x - 150.0).abs() < 0.2);
    assert!(transform.translation.y.abs() < 0.2);
    assert!((transform.translation.z - 0.3).abs() < 0.01);
}

/// Verifies that changing the active terminal layout contract causes immediate presentation snapping
/// instead of animating through stale geometry.
#[test]
fn active_terminal_snaps_immediately_when_active_layout_changes() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal(bridge);

    let initial_window = Window {
        resolution: (800, 600).into(),
        ..Default::default()
    };
    let final_window = Window {
        resolution: bevy::window::WindowResolution::new(2880, 1800).with_scale_factor_override(1.5),
        ..Default::default()
    };
    let hud_state = HudState::default();
    let view_state = TerminalViewState::default();
    let font_state = TerminalFontState::default();
    let initial_layout = active_terminal_layout(
        &initial_window,
        &hud_state.layout_state(),
        &view_state,
        &font_state,
    );
    let final_layout = active_terminal_layout(
        &final_window,
        &hud_state.layout_state(),
        &view_state,
        &font_state,
    );
    manager.get_mut(id).unwrap().snapshot.surface = Some(TerminalSurface::new(
        initial_layout.0.cols,
        initial_layout.0.rows,
    ));
    manager.get_mut(id).unwrap().surface_revision = 1;

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id,
        PresentedTerminal {
            image: Default::default(),
            texture_state: TerminalTextureState {
                texture_size: initial_layout.1.texture_size,
                cell_size: initial_layout.1.cell_size,
            },
            desired_texture_state: TerminalTextureState {
                texture_size: initial_layout.1.texture_size,
                cell_size: initial_layout.1.cell_size,
            },
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 1,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut app = App::new();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    app.insert_resource(time);
    insert_terminal_manager_resources_into_app(&mut app, manager);
    app.insert_resource(presentation_store);
    app.insert_resource(crate::hud::TerminalVisibilityState::default());
    app.insert_resource(view_state);
    insert_test_hud_state_into_app(&mut app, hud_state);
    app.add_systems(Update, sync_terminal_presentations);
    let window_entity = app.world_mut().spawn((initial_window, PrimaryWindow)).id();
    app.world_mut().spawn((
        TerminalPanel { id },
        TerminalPresentation {
            home_position: Vec2::ZERO,
            current_position: Vec2::ZERO,
            target_position: Vec2::ZERO,
            current_size: Vec2::ONE,
            target_size: Vec2::ONE,
            current_alpha: 1.0,
            target_alpha: 1.0,
            current_z: 0.0,
            target_z: 0.0,
        },
        Transform::default(),
        Sprite::default(),
        Visibility::Visible,
    ));

    app.update();

    {
        let world = app.world_mut();
        let mut window = world.get_mut::<Window>(window_entity).unwrap();
        *window = final_window.clone();
    }
    {
        let world = app.world_mut();
        let mut manager = world.resource_mut::<TerminalManager>();
        manager.get_mut(id).unwrap().snapshot.surface = Some(TerminalSurface::new(
            final_layout.0.cols,
            final_layout.0.rows,
        ));
        manager.get_mut(id).unwrap().surface_revision = 2;
    }
    {
        let world = app.world_mut();
        let mut store = world.resource_mut::<TerminalPresentationStore>();
        let presented = store.get_mut(id).unwrap();
        presented.texture_state = TerminalTextureState {
            texture_size: final_layout.1.texture_size,
            cell_size: final_layout.1.cell_size,
        };
        presented.desired_texture_state = presented.texture_state.clone();
        presented.uploaded_revision = 2;
    }

    app.update();

    let expected_size = terminal_texture_screen_size(
        &TerminalTextureState {
            texture_size: final_layout.1.texture_size,
            cell_size: final_layout.1.cell_size,
        },
        &TerminalViewState::default(),
        &final_window,
        &HudState::default().layout_state(),
        false,
    );
    let world = app.world_mut();
    let mut query = world.query::<&TerminalPresentation>();
    let presentation = query.single(world).unwrap();
    assert_eq!(presentation.current_size, expected_size);
    assert_eq!(presentation.target_size, expected_size);
}

/// Verifies that changing active focus/isolation snaps the new active terminal immediately instead of
/// blending from its old background presentation.
#[test]
fn switching_active_terminal_snaps_immediately_without_animation() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);
    manager.focus_terminal(id_one);

    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let mut hud_state = HudState::default();
    hud_state.insert_default_module(HudWidgetKey::AgentList);
    let rect = crate::hud::docked_agent_list_rect(&window);
    hud_state.set_module_shell_state(HudWidgetKey::AgentList, true, rect, rect, 1.0, 1.0);

    let view_state = TerminalViewState::default();
    let font_state = TerminalFontState::default();
    let active_layout =
        active_terminal_layout(&window, &hud_state.layout_state(), &view_state, &font_state);
    let dimensions = active_layout.0;
    let active_texture_state = TerminalTextureState {
        texture_size: active_layout.1.texture_size,
        cell_size: active_layout.1.cell_size,
    };
    let stale_background_texture_state = TerminalTextureState {
        texture_size: UVec2::new(
            dimensions.cols as u32 * DEFAULT_CELL_WIDTH_PX,
            dimensions.rows as u32 * DEFAULT_CELL_HEIGHT_PX,
        ),
        cell_size: UVec2::new(DEFAULT_CELL_WIDTH_PX, DEFAULT_CELL_HEIGHT_PX),
    };
    let expected_size = terminal_texture_screen_size(
        &active_texture_state,
        &view_state,
        &window,
        &hud_state.layout_state(),
        false,
    );

    for (_, terminal) in manager.iter_mut() {
        terminal.snapshot.surface = Some(TerminalSurface::new(dimensions.cols, dimensions.rows));
    }

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id_one,
        PresentedTerminal {
            image: Default::default(),
            texture_state: active_texture_state.clone(),
            desired_texture_state: active_texture_state,
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 0,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );
    presentation_store.register(
        id_two,
        PresentedTerminal {
            image: Default::default(),
            texture_state: stale_background_texture_state.clone(),
            desired_texture_state: stale_background_texture_state,
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 0,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut app = App::new();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    app.insert_resource(time);
    insert_terminal_manager_resources_into_app(&mut app, manager);
    app.insert_resource(presentation_store);
    app.insert_resource(crate::hud::TerminalVisibilityState {
        policy: crate::hud::TerminalVisibilityPolicy::ShowAll,
    });
    app.insert_resource(view_state);
    insert_test_hud_state_into_app(&mut app, hud_state);
    app.add_systems(Update, sync_terminal_presentations);
    app.world_mut().spawn((window, PrimaryWindow));
    app.world_mut().spawn((
        TerminalPanel { id: id_one },
        TerminalPresentation {
            home_position: Vec2::new(-360.0, 120.0),
            current_position: Vec2::new(-360.0, 120.0),
            target_position: Vec2::new(-360.0, 120.0),
            current_size: Vec2::new(200.0, 120.0),
            target_size: Vec2::new(200.0, 120.0),
            current_alpha: 1.0,
            target_alpha: 1.0,
            current_z: 0.3,
            target_z: 0.3,
        },
        Transform::default(),
        Sprite::default(),
        Visibility::Visible,
    ));
    app.world_mut().spawn((
        TerminalPanel { id: id_two },
        TerminalPresentation {
            home_position: Vec2::new(0.0, 120.0),
            current_position: Vec2::new(0.0, 120.0),
            target_position: Vec2::new(0.0, 120.0),
            current_size: Vec2::new(200.0, 120.0),
            target_size: Vec2::new(200.0, 120.0),
            current_alpha: 0.84,
            target_alpha: 0.84,
            current_z: -0.05,
            target_z: -0.05,
        },
        Transform::default(),
        Sprite::default(),
        Visibility::Visible,
    ));

    app.update();

    {
        let focus_state = {
            let mut manager = app.world_mut().resource_mut::<TerminalManager>();
            manager.focus_terminal(id_two);
            manager.clone_focus_state()
        };
        app.world_mut().insert_resource(focus_state);
    }
    app.world_mut()
        .resource_mut::<crate::hud::TerminalVisibilityState>()
        .policy = crate::hud::TerminalVisibilityPolicy::Isolate(id_two);

    app.update();

    let world = app.world_mut();
    let mut query = world.query::<(&TerminalPanel, &TerminalPresentation, &Visibility)>();
    let rows = query.iter(world).collect::<Vec<_>>();
    let first = rows
        .iter()
        .find(|(panel, _, _)| panel.id == id_one)
        .unwrap();
    let second = rows
        .iter()
        .find(|(panel, _, _)| panel.id == id_two)
        .unwrap();
    assert_eq!(*first.2, Visibility::Hidden);
    assert_eq!(second.1.current_position, Vec2::new(150.0, 0.0));
    assert_eq!(second.1.current_size, expected_size);
    assert_eq!(second.1.current_alpha, 1.0);
    assert_eq!(second.1.current_z, 0.3);
}

/// Verifies that when focus switches to a terminal whose active-layout upload is not ready yet, the
/// cached frame stays visible rather than disappearing.
#[test]
fn switching_active_terminal_keeps_cached_frame_visible_until_resized_surface_arrives() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);

    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let mut hud_state = HudState::default();
    hud_state.insert_default_module(HudWidgetKey::AgentList);
    let rect = crate::hud::docked_agent_list_rect(&window);
    hud_state.set_module_shell_state(HudWidgetKey::AgentList, true, rect, rect, 1.0, 1.0);

    let mut view_state = TerminalViewState::default();
    view_state.distance = 5.0;
    let layout_state = hud_state.layout_state();
    let font_state = TerminalFontState::default();
    let active_layout = active_terminal_layout(&window, &layout_state, &view_state, &font_state);
    let dimensions = active_layout.0;
    let active_texture_state = TerminalTextureState {
        texture_size: active_layout.1.texture_size,
        cell_size: active_layout.1.cell_size,
    };
    let cached_background_state = TerminalTextureState {
        texture_size: UVec2::new(
            dimensions.cols as u32 * DEFAULT_CELL_WIDTH_PX,
            dimensions.rows as u32 * DEFAULT_CELL_HEIGHT_PX,
        ),
        cell_size: UVec2::new(DEFAULT_CELL_WIDTH_PX, DEFAULT_CELL_HEIGHT_PX),
    };
    let expected_size = terminal_texture_screen_size(
        &cached_background_state,
        &view_state,
        &window,
        &layout_state,
        false,
    );

    manager.focus_terminal(id_one);
    for (_, terminal) in manager.iter_mut() {
        terminal.snapshot.surface = Some(TerminalSurface::new(dimensions.cols, dimensions.rows));
        terminal.surface_revision = 1;
    }

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id_one,
        PresentedTerminal {
            image: Default::default(),
            texture_state: active_texture_state.clone(),
            desired_texture_state: active_texture_state.clone(),
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 1,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );
    presentation_store.register(
        id_two,
        PresentedTerminal {
            image: Default::default(),
            texture_state: cached_background_state.clone(),
            desired_texture_state: cached_background_state.clone(),
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 1,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut app = App::new();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    app.insert_resource(time);
    insert_terminal_manager_resources_into_app(&mut app, manager);
    app.insert_resource(presentation_store);
    app.insert_resource(crate::hud::TerminalVisibilityState {
        policy: crate::hud::TerminalVisibilityPolicy::ShowAll,
    });
    app.insert_resource(view_state);
    insert_test_hud_state_into_app(&mut app, hud_state);
    app.add_systems(Update, sync_terminal_presentations);
    app.world_mut().spawn((window, PrimaryWindow));
    app.world_mut().spawn((
        TerminalPanel { id: id_one },
        TerminalPresentation {
            home_position: Vec2::new(-360.0, 120.0),
            current_position: Vec2::new(-360.0, 120.0),
            target_position: Vec2::new(-360.0, 120.0),
            current_size: Vec2::new(200.0, 120.0),
            target_size: Vec2::new(200.0, 120.0),
            current_alpha: 1.0,
            target_alpha: 1.0,
            current_z: 0.3,
            target_z: 0.3,
        },
        Transform::default(),
        Sprite::default(),
        Visibility::Visible,
    ));
    app.world_mut().spawn((
        TerminalPanel { id: id_two },
        TerminalPresentation {
            home_position: Vec2::new(0.0, 120.0),
            current_position: Vec2::new(0.0, 120.0),
            target_position: Vec2::new(0.0, 120.0),
            current_size: Vec2::new(200.0, 120.0),
            target_size: Vec2::new(200.0, 120.0),
            current_alpha: 0.84,
            target_alpha: 0.84,
            current_z: -0.05,
            target_z: -0.05,
        },
        Transform::default(),
        Sprite::default(),
        Visibility::Visible,
    ));

    app.update();

    {
        let focus_state = {
            let mut manager = app.world_mut().resource_mut::<TerminalManager>();
            manager.focus_terminal(id_two);
            manager.clone_focus_state()
        };
        app.world_mut().insert_resource(focus_state);
    }
    app.world_mut()
        .resource_mut::<crate::hud::TerminalVisibilityState>()
        .policy = crate::hud::TerminalVisibilityPolicy::Isolate(id_two);

    app.update();

    let world = app.world_mut();
    let mut query = world.query::<(&TerminalPanel, &TerminalPresentation, &Visibility)>();
    let rows = query.iter(world).collect::<Vec<_>>();
    let first = rows
        .iter()
        .find(|(panel, _, _)| panel.id == id_one)
        .unwrap();
    let second = rows
        .iter()
        .find(|(panel, _, _)| panel.id == id_two)
        .unwrap();
    assert_eq!(*first.2, Visibility::Hidden);
    assert_eq!(*second.2, Visibility::Visible);
    assert_eq!(second.1.current_position, Vec2::new(150.0, 0.0));
    assert_eq!(second.1.current_size, expected_size);
    assert_eq!(second.1.current_alpha, 1.0);
    assert_eq!(second.1.current_z, 0.3);
}

/// Verifies that the active PTY is resized to the fixed-cell grid that fits the remaining HUD
/// viewport, independent of zoom distance.
#[test]
fn active_terminal_resize_requests_follow_viewport_grid_policy() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let client = Arc::new(FakeDaemonClient::default());
    client
        .sessions
        .lock()
        .unwrap()
        .insert("neozeus-session-1".into());
    let runtime_spawner = fake_runtime_spawner(client.clone());
    let (bridge, _) = test_bridge();

    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal_with_session(bridge, "neozeus-session-1".into());
    manager.get_mut(terminal_id).unwrap().snapshot.surface = Some(TerminalSurface::new(120, 38));

    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let mut hud_state = HudState::default();
    hud_state.insert_default_module(HudWidgetKey::AgentList);
    let rect = crate::hud::docked_agent_list_rect(&window);
    hud_state.set_module_shell_state(HudWidgetKey::AgentList, true, rect, rect, 1.0, 1.0);

    let mut view_state = TerminalViewState::default();
    view_state.distance = 5.0;

    let mut world = World::default();
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(TerminalFontState::default());
    world.insert_resource(runtime_spawner);
    world.insert_resource(view_state);
    insert_test_hud_state(&mut world, hud_state);
    world.spawn((window, PrimaryWindow));

    world
        .run_system_once(sync_active_terminal_dimensions)
        .unwrap();

    let requests = client.resize_requests.lock().unwrap().clone();
    assert_eq!(requests, vec![("neozeus-session-1".into(), 118, 54)]);
}

/// Verifies that in `ShowAll` mode with no active terminal, background terminal presentations remain
/// visible instead of all being hidden.
#[test]
fn show_all_presentations_remain_visible_when_no_terminal_is_active() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal_without_focus(bridge);
    for (_, terminal) in manager.iter_mut() {
        terminal.snapshot.surface = Some(TerminalSurface::new(2, 2));
    }

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id,
        PresentedTerminal {
            image: Default::default(),
            texture_state: TerminalTextureState {
                texture_size: UVec2::new(100, 100),
                cell_size: UVec2::new(10, 20),
            },
            desired_texture_state: TerminalTextureState {
                texture_size: UVec2::new(100, 100),
                cell_size: UVec2::new(10, 20),
            },
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 0,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(presentation_store);
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    insert_default_hud_resources(&mut world);
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        },
        PrimaryWindow,
    ));
    world.spawn((
        TerminalPanel { id },
        TerminalPresentation {
            home_position: Vec2::ZERO,
            current_position: Vec2::ZERO,
            target_position: Vec2::ZERO,
            current_size: Vec2::ONE,
            target_size: Vec2::ONE,
            current_alpha: 1.0,
            target_alpha: 1.0,
            current_z: 0.0,
            target_z: 0.0,
        },
        Transform::default(),
        Sprite::default(),
        Visibility::Visible,
    ));

    world.run_system_once(sync_terminal_presentations).unwrap();

    let mut query = world.query::<(&TerminalPanel, &Visibility)>();
    let vis = query
        .iter(&world)
        .map(|(panel, visibility)| (panel.id, *visibility))
        .collect::<Vec<_>>();
    assert_eq!(vis, vec![(id, Visibility::Visible)]);
}

/// Verifies that panel frame sprites default to hidden when no direct-input or runtime-status frame
/// should be shown.
#[test]
fn terminal_panel_frames_are_hidden_without_direct_input_mode() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut world = World::default();
    insert_default_hud_resources(&mut world);
    world.insert_resource(TerminalManager::default());
    world.insert_resource(TerminalPresentationStore::default());
    world.spawn((
        TerminalPanelFrame {
            id: crate::terminals::TerminalId(1),
        },
        Transform::default(),
        Sprite::default(),
        Visibility::Visible,
    ));

    run_panel_frame_sync(&mut world);

    let mut query = world.query::<(&TerminalPanelFrame, &Visibility)>();
    let vis = query.iter(&world).collect::<Vec<_>>();
    assert_eq!(vis.len(), 1);
    assert_eq!(*vis[0].1, Visibility::Hidden);
}

/// Verifies that direct-input mode shows the orange focus frame around the active terminal panel.
#[test]
fn direct_input_mode_shows_orange_terminal_frame() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal(bridge);

    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_direct_terminal_input(terminal_id);

    let mut world = World::default();
    insert_test_hud_state(&mut world, hud_state);
    world.insert_resource(manager);
    let panel_entity = world
        .spawn((
            TerminalPanel { id: terminal_id },
            TerminalPresentation {
                home_position: Vec2::ZERO,
                current_position: Vec2::new(30.0, -20.0),
                target_position: Vec2::ZERO,
                current_size: Vec2::new(320.0, 180.0),
                target_size: Vec2::ZERO,
                current_alpha: 1.0,
                target_alpha: 1.0,
                current_z: 0.5,
                target_z: 0.0,
            },
            Visibility::Visible,
        ))
        .id();
    let frame_entity = world
        .spawn((
            TerminalPanelFrame { id: terminal_id },
            Transform::default(),
            Sprite::default(),
            Visibility::Hidden,
        ))
        .id();
    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        terminal_id,
        PresentedTerminal {
            image: Default::default(),
            texture_state: Default::default(),
            desired_texture_state: Default::default(),
            display_mode: Default::default(),
            uploaded_revision: 0,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            panel_entity,
            frame_entity,
        },
    );
    world.insert_resource(presentation_store);

    run_panel_frame_sync(&mut world);

    let mut query = world.query::<(&TerminalPanelFrame, &Transform, &Sprite, &Visibility)>();
    let frames = query.iter(&world).collect::<Vec<_>>();
    assert_eq!(frames.len(), 1);
    assert_eq!(*frames[0].3, Visibility::Visible);
    assert_eq!(frames[0].1.translation, Vec3::new(30.0, -20.0, 0.48));
    assert_eq!(frames[0].2.custom_size, Some(Vec2::new(332.0, 192.0)));
    assert_eq!(frames[0].2.color, Color::srgba(1.0, 0.48, 0.08, 0.96));
}

/// Verifies that working activity does not draw a terminal frame when direct input is closed.
#[test]
fn working_terminal_keeps_frame_hidden_without_direct_input() {
    let mut world = World::default();
    insert_default_hud_resources(&mut world);
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal(bridge);
    manager
        .get_mut(terminal_id)
        .expect("terminal should exist")
        .snapshot
        .surface = Some({
        let mut surface = TerminalSurface::new(120, 8);
        surface.set_text_cell(0, 0, "header");
        surface.set_text_cell(1, 3, "⠋ Working...");
        surface
    });

    let mut catalog = crate::agents::AgentCatalog::default();
    let agent_id = catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Pi,
        crate::agents::AgentKind::Pi.capabilities(),
    );
    let mut runtime_index = crate::agents::AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_id, terminal_id, "session-1".into(), None);

    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    world.insert_resource(catalog);
    world.insert_resource(runtime_index);
    world.insert_resource(crate::agents::AgentStatusStore::default());
    world.insert_resource(manager);
    world
        .run_system_once(crate::agents::sync_agent_status)
        .expect("status sync should succeed");
    let panel_entity = world
        .spawn((
            TerminalPanel { id: terminal_id },
            TerminalPresentation {
                home_position: Vec2::ZERO,
                current_position: Vec2::new(10.0, 15.0),
                target_position: Vec2::ZERO,
                current_size: Vec2::new(300.0, 160.0),
                target_size: Vec2::ZERO,
                current_alpha: 1.0,
                target_alpha: 1.0,
                current_z: 0.5,
                target_z: 0.0,
            },
            Visibility::Visible,
        ))
        .id();
    let frame_entity = world
        .spawn((
            TerminalPanelFrame { id: terminal_id },
            Transform::default(),
            Sprite::default(),
            Visibility::Visible,
        ))
        .id();
    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        terminal_id,
        PresentedTerminal {
            image: Default::default(),
            texture_state: Default::default(),
            desired_texture_state: Default::default(),
            display_mode: Default::default(),
            uploaded_revision: 0,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            panel_entity,
            frame_entity,
        },
    );
    world.insert_resource(presentation_store);

    run_panel_frame_sync(&mut world);

    let mut query = world.query::<(&TerminalPanelFrame, &Visibility)>();
    let frames = query.iter(&world).collect::<Vec<_>>();
    assert_eq!(frames.len(), 1);
    assert_eq!(*frames[0].1, Visibility::Hidden);
}

/// Verifies that direct input keeps the orange frame even when the terminal is working.
#[test]
fn direct_input_mode_keeps_orange_frame_when_terminal_is_working() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal(bridge);
    manager
        .get_mut(terminal_id)
        .expect("terminal should exist")
        .snapshot
        .surface = Some({
        let mut surface = TerminalSurface::new(120, 8);
        surface.set_text_cell(0, 0, "header");
        surface.set_text_cell(1, 3, "⠋ Working...");
        surface
    });

    let mut hud_state = crate::hud::HudState::default();
    hud_state.open_direct_terminal_input(terminal_id);

    let mut catalog = crate::agents::AgentCatalog::default();
    let agent_id = catalog.create_agent(
        Some("alpha".into()),
        crate::agents::AgentKind::Pi,
        crate::agents::AgentKind::Pi.capabilities(),
    );
    let mut runtime_index = crate::agents::AgentRuntimeIndex::default();
    runtime_index.link_terminal(agent_id, terminal_id, "session-1".into(), None);

    let mut world = World::default();
    insert_test_hud_state(&mut world, hud_state);
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs(1));
    world.insert_resource(time);
    world.insert_resource(catalog);
    world.insert_resource(runtime_index);
    world.insert_resource(crate::agents::AgentStatusStore::default());
    world.insert_resource(manager);
    world
        .run_system_once(crate::agents::sync_agent_status)
        .expect("status sync should succeed");
    let panel_entity = world
        .spawn((
            TerminalPanel { id: terminal_id },
            TerminalPresentation {
                home_position: Vec2::ZERO,
                current_position: Vec2::new(30.0, -20.0),
                target_position: Vec2::ZERO,
                current_size: Vec2::new(320.0, 180.0),
                target_size: Vec2::ZERO,
                current_alpha: 1.0,
                target_alpha: 1.0,
                current_z: 0.5,
                target_z: 0.0,
            },
            Visibility::Visible,
        ))
        .id();
    let frame_entity = world
        .spawn((
            TerminalPanelFrame { id: terminal_id },
            Transform::default(),
            Sprite::default(),
            Visibility::Hidden,
        ))
        .id();
    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        terminal_id,
        PresentedTerminal {
            image: Default::default(),
            texture_state: Default::default(),
            desired_texture_state: Default::default(),
            display_mode: Default::default(),
            uploaded_revision: 0,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            panel_entity,
            frame_entity,
        },
    );
    world.insert_resource(presentation_store);

    run_panel_frame_sync(&mut world);

    let mut query = world.query::<(&TerminalPanelFrame, &Transform, &Sprite, &Visibility)>();
    let frames = query.iter(&world).collect::<Vec<_>>();
    assert_eq!(frames.len(), 1);
    assert_eq!(*frames[0].3, Visibility::Visible);
    assert_eq!(frames[0].1.translation, Vec3::new(30.0, -20.0, 0.48));
    assert_eq!(frames[0].2.custom_size, Some(Vec2::new(332.0, 192.0)));
    assert_eq!(frames[0].2.color, Color::srgba(1.0, 0.48, 0.08, 0.96));
}

/// Verifies that a disconnected terminal shows the red runtime-status frame instead of the direct
/// input frame styling.
#[test]
fn disconnected_terminal_shows_red_status_frame() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let mut world = World::default();
    insert_default_hud_resources(&mut world);
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal(bridge);
    manager
        .get_mut(terminal_id)
        .expect("terminal should exist")
        .snapshot
        .runtime = crate::terminals::TerminalRuntimeState::disconnected("dead session");
    world.insert_resource(manager);
    let panel_entity = world
        .spawn((
            TerminalPanel { id: terminal_id },
            TerminalPresentation {
                home_position: Vec2::ZERO,
                current_position: Vec2::new(10.0, 15.0),
                target_position: Vec2::ZERO,
                current_size: Vec2::new(300.0, 160.0),
                target_size: Vec2::ZERO,
                current_alpha: 1.0,
                target_alpha: 1.0,
                current_z: 0.5,
                target_z: 0.0,
            },
            Visibility::Visible,
        ))
        .id();
    let frame_entity = world
        .spawn((
            TerminalPanelFrame { id: terminal_id },
            Transform::default(),
            Sprite::default(),
            Visibility::Hidden,
        ))
        .id();
    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        terminal_id,
        PresentedTerminal {
            image: Default::default(),
            texture_state: Default::default(),
            desired_texture_state: Default::default(),
            display_mode: Default::default(),
            uploaded_revision: 0,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            panel_entity,
            frame_entity,
        },
    );
    world.insert_resource(presentation_store);

    run_panel_frame_sync(&mut world);

    let mut query = world.query::<(&TerminalPanelFrame, &Transform, &Sprite, &Visibility)>();
    let frames = query.iter(&world).collect::<Vec<_>>();
    assert_eq!(frames.len(), 1);
    assert_eq!(*frames[0].3, Visibility::Visible);
    assert_eq!(frames[0].1.translation, Vec3::new(10.0, 15.0, 0.48));
    assert_eq!(frames[0].2.custom_size, Some(Vec2::new(308.0, 168.0)));
    assert_eq!(frames[0].2.color, Color::srgba(0.86, 0.20, 0.20, 0.92));
}

/// Verifies that startup-loading terminals remain visible as non-white placeholders before their
/// first real surface upload arrives.
#[test]
fn startup_loading_shows_active_placeholder_before_first_surface_arrives() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal(bridge);

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id,
        PresentedTerminal {
            image: Default::default(),
            texture_state: Default::default(),
            desired_texture_state: Default::default(),
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 0,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    presentation_store.mark_startup_pending(id);

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(presentation_store);
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(TerminalViewState::default());
    insert_test_hud_state(&mut world, HudState::default());
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        },
        PrimaryWindow,
    ));
    world.spawn((
        TerminalPanel { id },
        TerminalPresentation {
            home_position: Vec2::ZERO,
            current_position: Vec2::ZERO,
            target_position: Vec2::ZERO,
            current_size: Vec2::ONE,
            target_size: Vec2::ONE,
            current_alpha: 1.0,
            target_alpha: 1.0,
            current_z: 0.0,
            target_z: 0.0,
        },
        Transform::default(),
        Sprite::default(),
        Visibility::Hidden,
    ));

    world.run_system_once(sync_terminal_presentations).unwrap();

    let mut query = world.query::<(&TerminalPanel, &Sprite, &Visibility)>();
    let (_, sprite, visibility) = query.single(&world).unwrap();
    assert_eq!(*visibility, Visibility::Visible);
    assert_ne!(sprite.color, Color::WHITE);
    assert!(sprite
        .custom_size
        .is_some_and(|size| size.x > 10.0 && size.y > 10.0));
}

/// Verifies that startup-loading state temporarily overrides isolate visibility so all pending
/// terminals stay visible until they are ready.
#[test]
fn startup_loading_temporarily_overrides_isolate_to_show_all_pending_terminals() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal_without_focus(bridge_one);
    let id_two = manager.create_terminal(bridge_two);

    let mut presentation_store = TerminalPresentationStore::default();
    for id in [id_one, id_two] {
        presentation_store.register(
            id,
            PresentedTerminal {
                image: Default::default(),
                texture_state: Default::default(),
                desired_texture_state: Default::default(),
                display_mode: TerminalDisplayMode::Smooth,
                uploaded_revision: 0,
                uploaded_active_override_revision: None,
                uploaded_text_selection_revision: None,
                panel_entity: Entity::PLACEHOLDER,
                frame_entity: Entity::PLACEHOLDER,
            },
        );
    }

    presentation_store.mark_startup_pending(id_one);
    presentation_store.mark_startup_pending(id_two);

    let visibility_state = crate::hud::TerminalVisibilityState {
        policy: crate::hud::TerminalVisibilityPolicy::Isolate(id_two),
    };

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(presentation_store);
    world.insert_resource(visibility_state);
    world.insert_resource(TerminalViewState::default());
    insert_test_hud_state(&mut world, HudState::default());
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        },
        PrimaryWindow,
    ));
    for id in [id_one, id_two] {
        world.spawn((
            TerminalPanel { id },
            TerminalPresentation {
                home_position: Vec2::ZERO,
                current_position: Vec2::ZERO,
                target_position: Vec2::ZERO,
                current_size: Vec2::ONE,
                target_size: Vec2::ONE,
                current_alpha: 1.0,
                target_alpha: 1.0,
                current_z: 0.0,
                target_z: 0.0,
            },
            Transform::default(),
            Sprite::default(),
            Visibility::Hidden,
        ));
    }

    world.run_system_once(sync_terminal_presentations).unwrap();

    let visible_count = world
        .query::<(&TerminalPanel, &Visibility)>()
        .iter(&world)
        .filter(|(_, visibility)| **visibility == Visibility::Visible)
        .count();
    assert_eq!(visible_count, 2);
}

/// Verifies that the active terminal does not disappear while its desired active-layout upload is
/// still pending; the cached frame stays visible.
#[test]
fn active_terminal_presentation_keeps_cached_frame_visible_until_active_layout_upload_is_ready() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal(bridge);
    manager.get_mut(id).unwrap().snapshot.surface = Some(TerminalSurface::new(80, 24));
    manager.get_mut(id).unwrap().surface_revision = 1;

    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let hud_state = crate::hud::HudState::default();
    let view_state = TerminalViewState::default();
    let font_state = TerminalFontState::default();
    let active_layout =
        active_terminal_layout(&window, &hud_state.layout_state(), &view_state, &font_state);

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id,
        PresentedTerminal {
            image: Default::default(),
            texture_state: TerminalTextureState {
                texture_size: UVec2::new(80 * DEFAULT_CELL_WIDTH_PX, 24 * DEFAULT_CELL_HEIGHT_PX),
                cell_size: UVec2::new(DEFAULT_CELL_WIDTH_PX, DEFAULT_CELL_HEIGHT_PX),
            },
            desired_texture_state: TerminalTextureState {
                texture_size: active_layout.1.texture_size,
                cell_size: active_layout.1.cell_size,
            },
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 1,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(presentation_store);
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(view_state);
    insert_test_hud_state(&mut world, hud_state);
    world.spawn((window, PrimaryWindow));
    world.spawn((
        TerminalPanel { id },
        TerminalPresentation {
            home_position: Vec2::ZERO,
            current_position: Vec2::ZERO,
            target_position: Vec2::ZERO,
            current_size: Vec2::ONE,
            target_size: Vec2::ONE,
            current_alpha: 1.0,
            target_alpha: 1.0,
            current_z: 0.0,
            target_z: 0.0,
        },
        Transform::default(),
        Sprite::default(),
        Visibility::Visible,
    ));

    world.run_system_once(sync_terminal_presentations).unwrap();

    let mut query = world.query::<(&TerminalPanel, &Visibility)>();
    let vis = query.iter(&world).collect::<Vec<_>>();
    assert_eq!(vis.len(), 1);
    assert_eq!(*vis[0].1, Visibility::Visible);
}

/// Verifies that once a terminal becomes ready for the new active layout, it reappears already
/// snapped to the final geometry rather than animating in.
#[test]
fn active_terminal_reappears_snapped_after_becoming_ready_for_new_layout() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal(bridge);

    let initial_window = Window {
        resolution: (800, 600).into(),
        ..Default::default()
    };
    let final_window = Window {
        resolution: bevy::window::WindowResolution::new(2880, 1800).with_scale_factor_override(1.5),
        ..Default::default()
    };
    let hud_state = HudState::default();
    let view_state = TerminalViewState::default();
    let font_state = TerminalFontState::default();
    let initial_layout = active_terminal_layout(
        &initial_window,
        &hud_state.layout_state(),
        &view_state,
        &font_state,
    );
    let final_layout = active_terminal_layout(
        &final_window,
        &hud_state.layout_state(),
        &view_state,
        &font_state,
    );

    manager.get_mut(id).unwrap().snapshot.surface = Some(TerminalSurface::new(
        initial_layout.0.cols,
        initial_layout.0.rows,
    ));
    manager.get_mut(id).unwrap().surface_revision = 1;

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id,
        PresentedTerminal {
            image: Default::default(),
            texture_state: TerminalTextureState {
                texture_size: initial_layout.1.texture_size,
                cell_size: initial_layout.1.cell_size,
            },
            desired_texture_state: TerminalTextureState {
                texture_size: initial_layout.1.texture_size,
                cell_size: initial_layout.1.cell_size,
            },
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 1,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut app = App::new();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    app.insert_resource(time);
    insert_terminal_manager_resources_into_app(&mut app, manager);
    app.insert_resource(presentation_store);
    app.insert_resource(crate::hud::TerminalVisibilityState::default());
    app.insert_resource(view_state);
    insert_test_hud_state_into_app(&mut app, hud_state);
    app.add_systems(Update, sync_terminal_presentations);
    let window_entity = app.world_mut().spawn((initial_window, PrimaryWindow)).id();
    app.world_mut().spawn((
        TerminalPanel { id },
        TerminalPresentation {
            home_position: Vec2::ZERO,
            current_position: Vec2::ZERO,
            target_position: Vec2::ZERO,
            current_size: Vec2::ONE,
            target_size: Vec2::ONE,
            current_alpha: 1.0,
            target_alpha: 1.0,
            current_z: 0.0,
            target_z: 0.0,
        },
        Transform::default(),
        Sprite::default(),
        Visibility::Visible,
    ));

    app.update();

    {
        let world = app.world_mut();
        let mut window = world.get_mut::<Window>(window_entity).unwrap();
        *window = final_window.clone();
    }
    app.update();

    {
        let world = app.world_mut();
        let mut manager = world.resource_mut::<TerminalManager>();
        manager.get_mut(id).unwrap().snapshot.surface = Some(TerminalSurface::new(
            final_layout.0.cols,
            final_layout.0.rows,
        ));
        manager.get_mut(id).unwrap().surface_revision = 2;
    }
    {
        let world = app.world_mut();
        let mut store = world.resource_mut::<TerminalPresentationStore>();
        let presented = store.get_mut(id).unwrap();
        presented.texture_state = TerminalTextureState {
            texture_size: final_layout.1.texture_size,
            cell_size: final_layout.1.cell_size,
        };
        presented.desired_texture_state = presented.texture_state.clone();
        presented.uploaded_revision = 2;
    }

    app.update();

    let expected_size = terminal_texture_screen_size(
        &TerminalTextureState {
            texture_size: final_layout.1.texture_size,
            cell_size: final_layout.1.cell_size,
        },
        &TerminalViewState::default(),
        &final_window,
        &HudState::default().layout_state(),
        false,
    );
    let world = app.world_mut();
    let mut query = world.query::<(&TerminalPresentation, &Visibility)>();
    let (presentation, visibility) = query.single(world).unwrap();
    assert_eq!(*visibility, Visibility::Visible);
    assert_eq!(presentation.current_size, expected_size);
    assert_eq!(presentation.target_size, expected_size);
}

/// Verifies that an active terminal presentation becomes visible as soon as its uploaded texture
/// contract matches the active layout.
#[test]
fn active_terminal_presentation_becomes_visible_once_active_layout_upload_is_ready() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal(bridge);

    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let hud_state = crate::hud::HudState::default();
    let view_state = TerminalViewState::default();
    let font_state = TerminalFontState::default();
    let active_layout =
        active_terminal_layout(&window, &hud_state.layout_state(), &view_state, &font_state);

    manager.get_mut(id).unwrap().snapshot.surface = Some(TerminalSurface::new(
        active_layout.0.cols,
        active_layout.0.rows,
    ));
    manager.get_mut(id).unwrap().surface_revision = 1;

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id,
        PresentedTerminal {
            image: Default::default(),
            texture_state: TerminalTextureState {
                texture_size: active_layout.1.texture_size,
                cell_size: active_layout.1.cell_size,
            },
            desired_texture_state: TerminalTextureState {
                texture_size: active_layout.1.texture_size,
                cell_size: active_layout.1.cell_size,
            },
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 1,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(presentation_store);
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(view_state);
    insert_test_hud_state(&mut world, hud_state);
    world.spawn((window, PrimaryWindow));
    world.spawn((
        TerminalPanel { id },
        TerminalPresentation {
            home_position: Vec2::ZERO,
            current_position: Vec2::ZERO,
            target_position: Vec2::ZERO,
            current_size: Vec2::ONE,
            target_size: Vec2::ONE,
            current_alpha: 1.0,
            target_alpha: 1.0,
            current_z: 0.0,
            target_z: 0.0,
        },
        Transform::default(),
        Sprite::default(),
        Visibility::Hidden,
    ));

    world.run_system_once(sync_terminal_presentations).unwrap();

    let mut query = world.query::<(&TerminalPanel, &Visibility)>();
    let vis = query.iter(&world).collect::<Vec<_>>();
    assert_eq!(vis.len(), 1);
    assert_eq!(*vis[0].1, Visibility::Visible);
}

/// Verifies that opening the message box does not itself hide the underlying terminal presentation.
#[test]
fn message_box_keeps_terminal_presentations_visible() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal(bridge);

    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let mut hud_state = crate::hud::HudState::default();
    let view_state = TerminalViewState::default();
    let font_state = TerminalFontState::default();
    let active_layout =
        active_terminal_layout(&window, &hud_state.layout_state(), &view_state, &font_state);
    hud_state.open_message_box(id);

    let terminal = manager.get_mut(id).unwrap();
    terminal.snapshot.surface = Some(TerminalSurface::new(
        active_layout.0.cols,
        active_layout.0.rows,
    ));
    terminal.surface_revision = 1;

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id,
        PresentedTerminal {
            image: Default::default(),
            texture_state: TerminalTextureState {
                texture_size: active_layout.1.texture_size,
                cell_size: active_layout.1.cell_size,
            },
            desired_texture_state: TerminalTextureState {
                texture_size: active_layout.1.texture_size,
                cell_size: active_layout.1.cell_size,
            },
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 1,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(presentation_store);
    world.insert_resource(crate::hud::TerminalVisibilityState::default());
    world.insert_resource(view_state);
    insert_test_hud_state(&mut world, hud_state);
    world.spawn((window, PrimaryWindow));
    world.spawn((
        TerminalPanel { id },
        TerminalPresentation {
            home_position: Vec2::ZERO,
            current_position: Vec2::ZERO,
            target_position: Vec2::ZERO,
            current_size: Vec2::ONE,
            target_size: Vec2::ONE,
            current_alpha: 1.0,
            target_alpha: 1.0,
            current_z: 0.0,
            target_z: 0.0,
        },
        Transform::default(),
        Sprite::default(),
        Visibility::Visible,
    ));

    world.run_system_once(sync_terminal_presentations).unwrap();

    let mut query = world.query::<(&TerminalPanel, &Visibility)>();
    let vis = query.iter(&world).collect::<Vec<_>>();
    assert_eq!(vis.len(), 1);
    assert_eq!(*vis[0].1, Visibility::Visible);
}

/// Verifies that a stale isolate target degrades to `ShowAll` behavior instead of hiding every
/// terminal panel.
#[test]
fn isolate_visibility_policy_with_missing_terminal_degrades_to_show_all() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id = manager.create_terminal_without_focus(bridge);
    for (_, terminal) in manager.iter_mut() {
        terminal.snapshot.surface = Some(TerminalSurface::new(2, 2));
    }

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id,
        PresentedTerminal {
            image: Default::default(),
            texture_state: TerminalTextureState {
                texture_size: UVec2::new(100, 100),
                cell_size: UVec2::new(10, 20),
            },
            desired_texture_state: TerminalTextureState {
                texture_size: UVec2::new(100, 100),
                cell_size: UVec2::new(10, 20),
            },
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 0,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(presentation_store);
    world.insert_resource(crate::hud::TerminalVisibilityState {
        policy: crate::hud::TerminalVisibilityPolicy::Isolate(crate::terminals::TerminalId(999)),
    });
    world.insert_resource(TerminalViewState::default());
    insert_default_hud_resources(&mut world);
    world.spawn((
        Window {
            resolution: (1400, 900).into(),
            ..Default::default()
        },
        PrimaryWindow,
    ));
    world.spawn((
        TerminalPanel { id },
        TerminalPresentation {
            home_position: Vec2::ZERO,
            current_position: Vec2::ZERO,
            target_position: Vec2::ZERO,
            current_size: Vec2::ONE,
            target_size: Vec2::ONE,
            current_alpha: 1.0,
            target_alpha: 1.0,
            current_z: 0.0,
            target_z: 0.0,
        },
        Transform::default(),
        Sprite::default(),
        Visibility::Visible,
    ));

    world.run_system_once(sync_terminal_presentations).unwrap();

    let mut query = world.query::<(&TerminalPanel, &Visibility)>();
    let vis = query.iter(&world).collect::<Vec<_>>();
    assert_eq!(vis.len(), 1);
    assert_eq!(*vis[0].1, Visibility::Visible);
}

/// Verifies that projection sync creates missing panel/frame entities for terminals that exist only
/// in authoritative terminal state.
#[test]
fn projection_sync_spawns_missing_terminal_entities() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal_without_focus(bridge);
    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(manager);
    world.insert_resource(TerminalPresentationStore::default());

    world
        .run_system_once(sync_terminal_projection_entities)
        .unwrap();

    let store = world.resource::<TerminalPresentationStore>();
    let presented = store
        .get(terminal_id)
        .expect("projection sync should register presentation state");
    assert_ne!(presented.panel_entity, Entity::PLACEHOLDER);
    assert_ne!(presented.frame_entity, Entity::PLACEHOLDER);
    assert_eq!(world.query::<&TerminalPanel>().iter(&world).count(), 1);
    assert_eq!(world.query::<&TerminalPanelFrame>().iter(&world).count(), 1);
}

/// Verifies that projection sync removes stale panel/frame entities after authoritative terminal
/// state drops the terminal.
#[test]
fn projection_sync_despawns_stale_terminal_entities() {
    let (bridge, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let terminal_id = manager.create_terminal_without_focus(bridge);
    let mut world = World::default();
    world.insert_resource(Assets::<Image>::default());
    world.insert_resource(manager);
    world.insert_resource(TerminalPresentationStore::default());

    world
        .run_system_once(sync_terminal_projection_entities)
        .unwrap();
    {
        let mut manager = world.resource_mut::<TerminalManager>();
        let _ = manager.remove_terminal(terminal_id);
    }

    world
        .run_system_once(sync_terminal_projection_entities)
        .unwrap();

    assert!(world
        .resource::<TerminalPresentationStore>()
        .get(terminal_id)
        .is_none());
    assert_eq!(world.query::<&TerminalPanel>().iter(&world).count(), 0);
    assert_eq!(world.query::<&TerminalPanelFrame>().iter(&world).count(), 0);
}

/// Verifies the current presentation policy in `ShowAll`: even then, only the active terminal panel
/// remains visible once focus exists.
#[test]
fn terminal_visibility_policy_show_all_keeps_only_active_terminal_visible() {
    // Arrange a representative scenario, run the behavior under test, and then assert the externally visible result.
    let (bridge_one, _) = test_bridge();
    let (bridge_two, _) = test_bridge();
    let mut manager = TerminalManager::default();
    let id_one = manager.create_terminal(bridge_one);
    let id_two = manager.create_terminal(bridge_two);
    manager.focus_terminal(id_one);

    let window = Window {
        resolution: (1400, 900).into(),
        ..Default::default()
    };
    let hud_state = crate::hud::HudState::default();
    let view_state = TerminalViewState::default();
    let font_state = TerminalFontState::default();
    let active_layout =
        active_terminal_layout(&window, &hud_state.layout_state(), &view_state, &font_state);

    manager.get_mut(id_one).unwrap().snapshot.surface = Some(TerminalSurface::new(
        active_layout.0.cols,
        active_layout.0.rows,
    ));
    manager.get_mut(id_one).unwrap().surface_revision = 1;
    manager.get_mut(id_two).unwrap().snapshot.surface = Some(TerminalSurface::new(2, 2));
    manager.get_mut(id_two).unwrap().surface_revision = 1;

    let mut presentation_store = TerminalPresentationStore::default();
    presentation_store.register(
        id_one,
        PresentedTerminal {
            image: Default::default(),
            texture_state: TerminalTextureState {
                texture_size: active_layout.1.texture_size,
                cell_size: active_layout.1.cell_size,
            },
            desired_texture_state: TerminalTextureState {
                texture_size: active_layout.1.texture_size,
                cell_size: active_layout.1.cell_size,
            },
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 1,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );
    presentation_store.register(
        id_two,
        PresentedTerminal {
            image: Default::default(),
            texture_state: TerminalTextureState {
                texture_size: UVec2::new(100, 100),
                cell_size: UVec2::new(10, 20),
            },
            desired_texture_state: TerminalTextureState {
                texture_size: UVec2::new(100, 100),
                cell_size: UVec2::new(10, 20),
            },
            display_mode: TerminalDisplayMode::Smooth,
            uploaded_revision: 1,
            uploaded_active_override_revision: None,
            uploaded_text_selection_revision: None,
            panel_entity: Entity::PLACEHOLDER,
            frame_entity: Entity::PLACEHOLDER,
        },
    );

    let mut world = World::default();
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    world.insert_resource(time);
    insert_terminal_manager_resources(&mut world, manager);
    world.insert_resource(presentation_store);
    world.insert_resource(crate::hud::TerminalVisibilityState {
        policy: crate::hud::TerminalVisibilityPolicy::Isolate(id_one),
    });
    world.insert_resource(view_state);
    insert_test_hud_state(&mut world, hud_state);
    world.spawn((window, PrimaryWindow));
    world.spawn((
        TerminalPanel { id: id_one },
        TerminalPresentation {
            home_position: Vec2::ZERO,
            current_position: Vec2::ZERO,
            target_position: Vec2::ZERO,
            current_size: Vec2::ONE,
            target_size: Vec2::ONE,
            current_alpha: 1.0,
            target_alpha: 1.0,
            current_z: 0.0,
            target_z: 0.0,
        },
        Transform::default(),
        Sprite::default(),
        Visibility::Visible,
    ));
    world.spawn((
        TerminalPanel { id: id_two },
        TerminalPresentation {
            home_position: Vec2::ZERO,
            current_position: Vec2::ZERO,
            target_position: Vec2::ZERO,
            current_size: Vec2::ONE,
            target_size: Vec2::ONE,
            current_alpha: 1.0,
            target_alpha: 1.0,
            current_z: 0.0,
            target_z: 0.0,
        },
        Transform::default(),
        Sprite::default(),
        Visibility::Visible,
    ));

    world.run_system_once(sync_terminal_presentations).unwrap();
    {
        let manager = world.resource::<TerminalManager>();
        assert_eq!(manager.terminal_ids().len(), 2);
    }
    let mut query = world.query::<(&TerminalPanel, &Visibility)>();
    let mut vis = query
        .iter(&world)
        .map(|(panel, visibility)| (panel.id, *visibility))
        .collect::<Vec<_>>();
    vis.sort_by_key(|(id, _)| id.0);
    assert_eq!(vis[0], (id_one, Visibility::Visible));
    assert_eq!(vis[1], (id_two, Visibility::Hidden));

    world
        .resource_mut::<crate::hud::TerminalVisibilityState>()
        .policy = crate::hud::TerminalVisibilityPolicy::ShowAll;
    world.run_system_once(sync_terminal_presentations).unwrap();
    let mut query = world.query::<(&TerminalPanel, &Visibility)>();
    let mut vis = query
        .iter(&world)
        .map(|(panel, visibility)| (panel.id, *visibility))
        .collect::<Vec<_>>();
    vis.sort_by_key(|(id, _)| id.0);
    assert_eq!(vis[0], (id_one, Visibility::Visible));
    assert_eq!(vis[1], (id_two, Visibility::Hidden));
}
