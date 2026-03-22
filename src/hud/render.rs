use crate::{
    hud::{
        modules, AgentDirectory, HudMessageBoxState, HudModuleId, HudRect, HudState,
        HUD_TITLEBAR_HEIGHT,
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

pub(crate) struct HudColors;

impl HudColors {
    pub(crate) const FRAME: peniko::Color = peniko::Color::from_rgba8(32, 42, 46, 232);
    pub(crate) const TITLE: peniko::Color = peniko::Color::from_rgba8(18, 24, 27, 240);
    pub(crate) const BORDER: peniko::Color = peniko::Color::from_rgba8(255, 140, 32, 210);
    pub(crate) const TEXT: peniko::Color = peniko::Color::from_rgba8(235, 235, 235, 255);
    pub(crate) const TEXT_MUTED: peniko::Color = peniko::Color::from_rgba8(160, 175, 180, 255);
    pub(crate) const BUTTON: peniko::Color = peniko::Color::from_rgba8(42, 54, 59, 232);
    pub(crate) const BUTTON_ACTIVE: peniko::Color = peniko::Color::from_rgba8(80, 112, 108, 240);
    pub(crate) const BUTTON_BORDER: peniko::Color = peniko::Color::from_rgba8(255, 140, 32, 180);
    pub(crate) const ROW: peniko::Color = peniko::Color::from_rgba8(26, 34, 38, 220);
    pub(crate) const ROW_HOVERED: peniko::Color = peniko::Color::from_rgba8(48, 62, 68, 230);
    pub(crate) const ROW_FOCUSED: peniko::Color = peniko::Color::from_rgba8(66, 98, 92, 236);
    pub(crate) const OVERLAY: peniko::Color = peniko::Color::from_rgba8(8, 10, 12, 214);
    pub(crate) const MESSAGE_BOX: peniko::Color = peniko::Color::from_rgba8(20, 24, 28, 252);
}

pub(crate) fn apply_alpha(color: peniko::Color, factor: f32) -> peniko::Color {
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

pub(crate) fn hud_rect_to_scene(window: &Window, rect: HudRect) -> Rect {
    let (x0, y0) = hud_to_scene(window, Vec2::new(rect.x, rect.y));
    let (x1, y1) = hud_to_scene(window, Vec2::new(rect.x + rect.w, rect.y + rect.h));
    Rect::new(x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1))
}

pub(crate) struct HudPainter<'scene, 'res> {
    scene: &'scene mut vello::Scene,
    fonts: &'res Assets<VelloFont>,
    window: &'res Window,
    alpha: f32,
}

impl<'scene, 'res> HudPainter<'scene, 'res> {
    pub(crate) fn new(
        scene: &'scene mut vello::Scene,
        fonts: &'res Assets<VelloFont>,
        window: &'res Window,
        alpha: f32,
    ) -> Self {
        Self {
            scene,
            fonts,
            window,
            alpha,
        }
    }

    pub(crate) fn fill_rect(&mut self, rect: HudRect, color: peniko::Color, radius: f64) {
        self.scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            apply_alpha(color, self.alpha),
            None,
            &RoundedRect::from_rect(hud_rect_to_scene(self.window, rect), radius),
        );
    }

    pub(crate) fn stroke_rect(&mut self, rect: HudRect, color: peniko::Color, radius: f64) {
        self.scene.stroke(
            &Stroke::new(1.5),
            Affine::IDENTITY,
            apply_alpha(color, self.alpha),
            None,
            &RoundedRect::from_rect(hud_rect_to_scene(self.window, rect), radius),
        );
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "Vello text drawing needs scene/font/window/position/style inputs together"
    )]
    pub(crate) fn label(
        &mut self,
        position: Vec2,
        text: &str,
        size: f32,
        color: peniko::Color,
        anchor: VelloTextAnchor,
    ) {
        let Some(font) = self.fonts.get(&Handle::<VelloFont>::default()) else {
            return;
        };

        let style = VelloTextStyle {
            font: Handle::default(),
            brush: peniko::Brush::Solid(apply_alpha(color, self.alpha)),
            font_size: size,
            ..Default::default()
        };
        let layout = font.layout(text, &style, VelloTextAlign::Start, None);
        let width = layout.width() as f64;
        let height = layout.height() as f64;
        let (x, y) = hud_to_scene(self.window, position);
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

                self.scene
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
}

pub(crate) struct HudRenderInputs<'a> {
    pub(crate) terminal_manager: &'a TerminalManager,
    pub(crate) presentation_store: &'a TerminalPresentationStore,
    pub(crate) view_state: &'a TerminalViewState,
    pub(crate) agent_directory: &'a AgentDirectory,
    pub(crate) hud_state: &'a HudState,
    pub(crate) font_state: &'a TerminalFontState,
}

