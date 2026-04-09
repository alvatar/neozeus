use crate::{
    app::{
        AegisDialogField, AppSessionState, CloneAgentDialogField, CreateAgentDialogField,
        RenameAgentDialogField, TextFieldState,
    },
    composer::{
        aegis_dialog_rect, aegis_enable_button_rect, clone_agent_dialog_rect,
        clone_agent_name_field_rect, clone_agent_submit_button_rect, clone_agent_workdir_rect,
        create_agent_create_button_rect, create_agent_dialog_rect, create_agent_kind_option_rects,
        create_agent_name_field_rect, create_agent_starting_folder_rect,
        message_box_action_buttons, message_box_rect, rename_agent_dialog_rect,
        rename_agent_name_field_rect, rename_agent_submit_button_rect, task_dialog_action_buttons,
        task_dialog_rect, wrapped_text_rows_measured, MessageDialogFocus, TaskDialogFocus,
        TextEditorState,
    },
    startup::DaemonConnectionState,
};

use super::{
    modules,
    state::{
        AgentListUiState, ConversationListUiState, HudLayoutState, HudRect, HUD_TITLEBAR_HEIGHT,
    },
    view_models::{AgentListView, ComposerView, ConversationListView, InfoBarView, ThreadView},
    widgets::HudWidgetKey,
};
use bevy::{prelude::*, window::PrimaryWindow};
use bevy_vello::{
    parley::PositionedLayoutItem,
    prelude::{
        kurbo::{Affine, Line, Rect, RoundedRect, Stroke},
        peniko::{self, Fill},
        vello, VelloFont, VelloScene2d, VelloTextAlign, VelloTextAnchor, VelloTextStyle,
    },
};
use std::env;

#[derive(Component)]
pub(crate) struct HudVectorSceneMarker;

#[derive(Component)]
pub(crate) struct HudModalVectorSceneMarker;

#[derive(Component)]
pub(crate) struct HudModalCameraMarker;

pub(crate) const HUD_MODAL_RENDER_LAYER: usize = 33;
pub(crate) const HUD_MODAL_CAMERA_ORDER: isize = 101;

pub(crate) struct HudColors;

impl HudColors {
    pub(crate) const FRAME: peniko::Color = peniko::Color::from_rgba8(7, 7, 7, 255);
    const TITLE: peniko::Color = peniko::Color::from_rgba8(7, 7, 7, 255);
    pub(crate) const BORDER: peniko::Color = peniko::Color::from_rgba8(57, 26, 6, 255);
    pub(crate) const TEXT: peniko::Color = peniko::Color::from_rgba8(238, 96, 2, 255);
    pub(crate) const TEXT_MUTED: peniko::Color = peniko::Color::from_rgba8(216, 196, 162, 255);
    pub(crate) const BUTTON: peniko::Color = peniko::Color::from_rgba8(26, 26, 26, 255);
    pub(crate) const BUTTON_BORDER: peniko::Color = peniko::Color::from_rgba8(57, 26, 6, 255);
    pub(crate) const ROW_HOVERED: peniko::Color = peniko::Color::from_rgba8(44, 32, 24, 255);
    pub(crate) const ROW_FOCUSED: peniko::Color = peniko::Color::from_rgba8(44, 32, 24, 255);
    const MESSAGE_BOX: peniko::Color = peniko::Color::from_rgba8(0, 0, 0, 255);
}

/// Scales a color's alpha channel by a clamped factor while leaving RGB untouched.
///
/// HUD rendering keeps colors in `peniko::Color`, so this helper is the common "apply module fade"
/// operation.
pub(crate) fn apply_alpha(color: peniko::Color, factor: f32) -> peniko::Color {
    let rgba = color.to_rgba8();
    let alpha = ((rgba.a as f32) * factor.clamp(0.0, 1.0)).round() as u8;
    peniko::Color::from_rgba8(rgba.r, rgba.g, rgba.b, alpha)
}

/// Linearly interpolates between two HUD colors in RGBA space.
pub(crate) fn interpolate_color(a: peniko::Color, b: peniko::Color, t: f32) -> peniko::Color {
    let a = a.to_rgba8();
    let b = b.to_rgba8();
    let t = t.clamp(0.0, 1.0);
    peniko::Color::from_rgba8(
        (a.r as f32 + (b.r as f32 - a.r as f32) * t).round() as u8,
        (a.g as f32 + (b.g as f32 - a.g as f32) * t).round() as u8,
        (a.b as f32 + (b.b as f32 - a.b as f32) * t).round() as u8,
        (a.a as f32 + (b.a as f32 - a.a as f32) * t).round() as u8,
    )
}

/// Converts a HUD-space point into Vello scene coordinates centered on the window.
///
/// HUD layout uses a top-left origin; the vector scene is centered at window midpoint.
fn hud_to_scene(window: &Window, point: Vec2) -> (f64, f64) {
    (
        f64::from(point.x - window.width() * 0.5),
        f64::from(point.y - window.height() * 0.5),
    )
}

