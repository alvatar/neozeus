use crate::{
    hud::{
        message_box_action_buttons, message_box_rect, modules, AgentDirectory, HudMessageBoxState,
        HudModuleId, HudRect, HudState, HUD_TITLEBAR_HEIGHT,
    },
    terminals::{
        TerminalFontState, TerminalManager, TerminalNotesState, TerminalPresentationStore,
        TerminalViewState,
    },
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
    pub(crate) const FRAME: peniko::Color = peniko::Color::from_rgba8(7, 7, 7, 255);
    pub(crate) const TITLE: peniko::Color = peniko::Color::from_rgba8(7, 7, 7, 255);
    pub(crate) const BORDER: peniko::Color = peniko::Color::from_rgba8(57, 26, 6, 255);
    pub(crate) const TEXT: peniko::Color = peniko::Color::from_rgba8(238, 96, 2, 255);
    pub(crate) const TEXT_MUTED: peniko::Color = peniko::Color::from_rgba8(216, 196, 162, 255);
    pub(crate) const TEXT_ON_ACCENT: peniko::Color = peniko::Color::from_rgba8(0, 0, 0, 255);
    pub(crate) const BUTTON: peniko::Color = peniko::Color::from_rgba8(26, 26, 26, 255);
    pub(crate) const BUTTON_ACTIVE: peniko::Color = peniko::Color::from_rgba8(255, 102, 0, 255);
    pub(crate) const BUTTON_BORDER: peniko::Color = peniko::Color::from_rgba8(57, 26, 6, 255);
    pub(crate) const ROW_HOVERED: peniko::Color = peniko::Color::from_rgba8(44, 32, 24, 255);
    pub(crate) const ROW_FOCUSED: peniko::Color = peniko::Color::from_rgba8(44, 32, 24, 255);
    pub(crate) const MESSAGE_BOX: peniko::Color = peniko::Color::from_rgba8(7, 7, 7, 255);
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

    pub(crate) fn fill_rect(&mut self, rect: HudRect, color: peniko::Color, _radius: f64) {
        self.scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            apply_alpha(color, self.alpha),
            None,
            &RoundedRect::from_rect(hud_rect_to_scene(self.window, rect), 0.0),
        );
    }

    pub(crate) fn stroke_rect(&mut self, rect: HudRect, color: peniko::Color, _radius: f64) {
        self.scene.stroke(
            &Stroke::new(1.5),
            Affine::IDENTITY,
            apply_alpha(color, self.alpha),
            None,
            &RoundedRect::from_rect(hud_rect_to_scene(self.window, rect), 0.0),
        );
    }

    pub(crate) fn text_size(&self, text: &str, size: f32) -> Vec2 {
        let Some(font) = self.fonts.get(&Handle::<VelloFont>::default()) else {
            return Vec2::ZERO;
        };
        let style = VelloTextStyle {
            font: Handle::default(),
            brush: peniko::Brush::Solid(apply_alpha(HudColors::TEXT, self.alpha)),
            font_size: size,
            ..Default::default()
        };
        let layout = font.layout(text, &style, VelloTextAlign::Start, None);
        Vec2::new(layout.width() as f32, layout.height() as f32)
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
        self.label_scaled(position, text, size, color, anchor, 1.0, 1.0);
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "scaled Vello text drawing needs scene/font/window/position/style inputs together"
    )]
    pub(crate) fn label_scaled(
        &mut self,
        position: Vec2,
        text: &str,
        size: f32,
        color: peniko::Color,
        anchor: VelloTextAnchor,
        scale_x: f32,
        scale_y: f32,
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
        let width = layout.width() as f64 * scale_x as f64;
        let height = layout.height() as f64 * scale_y as f64;
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
        let transform = Affine::translate((x + dx, y + dy))
            * Affine::scale_non_uniform(scale_x as f64, scale_y as f64);

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
    pub(crate) notes_state: &'a TerminalNotesState,
    pub(crate) hud_state: &'a HudState,
    pub(crate) font_state: &'a TerminalFontState,
}

fn slice_chars(text: &str, start_chars: usize, max_chars: usize) -> String {
    text.chars().skip(start_chars).take(max_chars).collect()
}

