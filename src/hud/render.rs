use crate::{
    hud::{
        modules, AgentDirectory, HudModuleId, HudModuleModel, HudRect, HudState, HUD_BUTTON_HEIGHT,
        HUD_MODULE_PADDING, HUD_ROW_HEIGHT, HUD_TITLEBAR_HEIGHT,
    },
    terminals::{TerminalFontState, TerminalManager, TerminalPresentationStore, TerminalViewState},
};
use bevy::{prelude::*, window::PrimaryWindow};
use bevy_vello::{
    parley::PositionedLayoutItem,
    prelude::{
        kurbo::{Affine, Rect, RoundedRect, Stroke},
        peniko::{self, Fill},
        vello, VelloFont, VelloScene2d, VelloTextAlign, VelloTextAnchor, VelloTextStyle,
    },
};

#[derive(Component)]
pub(crate) struct HudVectorSceneMarker;

struct HudColors;

impl HudColors {
    const FRAME: peniko::Color = peniko::Color::from_rgba8(32, 42, 46, 232);
    const TITLE: peniko::Color = peniko::Color::from_rgba8(18, 24, 27, 240);
    const BORDER: peniko::Color = peniko::Color::from_rgba8(255, 140, 32, 210);
    const TEXT: peniko::Color = peniko::Color::from_rgba8(235, 235, 235, 255);
    const TEXT_MUTED: peniko::Color = peniko::Color::from_rgba8(160, 175, 180, 255);
    const BUTTON: peniko::Color = peniko::Color::from_rgba8(42, 54, 59, 232);
    const BUTTON_ACTIVE: peniko::Color = peniko::Color::from_rgba8(80, 112, 108, 240);
    const BUTTON_BORDER: peniko::Color = peniko::Color::from_rgba8(255, 140, 32, 180);
    const ROW: peniko::Color = peniko::Color::from_rgba8(26, 34, 38, 220);
    const ROW_FOCUSED: peniko::Color = peniko::Color::from_rgba8(66, 98, 92, 236);
}

fn apply_alpha(color: peniko::Color, factor: f32) -> peniko::Color {
    let rgba = color.to_rgba8();
    let alpha = ((rgba.a as f32) * factor.clamp(0.0, 1.0)).round() as u8;
    peniko::Color::from_rgba8(rgba.r, rgba.g, rgba.b, alpha)
}

fn hud_to_scene(window: &Window, point: Vec2) -> (f64, f64) {
    (
        f64::from(point.x - window.width() * 0.5),
        f64::from(point.y - window.height() * 0.5),
    )
}

fn hud_rect_to_scene(window: &Window, rect: HudRect) -> Rect {
    let (x0, y0) = hud_to_scene(window, Vec2::new(rect.x, rect.y));
    let (x1, y1) = hud_to_scene(window, Vec2::new(rect.x + rect.w, rect.y + rect.h));
    Rect::new(x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1))
}

fn draw_filled_rect(scene: &mut vello::Scene, rect: Rect, color: peniko::Color, radius: f64) {
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        color,
        None,
        &RoundedRect::from_rect(rect, radius),
    );
}

fn draw_stroked_rect(scene: &mut vello::Scene, rect: Rect, color: peniko::Color, radius: f64) {
    scene.stroke(
        &Stroke::new(1.5),
        Affine::IDENTITY,
        color,
        None,
        &RoundedRect::from_rect(rect, radius),
    );
}

#[allow(
    clippy::too_many_arguments,
    reason = "Vello text drawing needs scene/font/window/position/style inputs together"
)]
fn draw_label(
    scene: &mut vello::Scene,
    fonts: &Assets<VelloFont>,
    window: &Window,
    position: Vec2,
    text: &str,
    size: f32,
    color: peniko::Color,
    anchor: VelloTextAnchor,
) {
    let Some(font) = fonts.get(&Handle::<VelloFont>::default()) else {
        return;
    };

    let style = VelloTextStyle {
        font: Handle::default(),
        brush: peniko::Brush::Solid(color),
        font_size: size,
        ..Default::default()
    };
    let layout = font.layout(text, &style, VelloTextAlign::Start, None);
    let width = layout.width() as f64;
    let height = layout.height() as f64;
    let (x, y) = hud_to_scene(window, position);
    let (dx, dy) = match anchor {
        VelloTextAnchor::TopLeft => (0.0, 0.0),
        VelloTextAnchor::Left => (0.0, -height / 2.0),
        VelloTextAnchor::BottomLeft => (0.0, -height),
        VelloTextAnchor::Top => (-width / 2.0, 0.0),
        VelloTextAnchor::Center => (-width / 2.0, -height / 2.0),
        VelloTextAnchor::Bottom => (-width / 2.0, -height),
        VelloTextAnchor::TopRight => (-width, 0.0),
        VelloTextAnchor::Right => (-width, -height / 2.0),
        VelloTextAnchor::BottomRight => (-width, -height),
    };
    let transform = Affine::translate((x + dx, y + dy));

    for line in layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };
            let mut glyph_x = glyph_run.offset();
            let glyph_y = glyph_run.baseline();
            let run = glyph_run.run();
            let synthesis = run.synthesis();
            let glyph_transform = synthesis
                .skew()
                .map(|angle| Affine::skew(angle.to_radians().tan() as f64, 0.0));

            scene
                .draw_glyphs(run.font())
                .brush(&style.brush)
                .hint(true)
                .transform(transform)
                .glyph_transform(glyph_transform)
                .font_size(run.font_size())
                .normalized_coords(run.normalized_coords())
                .draw(
                    Fill::NonZero,
                    glyph_run.glyphs().map(|glyph| {
                        let gx = glyph_x + glyph.x;
                        let gy = glyph_y - glyph.y;
                        glyph_x += glyph.advance;
                        vello::Glyph {
                            id: glyph.id as _,
                            x: gx,
                            y: gy,
                        }
                    }),
                );
        }
    }
}

