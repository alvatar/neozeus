use crate::*;

pub(crate) fn build_app() -> Result<App, String> {
    let mut app = App::new();
    let previous_hook = Arc::new(std::panic::take_hook());
    let forwarding_hook = previous_hook.clone();

    std::panic::set_hook(Box::new(move |info| {
        if panic_payload_message(info.payload()).is_some_and(is_missing_gpu_panic) {
            return;
        }
        (*forwarding_hook)(info);
    }));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| configure_app(&mut app)));

    let restore_hook = previous_hook.clone();
    std::panic::set_hook(Box::new(move |info| (*restore_hook)(info)));

    match result {
        Ok(()) => Ok(app),
        Err(payload) => {
            if let Some(error) = format_startup_panic(payload.as_ref()) {
                Err(error)
            } else {
                std::panic::resume_unwind(payload)
            }
        }
    }
}

fn configure_app(app: &mut App) {
    app.add_plugins(
        DefaultPlugins
            .set(RenderPlugin {
                render_creation: WgpuSettings {
                    force_fallback_adapter: true,
                    ..default()
                }
                .into(),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: env::var("NEOZEUS_WINDOW_TITLE")
                        .unwrap_or_else(|_| "neozeus".to_owned()),
                    resolution: (1400, 900).into(),
                    ..default()
                }),
                ..default()
            }),
    )
    .add_plugins((EguiPlugin::default(), VelloPlugin::default()));

    let event_loop_proxy = {
        let proxy = app.world().resource::<EventLoopProxyWrapper>();
        (**proxy).clone()
    };

    app.insert_resource(ClearColor(Color::srgb(0.02, 0.02, 0.02)))
        .insert_resource(WinitSettings::desktop_app())
        .insert_resource(TerminalBridge::spawn(event_loop_proxy))
        .insert_resource(TerminalView::default())
        .insert_resource(TerminalFontState::default())
        .insert_resource(TerminalPlaneState::default())
        .insert_resource(TerminalPointerState::default())
        .insert_resource(TerminalSceneState::default())
        .insert_resource(TerminalTextureState::default())
        .insert_resource(TerminalGlyphCache::default())
        .insert_resource(TerminalTextRenderer::default())
        .insert_resource(EvaVectorDemoState::default())
        .add_systems(Startup, setup_scene)
        .add_systems(
            Update,
            (
                poll_terminal_snapshots,
                configure_terminal_fonts,
                sync_terminal_font_helpers,
                sync_terminal_texture,
                drag_terminal_plane,
                zoom_terminal_plane,
                sync_terminal_plane_transform,
                sync_terminal_plane,
                sync_eva_vector_demo,
                forward_keyboard_input,
            )
                .chain(),
        )
        .add_systems(EguiPrimaryContextPass, ui_overlay);
}

pub(crate) fn format_startup_panic(payload: &(dyn Any + Send)) -> Option<String> {
    let message = panic_payload_message(payload)?;
    if !is_missing_gpu_panic(message) {
        return None;
    }

    Some(
        "neozeus failed to start: Bevy/WGPU could not find a usable graphics adapter. \
This environment is either headless or missing graphics/software-rendering drivers. \
Run it in a graphical session with a working GPU, or install a software renderer such as Mesa/llvmpipe."
            .to_owned(),
    )
}

fn is_missing_gpu_panic(message: &str) -> bool {
    message.contains(GPU_NOT_FOUND_PANIC_FRAGMENT)
}

fn panic_payload_message(payload: &(dyn Any + Send)) -> Option<&str> {
    if let Some(message) = payload.downcast_ref::<String>() {
        Some(message.as_str())
    } else if let Some(message) = payload.downcast_ref::<&'static str>() {
        Some(*message)
    } else {
        None
    }
}

fn setup_scene(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut texture_state: ResMut<TerminalTextureState>,
) {
    let image_handle = images.add(create_terminal_image(UVec2::ONE));

    commands.spawn((Camera2d, VelloView, TerminalCameraMarker));

    commands.spawn((
        Sprite::from_image(image_handle.clone()),
        Transform::default(),
        TerminalPlaneMarker,
    ));

    commands.spawn((
        VelloScene2d::default(),
        Transform::from_xyz(0.0, 0.0, EVA_DEMO_Z),
        NoFrustumCulling,
        EvaVectorDemoMarker,
    ));

    let primary = commands
        .spawn((
            TerminalFontRole::Primary,
            TextFont {
                font_size: DEFAULT_CELL_HEIGHT_PX as f32 * 0.9,
                ..default()
            },
        ))
        .id();
    let private_use = commands
        .spawn((
            TerminalFontRole::PrivateUse,
            TextFont {
                font_size: DEFAULT_CELL_HEIGHT_PX as f32 * 0.9,
                ..default()
            },
        ))
        .id();
    let emoji = commands
        .spawn((
            TerminalFontRole::Emoji,
            TextFont {
                font_size: DEFAULT_CELL_HEIGHT_PX as f32 * 0.9,
                ..default()
            },
        ))
        .id();

    texture_state.image = Some(image_handle);
    texture_state.helper_entities = Some(TerminalFontEntities {
        primary,
        private_use,
        emoji,
    });
    texture_state.texture_size = UVec2::ONE;
    texture_state.cell_size = UVec2::new(DEFAULT_CELL_WIDTH_PX, DEFAULT_CELL_HEIGHT_PX);
}