fn message_box_rect(window: &Window) -> HudRect {
    let size = Vec2::new(
        (window.width() * 0.84).clamp(520.0, 1680.0),
        (window.height() * 0.72).clamp(260.0, 980.0),
    );
    HudRect {
        x: window.width() * 0.5 - size.x * 0.5,
        y: window.height() * 0.5 - size.y * 0.5,
        w: size.x,
        h: size.y,
    }
}

fn draw_message_box(
    painter: &mut HudPainter,
    window: &Window,
    message_box: &HudMessageBoxState,
    agent_directory: &AgentDirectory,
) {
    if !message_box.visible {
        return;
    }

    let backdrop = HudRect {
        x: 0.0,
        y: 0.0,
        w: window.width(),
        h: window.height(),
    };
    painter.fill_rect(backdrop, HudColors::OVERLAY, 0.0);

    let rect = message_box_rect(window);
    painter.fill_rect(rect, HudColors::MESSAGE_BOX, 12.0);
    painter.stroke_rect(rect, HudColors::BORDER, 12.0);

    let title_rect = HudRect {
        x: rect.x,
        y: rect.y,
        w: rect.w,
        h: 44.0,
    };
    painter.fill_rect(title_rect, HudColors::TITLE, 12.0);

    let target_label = message_box
        .target_terminal
        .and_then(|terminal_id| agent_directory.labels.get(&terminal_id).cloned())
        .unwrap_or_else(|| {
            message_box
                .target_terminal
                .map(|terminal_id| format!("terminal {}", terminal_id.0))
                .unwrap_or_else(|| "no target".to_owned())
        });
    painter.label(
        Vec2::new(rect.x + 24.0, rect.y + 12.0),
        &format!("Message {}", target_label),
        18.0,
        HudColors::TEXT,
        VelloTextAnchor::TopLeft,
    );
    painter.label(
        Vec2::new(rect.x + rect.w - 24.0, rect.y + 12.0),
        "Enter compose · Ctrl-S send · Esc cancel",
        16.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::TopRight,
    );

    let body_rect = HudRect {
        x: rect.x + 22.0,
        y: rect.y + 64.0,
        w: rect.w - 44.0,
        h: rect.h - 118.0,
    };
    painter.fill_rect(body_rect, HudColors::TITLE, 6.0);
    painter.stroke_rect(body_rect, HudColors::TEXT_MUTED, 4.0);

    let visible_text = if message_box.text.is_empty() {
        "█".to_owned()
    } else {
        format!("{}█", message_box.text)
    };
    painter.label(
        Vec2::new(body_rect.x + 18.0, body_rect.y + 16.0),
        &visible_text,
        18.0,
        HudColors::TEXT,
        VelloTextAnchor::TopLeft,
    );

    painter.label(
        Vec2::new(rect.x + 24.0, rect.y + rect.h - 32.0),
        "Type your message here. The box is modal and sends to the focused terminal.",
        15.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::TopLeft,
    );
}

fn draw_module_shell(painter: &mut HudPainter, module_id: HudModuleId, shell_rect: HudRect) {
    painter.fill_rect(shell_rect, HudColors::FRAME, 8.0);
    painter.stroke_rect(shell_rect, HudColors::BORDER, 8.0);
    painter.fill_rect(
        HudRect {
            x: shell_rect.x,
            y: shell_rect.y,
            w: shell_rect.w,
            h: HUD_TITLEBAR_HEIGHT.min(shell_rect.h),
        },
        HudColors::TITLE,
        8.0,
    );
    painter.label(
        Vec2::new(shell_rect.x + 12.0, shell_rect.y + 8.0),
        &format!("{} {}", module_id.number(), module_id.title()),
        16.0,
        HudColors::TEXT,
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
    let inputs = HudRenderInputs {
        terminal_manager: &terminal_manager,
        presentation_store: &presentation_store,
        view_state: &view_state,
        agent_directory: &agent_directory,
        hud_state: &hud_state,
        font_state: &font_state,
    };

    for module_id in hud_state.iter_z_order() {
        let Some(module) = hud_state.get(module_id) else {
            continue;
        };
        if !module.shell.enabled && module.shell.current_alpha <= 0.01 {
            continue;
        }

        let shell_rect = module.shell.current_rect;
        let alpha = module.shell.current_alpha.max(0.0);
        let mut painter = HudPainter::new(&mut built, &fonts, &primary_window, alpha);
        draw_module_shell(&mut painter, module_id, shell_rect);

        let content_rect = module.shell.content_rect();
        built.push_clip_layer(
            Fill::NonZero,
            Affine::IDENTITY,
            &hud_rect_to_scene(&primary_window, content_rect),
        );
        let mut painter = HudPainter::new(&mut built, &fonts, &primary_window, alpha);
        modules::render_module_content(
            module_id,
            &module.model,
            content_rect,
            &mut painter,
            &inputs,
        );
        built.pop_layer();
    }

    let mut painter = HudPainter::new(&mut built, &fonts, &primary_window, 1.0);
    draw_message_box(
        &mut painter,
        &primary_window,
        &hud_state.message_box,
        &agent_directory,
    );

    **scene = VelloScene2d::from(built);
}