fn draw_module_shell(
    scene: &mut vello::Scene,
    fonts: &Assets<VelloFont>,
    window: &Window,
    module_id: HudModuleId,
    shell_rect: HudRect,
    alpha: f32,
) {
    let frame = hud_rect_to_scene(window, shell_rect);
    let title_rect = hud_rect_to_scene(
        window,
        HudRect {
            x: shell_rect.x,
            y: shell_rect.y,
            w: shell_rect.w,
            h: HUD_TITLEBAR_HEIGHT.min(shell_rect.h),
        },
    );
    draw_filled_rect(scene, frame, apply_alpha(HudColors::FRAME, alpha), 8.0);
    draw_stroked_rect(scene, frame, apply_alpha(HudColors::BORDER, alpha), 8.0);
    draw_filled_rect(scene, title_rect, apply_alpha(HudColors::TITLE, alpha), 8.0);
    draw_label(
        scene,
        fonts,
        window,
        Vec2::new(shell_rect.x + 12.0, shell_rect.y + 8.0),
        &format!("{} {}", module_id.number(), module_id.title()),
        16.0,
        apply_alpha(HudColors::TEXT, alpha),
        VelloTextAnchor::TopLeft,
    );
}

fn draw_button(
    scene: &mut vello::Scene,
    fonts: &Assets<VelloFont>,
    window: &Window,
    rect: HudRect,
    label: &str,
    active: bool,
    alpha: f32,
) {
    let scene_rect = hud_rect_to_scene(window, rect);
    draw_filled_rect(
        scene,
        scene_rect,
        if active {
            apply_alpha(HudColors::BUTTON_ACTIVE, alpha)
        } else {
            apply_alpha(HudColors::BUTTON, alpha)
        },
        6.0,
    );
    draw_stroked_rect(
        scene,
        scene_rect,
        apply_alpha(HudColors::BUTTON_BORDER, alpha),
        6.0,
    );
    draw_label(
        scene,
        fonts,
        window,
        Vec2::new(rect.x + 10.0, rect.y + 6.0),
        label,
        14.0,
        apply_alpha(HudColors::TEXT, alpha),
        VelloTextAnchor::TopLeft,
    );
}

fn draw_agent_row(
    scene: &mut vello::Scene,
    fonts: &Assets<VelloFont>,
    window: &Window,
    rect: HudRect,
    label: &str,
    focused: bool,
    alpha: f32,
) {
    let scene_rect = hud_rect_to_scene(window, rect);
    draw_filled_rect(
        scene,
        scene_rect,
        if focused {
            apply_alpha(HudColors::ROW_FOCUSED, alpha)
        } else {
            apply_alpha(HudColors::ROW, alpha)
        },
        6.0,
    );
    draw_label(
        scene,
        fonts,
        window,
        Vec2::new(rect.x + 10.0, rect.y + 7.0),
        label,
        15.0,
        apply_alpha(HudColors::TEXT, alpha),
        VelloTextAnchor::TopLeft,
    );
}

