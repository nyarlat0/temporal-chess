use crate::{
    controller::{GameUi, side_name},
    icons::{Icon, icon_button},
    piece_art::PieceArt,
    theme::*,
    view::{ShellState, piece_name},
};
use bevy_egui::egui::{self, Vec2};

pub fn draw(ctx: &egui::Context, game: &mut GameUi, shell: &mut ShellState, art: &PieceArt) {
    #[cfg(target_arch = "wasm32")]
    let browser_import = mobile_import(ctx, game).unwrap_or(false);
    #[cfg(not(target_arch = "wasm32"))]
    let browser_import = false;
    if game.undo_in_progress() {
        egui::Modal::new("undo_in_progress".into()).show(ctx, |ui| {
            ui.heading(crate::tr!("Отмена хода", "Undo move"));
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(crate::tr!(
                    "Восстанавливаем партию из начальной позиции…",
                    "Replaying the game from the initial position…"
                ));
            });
        });
    }
    if game.import_open && !browser_import {
        egui::Modal::new("import_game".into()).show(ctx, |ui| {
            ui.set_width(560.0_f32.min(ctx.viewport_rect().width() - 60.0));
            ui.heading(crate::tr!("Загрузить партию", "Import game"));
            ui.colored_label(
                SUBTLE,
                crate::tr!(
                    "Вставьте запись партии · Ctrl+V",
                    "Paste a game record · Ctrl+V"
                ),
            );
            let loading = game.replay_job.is_some();
            ui.add_enabled_ui(!loading, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(340.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut game.import_text)
                                .font(egui::TextStyle::Monospace)
                                .desired_width(f32::INFINITY)
                                .desired_rows(13)
                                .hint_text("NOLAN-CHESS 1\nmove 0 12:0:0 e2 e4 + normal"),
                        );
                    });
            });
            if let Some(error) = &game.import_error {
                ui.colored_label(CAPTURE, error.text());
            }
            ui.horizontal(|ui| {
                if icon_button(
                    ui,
                    Icon::Check,
                    crate::tr!("Начать из записи", "Play from record"),
                    false,
                    !loading && !game.import_text.trim().is_empty(),
                )
                .clicked()
                {
                    game.begin_import();
                }
                if icon_button(
                    ui,
                    Icon::Close,
                    crate::tr!("Отмена", "Cancel"),
                    false,
                    !loading,
                )
                .clicked()
                {
                    game.import_open = false;
                }
                if loading {
                    ui.spinner();
                    ui.label(crate::tr!("Проверка истории…", "Validating history…"));
                }
            });
        });
    }
    if !game.promotion.is_empty() {
        egui::Modal::new("promotion".into()).show(ctx, |ui| {
            ui.heading(crate::tr!("Превращение пешки", "Pawn promotion"));
            ui.horizontal(|ui| {
                for mv in game.promotion.clone() {
                    if let Some(kind) = mv.promotion() {
                        let (rect, response) =
                            ui.allocate_exact_size(Vec2::splat(76.0), egui::Sense::click());
                        ui.painter().rect_filled(
                            rect,
                            10.0,
                            if response.hovered() { SURFACE } else { PANEL },
                        );
                        art.paint(ui.painter(), kind, game.engine.turn(), rect, 1.0);
                        if crate::input::clicked(&response.on_hover_text(piece_name(kind))) {
                            game.pending_move = Some(mv);
                        }
                    }
                }
            });
            if icon_button(ui, Icon::Close, crate::tr!("Отмена", "Cancel"), false, true).clicked()
            {
                game.promotion.clear();
                game.move_wait_until = None;
            }
        });
    }
    if !game.overlapping.is_empty() {
        egui::Modal::new("overlapping".into()).show(ctx, |ui| {
            ui.heading(crate::tr!("Выберите воплощение", "Choose an incarnation"));
            for piece in game.overlapping.clone() {
                let response = ui
                    .horizontal(|ui| {
                        let (rect, response) =
                            ui.allocate_exact_size(Vec2::splat(56.0), egui::Sense::click());
                        ui.painter().rect_filled(
                            rect,
                            10.0,
                            if response.hovered() { SURFACE } else { PANEL },
                        );
                        art.paint_styled(
                            ui.painter(),
                            piece.kind,
                            piece.color,
                            rect,
                            1.0,
                            crate::piece_art::PieceStyle {
                                inverted: piece.inverted,
                                ..Default::default()
                            },
                        );
                        ui.vertical(|ui| {
                            ui.label(format!(
                                "{} · {}",
                                side_name(piece.color),
                                piece_name(piece.kind)
                            ));
                            ui.colored_label(
                                SUBTLE,
                                crate::tr_format!(
                                    "#{} · возраст {} · петля {}",
                                    "#{} · age {} · loop {}",
                                    piece.origin,
                                    piece.age,
                                    piece.loop_turn
                                ),
                            );
                        });
                        response
                    })
                    .inner;
                if crate::input::clicked(
                    &response.on_hover_text(crate::tr!("Выбрать воплощение", "Select incarnation")),
                ) {
                    game.choose(piece);
                }
            }
            if icon_button(ui, Icon::Close, crate::tr!("Отмена", "Cancel"), false, true).clicked()
            {
                game.overlapping.clear();
            }
        });
    }
    if game.confirm_restart {
        egui::Modal::new("restart".into()).show(ctx, |ui| {
            ui.set_width(460.0_f32.min(ctx.viewport_rect().width() - 40.0));
            // The mode may change from a short local form to a longer online
            // form. Let the scroll area grow past the previous frame's height.
            ui.set_max_height((ctx.viewport_rect().height() - 40.0).max(0.0));
            ui.heading(crate::tr!("Новая партия", "New game"));
            egui::ScrollArea::vertical()
                .id_salt("new_game_options")
                .max_height((ctx.viewport_rect().height() - 150.0).max(100.0))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    crate::tempfish::creation_settings(
                        ui,
                        &mut game.new_game_bot,
                        &mut game.new_game_online,
                    );
                    if game.new_game_online {
                        crate::multiplayer::creation_panel(ui, game);
                    } else {
                        crate::view::starting_position_controls(ui, &mut game.new_game_start);
                        crate::view::rule_controls(ui, &mut game.new_game_rules);
                    }
                });
            let start = ui
                .horizontal(|ui| {
                    let start = !game.new_game_online
                        && icon_button(
                            ui,
                            Icon::Check,
                            if game.new_game_start
                                == temporal_chess_engine::model::StartingPosition::Lab
                            {
                                crate::tr!("Открыть Lab", "Open Lab")
                            } else {
                                crate::tr!("Начать новую партию", "Start new game")
                            },
                            false,
                            game.new_game_start.is_valid(),
                        )
                        .clicked();
                    if icon_button(ui, Icon::Close, crate::tr!("Закрыть", "Close"), false, true)
                        .clicked()
                    {
                        game.confirm_restart = false;
                    }
                    start
                })
                .inner;
            if start {
                game.request_new_local();
            }
        });
    }
    if shell.help_open
        && let Some(resume) = crate::help::show(
            ctx,
            game.engine.history().rules(),
            &mut shell.help_open,
            art,
        )
    {
        shell.help_open = false;
        crate::tutorial::replay(game, resume);
        ctx.request_repaint();
    }
}

