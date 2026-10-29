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
        ui.label("Состояния больше нет в истории.");
        return;
    };
    let line = &game.engine.history().worldlines()[id.piece.wid];
    ui.label(format!(
        "Фигура #{} · {} состояний",
        id.piece.wid,
        line.len()
    ));
    egui::CollapsingHeader::new("Все поля состояния")
        .default_open(true)
        .show(ui, |ui| {
            ui.monospace(format!("{state:#?}"));
        });
    egui::CollapsingHeader::new("Причинные разрывы").show(ui, |ui| {
        ui.monospace(format!("{:#?}", line.gaps()));
    });
    egui::CollapsingHeader::new("Записанные ходы").show(ui, |ui| {
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
    egui::CollapsingHeader::new("Мировая линия").show(ui, |ui| {
        for (age, state) in line.iter().enumerate() {
            egui::CollapsingHeader::new(format!(
                "age={age} · t={} · {}",
                state.t,
                if state.alive {
                    "жива"
                } else {
                    "мертва"
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
            ui.heading("Отмена хода");
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Восстанавливаем партию из начальной позиции…");
            });
        });
    }
    if game.import_open {
        egui::Modal::new("import_game".into()).show(ctx, |ui| {
            ui.set_width(560.0_f32.min(ctx.viewport_rect().width() - 60.0));
            ui.heading("Загрузить партию");
            ui.colored_label(SUBTLE, "Вставьте запись партии · Ctrl+V");
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
                ui.colored_label(CAPTURE, error);
            }
            ui.horizontal(|ui| {
                if icon_button(
                    ui,
                    Icon::Check,
                    "Начать из записи",
                    false,
                    !loading && !game.import_text.trim().is_empty(),
                )
                .clicked()
                {
                    game.begin_import();
                }
                if icon_button(ui, Icon::Close, "Отмена", false, !loading).clicked() {
                    game.import_open = false;
                }
                if loading {
                    ui.spinner();
                    ui.label("Проверка истории…");
                }
            });
        });
    }
    if !game.promotion.is_empty() {
        egui::Modal::new("promotion".into()).show(ctx, |ui| {
            ui.heading("Превращение пешки");
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
            if icon_button(ui, Icon::Close, "Отмена", false, true).clicked() {
                game.promotion.clear();
            }
        });
    }
    if !game.overlapping.is_empty() {
        egui::Modal::new("overlapping".into()).show(ctx, |ui| {
            ui.heading("Выберите воплощение");
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
                                format!(
                                    "#{} · возраст {} · петля {}",
                                    piece.origin, piece.age, piece.loop_turn
                                ),
                            );
                        });
                        response
                    })
                    .inner;
                if crate::input::clicked(&response.on_hover_text("Выбрать воплощение"))
                {
                    game.choose(piece);
                }
            }
            if icon_button(ui, Icon::Close, "Отмена", false, true).clicked() {
                game.overlapping.clear();
            }
        });
    }
    if game.confirm_restart {
        egui::Modal::new("restart".into()).show(ctx, |ui| {
            ui.heading("Начать новую партию?");
            ui.label("Текущая партия будет сброшена.");
            ui.horizontal(|ui| {
                if icon_button(ui, Icon::Check, "Начать новую партию", false, true).clicked()
                {
                    *game = GameUi {
                        cell_size: game.cell_size,
                        ..GameUi::default()
                    };
                }
                if icon_button(ui, Icon::Close, "Отмена", false, true).clicked() {
                    game.confirm_restart = false;
                }
            });
        });
    }
    if shell.help_open {
        egui::Modal::new("help".into()).show(ctx, |ui| {
            ui.set_width(420.0_f32.min(ctx.viewport_rect().width() - 60.0));
            ui.horizontal(|ui| {
                ui.heading("Управление");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(ui, Icon::Close, "Закрыть справку", false, true).clicked() { shell.help_open = false; }
                });
            });
            egui::ScrollArea::vertical().max_height((ctx.viewport_rect().height()-180.0).max(100.0)).show(ui, |ui| {
                egui::Grid::new("controls").num_columns(2).spacing([24.0, 12.0]).show(ui, |ui| {
                    for (key, text) in [
                        ("ЛКМ", "Выбрать фигуру / сделать ход"),
                        ("СКМ", "Сменить направление времени"),
                        ("ПКМ / Esc", "Снять выделение"),
                        ("Колесо", "Прокрутить историю"),
                        ("ПКМ + колесо", "Изменить масштаб досок"),
                        ("Ctrl+Z", "Отменить последний ход"),
                        ("I", "Инспекция фигуры"),
                        ("C", "Продолжить просчёт"),
                    ] { ui.strong(key); ui.label(text); ui.end_row(); }
                });
                ui.add_space(12.0); ui.separator(); ui.add_space(8.0);
                for (color, text) in [
                    (FUTURE, "Ход в будущее"), (PAST, "Ход в прошлое"),
                    (ORDINARY, "Обычная активность"), (INFORMATION, "Новая информация"),
                    (REACTION, "Отменённый ход"), (CAPTURE, "Взятие / видимый шах"),
                    (egui::Color32::from_rgb(239,137,185), "Угроза только из будущего"),
                    (FUTURE, "Стрелка: сыгранный ход в будущее"), (PAST, "Стрелка: сыгранный ход в прошлое"),
                ] { ui.horizontal(|ui| { let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover()); ui.painter().circle_filled(rect.center(), 4.0, color); ui.label(text); }); }
                ui.add_space(10.0);
                ui.label("Для бесплатного ожидания выберите ту же клетку на другой доске по направлению фигуры. Доступные доски отмечены кольцом с точкой.");
                ui.colored_label(SUBTLE, "Инвертированные фигуры: белые — голубые, чёрные — карминовые.");
                ui.add_space(10.0);
                ui.small("Фигуры: Cburnett / Lichess · GPL-2.0+");
            });
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            shell.help_open = false;
        }
    }
}