#[allow(
    clippy::too_many_arguments,
    reason = "HUD scene rebuild reads HUD, terminal, font, and Vello scene resources together"
)]
pub(crate) fn render_hud_scene(
    primary_window: Single<&Window, With<PrimaryWindow>>,
    hud_state: Res<HudState>,
    terminal_manager: Res<TerminalManager>,
    presentation_store: Res<TerminalPresentationStore>,
    view_state: Res<TerminalViewState>,
    agent_directory: Res<AgentDirectory>,
    font_state: Res<TerminalFontState>,
    fonts: Res<Assets<VelloFont>>,
    mut scene: Single<&mut VelloScene2d, With<HudVectorSceneMarker>>,
) {
    let mut built = vello::Scene::new();

    for module_id in hud_state.iter_z_order() {
        let Some(module) = hud_state.get(module_id) else {
            continue;
        };
        if !module.shell.enabled && module.shell.current_alpha <= 0.01 {
            continue;
        }
        let shell_rect = module.shell.current_rect;
        let alpha = module.shell.current_alpha.max(0.0);
        draw_module_shell(
            &mut built,
            &fonts,
            &primary_window,
            module_id,
            shell_rect,
            alpha,
        );

        let content_rect = module.shell.content_rect();
        built.push_clip_layer(
            Fill::NonZero,
            Affine::IDENTITY,
            &hud_rect_to_scene(&primary_window, content_rect),
        );

        match (&module.model, module_id) {
            (HudModuleModel::DebugToolbar(_), HudModuleId::DebugToolbar) => {
                let buttons = modules::debug_toolbar_buttons(
                    content_rect,
                    &terminal_manager,
                    &presentation_store,
                    &view_state,
                );
                let active_status = terminal_manager
                    .active_snapshot()
                    .map(|snapshot| snapshot.runtime.status.as_str())
                    .unwrap_or("no active terminal");
                let active_id = terminal_manager
                    .active_id()
                    .map(|id| id.0)
                    .unwrap_or_default();
                let debug_stats = terminal_manager.active_debug_stats();
                let font_summary = match font_state.report.as_ref() {
                    Some(Ok(report)) => format!("font {}", report.primary.family),
                    Some(Err(error)) => format!("font error {error}"),
                    None => "font loading".to_owned(),
                };
                draw_label(
                    &mut built,
                    &fonts,
                    &primary_window,
                    Vec2::new(content_rect.x, content_rect.y + HUD_BUTTON_HEIGHT + 8.0),
                    &format!(
                        "terms {} · active {} · {} · zoom {:.2}",
                        terminal_manager.terminal_ids().len(),
                        active_id,
                        active_status,
                        view_state.distance,
                    ),
                    14.0,
                    apply_alpha(HudColors::TEXT_MUTED, alpha),
                    VelloTextAnchor::TopLeft,
                );
                draw_label(
                    &mut built,
                    &fonts,
                    &primary_window,
                    Vec2::new(
                        content_rect.x + 430.0,
                        content_rect.y + HUD_BUTTON_HEIGHT + 8.0,
                    ),
                    &font_summary,
                    14.0,
                    apply_alpha(HudColors::TEXT_MUTED, alpha),
                    VelloTextAnchor::TopLeft,
                );
                draw_label(
                    &mut built,
                    &fonts,
                    &primary_window,
                    Vec2::new(
                        content_rect.x + 620.0,
                        content_rect.y + HUD_BUTTON_HEIGHT + 8.0,
                    ),
                    &format!(
                        "keys {} drop {} rows {}",
                        debug_stats.key_events_seen,
                        debug_stats.updates_dropped,
                        debug_stats.dirty_rows_uploaded,
                    ),
                    14.0,
                    apply_alpha(HudColors::TEXT_MUTED, alpha),
                    VelloTextAnchor::TopLeft,
                );
                for button in buttons {
                    draw_button(
                        &mut built,
                        &fonts,
                        &primary_window,
                        button.rect,
                        &button.label,
                        button.active,
                        alpha,
                    );
                }
            }
            (HudModuleModel::AgentList(state), HudModuleId::AgentList) => {
                for row in modules::agent_rows(
                    content_rect,
                    state.scroll_offset,
                    &terminal_manager,
                    &agent_directory,
                ) {
                    if row.rect.y + row.rect.h < content_rect.y
                        || row.rect.y > content_rect.y + content_rect.h
                    {
                        continue;
                    }
                    draw_agent_row(
                        &mut built,
                        &fonts,
                        &primary_window,
                        row.rect,
                        &row.label,
                        row.focused,
                        alpha,
                    );
                }
                draw_label(
                    &mut built,
                    &fonts,
                    &primary_window,
                    Vec2::new(
                        content_rect.x + HUD_MODULE_PADDING,
                        content_rect.y + content_rect.h - HUD_ROW_HEIGHT,
                    ),
                    "click row: focus + isolate",
                    13.0,
                    apply_alpha(HudColors::TEXT_MUTED, alpha),
                    VelloTextAnchor::TopLeft,
                );
            }
            _ => {}
        }

        built.pop_layer();
    }

    **scene = VelloScene2d::from(built);
}
