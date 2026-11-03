//! Global shortcuts and the right-button zoom gesture. Process input once even
//! if egui requests another layout pass in the same frame.
use crate::{controller::GameUi, view::ShellState};
use bevy_egui::egui::{self, Key, PointerButton};
use temporal_chess_engine::engine::CalculationStatus;

pub fn handle(ctx: &egui::Context, game: &mut GameUi, shell: &mut ShellState) {
    let frame = ctx.cumulative_frame_nr();
    if shell.input_frame == Some(frame) {
        return;
    }
    shell.input_frame = Some(frame);
    let modal = game.import_open
        || game.confirm_restart
        || game.replay_job.is_some()
        || !game.promotion.is_empty()
        || !game.overlapping.is_empty()
        || shell.help_open;
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
    if escape && modal && game.replay_job.is_none() {
        game.import_open = false;
        game.confirm_restart = false;
        game.promotion.clear();
        game.overlapping.clear();
        shell.help_open = false;
    }
    if pressed {
        shell.zoom_gesture = false;
    }
    let wheel_delta = ctx.input(|i| {
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
    });
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
            game.jump_to = None;
        }
    }
    if shell.suppress_wheel {
        ctx.input_mut(|i| {
            i.smooth_scroll_delta = egui::Vec2::ZERO;
        });
        if !ctx.input(|i| i.is_scrolling()) {
            shell.suppress_wheel = false;
        }
    }
    if !modal {
        if escape || (right_click && !shell.zoom_gesture) {
            game.clear_selection();
        }
        if toggle {
            game.toggle_direction();
        }
    }
    if pressed || escape {
        game.worldline_mode = false;
        game.worldline_selected = None;
    }
    if released {
        shell.zoom_gesture = false;
    }
    if modal || game.playback_busy() || game.facing.animating() || ctx.egui_wants_keyboard_input() {
        return;
    }
    ctx.input(|i| {
        if i.key_pressed(Key::I) && !i.modifiers.command && !i.modifiers.ctrl {
            game.clear_selection();
            game.inspect_mode = !game.inspect_mode;
            game.worldline_mode = false;
            game.worldline_selected = None;
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
    if !response.clicked() {
        return false;
    }
    let frame = response.ctx.cumulative_frame_nr();
    let key = response.id.with("handled_click_frame");
    response.ctx.data_mut(|data| {
        if data.get_temp::<u64>(key) == Some(frame) {
            return false;
        }
        data.insert_temp(key, frame);
        true
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
