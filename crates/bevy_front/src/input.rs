//! Global shortcuts and the right-button zoom gesture. Process input once even
//! if egui requests another layout pass in the same frame.
use crate::{controller::GameUi, view::ShellState};
use bevy_egui::egui::{self, Key, PointerButton};
use temporal_chess_engine::engine::CalculationStatus;

#[derive(Clone, Default)]
pub struct TouchNavigation {
    fingers: std::collections::BTreeMap<egui::TouchId, egui::Pos2>,
    origin: Option<egui::Pos2>,
    captured: bool,
    moved: bool,
    pub suppress_click: bool,
    pub scroll: f32,
    pub zoom: f32,
    pub down: bool,
    pub cancel_coast: bool,
    pinched: bool,
}

impl TouchNavigation {
    fn update(&mut self, events: &[egui::Event], rect: egui::Rect, enabled: bool) {
        self.scroll = 0.0;
        self.zoom = 1.0;
        self.suppress_click = self.moved;
        self.cancel_coast = !enabled || self.pinched;
        for event in events {
            let egui::Event::Touch { id, phase, pos, .. } = event else {
                continue;
            };
            let before = self.geometry();
            if self.fingers.is_empty() && *phase == egui::TouchPhase::Start {
                self.captured = enabled && rect.contains(*pos);
                self.origin = Some(*pos);
                self.moved = false;
                self.suppress_click = false;
                self.pinched = false;
                self.cancel_coast = !enabled;
            }
            match phase {
                egui::TouchPhase::Start | egui::TouchPhase::Move => {
                    self.fingers.insert(*id, *pos);
                }
                egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                    self.fingers.remove(id);
                }
            }
            if !enabled {
                self.captured = false;
            }
            if self.captured && *phase == egui::TouchPhase::Move {
                if self.fingers.len() >= 2 || self.origin.is_some_and(|p| p.distance(*pos) > 8.0) {
                    self.moved = true;
                }
                if self.moved
                    && let (Some((center, distance)), Some((old, old_distance))) =
                        (self.geometry(), before)
                {
                    self.scroll += center.y - old.y;
                    if self.fingers.len() >= 2 && old_distance > 1.0 {
                        self.zoom *= distance / old_distance;
                    }
                }
            }
            if self.captured && self.fingers.len() >= 2 {
                self.moved = true;
                self.pinched = true;
            }
            self.cancel_coast |= *phase == egui::TouchPhase::Cancel || self.pinched;
            self.suppress_click |= self.moved;
        }
        // Retain suppression through the release frame to prevent a drag becoming a move.
        if self.fingers.is_empty() {
            self.moved = false;
            self.origin = None;
        }
        self.down = self.captured && !self.fingers.is_empty();
    }

    fn geometry(&self) -> Option<(egui::Pos2, f32)> {
        let mut points = self.fingers.values();
        let a = *points.next()?;
        Some(
            points
                .next()
                .map_or((a, 0.0), |b| (a.lerp(*b, 0.5), a.distance(*b))),
        )
    }
}

