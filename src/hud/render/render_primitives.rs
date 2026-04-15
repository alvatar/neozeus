use super::*;
use std::env;

pub(crate) struct HudColors;

impl HudColors {
    pub(crate) const FRAME: peniko::Color = peniko::Color::from_rgba8(7, 7, 7, 255);
    pub(crate) const TITLE: peniko::Color = peniko::Color::from_rgba8(7, 7, 7, 255);
    pub(crate) const BORDER: peniko::Color = peniko::Color::from_rgba8(57, 26, 6, 255);
    pub(crate) const TEXT: peniko::Color = peniko::Color::from_rgba8(238, 96, 2, 255);
    pub(crate) const TEXT_MUTED: peniko::Color = peniko::Color::from_rgba8(216, 196, 162, 255);
    pub(crate) const BUTTON: peniko::Color = peniko::Color::from_rgba8(26, 26, 26, 255);
    pub(crate) const BUTTON_BORDER: peniko::Color = peniko::Color::from_rgba8(57, 26, 6, 255);
    pub(crate) const ROW_HOVERED: peniko::Color = peniko::Color::from_rgba8(44, 32, 24, 255);
    pub(crate) const ROW_FOCUSED: peniko::Color = peniko::Color::from_rgba8(44, 32, 24, 255);
    pub(crate) const MESSAGE_BOX: peniko::Color = peniko::Color::from_rgba8(0, 0, 0, 255);
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
pub(super) fn hud_rect_to_scene(window: &Window, rect: HudRect) -> Rect {
    let (x0, y0) = hud_to_scene(window, Vec2::new(rect.x, rect.y));
    let (x1, y1) = hud_to_scene(window, Vec2::new(rect.x + rect.w, rect.y + rect.h));
    Rect::new(x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1))
}

pub(crate) struct HudPainter<'scene, 'res> {
    pub(super) scene: &'scene mut vello::Scene,
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
    pub(crate) agent_catalog: Option<&'a crate::agents::AgentCatalog>,
    pub(crate) aegis_policy: &'a crate::aegis::AegisPolicyStore,
}

/// Logs a low-level color-presence diagnostic for HUD draw data when explicitly requested.
///
/// This is a debugging hook for color-conversion issues: it inspects encoded scene words for known
/// orange/yellow values and writes the result to the terminal debug log.
pub(super) fn log_hud_draw_colors_if_requested(scene: &vello::Scene) {
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
