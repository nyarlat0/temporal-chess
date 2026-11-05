use crate::{
    controller::{GameUi, side_name},
    icons::{Icon, icon_button},
    piece_art::PieceArt,
    theme::*,
    view::{ShellState, piece_name},
};
use bevy_egui::egui::{self, Vec2};

pub fn inspector(ui: &mut egui::Ui, game: &GameUi) {
    let Some(id) = game.inspected else {
        return;
    };
    let Some(state) = game.engine.piece_state(id) else {
        ui.label(crate::tr!(
            "Состояния больше нет в истории.",
            "This state is no longer in the history."
        ));
        return;
    };
    let line = &game.engine.history().worldlines()[id.piece.wid];
    ui.label(crate::tr_format!(
        "Фигура #{} · {} состояний",
        "Piece #{} · {} states",
        id.piece.wid,
        line.len()
    ));
    egui::CollapsingHeader::new(crate::tr!("Все поля состояния", "All state fields"))
        .default_open(true)
        .show(ui, |ui| {
            ui.monospace(format!("{state:#?}"));
        });
    egui::CollapsingHeader::new(crate::tr!("Причинные разрывы", "Causal gaps")).show(ui, |ui| {
        ui.monospace(format!("{:#?}", line.gaps()));
    });
    egui::CollapsingHeader::new(crate::tr!("Записанные ходы", "Recorded moves")).show(ui, |ui| {
        for mv in game
            .engine
            .history()
            .moves()
            .values()
            .filter(|mv| mv.pid.wid == id.piece.wid)
        {
            ui.monospace(format!("{mv:#?}"));
        }
    });
    egui::CollapsingHeader::new(crate::tr!("Мировая линия", "Worldline")).show(ui, |ui| {
        for (age, state) in line.iter().enumerate() {
            egui::CollapsingHeader::new(format!(
                "age={age} · t={} · {}",
                state.t,
                if state.alive {
                    crate::tr!("жива", "alive")
                } else {
                    crate::tr!("мертва", "dead")
                }
            ))
            .id_salt((id.piece.wid, age))
            .show(ui, |ui| {
                ui.monospace(format!("{state:#?}"));
            });
        }
    });
    ui.separator();
}

pub fn draw(ctx: &egui::Context, game: &mut GameUi, shell: &mut ShellState, art: &PieceArt) {
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
    if game.import_open {
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
            ui.heading(crate::tr!("Начать новую партию?", "Start a new game?"));
            ui.label(crate::tr!(
                "Текущая партия будет сброшена.",
                "The current game will be reset."
            ));
            crate::tempfish::settings(ui, &mut game.new_game_bot);
            crate::view::starting_position_controls(ui, &mut game.new_game_start);
            crate::view::rule_controls(ui, &mut game.new_game_rules);
            ui.horizontal(|ui| {
                if icon_button(
                    ui,
                    Icon::Check,
                    crate::tr!("Начать новую партию", "Start new game"),
                    false,
                    game.new_game_start.is_valid(),
                )
                .clicked()
                {
                    *game = GameUi {
                        cell_size: game.cell_size,
                        local_auto_flip: game.local_auto_flip,
                        bot: crate::tempfish::BotState {
                            settings: game.new_game_bot,
                            ..Default::default()
                        },
                        new_game_bot: game.new_game_bot,
                        ..GameUi::with_start(game.new_game_rules, game.new_game_start)
                    };
                }
                if icon_button(ui, Icon::Close, crate::tr!("Отмена", "Cancel"), false, true)
                    .clicked()
                {
                    game.confirm_restart = false;
                }
            });
        });
    }
    if shell.help_open {
        egui::Modal::new("help".into()).show(ctx, |ui| {
            ui.set_width(420.0_f32.min(ctx.viewport_rect().width() - 60.0));
            ui.horizontal(|ui| {
                ui.heading(crate::tr!("Управление", "Controls"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(ui, Icon::Close, crate::tr!("Закрыть справку", "Close help"), false, true).clicked() { shell.help_open = false; }
                });
            });
            egui::ScrollArea::vertical().max_height((ctx.viewport_rect().height()-180.0).max(100.0)).show(ui, |ui| {
                egui::Grid::new("controls").num_columns(2).spacing([24.0, 12.0]).show(ui, |ui| {
                    for (key, text) in [
                        (crate::tr!("ЛКМ", "Left click"), crate::tr!("Выбрать фигуру / сделать ход", "Select a piece / make a move")),
                        (crate::tr!("СКМ", "Middle click"), crate::tr!("Сменить направление времени", "Change time direction")),
                        (crate::tr!("ПКМ / Esc", "Right click / Esc"), crate::tr!("Снять выделение", "Clear selection")),
                        (crate::tr!("Колесо", "Mouse wheel"), crate::tr!("Прокрутить историю", "Scroll history")),
                        (crate::tr!("ПКМ + колесо", "Right mouse button + wheel"), crate::tr!("Изменить масштаб досок", "Zoom boards")),
                        ("Ctrl+Z", crate::tr!("Отменить последний ход", "Undo last move")),
                        ("I", crate::tr!("Инспекция фигуры", "Inspect piece")),
                        ("C", crate::tr!("Продолжить просчёт", "Continue calculation")),
                    ] { ui.strong(key); ui.label(text); ui.end_row(); }
                });
                ui.add_space(12.0); ui.separator(); ui.add_space(8.0);
                for (color, text) in [
                    (FUTURE, crate::tr!("Ход в будущее", "Move into the future")), (PAST, crate::tr!("Ход в прошлое", "Move into the past")),
                    (ORDINARY, crate::tr!("Зелёная клетка — обычная активность", "Green square — ordinary activity")), (INFORMATION, crate::tr!("Глаз над фигурой — новая информация", "Eye above a piece — new information")),
                    (REACTION, crate::tr!("Перечёркнутая стрелка — отменённый ход", "Crossed-out arrow — cancelled move")), (CAPTURE, crate::tr!("Взятие / видимый шах", "Capture / visible check")),
                    (egui::Color32::from_rgb(239,137,185), crate::tr!("Угроза только из будущего", "Threat from the future only")),
                    (FUTURE, crate::tr!("Стрелка: сыгранный ход в будущее", "Arrow: played move into the future")), (PAST, crate::tr!("Стрелка: сыгранный ход в прошлое", "Arrow: played move into the past")),
                ] { ui.horizontal(|ui| { let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover()); ui.painter().circle_filled(rect.center(), 4.0, color); ui.label(text); }); }
                ui.add_space(10.0);
                ui.label(crate::tr!("Для бесплатного ожидания выберите ту же клетку на другой доске по направлению фигуры. Доступные доски отмечены кольцом с точкой.", "To wait without using a turn, select the same square on another board in the piece's time direction. Available boards are marked with a ring and dot."));
                ui.colored_label(SUBTLE, crate::tr!("Инвертированные фигуры выделяются цветом и свечением.", "Inverted pieces have distinct colors and a glow."));
                ui.add_space(10.0);
                ui.small(crate::tr!("Фигуры: Cburnett / Lichess · GPL-2.0+", "Pieces: Cburnett / Lichess · GPL-2.0+"));
            });
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            shell.help_open = false;
        }
    }
}