/// Converts a HUD rectangle into a Vello `Rect` in centered scene coordinates.
///
/// The helper computes both corners through [`hud_to_scene`] so inverted axes are normalized safely.
fn hud_rect_to_scene(window: &Window, rect: HudRect) -> Rect {
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
    /// Creates a painter bound to one Vello scene, font set, window transform, and global alpha.
    ///
    /// The painter is a thin convenience wrapper so HUD rendering code can issue higher-level drawing
    /// operations without repeating the same scene/window/font plumbing everywhere.
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

    /// Fills a HUD rectangle in the bound scene.
    ///
    /// Rounded-corner radius is currently ignored; all HUD fills are emitted as square-cornered Vello
    /// rounded rects with radius zero.
    pub(crate) fn fill_rect(&mut self, rect: HudRect, color: peniko::Color, _radius: f64) {
        self.scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            apply_alpha(color, self.alpha),
            None,
            &RoundedRect::from_rect(hud_rect_to_scene(self.window, rect), 0.0),
        );
    }

    /// Strokes a HUD rectangle using the default border width.
    ///
    /// Radius is ignored for the same reason as [`Self::fill_rect`].
    pub(crate) fn stroke_rect(&mut self, rect: HudRect, color: peniko::Color, _radius: f64) {
        self.stroke_rect_width(rect, color, 1.5);
    }

    /// Strokes a HUD rectangle with an explicit border width.
    ///
    /// This is the low-level border primitive used by helpers that need heavier outlines than the HUD
    /// default.
    pub(crate) fn stroke_rect_width(&mut self, rect: HudRect, color: peniko::Color, width: f64) {
        self.scene.stroke(
            &Stroke::new(width),
            Affine::IDENTITY,
            apply_alpha(color, self.alpha),
            None,
            &RoundedRect::from_rect(hud_rect_to_scene(self.window, rect), 0.0),
        );
    }

    /// Draws a straight HUD line segment with the requested stroke width.
    pub(crate) fn stroke_line(&mut self, start: Vec2, end: Vec2, color: peniko::Color, width: f64) {
        let (x0, y0) = hud_to_scene(self.window, start);
        let (x1, y1) = hud_to_scene(self.window, end);
        self.scene.stroke(
            &Stroke::new(width),
            Affine::IDENTITY,
            apply_alpha(color, self.alpha),
            None,
            &Line::new((x0, y0), (x1, y1)),
        );
    }

    /// Measures the laid-out size of a text run using the default Vello font.
    ///
    /// If the default font asset has not loaded yet, the function reports zero size instead of
    /// panicking.
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
    /// Draws one text label with uniform scale.
    ///
    /// This is the common convenience wrapper around [`Self::label_scaled`] for ordinary HUD text.
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
    /// Draws one text label with explicit anchor and non-uniform scale.
    ///
    /// The function lays text out once, computes an anchor offset in scaled coordinates, then emits the
    /// underlying glyph runs into the Vello scene.
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
        // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
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
    pub(crate) agent_list_view: &'a AgentListView,
    pub(crate) conversation_list_view: &'a ConversationListView,
    pub(crate) thread_view: &'a ThreadView,
    pub(crate) info_bar_view: &'a InfoBarView,
    pub(crate) agent_list_text_selection: &'a crate::text_selection::AgentListTextSelectionState,
}

/// Logs a low-level color-presence diagnostic for HUD draw data when explicitly requested.
///
/// This is a debugging hook for color-conversion issues: it inspects encoded scene words for known
/// orange/yellow values and writes the result to the terminal debug log.
fn log_hud_draw_colors_if_requested(scene: &vello::Scene) {
    // Keep the steps explicit so state transitions remain easy to audit and edge cases stay localized.
    let enabled = env::var("NEOZEUS_LOG_HUD_DRAW_COLORS")
        .ok()
        .is_some_and(|value| value == "1");
    if !enabled {
        return;
    }

    let encoding = scene.encoding();
    let requested_orange = u32::from_le_bytes([225, 129, 10, 255]);
    let observed_yellow = u32::from_le_bytes([255, 177, 18, 255]);
    let requested_present = encoding.draw_data.contains(&requested_orange);
    let observed_present = encoding.draw_data.contains(&observed_yellow);
    crate::terminals::append_debug_log(format!(
        "hud draw data words={} requested_orange_present={} observed_yellow_present={} requested_orange=0x{requested_orange:08x} observed_yellow=0x{observed_yellow:08x}",
        encoding.draw_data.len(),
        requested_present,
        observed_present,
    ));
}

/// Extracts a substring by character indices rather than byte indices.
///
/// The editor viewport logic uses this to window UTF-8 text safely.
fn slice_chars(text: &str, start_chars: usize, max_chars: usize) -> String {
    text.chars().skip(start_chars).take(max_chars).collect()
}

/// Splits editor text into lines while preserving byte bounds for each line.
///
/// Returning `(start, end, line)` triples lets selection logic translate between line-local character
/// columns and whole-buffer byte ranges.
#[cfg(test)]
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

/// Builds the short status string describing the editor's current selection state.
///
/// A real region wins, then a bare mark, then the default "no mark" message.
fn editor_selection_status(editor: &TextEditorState) -> String {
    editor
        .region_bounds()
        .map(|(start, end)| format!("Region {} chars", editor.text[start..end].chars().count()))
        .or_else(|| editor.mark.map(|_| "Mark set".to_owned()))
        .unwrap_or_else(|| "No mark".to_owned())
}

#[cfg(test)]
type WrappedEditorRow<'a> = crate::composer::WrappedTextRow<'a>;

#[derive(Clone, Debug, PartialEq, Eq)]
enum CursorVisualSpan<'a> {
    InvertedGlyph {
        leading_text: &'a str,
        glyph: &'a str,
        trailing_text: &'a str,
    },
    BoundaryBlock {
        leading_text: &'a str,
    },
}

fn byte_index_for_char(text: &str, chars: usize) -> usize {
    let mut byte_index = 0;
    for ch in text.chars().take(chars) {
        byte_index += ch.len_utf8();
    }
    byte_index
}