fn message_box_lines(text: &str) -> Vec<(usize, usize, &str)> {
    text.split('\n')
        .scan(0usize, |start, line| {
            let line_start = *start;
            let line_end = line_start + line.len();
            *start = line_end.saturating_add(1);
            Some((line_start, line_end, line))
        })
        .collect()
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
    let button_row_y = message_box_action_buttons(window)[0].rect.y;
    let info_row_y = button_row_y - 26.0;
    let body_rect = HudRect {
        x: rect.x + 22.0,
        y: rect.y + 64.0,
        w: rect.w - 44.0,
        h: (info_row_y - 12.0 - (rect.y + 64.0)).max(96.0),
    };
    painter.fill_rect(body_rect, HudColors::TITLE, 6.0);
    painter.stroke_rect(body_rect, HudColors::TEXT_MUTED, 4.0);

    let line_height = 24.0;
    let text_size = 18.0;
    let content_x = body_rect.x + 18.0;
    let content_y = body_rect.y + 16.0;
    let max_visible_lines = ((body_rect.h - 24.0) / line_height).floor().max(1.0) as usize;
    let max_visible_cols = ((body_rect.w - 36.0) / 10.0).floor().max(8.0) as usize;
    let lines = message_box_lines(&message_box.text);
    let (cursor_line, cursor_col) = message_box.cursor_line_and_column();
    let selection = message_box.region_bounds();
    let start_line = cursor_line.saturating_sub(max_visible_lines.saturating_sub(1));
    let end_line = (start_line + max_visible_lines).min(lines.len());

    painter.scene.push_clip_layer(
        Fill::NonZero,
        Affine::IDENTITY,
        &hud_rect_to_scene(window, body_rect),
    );

    for (visible_index, line_index) in (start_line..end_line).enumerate() {
        let (line_start_byte, line_end_byte, line) = lines[line_index];
        let start_col = if line_index == cursor_line {
            cursor_col.saturating_sub(max_visible_cols.saturating_sub(1))
        } else {
            0
        };
        let display_text = slice_chars(line, start_col, max_visible_cols);
        let y = content_y + visible_index as f32 * line_height;

        if line_index == cursor_line {
            painter.fill_rect(
                HudRect {
                    x: body_rect.x + 8.0,
                    y: y - 3.0,
                    w: body_rect.w - 16.0,
                    h: line_height,
                },
                HudColors::ROW_HOVERED,
                4.0,
            );
        }

        if let Some((selection_start, selection_end)) = selection {
            let line_selection_start = selection_start.max(line_start_byte);
            let line_selection_end = selection_end.min(line_end_byte);
            if line_selection_start < line_selection_end {
                let selection_start_col = message_box.text[line_start_byte..line_selection_start]
                    .chars()
                    .count();
                let selection_end_col = message_box.text[line_start_byte..line_selection_end]
                    .chars()
                    .count();
                let visible_selection_start = selection_start_col.max(start_col) - start_col;
                let visible_selection_end = selection_end_col
                    .min(start_col.saturating_add(max_visible_cols))
                    .saturating_sub(start_col);
                if visible_selection_start < visible_selection_end {
                    let before_selection = slice_chars(line, start_col, visible_selection_start);
                    let before_selection_end = slice_chars(line, start_col, visible_selection_end);
                    let selection_x = content_x + painter.text_size(&before_selection, text_size).x;
                    let selection_end_x =
                        content_x + painter.text_size(&before_selection_end, text_size).x;
                    painter.fill_rect(
                        HudRect {
                            x: selection_x,
                            y: y - 2.0,
                            w: (selection_end_x - selection_x).max(6.0),
                            h: line_height - 4.0,
                        },
                        HudColors::ROW_FOCUSED,
                        3.0,
                    );
                }
            }
        }

        if !display_text.is_empty() {
            painter.label(
                Vec2::new(content_x, y),
                &display_text,
                text_size,
                HudColors::TEXT,
                VelloTextAnchor::TopLeft,
            );
        }

        if line_index == cursor_line {
            let visible_cursor_col = cursor_col.saturating_sub(start_col);
            let before_cursor = slice_chars(line, start_col, visible_cursor_col);
            let cursor_x = content_x + painter.text_size(&before_cursor, text_size).x;
            painter.fill_rect(
                HudRect {
                    x: cursor_x,
                    y,
                    w: 2.5,
                    h: 20.0,
                },
                HudColors::BORDER,
                1.0,
            );
        }
    }

    painter.scene.pop_layer();

    let (line_number, column_number) = message_box.cursor_line_and_column();
    let selection_status = message_box
        .region_bounds()
        .map(|(start, end)| {
            format!(
                "Region {} chars",
                message_box.text[start..end].chars().count()
            )
        })
        .or_else(|| message_box.mark.map(|_| "Mark set".to_owned()))
        .unwrap_or_else(|| "No mark".to_owned());
    for button in message_box_action_buttons(window) {
        painter.fill_rect(button.rect, HudColors::BUTTON, 0.0);
        painter.stroke_rect(button.rect, HudColors::BUTTON_BORDER, 0.0);
        painter.label(
            Vec2::new(button.rect.x + 10.0, button.rect.y + 6.0),
            button.label,
            14.0,
            HudColors::TEXT,
            VelloTextAnchor::TopLeft,
        );
    }

    painter.label(
        Vec2::new(rect.x + 24.0, info_row_y),
        &format!(
            "Ln {} · Col {} · {} · Enter newline · Ctrl-S send · Esc cancel · C-Space mark · C-w cut · M-w copy · C-y yank · M-y ring · Ctrl-T append · Ctrl-Shift-T prepend",
            line_number + 1,
            column_number + 1,
            selection_status
        ),
        15.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::TopLeft,
    );
}

fn module_content_rect(module_id: HudModuleId, shell_rect: HudRect) -> HudRect {
    if module_id == HudModuleId::AgentList {
        return shell_rect;
    }
    HudRect {
        x: shell_rect.x,
        y: shell_rect.y + HUD_TITLEBAR_HEIGHT.min(shell_rect.h),
        w: shell_rect.w,
        h: (shell_rect.h - HUD_TITLEBAR_HEIGHT.min(shell_rect.h)).max(0.0),
    }
}

fn draw_module_shell(painter: &mut HudPainter, module_id: HudModuleId, shell_rect: HudRect) {
    if module_id == HudModuleId::AgentList {
        return;
    }
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
    notes_state: Res<TerminalNotesState>,
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
        notes_state: &notes_state,
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

        let content_rect = module_content_rect(module_id, module.shell.current_rect);
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
