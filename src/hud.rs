use crate::terminals::{
    append_debug_log, spawn_terminal_presentation, TerminalDisplayMode, TerminalFontState,
    TerminalManager, TerminalPresentationStore, TerminalRuntimeSpawner, TerminalViewState,
};
use bevy::{prelude::*, window::PrimaryWindow};
use bevy_egui::{egui, EguiContexts};
use bevy_vello::{
    prelude::{kurbo, peniko, VelloScene2d},
    vello,
};

#[derive(Resource)]
pub(crate) struct EvaVectorDemoState {
    pub(crate) enabled: bool,
}

impl Default for EvaVectorDemoState {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Component)]
pub(crate) struct EvaVectorDemoMarker;

fn eva_color(r: u8, g: u8, b: u8, a: u8) -> peniko::Color {
    peniko::Color::from_rgba8(r, g, b, a)
}

fn eva_arc(center: Vec2, radius: f64, start: f64, sweep: f64, segments: usize) -> kurbo::BezPath {
    let mut path = kurbo::BezPath::new();
    let steps = segments.max(2);
    for index in 0..=steps {
        let t = index as f64 / steps as f64;
        let angle = start + sweep * t;
        let point = kurbo::Point::new(
            f64::from(center.x) + radius * angle.cos(),
            f64::from(center.y) + radius * angle.sin(),
        );
        if index == 0 {
            path.move_to(point);
        } else {
            path.line_to(point);
        }
    }
    path
}

fn eva_polyline(points: &[Vec2]) -> kurbo::BezPath {
    let mut path = kurbo::BezPath::new();
    for (index, point) in points.iter().enumerate() {
        let point = kurbo::Point::new(f64::from(point.x), f64::from(point.y));
        if index == 0 {
            path.move_to(point);
        } else {
            path.line_to(point);
        }
    }
    path
}

fn eva_polygon(points: &[Vec2]) -> kurbo::BezPath {
    let mut path = eva_polyline(points);
    path.close_path();
    path
}

fn stroke_path(
    scene: &mut vello::Scene,
    width: f64,
    color: peniko::Color,
    path: &impl kurbo::Shape,
) {
    scene.stroke(
        &kurbo::Stroke::new(width),
        kurbo::Affine::IDENTITY,
        color,
        None,
        path,
    );
}

fn fill_path(scene: &mut vello::Scene, color: peniko::Color, path: &impl kurbo::Shape) {
    scene.fill(
        peniko::Fill::NonZero,
        kurbo::Affine::IDENTITY,
        color,
        None,
        path,
    );
}