pub fn handle(ctx: &egui::Context, game: &mut GameUi, shell: &mut ShellState) {
    let frame = ctx.cumulative_frame_nr();
    if shell.input_frame == Some(frame) {
        return;
    }
    shell.input_frame = Some(frame);
    if ctx.input(|i| i.pointer.any_pressed()) {
        game.viewport.wheel.stop();
        game.viewport.touch_inertia.stop();
    }
    let modal = game.import_open
        || game.confirm_restart
        || game.replay_job.is_some()
        || !game.promotion.is_empty()
        || !game.overlapping.is_empty()
        || shell.help_open
        || shell.settings_open;
    let touch_enabled = !modal && !game.playback_busy() && !game.facing.animating();
    if !ctx.input(|i| i.focused) {
        shell.touch = TouchNavigation::default();
        game.viewport.touch_inertia.stop();
    }
    ctx.input(|i| {
        shell
            .touch
            .update(&i.events, shell.ribbon_rect, touch_enabled)
    });
    ctx.data_mut(|d| {
        d.insert_temp(
            egui::Id::new("touch_navigation_drag"),
            shell.touch.suppress_click,
        )
    });
    if touch_enabled && shell.touch.zoom != 1.0 {
        let (visible_cell, _, _) = crate::board_view::ribbon_layout(
            shell.ribbon_rect.width(),
            game.cell_size,
            crate::view::mobile_layout(ctx),
        );
        game.cell_size = (visible_cell * shell.touch.zoom).clamp(9.6, 90.0);
        game.viewport.wheel.stop();
        game.viewport.scroll = None;
        game.jump_to = None;
    }
    let (pressed, released, down, right_click, escape, toggle) = ctx.input(|i| {
        (
            i.pointer.button_pressed(PointerButton::Secondary),
            i.pointer.button_released(PointerButton::Secondary),
            i.pointer.button_down(PointerButton::Secondary),
            i.pointer.button_clicked(PointerButton::Secondary),
            i.key_pressed(Key::Escape),
            i.pointer.button_clicked(PointerButton::Middle),
        )
    });
    if escape && !modal && (game.history_playback || game.replaying_move) {
        game.reset_playback();
        game.viewport.scroll = None;
        game.jump_to = None;
    }
    if escape && modal && game.replay_job.is_none() {
        game.import_open = false;
        game.confirm_restart = false;
        game.promotion.clear();
        game.move_wait_until = None;
        game.overlapping.clear();
        shell.help_open = false;
    }
    if pressed {
        shell.zoom_gesture = false;
    }
    let wheel_delta = ctx.input(wheel_delta);
    let shift = ctx.input(|i| i.modifiers.shift);
    if !down && !shift && wheel_delta != 0.0 {
        shell.suppress_wheel = false;
    }
    if !modal && !game.playback_busy() && !game.facing.animating() && (down || shift) {
        let over_boards = ctx.pointer_hover_pos().is_some_and(|pos| {
            shell.ribbon_rect.contains(pos)
                && ctx
                    .layer_id_at(pos)
                    .is_none_or(|layer| layer.order == egui::Order::Background)
        });
        // Panels use the background layer; floating inspectors and dialogs
        // must not zoom the board underneath them.
        if over_boards && wheel_delta != 0.0 {
            shell.zoom_gesture = true;
            shell.suppress_wheel = true;
            game.cell_size = (game.cell_size * (wheel_delta * 0.0025).exp()).clamp(9.6, 90.0);
            game.viewport.scroll = None;
            game.viewport.wheel.stop();
            game.jump_to = None;
        }
    }
    if shell.suppress_wheel {
        ctx.input_mut(|i| {
            i.smooth_scroll_delta = egui::Vec2::ZERO;
            i.events
                .retain(|event| !matches!(event, egui::Event::MouseWheel { .. }));
        });
        if !ctx.input(|i| i.is_scrolling()) {
            shell.suppress_wheel = false;
        }
    }
    if !modal {
        if (escape || (right_click && !shell.zoom_gesture))
            && (game.tutorial.is_none() || !game.inspect_mode && !game.worldline_mode)
        {
            game.clear_selection();
        }
        if toggle
            && !ctx.input(|i| {
                i.pointer
                    .interact_pos()
                    .is_some_and(|pos| shell.ribbon_rect.contains(pos))
            })
        {
            game.toggle_direction();
        }
    }
    if (pressed || escape) && game.tutorial.is_none() {
        if game.inspect_mode {
            game.inspect_mode = false;
            game.clear_selection();
        }
        game.worldline_mode = false;
        game.worldline_selected = None;
        game.worldline_state = None;
        game.worldline_transition = None;
    }
    if released {
        shell.zoom_gesture = false;
    }
    if modal || game.playback_busy() || game.facing.animating() || ctx.egui_wants_keyboard_input() {
        return;
    }
    if game.tutorial.is_some() {
        return;
    }
    ctx.input(|i| {
        if i.key_pressed(Key::I) && !i.modifiers.command && !i.modifiers.ctrl {
            game.toggle_inspector();
        }
        if i.key_pressed(Key::C)
            && !i.modifiers.command
            && !i.modifiers.ctrl
            && matches!(
                game.engine.calculation_status(),
                CalculationStatus::Paused { .. }
            )
        {
            game.continue_calculation();
        }
        if i.key_pressed(Key::Z) && i.modifiers.command {
            game.undo_last_move();
        }
    });
}

/// A layout retry must not execute a button action or board click twice.
pub fn clicked(response: &egui::Response) -> bool {
    clicked_by(response, PointerButton::Primary)
}

pub fn clicked_by(response: &egui::Response, button: PointerButton) -> bool {
    if response.ctx.data(|d| {
        d.get_temp::<bool>(egui::Id::new("touch_navigation_drag"))
            .unwrap_or(false)
    }) {
        return false;
    }
    if !response.clicked_by(button) {
        return false;
    }
    let frame = response.ctx.cumulative_frame_nr();
    let key = response.id.with(("handled_click_frame", button as u8));
    response.ctx.data_mut(|data| {
        if data.get_temp::<u64>(key) == Some(frame) {
            return false;
        }
        data.insert_temp(key, frame);
        true
    })
}