#[cfg(target_arch = "wasm32")]
fn mobile_import(ctx: &egui::Context, game: &mut GameUi) -> Option<bool> {
    use wasm_bindgen::JsCast;
    use web_sys::{HtmlDialogElement, HtmlFieldSetElement, HtmlTextAreaElement};

    let document = web_sys::window()?.document()?;
    let dialog = document
        .get_element_by_id("mobile-import")?
        .dyn_into::<HtmlDialogElement>()
        .ok()?;
    if !game.import_open {
        if dialog.open() {
            dialog.close();
        }
        return Some(false);
    }
    if !dialog.open() && !crate::view::mobile_layout(ctx) {
        return Some(false);
    }

    let fieldset = document
        .get_element_by_id("import-fields")?
        .dyn_into::<HtmlFieldSetElement>()
        .ok()?;
    let textarea = document
        .get_element_by_id("import-record")?
        .dyn_into::<HtmlTextAreaElement>()
        .ok()?;
    let status = document.get_element_by_id("import-status")?;
    if !dialog.open() {
        textarea.set_value(&game.import_text);
        let _ = dialog.remove_attribute("data-action");
        dialog.show_modal().ok()?;
    }
    if let Some(action) = dialog.get_attribute("data-action") {
        let _ = dialog.remove_attribute("data-action");
        if game.replay_job.is_none() {
            game.import_text = textarea.value();
            match action.as_str() {
                "submit" if !game.import_text.trim().is_empty() => game.begin_import(),
                "cancel" => {
                    game.import_open = false;
                    dialog.close();
                    ctx.request_repaint();
                    return Some(true);
                }
                _ => {}
            }
        }
    }
    let loading = game.replay_job.is_some();
    if fieldset.disabled() != loading {
        fieldset.set_disabled(loading);
    }
    let error = game.import_error.as_ref().map(|error| error.text());
    let message = if loading {
        crate::tr!("Проверка истории…", "Validating history…")
    } else {
        error.unwrap_or("")
    };
    if status.text_content().as_deref() != Some(message) {
        status.set_text_content(Some(message));
    }
    status
        .set_attribute("data-error", if error.is_some() { "true" } else { "false" })
        .ok()?;
    if loading {
        // Keep polling for the replay result while the canvas lacks focus.
        ctx.request_repaint();
    }
    Some(true)
}