fn rebuild_eva_vector_demo_scene(scene: &mut VelloScene2d, window: &Window, elapsed: f32) {
    let width = window.width();
    let height = window.height();
    let half_w = width * 0.5;
    let half_h = height * 0.5;
    let orange = eva_color(255, 140, 32, 230);
    let orange_dim = eva_color(255, 140, 32, 92);
    let orange_fill = eva_color(255, 140, 32, 28);
    let green = eva_color(120, 255, 196, 220);
    let green_dim = eva_color(120, 255, 196, 72);
    let red = eva_color(255, 72, 72, 200);
    let red_fill = eva_color(255, 72, 72, 26);

    let mut built = vello::Scene::new();

    let outer = kurbo::Rect::new(
        f64::from(-half_w + 42.0),
        f64::from(-half_h + 84.0),
        f64::from(half_w - 42.0),
        f64::from(half_h - 42.0),
    );
    stroke_path(&mut built, 2.2, orange_dim, &outer);

    let left_bracket = eva_polyline(&[
        Vec2::new(-half_w + 48.0, -half_h + 154.0),
        Vec2::new(-half_w + 148.0, -half_h + 154.0),
        Vec2::new(-half_w + 148.0, -half_h + 112.0),
        Vec2::new(-half_w + 244.0, -half_h + 112.0),
    ]);
    stroke_path(&mut built, 5.0, orange, &left_bracket);

    let right_bracket = eva_polyline(&[
        Vec2::new(half_w - 48.0, -half_h + 154.0),
        Vec2::new(half_w - 148.0, -half_h + 154.0),
        Vec2::new(half_w - 148.0, -half_h + 112.0),
        Vec2::new(half_w - 244.0, -half_h + 112.0),
    ]);
    stroke_path(&mut built, 5.0, orange, &right_bracket);

    for index in 0..14 {
        let y = -half_h + 168.0 + index as f32 * 28.0;
        let width_scale = if index % 3 == 0 { 0.44 } else { 0.34 };
        let path = eva_polyline(&[
            Vec2::new(-half_w + 76.0, y),
            Vec2::new(-half_w + 76.0 + width * width_scale * 0.32, y),
        ]);
        stroke_path(&mut built, 1.2, green_dim, &path);
    }

    let panel_rect = kurbo::RoundedRect::new(
        f64::from(-half_w + 86.0),
        f64::from(half_h - 300.0),
        f64::from(-half_w + 430.0),
        f64::from(half_h - 96.0),
        18.0,
    );
    built.draw_blurred_rounded_rect(
        kurbo::Affine::IDENTITY,
        kurbo::Rect::new(
            f64::from(-half_w + 98.0),
            f64::from(half_h - 288.0),
            f64::from(-half_w + 418.0),
            f64::from(half_h - 108.0),
        ),
        orange_fill,
        18.0,
        10.0,
    );
    stroke_path(&mut built, 2.0, orange, &panel_rect);

    let sweep = ((elapsed * 1.7).sin() * 0.5 + 0.5) * 220.0;
    let sweep_bar = eva_polygon(&[
        Vec2::new(-half_w + 112.0, half_h - 132.0),
        Vec2::new(-half_w + 112.0 + sweep, half_h - 132.0),
        Vec2::new(-half_w + 92.0 + sweep, half_h - 150.0),
        Vec2::new(-half_w + 92.0, half_h - 150.0),
    ]);
    fill_path(&mut built, orange_fill, &sweep_bar);

    for index in 0..6 {
        let y = half_h - 246.0 + index as f32 * 28.0;
        let row = eva_polyline(&[Vec2::new(-half_w + 110.0, y), Vec2::new(-half_w + 392.0, y)]);
        stroke_path(&mut built, 1.1, orange_dim, &row);
    }

    let reticle_center = Vec2::new(half_w * 0.30, -half_h * 0.12);
    for radius in [54.0, 88.0, 126.0, 172.0] {
        stroke_path(
            &mut built,
            if radius < 100.0 { 2.4 } else { 1.4 },
            if radius < 100.0 { orange } else { orange_dim },
            &eva_arc(reticle_center, radius, 0.0, std::f64::consts::TAU, 144),
        );
    }

    for (start, sweep, radius, color) in [
        (-1.9, 0.9, 198.0, red),
        (-0.35, 0.7, 146.0, orange),
        (1.05, 0.82, 110.0, green),
        (2.35, 0.5, 82.0, orange),
    ] {
        let arc = eva_arc(reticle_center, radius, start, sweep, 48);
        stroke_path(&mut built, 7.0, color, &arc);
    }

    let sweep_angle = elapsed * 0.9;
    let sweep_path = eva_polyline(&[
        reticle_center,
        reticle_center + Vec2::new(sweep_angle.cos() * 178.0, sweep_angle.sin() * 178.0),
    ]);
    stroke_path(&mut built, 2.0, green, &sweep_path);

    let crosshair_h = eva_polyline(&[
        reticle_center + Vec2::new(-214.0, 0.0),
        reticle_center + Vec2::new(214.0, 0.0),
    ]);
    let crosshair_v = eva_polyline(&[
        reticle_center + Vec2::new(0.0, -214.0),
        reticle_center + Vec2::new(0.0, 214.0),
    ]);
    stroke_path(&mut built, 1.2, orange_dim, &crosshair_h);
    stroke_path(&mut built, 1.2, orange_dim, &crosshair_v);

    let waveform_origin = Vec2::new(-width * 0.14, height * 0.23);
    let mut waveform_points = Vec::with_capacity(72);
    for index in 0..72 {
        let x = waveform_origin.x + index as f32 * 12.0;
        let normalized = index as f32 / 71.0;
        let envelope = 1.0 - ((normalized - 0.52).abs() * 1.4).clamp(0.0, 1.0);
        let y = waveform_origin.y
            + (elapsed * 2.8 + normalized * 10.0).sin() * 26.0 * envelope
            + (elapsed * 0.8 + normalized * 18.0).cos() * 8.0;
        waveform_points.push(Vec2::new(x, y));
    }
    let waveform = eva_polyline(&waveform_points);
    stroke_path(&mut built, 2.0, green, &waveform);

    let warning_band = eva_polygon(&[
        Vec2::new(width * 0.08, height * 0.28),
        Vec2::new(width * 0.32, height * 0.18),
        Vec2::new(width * 0.34, height * 0.23),
        Vec2::new(width * 0.10, height * 0.33),
    ]);
    fill_path(&mut built, red_fill, &warning_band);
    stroke_path(&mut built, 2.6, red, &warning_band);

    for index in 0..7 {
        let offset = index as f32 * 34.0;
        let slash = eva_polyline(&[
            Vec2::new(width * 0.11 + offset, height * 0.33),
            Vec2::new(width * 0.16 + offset, height * 0.20),
        ]);
        stroke_path(&mut built, 3.0, red, &slash);
    }

    let lower_frame = eva_polyline(&[
        Vec2::new(-width * 0.44, height * 0.34),
        Vec2::new(-width * 0.10, height * 0.34),
        Vec2::new(-width * 0.08, height * 0.30),
        Vec2::new(width * 0.06, height * 0.30),
    ]);
    stroke_path(&mut built, 4.0, orange, &lower_frame);

    *scene = VelloScene2d::from(built);
}