pub fn wheel_delta(i: &egui::InputState) -> f32 {
    i.events
        .iter()
        .filter_map(|event| {
            if let egui::Event::MouseWheel { unit, delta, .. } = event {
                Some(
                    (if delta.y != 0.0 { delta.y } else { delta.x })
                        * match unit {
                            egui::MouseWheelUnit::Point => 1.0,
                            egui::MouseWheelUnit::Line => 32.0,
                            egui::MouseWheelUnit::Page => 240.0,
                        },
                )
            } else {
                None
            }
        })
        .sum::<f32>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn right_press_exits_piece_inspection_and_clears_the_piece() {
        let ctx = egui::Context::default();
        let mut game = GameUi {
            inspect_mode: true,
            ..Default::default()
        };
        game.click_square(0, temporal_chess_engine::model::Sq(0, 6));
        assert!(game.inspected.is_some());
        let before = game.engine.clone();
        let mut shell = ShellState::default();
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::PointerButton {
                    pos: egui::pos2(50.0, 50.0),
                    button: PointerButton::Secondary,
                    pressed: true,
                    modifiers: Default::default(),
                }],
                ..Default::default()
            },
            |ui| handle(ui.ctx(), &mut game, &mut shell),
        );
        output.textures_delta.clear();
        assert!(!game.inspect_mode && game.inspected.is_none());
        assert_eq!(game.engine, before);
    }

    fn touch(id: u64, phase: egui::TouchPhase, x: f32, y: f32) -> egui::Event {
        egui::Event::Touch {
            device_id: egui::TouchDeviceId(0),
            id: egui::TouchId(id),
            phase,
            pos: egui::pos2(x, y),
            force: None,
        }
    }

    #[test]
    fn touch_pan_pinch_release_and_ui_boundaries() {
        use egui::TouchPhase::*;
        let rect = egui::Rect::from_min_max(egui::pos2(0.0, 100.0), egui::pos2(375.0, 800.0));
        let mut nav = TouchNavigation::default();
        nav.update(&[touch(0, Start, 100.0, 200.0)], rect, true);
        nav.update(&[touch(0, End, 100.0, 200.0)], rect, true);
        assert!(!nav.suppress_click, "tapping must still select pieces");
        nav.update(&[touch(0, Start, 100.0, 300.0)], rect, true);
        nav.update(&[touch(0, Move, 100.0, 200.0)], rect, true);
        assert_eq!(nav.scroll, -100.0);
        assert!(nav.suppress_click);
        nav.update(&[touch(1, Start, 200.0, 200.0)], rect, true);
        nav.update(&[touch(1, Move, 300.0, 200.0)], rect, true);
        assert_eq!(nav.zoom, 2.0);
        nav.update(
            &[touch(1, Cancel, 300.0, 200.0), touch(0, End, 100.0, 200.0)],
            rect,
            true,
        );
        assert!(nav.suppress_click, "pinch release must not execute a move");
        nav.update(&[], rect, true);
        assert!(!nav.suppress_click);
        nav.update(
            &[touch(0, Start, 100.0, 40.0), touch(0, Move, 100.0, 150.0)],
            rect,
            true,
        );
        assert_eq!(nav.scroll, 0.0, "a gesture starting in UI stays in UI");
        nav.update(
            &[
                touch(0, End, 100.0, 150.0),
                touch(0, Start, 100.0, 200.0),
                touch(0, Move, 100.0, 300.0),
            ],
            rect,
            false,
        );
        assert_eq!(nav.scroll, 0.0);
    }

    #[test]
    fn shift_wheel_reaches_fifteen_percent_and_right_press_exits_worldline() {
        let ctx = egui::Context::default();
        let mut game = GameUi {
            worldline_mode: true,
            ..Default::default()
        };
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::splat(500.0));
        let mut shell = ShellState::default();
        shell.ribbon_rect = rect;
        let pos = rect.center();
        let modifiers = egui::Modifiers {
            shift: true,
            ..Default::default()
        };
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(rect),
                events: vec![
                    egui::Event::ModifiersChanged(modifiers),
                    egui::Event::PointerMoved(pos),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        phase: egui::TouchPhase::Move,
                        delta: egui::Vec2::new(0.0, -10000.0),
                        modifiers,
                    },
                ],
                ..Default::default()
            },
            |ui| handle(ui.ctx(), &mut game, &mut shell),
        );
        output.textures_delta.clear();
        assert!((game.cell_size / 64.0 - 0.15).abs() < 0.0001);
        assert!(game.worldline_mode);
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(rect),
                events: vec![egui::Event::PointerButton {
                    pos,
                    button: PointerButton::Secondary,
                    pressed: true,
                    modifiers: Default::default(),
                }],
                ..Default::default()
            },
            |ui| handle(ui.ctx(), &mut game, &mut shell),
        );
        output.textures_delta.clear();
        assert!(!game.worldline_mode);
    }
}
