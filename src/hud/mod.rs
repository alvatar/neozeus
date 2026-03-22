mod animation;
mod dispatcher;
mod input;
mod message_box;
mod messages;
mod modules;
mod persistence;
mod render;
mod state;

pub(crate) use animation::animate_hud_modules;
#[cfg(test)]
pub(crate) use dispatcher::kill_active_terminal;
pub(crate) use dispatcher::{
    apply_hud_module_requests, apply_terminal_focus_requests, apply_terminal_lifecycle_requests,
    apply_terminal_send_requests, apply_terminal_view_requests, apply_visibility_requests,
    dispatch_hud_intents,
};
pub(crate) use input::{handle_hud_module_shortcuts, handle_hud_pointer_input};
pub(crate) use message_box::HudMessageBoxState;
pub(crate) use messages::{
    HudIntent, HudModuleRequest, TerminalFocusRequest, TerminalLifecycleRequest,
    TerminalSendRequest, TerminalViewRequest, TerminalVisibilityRequest,
};
#[cfg(test)]
pub(crate) use persistence::{
    apply_persisted_layout, parse_persisted_hud_state, resolve_hud_layout_path_with,
    serialize_persisted_hud_state, PersistedHudModuleState, PersistedHudState,
};
pub(crate) use persistence::{save_hud_layout_if_dirty, HudPersistenceState};
pub(crate) use render::{render_hud_scene, sync_message_box_overlay, HudVectorSceneMarker};
#[cfg(test)]
pub(crate) use render::HudMessageBoxOverlayRoot;
pub(crate) use state::{
    default_hud_module_instance, AgentDirectory, HudDragState, HudModuleId, HudModuleModel,
    HudRect, HudState, TerminalVisibilityPolicy, TerminalVisibilityState, HUD_BUTTON_GAP,
    HUD_BUTTON_HEIGHT, HUD_BUTTON_MIN_WIDTH, HUD_MODULE_DEFINITIONS, HUD_MODULE_PADDING,
    HUD_ROW_HEIGHT, HUD_TITLEBAR_HEIGHT,
};

use bevy::{
    camera::{
        visibility::{NoFrustumCulling, RenderLayers},
        ClearColorConfig,
    },
    prelude::*,
    window::RequestRedraw,
};
use bevy_vello::prelude::{VelloScene2d, VelloView};

pub(crate) fn append_hud_log(message: impl AsRef<str>) {
    crate::terminals::append_debug_log(format!("hud: {}", message.as_ref()));
}

const HUD_RENDER_LAYER: usize = 1;

pub(crate) fn setup_hud(
    mut commands: Commands,
    mut hud_state: ResMut<HudState>,
    mut persistence_state: ResMut<HudPersistenceState>,
    mut redraws: MessageWriter<RequestRedraw>,
) {
    persistence_state.path = persistence::resolve_hud_layout_path();
    let persisted = persistence_state
        .path
        .as_ref()
        .map(persistence::load_persisted_hud_state_from)
        .unwrap_or_default();
    hud_state.modules.clear();
    hud_state.z_order.clear();
    hud_state.drag = None;
    hud_state.dirty_layout = false;
    hud_state.message_box = HudMessageBoxState::default();
    hud_state.direct_input_terminal = None;
    for definition in HUD_MODULE_DEFINITIONS.iter() {
        let mut module = default_hud_module_instance(definition);
        if let Some(saved) = persisted.modules.get(&definition.id) {
            module.shell.enabled = saved.enabled;
            module.shell.target_rect = saved.rect;
            module.shell.current_rect = saved.rect;
            module.shell.target_alpha = if saved.enabled { 1.0 } else { 0.0 };
            module.shell.current_alpha = module.shell.target_alpha;
        }
        hud_state.insert(definition.id, module);
    }

    commands.spawn((
        Camera2d,
        Camera {
            order: 100,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        VelloView,
        RenderLayers::layer(HUD_RENDER_LAYER),
    ));

    commands.spawn((
        VelloScene2d::default(),
        Transform::from_xyz(0.0, 0.0, 50.0),
        NoFrustumCulling,
        RenderLayers::layer(HUD_RENDER_LAYER),
        HudVectorSceneMarker,
    ));

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            GlobalZIndex(10_000),
            Visibility::Hidden,
            render::HudMessageBoxOverlayRoot,
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.03, 0.04, 0.05, 0.82)),
            ));
            root.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Px(640.0),
                    height: Val::Px(360.0),
                    border: UiRect::all(Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.09, 0.11, 0.13, 0.98)),
                BorderColor::all(Color::srgb(1.0, 0.55, 0.12)),
                render::HudMessageBoxOverlayPanel,
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new(""),
                    TextFont {
                        font_size: 22.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.95, 0.95, 0.95)),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(24.0),
                        top: Val::Px(16.0),
                        ..default()
                    },
                    render::HudMessageBoxOverlayTitle,
                ));
                panel.spawn((
                    Text::new(""),
                    TextFont {
                        font_size: 16.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.74, 0.78, 0.8)),
                    Node {
                        position_type: PositionType::Absolute,
                        right: Val::Px(24.0),
                        top: Val::Px(18.0),
                        ..default()
                    },
                    render::HudMessageBoxOverlayHelp,
                ));
                panel
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(22.0),
                            right: Val::Px(22.0),
                            top: Val::Px(64.0),
                            bottom: Val::Px(76.0),
                            border: UiRect::all(Val::Px(1.0)),
                            padding: UiRect::all(Val::Px(16.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.07, 0.09, 0.11, 0.98)),
                        BorderColor::all(Color::srgba(0.62, 0.68, 0.7, 0.8)),
                    ))
                    .with_children(|body_container| {
                        body_container.spawn((
                            Text::new(""),
                            TextFont {
                                font_size: 18.0,
                                ..default()
                            },
                            TextColor(Color::srgb(0.95, 0.95, 0.95)),
                            Node::default(),
                            render::HudMessageBoxOverlayBody,
                        ));
                    });
                panel.spawn((
                    Text::new(""),
                    TextFont {
                        font_size: 16.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.74, 0.78, 0.8)),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(24.0),
                        bottom: Val::Px(18.0),
                        ..default()
                    },
                    render::HudMessageBoxOverlayFooter,
                ));
            });
        });
    redraws.write(RequestRedraw);
}

pub(crate) fn hud_needs_redraw(hud_state: &HudState) -> bool {
    hud_state.drag.is_some() || hud_state.is_animating()
}

#[cfg(test)]
pub(crate) use modules::{
    agent_rows, debug_toolbar_buttons, handle_pointer_click as dispatch_hud_pointer_click,
    handle_scroll as dispatch_hud_scroll, resolve_agent_label,
};