pub(crate) fn sync_eva_vector_demo(
    state: Res<EvaVectorDemoState>,
    time: Res<Time>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    mut scene: Single<&mut VelloScene2d, With<EvaVectorDemoMarker>>,
) {
    if !state.enabled {
        **scene = VelloScene2d::from(vello::Scene::new());
        return;
    }

    rebuild_eva_vector_demo_scene(&mut scene, &primary_window, time.elapsed_secs());
}

#[allow(
    clippy::too_many_arguments,
    reason = "ui overlay needs app resources, terminal state, and bevy contexts together"
)]
pub(crate) fn ui_overlay(
    mut contexts: EguiContexts,
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut terminal_manager: ResMut<TerminalManager>,
    mut presentation_store: ResMut<TerminalPresentationStore>,
    runtime_spawner: Res<TerminalRuntimeSpawner>,
    font_state: Res<TerminalFontState>,
    mut view_state: ResMut<TerminalViewState>,
    mut eva_demo: ResMut<EvaVectorDemoState>,
) -> bevy::ecs::error::Result {
    let ctx = contexts.ctx_mut()?;

    egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
        ui.horizontal_wrapped(|ui| {
            let active_status = terminal_manager
                .active_snapshot()
                .map(|snapshot| snapshot.runtime.status.as_str())
                .unwrap_or("no active terminal");
            let active_error = terminal_manager
                .active_snapshot()
                .and_then(|snapshot| snapshot.runtime.last_error.as_deref());
            let debug = terminal_manager.active_debug_stats();

            ui.label(egui::RichText::new("neozeus").strong());
            ui.separator();
            ui.label(active_status);
            ui.separator();
            ui.label(format!(
                "terms {} · active {}",
                terminal_manager.terminal_ids().len(),
                terminal_manager.active_id().map(|id| id.0).unwrap_or_default(),
            ));
            ui.separator();
            ui.label(format!(
                "keys {} · queued {} · wr {} · rd {} · sent {} · applied {} · drop {} · rows {} · compose {}us",
                debug.key_events_seen,
                debug.commands_queued,
                debug.pty_bytes_written,
                debug.pty_bytes_read,
                debug.snapshots_sent,
                debug.snapshots_applied,
                debug.updates_dropped,
                debug.dirty_rows_uploaded,
                debug.compose_micros,
            ));
            ui.separator();
            if !debug.last_key.is_empty() {
                ui.label(format!("last key {}", debug.last_key));
                ui.separator();
            }
            if !debug.last_command.is_empty() {
                ui.label(format!("last cmd {}", debug.last_command));
                ui.separator();
            }
            if let Some(error) = active_error {
                ui.colored_label(egui::Color32::LIGHT_RED, format!("last err {error}"));
                ui.separator();
            } else if !debug.last_error.is_empty() {
                ui.colored_label(
                    egui::Color32::LIGHT_RED,
                    format!("last err {}", debug.last_error),
                );
                ui.separator();
            }
            match font_state.report.as_ref() {
                Some(Ok(report)) => {
                    ui.label(format!("font: {}", report.primary.family));
                    ui.separator();
                    ui.label(format!("requested: {}", report.requested_family));
                    ui.separator();
                }
                Some(Err(error)) => {
                    ui.colored_label(egui::Color32::LIGHT_RED, format!("font error: {error}"));
                    ui.separator();
                }
                None => {
                    ui.label("font: loading");
                    ui.separator();
                }
            }
            ui.label(format!("zoom {:.2}", view_state.distance));
            ui.separator();
            ui.label(format!(
                "offset {:.2},{:.2}",
                view_state.offset.x, view_state.offset.y
            ));
            ui.separator();
            let display_mode = presentation_store
                .active_display_mode(terminal_manager.active_id())
                .unwrap_or(TerminalDisplayMode::Smooth);
            let pixel_perfect = display_mode == TerminalDisplayMode::PixelPerfect;
            if ui
                .selectable_label(pixel_perfect, "pixel perfect HUD")
                .clicked()
            {
                presentation_store.toggle_active_display_mode(terminal_manager.active_id());
            }
            ui.separator();
            ui.label("MMB drag: scrollback · Shift+MMB drag: pan · Shift+wheel: zoom");
            ui.separator();
            ui.checkbox(&mut eva_demo.enabled, "EVA vector demo");
            ui.separator();
            if ui.button("new terminal").clicked() {
                let bridge = runtime_spawner.spawn();
                let (terminal_id, slot) = terminal_manager.create_terminal_with_slot(bridge);
                spawn_terminal_presentation(
                    &mut commands,
                    &mut images,
                    &mut presentation_store,
                    terminal_id,
                    slot,
                );
                append_debug_log(format!("spawned terminal {}", terminal_id.0));
            }
            for terminal_id in terminal_manager.terminal_ids().to_vec() {
                let selected = terminal_manager.active_id() == Some(terminal_id);
                if ui
                    .selectable_label(selected, format!("t{}", terminal_id.0))
                    .clicked()
                {
                    terminal_manager.focus_terminal(terminal_id);
                }
            }
            ui.separator();
            if ui.button("reset view").clicked() {
                view_state.distance = 10.0;
                view_state.offset = Vec2::ZERO;
            }
            for command in ["pwd", "ls", "clear", "btop", "tmux"] {
                if ui.button(command).clicked() {
                    append_debug_log(format!("ui button clicked: {command}"));
                    if let Some(bridge) = terminal_manager.active_bridge() {
                        bridge.send(crate::terminals::TerminalCommand::SendCommand(command.into()));
                    }
                }
            }
        });
    });

    Ok(())
}
