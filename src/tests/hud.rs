use super::{temp_dir, test_bridge};
use crate::hud::{
    agent_rows, apply_persisted_layout, debug_toolbar_buttons, dispatch_hud_pointer_click,
    dispatch_hud_scroll, handle_hud_pointer_input, hud_needs_redraw, parse_persisted_hud_state,
    resolve_agent_label, resolve_hud_layout_path_with, save_hud_layout_if_dirty,
    serialize_persisted_hud_state, AgentDirectory, HudDispatcher, HudDragState, HudModuleId,
    HudModuleModel, HudPersistenceState, HudRect, HudState, PersistedHudModuleState,
    PersistedHudState, TerminalVisibilityPolicy,
};
use crate::terminals::{TerminalManager, TerminalPresentationStore, TerminalViewState};
use bevy::{
    ecs::system::RunSystemOnce, input::mouse::MouseWheel, prelude::*, window::PrimaryWindow,
};
use std::{fs, path::PathBuf, time::Duration};

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

    let rows = agent_rows(
        HudRect {
            x: 24.0,
            y: 96.0,
            w: 300.0,
            h: 420.0,
        },
        0.0,
        None,
        &manager,
        &AgentDirectory::default(),
    );
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].terminal_id, id_one);
    assert_eq!(rows[0].label, "agent-1");
    assert_eq!(rows[1].terminal_id, id_two);
    assert!(rows[1].focused);
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
fn hud_pointer_drag_updates_module_target_rect_and_marks_layout_dirty() {
    let mut world = World::default();
    let mut hud_state = HudState::default();
    hud_state.insert(
        HudModuleId::AgentList,
        crate::hud::default_hud_module_instance(&crate::hud::HUD_MODULE_DEFINITIONS[1]),
    );
    let mut window = Window {
        focused: true,
        ..Default::default()
    };
    window.set_cursor_position(Some(Vec2::new(40.0, 110.0)));

    world.insert_resource(ButtonInput::<MouseButton>::default());
    world.insert_resource(Messages::<MouseWheel>::default());
    world.insert_resource(hud_state);
    world.insert_resource(TerminalManager::default());
    world.insert_resource(TerminalPresentationStore::default());
    world.insert_resource(TerminalViewState::default());
    world.insert_resource(AgentDirectory::default());
    world.insert_resource(HudDispatcher::default());
    world.spawn((window, PrimaryWindow));

    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    world.run_system_once(handle_hud_pointer_input).unwrap();

    {
        let hud_state = world.resource::<HudState>();
        assert!(hud_state.drag.is_some());
    }

    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear_just_pressed(MouseButton::Left);
    {
        let mut query = world.query_filtered::<&mut Window, With<PrimaryWindow>>();
        let mut window = query
            .single_mut(&mut world)
            .expect("primary window missing");
        window.set_cursor_position(Some(Vec2::new(220.0, 180.0)));
    }
    world.run_system_once(handle_hud_pointer_input).unwrap();

    let hud_state = world.resource::<HudState>();
    let module = hud_state.get(HudModuleId::AgentList).unwrap();
    assert!(hud_state.dirty_layout);
    assert!(module.shell.target_rect.x > crate::hud::HUD_MODULE_DEFINITIONS[1].default_rect.x);
    assert!(module.shell.target_rect.y > crate::hud::HUD_MODULE_DEFINITIONS[1].default_rect.y);
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
    world.insert_resource(hud_state);

    world
        .run_system_once(crate::hud::animate_hud_modules)
        .unwrap();

    let hud_state = world.resource::<HudState>();
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
    world.insert_resource(hud_state);
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
    assert!(serialized.contains("AgentList enabled=1 x=321 y=222 w=333 h=444"));
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
    let mut dispatcher = HudDispatcher::default();
    let buttons = debug_toolbar_buttons(
        HudRect {
            x: 24.0,
            y: 52.0,
            w: 920.0,
            h: 36.0,
        },
        &manager,
        &Default::default(),
        &TerminalViewState::default(),
        &hud_state,
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
        &Default::default(),
        &TerminalViewState::default(),
        &AgentDirectory::default(),
        &hud_state,
        &mut dispatcher,
    );

    assert_eq!(
        dispatcher.commands,
        vec![crate::hud::HudCommand::SpawnTerminal]
    );
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
    let mut dispatcher = HudDispatcher::default();
    let buttons = debug_toolbar_buttons(
        HudRect {
            x: 24.0,
            y: 52.0,
            w: 920.0,
            h: 36.0,
        },
        &manager,
        &Default::default(),
        &TerminalViewState::default(),
        &hud_state,
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
        &Default::default(),
        &TerminalViewState::default(),
        &AgentDirectory::default(),
        &hud_state,
        &mut dispatcher,
    );

    assert_eq!(
        dispatcher.commands,
        vec![crate::hud::HudCommand::SendActiveTerminalCommand(
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
    let mut dispatcher = HudDispatcher::default();
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
        &Default::default(),
        &TerminalViewState::default(),
        &AgentDirectory::default(),
        &hud_state,
        &mut dispatcher,
    );

    assert_eq!(dispatcher.commands.len(), 2);
    assert_eq!(
        dispatcher.commands[0],
        crate::hud::HudCommand::FocusTerminal(id_two)
    );
    assert_eq!(
        dispatcher.commands[1],
        crate::hud::HudCommand::HideAllButTerminal(id_two)
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
    assert_eq!(state.scroll_offset, 28.0);
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
        &Default::default(),
        &TerminalViewState::default(),
        &hud_state,
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
        &Default::default(),
        &TerminalViewState::default(),
        &hud_state,
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
    assert!(!hud_needs_redraw(&state));
    state.drag = Some(HudDragState {
        module_id: HudModuleId::AgentList,
        grab_offset: Vec2::ZERO,
    });
    assert!(hud_needs_redraw(&state));
    state.drag = None;
    let module = state.get_mut(HudModuleId::AgentList).unwrap();
    module.shell.current_rect.x = 0.0;
    module.shell.target_rect.x = 10.0;
    assert!(hud_needs_redraw(&state));
}

#[test]
fn terminal_visibility_policy_defaults_to_show_all() {
    assert_eq!(
        TerminalVisibilityPolicy::default(),
        TerminalVisibilityPolicy::ShowAll
    );
}