fn cursor_visual_span(display_text: &str, cursor_col: usize) -> CursorVisualSpan<'_> {
    let display_len = display_text.chars().count();
    if cursor_col < display_len {
        let glyph_start = byte_index_for_char(display_text, cursor_col);
        let glyph_end = byte_index_for_char(display_text, cursor_col + 1);
        CursorVisualSpan::InvertedGlyph {
            leading_text: &display_text[..glyph_start],
            glyph: &display_text[glyph_start..glyph_end],
            trailing_text: &display_text[glyph_end..],
        }
    } else {
        CursorVisualSpan::BoundaryBlock {
            leading_text: display_text,
        }
    }
}

#[cfg(test)]
fn cursor_byte_for_line_column(text: &str, cursor_line: usize, cursor_col: usize) -> usize {
    let Some((line_start_byte, line_end_byte, line_text)) =
        message_box_lines(text).into_iter().nth(cursor_line)
    else {
        return text.len();
    };
    let bounded_cursor = cursor_col.min(line_text.chars().count());
    line_start_byte
        + byte_index_for_char(line_text, bounded_cursor).min(line_end_byte - line_start_byte)
}

fn active_line_bounds(text: &str, cursor: usize) -> (usize, usize) {
    let line_start = text[..cursor]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    let line_end = text[cursor..]
        .find('\n')
        .map(|offset| cursor + offset)
        .unwrap_or(text.len());
    (line_start, line_end)
}

fn wrapped_row_is_active(
    row: &crate::composer::WrappedTextRow<'_>,
    active_line: (usize, usize),
) -> bool {
    (row.line_start_byte, row.line_end_byte) == active_line
}

#[cfg(test)]
fn wrapped_editor_rows<'a>(
    text: &'a str,
    max_visible_cols: usize,
    cursor_line: usize,
    cursor_col: usize,
) -> (Vec<WrappedEditorRow<'a>>, usize) {
    crate::composer::wrapped_text_rows(
        text,
        max_visible_cols,
        cursor_byte_for_line_column(text, cursor_line, cursor_col),
    )
}

/// Draws the wrapped modal text editor body, including visible lines, selection, and cursor.
fn draw_text_editor_body(
    painter: &mut HudPainter,
    window: &Window,
    editor: &TextEditorState,
    body_rect: HudRect,
    focused: bool,
) {
    painter.fill_rect(body_rect, HudColors::MESSAGE_BOX, 6.0);
    painter.stroke_rect(
        body_rect,
        if focused {
            HudColors::TEXT_MUTED
        } else {
            HudColors::BUTTON_BORDER
        },
        4.0,
    );

    let line_height = 24.0;
    let text_size = 18.0;
    let content_x = body_rect.x + 18.0;
    let content_y = body_rect.y + 16.0;
    let max_visible_lines = ((body_rect.h - 24.0) / line_height).floor().max(1.0) as usize;
    let selection = editor.region_bounds();
    let active_line = active_line_bounds(&editor.text, editor.cursor);
    let text_width = (body_rect.w - 36.0).max(1.0);
    let (rows, cursor_row) =
        wrapped_text_rows_measured(&editor.text, editor.cursor, text_width, |segment| {
            painter.text_size(segment, text_size).x
        });
    let start_row = cursor_row.saturating_sub(max_visible_lines.saturating_sub(1));
    let end_row = (start_row + max_visible_lines).min(rows.len());

    painter.scene.push_clip_layer(
        Fill::NonZero,
        Affine::IDENTITY,
        &hud_rect_to_scene(window, body_rect),
    );

    for (visible_index, row) in rows[start_row..end_row].iter().enumerate() {
        let y = content_y + visible_index as f32 * line_height;

        if wrapped_row_is_active(row, active_line) {
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
            let row_selection_start = selection_start.max(row.display_start_byte);
            let row_selection_end = selection_end.min(row.display_end_byte);
            if row_selection_start < row_selection_end {
                let local_start = editor.text[row.display_start_byte..row_selection_start]
                    .chars()
                    .count();
                let local_end = editor.text[row.display_start_byte..row_selection_end]
                    .chars()
                    .count();
                let before_selection = slice_chars(row.display_text, 0, local_start);
                let before_selection_end = slice_chars(row.display_text, 0, local_end);
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

        if focused {
            if let Some(visible_cursor_col) = row.cursor_col {
                match cursor_visual_span(row.display_text, visible_cursor_col) {
                    CursorVisualSpan::InvertedGlyph {
                        leading_text,
                        glyph,
                        trailing_text,
                    } => {
                        if !leading_text.is_empty() {
                            painter.label(
                                Vec2::new(content_x, y),
                                leading_text,
                                text_size,
                                HudColors::TEXT,
                                VelloTextAnchor::TopLeft,
                            );
                        }
                        let cursor_x = content_x + painter.text_size(leading_text, text_size).x;
                        let glyph_width = painter.text_size(glyph, text_size).x.max(10.0);
                        painter.fill_rect(
                            HudRect {
                                x: cursor_x - 1.0,
                                y: y - 1.0,
                                w: glyph_width + 2.0,
                                h: 22.0,
                            },
                            HudColors::TEXT,
                            2.0,
                        );
                        painter.label(
                            Vec2::new(cursor_x, y),
                            glyph,
                            text_size,
                            HudColors::MESSAGE_BOX,
                            VelloTextAnchor::TopLeft,
                        );
                        if !trailing_text.is_empty() {
                            painter.label(
                                Vec2::new(cursor_x + glyph_width, y),
                                trailing_text,
                                text_size,
                                HudColors::TEXT,
                                VelloTextAnchor::TopLeft,
                            );
                        }
                    }
                    CursorVisualSpan::BoundaryBlock { leading_text } => {
                        if !leading_text.is_empty() {
                            painter.label(
                                Vec2::new(content_x, y),
                                leading_text,
                                text_size,
                                HudColors::TEXT,
                                VelloTextAnchor::TopLeft,
                            );
                        }
                        let cursor_x = content_x + painter.text_size(leading_text, text_size).x;
                        painter.fill_rect(
                            HudRect {
                                x: cursor_x - 1.0,
                                y: y - 1.0,
                                w: 11.0,
                                h: 22.0,
                            },
                            HudColors::TEXT,
                            2.0,
                        );
                    }
                }
            } else if !row.display_text.is_empty() {
                painter.label(
                    Vec2::new(content_x, y),
                    row.display_text,
                    text_size,
                    HudColors::TEXT,
                    VelloTextAnchor::TopLeft,
                );
            }
        } else if !row.display_text.is_empty() {
            painter.label(
                Vec2::new(content_x, y),
                row.display_text,
                text_size,
                HudColors::TEXT,
                VelloTextAnchor::TopLeft,
            );
        }
    }

    painter.scene.pop_layer();
}

/// Draws a simple row of modal action buttons from precomputed rect/label pairs.
///
/// This helper is shared by both the message box and the task dialog so they keep identical button
/// chrome.
fn draw_dialog_button_row(
    painter: &mut HudPainter,
    buttons: impl IntoIterator<Item = (HudRect, &'static str, bool)>,
) {
    for (rect, label, focused) in buttons {
        painter.fill_rect(rect, HudColors::BUTTON, 0.0);
        painter.stroke_rect(
            rect,
            if focused {
                HudColors::TEXT
            } else {
                HudColors::BUTTON_BORDER
            },
            0.0,
        );
        painter.label(
            Vec2::new(rect.x + 10.0, rect.y + 6.0),
            label,
            14.0,
            if focused {
                HudColors::TEXT
            } else {
                HudColors::TEXT_MUTED
            },
            VelloTextAnchor::TopLeft,
        );
    }
}

fn single_line_field_viewport(
    text: &str,
    cursor: usize,
    max_visible_cols: usize,
) -> (usize, usize, String) {
    let safe_cursor = cursor.min(text.len());
    let cursor_col = text[..safe_cursor].chars().count();
    let start_col = cursor_col.saturating_sub(max_visible_cols.saturating_sub(1));
    let visible_cursor_col = cursor_col.saturating_sub(start_col);
    let display_text = slice_chars(text, start_col, max_visible_cols);
    (start_col, visible_cursor_col, display_text)
}

/// Draws one single-line dialog field with optional focus cursor.
fn draw_single_line_dialog_field(
    painter: &mut HudPainter,
    window: &Window,
    field: &TextFieldState,
    rect: HudRect,
    focused: bool,
) {
    painter.fill_rect(rect, HudColors::BUTTON, 4.0);
    painter.stroke_rect(
        rect,
        if focused {
            HudColors::TEXT
        } else {
            HudColors::BUTTON_BORDER
        },
        4.0,
    );

    let content_rect = HudRect {
        x: rect.x + 10.0,
        y: rect.y + 5.0,
        w: (rect.w - 20.0).max(1.0),
        h: (rect.h - 10.0).max(1.0),
    };
    let max_visible_cols = ((content_rect.w - 4.0) / 9.0).floor().max(4.0) as usize;
    let (_start_col, visible_cursor_col, display_text) =
        single_line_field_viewport(&field.text, field.cursor, max_visible_cols);

    painter.scene.push_clip_layer(
        Fill::NonZero,
        Affine::IDENTITY,
        &hud_rect_to_scene(window, content_rect),
    );
    if !display_text.is_empty() {
        painter.label(
            Vec2::new(content_rect.x, rect.y + 7.0),
            &display_text,
            15.0,
            HudColors::TEXT,
            VelloTextAnchor::TopLeft,
        );
    }
    if focused {
        let before_cursor = slice_chars(&display_text, 0, visible_cursor_col);
        let cursor_x = content_rect.x + painter.text_size(&before_cursor, 15.0).x;
        painter.fill_rect(
            HudRect {
                x: cursor_x,
                y: rect.y + 6.0,
                w: 2.0,
                h: rect.h - 12.0,
            },
            HudColors::TEXT,
            0.0,
        );
    }
    painter.scene.pop_layer();
}

/// Draws the centered create-agent dialog modal.
fn draw_create_agent_dialog(
    painter: &mut HudPainter,
    window: &Window,
    app_session: &AppSessionState,
) {
    if !app_session.create_agent_dialog.visible {
        return;
    }

    let dialog = &app_session.create_agent_dialog;
    let rect = create_agent_dialog_rect(window);
    painter.fill_rect(rect, HudColors::MESSAGE_BOX, 12.0);
    painter.stroke_rect(rect, HudColors::BORDER, 12.0);

    painter.label(
        Vec2::new(rect.x + 24.0, rect.y + 14.0),
        "Create agent",
        20.0,
        HudColors::TEXT,
        VelloTextAnchor::TopLeft,
    );

    let name_rect = create_agent_name_field_rect(window);
    let kind_options = create_agent_kind_option_rects(window);
    let folder_rect = create_agent_starting_folder_rect(window);
    let create_rect = create_agent_create_button_rect(window);

    painter.label(
        Vec2::new(rect.x + 24.0, name_rect.y + 7.0),
        "Name",
        15.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::TopLeft,
    );
    draw_single_line_dialog_field(
        painter,
        window,
        &dialog.name_field,
        name_rect,
        dialog.focus == CreateAgentDialogField::Name,
    );

    painter.label(
        Vec2::new(rect.x + 24.0, kind_options[0].1.y + 3.0),
        "Type",
        15.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::TopLeft,
    );
    for (kind, option_rect, label) in kind_options {
        let selected = dialog.kind == kind;
        let focused = dialog.focus == CreateAgentDialogField::Kind;
        let square_rect = HudRect {
            x: option_rect.x,
            y: option_rect.y + 2.0,
            w: 16.0,
            h: 16.0,
        };
        painter.fill_rect(
            square_rect,
            if selected {
                HudColors::TEXT
            } else {
                HudColors::BUTTON
            },
            0.0,
        );
        painter.stroke_rect(
            square_rect,
            if focused && selected {
                HudColors::TEXT
            } else {
                HudColors::BUTTON_BORDER
            },
            0.0,
        );
        painter.label(
            Vec2::new(option_rect.x + 26.0, option_rect.y),
            label,
            16.0,
            if selected {
                HudColors::TEXT
            } else {
                HudColors::TEXT_MUTED
            },
            VelloTextAnchor::TopLeft,
        );
    }

    painter.label(
        Vec2::new(rect.x + 24.0, folder_rect.y + 7.0),
        "cwd",
        15.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::TopLeft,
    );
    draw_single_line_dialog_field(
        painter,
        window,
        &dialog.cwd_field.field,
        folder_rect,
        dialog.focus == CreateAgentDialogField::StartingFolder,
    );

    draw_dialog_button_row(
        &mut *painter,
        [(
            create_rect,
            "Create",
            dialog.focus == CreateAgentDialogField::CreateButton,
        )],
    );

    if let Some(error) = dialog.error.as_deref() {
        painter.label(
            Vec2::new(rect.x + 24.0, create_rect.y - 26.0),
            error,
            14.0,
            peniko::Color::from_rgba8(220, 80, 80, 255),
            VelloTextAnchor::TopLeft,
        );
    }
}

/// Draws the centered clone-agent dialog modal.
fn draw_clone_agent_dialog(
    painter: &mut HudPainter,
    window: &Window,
    app_session: &AppSessionState,
) {
    if !app_session.clone_agent_dialog.visible {
        return;
    }

    let dialog = &app_session.clone_agent_dialog;
    let rect = clone_agent_dialog_rect(window);
    let name_rect = clone_agent_name_field_rect(window);
    let workdir_rect = clone_agent_workdir_rect(window);
    let clone_rect = clone_agent_submit_button_rect(window);

    painter.fill_rect(rect, HudColors::MESSAGE_BOX, 12.0);
    painter.stroke_rect(rect, HudColors::BORDER, 12.0);
    painter.label(
        Vec2::new(rect.x + 24.0, rect.y + 14.0),
        "Clone agent",
        20.0,
        HudColors::TEXT,
        VelloTextAnchor::TopLeft,
    );
    painter.label(
        Vec2::new(rect.x + 24.0, name_rect.y + 7.0),
        "Name",
        15.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::TopLeft,
    );
    draw_single_line_dialog_field(
        painter,
        window,
        &dialog.name_field,
        name_rect,
        dialog.focus == CloneAgentDialogField::Name,
    );

    painter.label(
        Vec2::new(rect.x + 24.0, workdir_rect.y + 3.0),
        "Mode",
        15.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::TopLeft,
    );
    painter.fill_rect(
        workdir_rect,
        if dialog.workdir {
            HudColors::TEXT
        } else {
            HudColors::BUTTON
        },
        0.0,
    );
    painter.stroke_rect(
        workdir_rect,
        if dialog.focus == CloneAgentDialogField::Workdir {
            HudColors::TEXT
        } else {
            HudColors::BUTTON_BORDER
        },
        0.0,
    );
    painter.label(
        Vec2::new(workdir_rect.x + workdir_rect.w + 12.0, workdir_rect.y - 1.0),
        "Create workdir",
        16.0,
        if dialog.workdir {
            HudColors::TEXT
        } else {
            HudColors::TEXT_MUTED
        },
        VelloTextAnchor::TopLeft,
    );

    draw_dialog_button_row(
        painter,
        [(
            clone_rect,
            "Clone",
            dialog.focus == CloneAgentDialogField::CloneButton,
        )],
    );

    if let Some(error) = dialog.error.as_deref() {
        painter.label(
            Vec2::new(rect.x + 24.0, clone_rect.y - 26.0),
            error,
            14.0,
            peniko::Color::from_rgba8(220, 80, 80, 255),
            VelloTextAnchor::TopLeft,
        );
    }
}

/// Draws the centered rename-agent dialog modal.
fn draw_rename_agent_dialog(
    painter: &mut HudPainter,
    window: &Window,
    app_session: &AppSessionState,
) {
    if !app_session.rename_agent_dialog.visible {
        return;
    }

    let dialog = &app_session.rename_agent_dialog;
    let rect = rename_agent_dialog_rect(window);
    let name_rect = rename_agent_name_field_rect(window);
    let rename_rect = rename_agent_submit_button_rect(window);

    painter.fill_rect(rect, HudColors::MESSAGE_BOX, 12.0);
    painter.stroke_rect(rect, HudColors::BORDER, 12.0);
    painter.label(
        Vec2::new(rect.x + 24.0, rect.y + 14.0),
        "Rename agent",
        20.0,
        HudColors::TEXT,
        VelloTextAnchor::TopLeft,
    );
    painter.label(
        Vec2::new(rect.x + 24.0, name_rect.y + 7.0),
        "Name",
        15.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::TopLeft,
    );
    draw_single_line_dialog_field(
        painter,
        window,
        &dialog.name_field,
        name_rect,
        dialog.focus == RenameAgentDialogField::Name,
    );
    draw_dialog_button_row(
        painter,
        [(
            rename_rect,
            "Rename",
            dialog.focus == RenameAgentDialogField::RenameButton,
        )],
    );

    if let Some(error) = dialog.error.as_deref() {
        painter.label(
            Vec2::new(rect.x + 24.0, rename_rect.y - 26.0),
            error,
            14.0,
            peniko::Color::from_rgba8(220, 80, 80, 255),
            VelloTextAnchor::TopLeft,
        );
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "shared editor dialog shell intentionally owns the common title/body/button/footer/error surface"
)]
fn draw_text_editor_dialog(
    painter: &mut HudPainter,
    window: &Window,
    rect: HudRect,
    title: &str,
    editor: &TextEditorState,
    editor_focused: bool,
    buttons: impl IntoIterator<Item = (HudRect, &'static str, bool)>,
    footer_text: Option<&str>,
    error_text: Option<&str>,
) {
    painter.fill_rect(rect, HudColors::MESSAGE_BOX, 12.0);
    painter.stroke_rect(rect, HudColors::BORDER, 12.0);

    let title_rect = HudRect {
        x: rect.x,
        y: rect.y,
        w: rect.w,
        h: 44.0,
    };
    painter.fill_rect(title_rect, HudColors::MESSAGE_BOX, 12.0);
    painter.label(
        Vec2::new(rect.x + 24.0, rect.y + 12.0),
        title,
        18.0,
        HudColors::TEXT,
        VelloTextAnchor::TopLeft,
    );

    let buttons = buttons.into_iter().collect::<Vec<_>>();
    let button_row_y = buttons[0].0.y;
    let info_row_y = button_row_y - 26.0;
    let body_rect = HudRect {
        x: rect.x + 22.0,
        y: rect.y + 64.0,
        w: rect.w - 44.0,
        h: (info_row_y - 12.0 - (rect.y + 64.0)).max(96.0),
    };
    draw_text_editor_body(painter, window, editor, body_rect, editor_focused);
    draw_dialog_button_row(painter, buttons);

    if let Some(footer_text) = footer_text {
        painter.label(
            Vec2::new(rect.x + 24.0, info_row_y),
            footer_text,
            15.0,
            HudColors::TEXT_MUTED,
            VelloTextAnchor::TopLeft,
        );
    }
    if let Some(error_text) = error_text {
        painter.label(
            Vec2::new(rect.x + 24.0, button_row_y - 26.0),
            error_text,
            14.0,
            peniko::Color::from_rgba8(220, 80, 80, 255),
            VelloTextAnchor::TopLeft,
        );
    }
}

/// Draws the message-box modal, including title, editor body, buttons, and status line.
fn draw_aegis_dialog(painter: &mut HudPainter, window: &Window, app_session: &AppSessionState) {
    if !app_session.aegis_dialog.visible {
        return;
    }

    let dialog = &app_session.aegis_dialog;
    let rect = aegis_dialog_rect(window);
    let enable_rect = aegis_enable_button_rect(window);
    let (line_number, column_number) = dialog.prompt_editor.cursor_line_and_column();
    let footer = format!(
        "Ln {} · Col {} · {} · Enter newline · Esc cancel",
        line_number + 1,
        column_number + 1,
        editor_selection_status(&dialog.prompt_editor)
    );
    draw_text_editor_dialog(
        painter,
        window,
        rect,
        "Aegis",
        &dialog.prompt_editor,
        dialog.focus == AegisDialogField::Prompt,
        [(
            enable_rect,
            "Enable",
            dialog.focus == AegisDialogField::EnableButton,
        )],
        Some(&footer),
        dialog.error.as_deref(),
    );
}

fn draw_message_box(
    painter: &mut HudPainter,
    window: &Window,
    message_box: &TextEditorState,
    title: &str,
    focus: MessageDialogFocus,
) {
    if !message_box.visible {
        return;
    }

    let rect = message_box_rect(window);
    let buttons = message_box_action_buttons(window);
    let (line_number, column_number) = message_box.cursor_line_and_column();
    let footer = format!(
        "Ln {} · Col {} · {} · Enter newline · Ctrl-S send · Esc cancel · C-Space mark · C-w cut · M-w copy · C-y yank · M-y ring",
        line_number + 1,
        column_number + 1,
        editor_selection_status(message_box)
    );
    draw_text_editor_dialog(
        painter,
        window,
        rect,
        title,
        message_box,
        focus == MessageDialogFocus::Editor,
        buttons.into_iter().map(|(action, rect, label)| {
            (
                rect,
                label,
                match action {
                    crate::composer::MessageBoxAction::AppendTask => {
                        focus == MessageDialogFocus::AppendButton
                    }
                    crate::composer::MessageBoxAction::PrependTask => {
                        focus == MessageDialogFocus::PrependButton
                    }
                },
            )
        }),
        Some(&footer),
        None,
    );
}

/// Draws the task-dialog modal, which reuses the shared text editor body with different title and
/// button copy.
fn draw_task_dialog(
    painter: &mut HudPainter,
    window: &Window,
    task_dialog: &TextEditorState,
    title: &str,
    focus: TaskDialogFocus,
) {
    if !task_dialog.visible {
        return;
    }

    let rect = task_dialog_rect(window);
    let buttons = task_dialog_action_buttons(window);
    let (line_number, column_number) = task_dialog.cursor_line_and_column();
    let footer = format!(
        "Ln {} · Col {} · {} · Format: - [] task or - [ ] task · Ctrl-T clear done · Esc close+persist",
        line_number + 1,
        column_number + 1,
        editor_selection_status(task_dialog)
    );
    draw_text_editor_dialog(
        painter,
        window,
        rect,
        title,
        task_dialog,
        focus == TaskDialogFocus::Editor,
        buttons.into_iter().map(|(action, rect, label)| {
            (
                rect,
                label,
                match action {
                    crate::composer::TaskDialogAction::ClearDone => {
                        focus == TaskDialogFocus::ClearDoneButton
                    }
                },
            )
        }),
        Some(&footer),
        None,
    );
}

/// Rendering is skipped entirely when the dialog is hidden.
fn startup_connect_rect(window: &Window) -> HudRect {
    let size = Vec2::new(
        (window.width() * 0.46).clamp(420.0, 760.0),
        (window.height() * 0.22).clamp(180.0, 280.0),
    );
    HudRect {
        x: window.width() * 0.5 - size.x * 0.5,
        y: window.height() * 0.5 - size.y * 0.5,
        w: size.x,
        h: size.y,
    }
}

/// Draws startup connect overlay.
fn draw_startup_connect_overlay(
    painter: &mut HudPainter,
    window: &Window,
    startup_connect: &DaemonConnectionState,
) {
    // Build the geometry or layout decisions first, then emit the matching draw operations against the prepared state.
    if !startup_connect.modal_visible() {
        return;
    }

    let rect = startup_connect_rect(window);
    let glow_rect = HudRect {
        x: rect.x - 10.0,
        y: rect.y - 10.0,
        w: rect.w + 20.0,
        h: rect.h + 20.0,
    };
    let glow = peniko::Color::from_rgba8(
        modules::AGENT_LIST_BLOOM_RED_R,
        modules::AGENT_LIST_BLOOM_RED_G,
        modules::AGENT_LIST_BLOOM_RED_B,
        255,
    );
    let border = peniko::Color::from_rgba8(
        modules::AGENT_LIST_BORDER_ORANGE_R,
        modules::AGENT_LIST_BORDER_ORANGE_G,
        modules::AGENT_LIST_BORDER_ORANGE_B,
        255,
    );

    // Match the active-agent look: one hard border with a soft emissive halo behind it.
    painter.fill_rect(glow_rect, apply_alpha(glow, 0.18), 0.0);
    painter.fill_rect(rect, HudColors::MESSAGE_BOX, 0.0);
    painter.stroke_rect_width(rect, border, 2.5);

    let title = startup_connect.title();
    if !title.is_empty() {
        painter.label(
            Vec2::new(rect.x + rect.w * 0.5, rect.y + 34.0),
            title,
            26.0,
            border,
            VelloTextAnchor::Top,
        );
    }
    painter.label(
        Vec2::new(rect.x + rect.w * 0.5, rect.y + 86.0),
        startup_connect.status(),
        18.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::Top,
    );
    painter.label(
        Vec2::new(rect.x + rect.w * 0.5, rect.y + rect.h - 34.0),
        "Window is live. Session restore will begin as soon as the runtime is ready.",
        15.0,
        HudColors::TEXT_MUTED,
        VelloTextAnchor::Bottom,
    );
}

/// Returns the drawable content rectangle inside a module shell.
///
/// Most modules exclude the titlebar from content rendering; the agent list is full-bleed and keeps
/// the entire shell rect.
fn module_content_rect(module_id: HudWidgetKey, shell_rect: HudRect) -> HudRect {
    if matches!(module_id, HudWidgetKey::AgentList | HudWidgetKey::InfoBar) {
        return shell_rect;
    }
    HudRect {
        x: shell_rect.x,
        y: shell_rect.y + HUD_TITLEBAR_HEIGHT.min(shell_rect.h),
        w: shell_rect.w,
        h: (shell_rect.h - HUD_TITLEBAR_HEIGHT.min(shell_rect.h)).max(0.0),
    }
}

/// Draws the shared shell chrome for a HUD module.
///
/// The agent list intentionally opts out because it has its own custom full-height framing.
fn draw_module_shell(painter: &mut HudPainter, module_id: HudWidgetKey, shell_rect: HudRect) {
    // Build the geometry or layout decisions first, then emit the matching draw operations against the prepared state.
    if module_id == HudWidgetKey::AgentList {
        return;
    }
    if module_id == HudWidgetKey::InfoBar {
        painter.fill_rect(shell_rect, modules::INFO_BAR_BACKGROUND, 0.0);
        painter.stroke_rect_width(shell_rect, modules::INFO_BAR_BORDER, 1.0);
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
/// Rebuilds the main HUD vector scene from retained module state and live terminal inputs.
///
/// The scene is reconstructed from scratch every frame: each visible module shell is drawn, its
/// content is clipped to the content rect, and module-specific rendering is delegated through the HUD
/// module dispatcher.
pub(crate) fn render_hud_scene(
    primary_window: Single<&Window, With<PrimaryWindow>>,
    layout_state: Res<HudLayoutState>,
    agent_list_state: Res<AgentListUiState>,
    conversation_list_state: Res<ConversationListUiState>,
    agent_list_view: Res<AgentListView>,
    conversation_list_view: Res<ConversationListView>,
    thread_view: Res<ThreadView>,
    info_bar_view: Res<InfoBarView>,
    agent_list_text_selection: Res<crate::text_selection::AgentListTextSelectionState>,
    fonts: Res<Assets<VelloFont>>,
    startup_connect: Option<Res<DaemonConnectionState>>,
    mut scene: Single<&mut VelloScene2d, With<HudVectorSceneMarker>>,
) {
    // Build the geometry or layout decisions first, then emit the matching draw operations against the prepared state.
    let mut built = vello::Scene::new();
    if startup_connect.is_some_and(|state| state.modal_visible()) {
        **scene = VelloScene2d::from(built);
        return;
    }
    let inputs = HudRenderInputs {
        agent_list_view: &agent_list_view,
        conversation_list_view: &conversation_list_view,
        thread_view: &thread_view,
        info_bar_view: &info_bar_view,
        agent_list_text_selection: &agent_list_text_selection,
    };

    for module_id in layout_state.iter_z_order() {
        let Some(module) = layout_state.get(module_id) else {
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
            content_rect,
            &mut painter,
            &inputs,
            &agent_list_state,
            &conversation_list_state,
        );
        built.pop_layer();
    }

    log_hud_draw_colors_if_requested(&built);
    **scene = VelloScene2d::from(built);
}

/// Rebuilds the separate modal HUD scene that contains the message box and task dialog overlays.
///
/// Modal rendering is isolated from the main HUD scene so compositor/layer logic can treat it as a
/// separate surface.
pub(crate) fn render_hud_modal_scene(
    primary_window: Single<&Window, With<PrimaryWindow>>,
    app_session: Res<AppSessionState>,
    composer_view: Res<ComposerView>,
    startup_connect: Option<Res<DaemonConnectionState>>,
    fonts: Res<Assets<VelloFont>>,
    mut scene: Single<&mut VelloScene2d, With<HudModalVectorSceneMarker>>,
) {
    // Build the geometry or layout decisions first, then emit the matching draw operations against the prepared state.
    let mut built = vello::Scene::new();
    let mut painter = HudPainter::new(&mut built, &fonts, &primary_window, 1.0);
    if let Some(startup_connect) = startup_connect.as_deref() {
        draw_startup_connect_overlay(&mut painter, &primary_window, startup_connect);
    }
    draw_create_agent_dialog(&mut painter, &primary_window, &app_session);
    draw_clone_agent_dialog(&mut painter, &primary_window, &app_session);
    draw_rename_agent_dialog(&mut painter, &primary_window, &app_session);
    draw_aegis_dialog(&mut painter, &primary_window, &app_session);
    draw_message_box(
        &mut painter,
        &primary_window,
        &app_session.composer.message_editor,
        composer_view.title.as_deref().unwrap_or("Message"),
        app_session.composer.message_dialog_focus,
    );
    draw_task_dialog(
        &mut painter,
        &primary_window,
        &app_session.composer.task_editor,
        composer_view.title.as_deref().unwrap_or("Tasks"),
        app_session.composer.task_dialog_focus,
    );
    **scene = VelloScene2d::from(built);
}

#[cfg(test)]
mod tests {
    use super::{
        active_line_bounds, cursor_visual_span, single_line_field_viewport, wrapped_editor_rows,
        wrapped_row_is_active, CursorVisualSpan,
    };

    #[test]
    fn single_line_field_viewport_keeps_cursor_visible_at_end_of_long_text() {
        let (_start, visible_cursor_col, display) =
            single_line_field_viewport("abcdefghijklmno", 15, 6);
        assert_eq!(display, "klmno");
        assert_eq!(visible_cursor_col, 5);
    }

    #[test]
    fn single_line_field_viewport_handles_utf8_cursor_boundaries() {
        let text = "aébΩz";
        let cursor = text.find('Ω').expect("omega should exist");
        let (_start, visible_cursor_col, display) = single_line_field_viewport(text, cursor, 4);
        assert_eq!(display, "aébΩ");
        assert_eq!(visible_cursor_col, 3);
    }

    #[test]
    fn wrapped_editor_rows_wraps_long_line_and_tracks_cursor_segment() {
        let (rows, cursor_row) = wrapped_editor_rows("abcdefghij", 4, 0, 9);
        let displays = rows.iter().map(|row| row.display_text).collect::<Vec<_>>();
        assert_eq!(displays, vec!["abcd", "efgh", "ij"]);
        assert_eq!(cursor_row, 2);
        assert_eq!(rows[2].cursor_col, Some(1));
    }

    #[test]
    fn wrapped_editor_rows_keeps_cursor_at_end_of_exact_boundary_segment() {
        let (rows, cursor_row) = wrapped_editor_rows("abcdefgh", 4, 0, 8);
        assert_eq!(rows.len(), 2);
        assert_eq!(cursor_row, 1);
        assert_eq!(rows[1].display_text, "efgh");
        assert_eq!(rows[1].cursor_col, Some(4));
    }

    #[test]
    fn wrapped_editor_rows_wraps_whole_words_without_hiding_characters() {
        let (rows, _cursor_row) = wrapped_editor_rows("hello world", 7, 0, 0);
        let displays = rows.iter().map(|row| row.display_text).collect::<Vec<_>>();
        assert_eq!(displays, vec!["hello ", "world"]);
    }

    #[test]
    fn cursor_visual_span_inverts_the_character_under_cursor() {
        assert_eq!(
            cursor_visual_span("abcd", 1),
            CursorVisualSpan::InvertedGlyph {
                leading_text: "a",
                glyph: "b",
                trailing_text: "cd",
            }
        );
    }

    #[test]
    fn wrapped_row_activity_marks_all_visual_rows_of_active_logical_line() {
        let text = "hello world\nnext";
        let (rows, _cursor_row) = wrapped_editor_rows(text, 7, 0, 8);
        let active_line = active_line_bounds(text, 8);
        let active_rows = rows
            .iter()
            .map(|row| wrapped_row_is_active(row, active_line))
            .collect::<Vec<_>>();
        assert_eq!(active_rows, vec![true, true, false]);
    }
}
