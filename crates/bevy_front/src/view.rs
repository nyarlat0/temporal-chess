use crate::{
    controller::{GameUi, direction_name, side_name},
    piece_art::PieceArt,
};
use bevy_egui::egui::{self, Color32, FontId, Vec2};
use temporal_chess_engine::{
    engine::CalculationStatus,
    model::{Color, PieceKind, TimeDirection},
};

pub const FUTURE: Color32 = Color32::from_rgb(241, 108, 169);
pub const PAST: Color32 = Color32::from_rgb(128, 21, 52);
pub const REACTION: Color32 = Color32::from_rgb(240, 125, 32);
pub const INFORMATION: Color32 = Color32::from_rgb(245, 205, 46);
pub const ORDINARY: Color32 = Color32::from_rgb(77, 185, 92);
pub const CAPTURE: Color32 = Color32::from_rgb(235, 48, 61);
pub const PLAYED_FUTURE: Color32 = Color32::from_rgb(65, 137, 235);
pub const PLAYED_PAST: Color32 = Color32::from_rgb(151, 85, 216);
pub const INVERTED: Color32 = Color32::from_rgb(150, 134, 190);
pub const MUTED: Color32 = Color32::from_rgb(158, 154, 146);
const TEXT: Color32 = Color32::from_rgb(226, 223, 215);
const PANEL: Color32 = Color32::from_rgb(43, 41, 38);

pub fn draw(ctx: &egui::Context, game: &mut GameUi) {
    game.poll_import();
    if game.import_job.is_some() {
        ctx.request_repaint_after(std::time::Duration::from_millis(80));
    }
    if !game.fonts_ready {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "text".into(),
            egui::FontData::from_static(include_bytes!("../assets/fonts/DejaVuSans.ttf")).into(),
        );
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "text".into());
        ctx.set_fonts(fonts);
        ctx.tessellation_options_mut(|options| {
            options.feathering = true;
            options.feathering_size_in_pixels = 1.0;
        });
        ctx.set_theme(egui::Theme::Dark);
        let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = PANEL;
        style.visuals.window_fill = PANEL;
        style.visuals.override_text_color = Some(TEXT);
        style.visuals.selection.bg_fill = Color32::from_rgb(77, 72, 88);
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(57, 54, 50);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(71, 67, 61);
        style.visuals.widgets.active.bg_fill = Color32::from_rgb(82, 77, 69);
        style.spacing.item_spacing = Vec2::new(10.0, 12.0);
        style.spacing.button_padding = Vec2::new(12.0, 8.0);
        style.spacing.interact_size.y = 34.0;
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(13.0));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, FontId::proportional(20.0));
        ctx.set_style_of(egui::Theme::Dark, style);
        game.fonts_ready = true;
    }
    if game.art.is_none() {
        game.art = Some(PieceArt::new(ctx));
    }
    let (cancel, toggle, inspect, advance) = ctx.input(|i| {
        (
            i.pointer.button_clicked(egui::PointerButton::Secondary)
                || i.key_pressed(egui::Key::Escape),
            i.pointer.button_clicked(egui::PointerButton::Middle),
            i.key_pressed(egui::Key::I) && !i.modifiers.command && !i.modifiers.ctrl,
            i.key_pressed(egui::Key::C) && !i.modifiers.command && !i.modifiers.ctrl,
        )
    });
    if cancel {
        game.clear_selection();
    }
    if toggle {
        game.toggle_direction();
    }
    let shortcuts = !ctx.egui_wants_keyboard_input() && !game.import_open && !game.confirm_restart;
    if inspect && shortcuts {
        game.clear_selection();
        game.inspect_mode = !game.inspect_mode;
    }
    if advance
        && shortcuts
        && matches!(
            game.engine.calculation_status(),
            CalculationStatus::Paused { .. }
        )
    {
        game.continue_calculation();
    }
    let mut root = egui::Ui::new(
        ctx.clone(),
        "viewport".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    egui::Panel::top("header")
        .frame(
            egui::Frame::new()
                .fill(PANEL)
                .inner_margin(egui::Margin::symmetric(20, 12)),
        )
        .show(&mut root, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Temporal Chess").size(23.0).strong());
                ui.colored_label(MUTED, "шахматы во времени");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Новая партия").clicked() {
                        game.confirm_restart = true;
                    }
                    if ui
                        .selectable_label(game.inspect_mode, "Инспекция")
                        .on_hover_text("I — инспекция фигуры")
                        .clicked()
                    {
                        game.clear_selection();
                        game.inspect_mode = !game.inspect_mode;
                    }
                });
            });
        });
    egui::Panel::right("details")
        .default_size(292.0)
        .min_size(260.0)
        .max_size(380.0)
        .frame(egui::Frame::new().fill(PANEL).inner_margin(18))
        .show(&mut root, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("details_scroll")
                .show(ui, |ui| sidebar(ui, game));
        });
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(Color32::from_rgb(32, 31, 29))
                .inner_margin(12),
        )
        .show(&mut root, |ui| {
            ui.horizontal(|ui| {
                ui.colored_label(MUTED, "ИСТОРИЯ");
                if ui.small_button("↑ Настоящее").clicked() {
                    game.jump_to = game.times().first().copied();
                }
                if ui.small_button("↓ Начало").clicked() {
                    game.jump_to = Some(0);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(egui::Slider::new(&mut game.cell_size, 30.0..=90.0).show_value(false))
                        .on_hover_text("Масштаб досок");
                    ui.small("Масштаб");
                });
            });
            crate::board_view::ribbon(ui, game);
        });
    dialogs(ctx, game);
}

fn sidebar(ui: &mut egui::Ui, game: &mut GameUi) {
    ui.add_space(4.0);
    ui.small("ЛОКАЛЬНАЯ ПАРТИЯ");
    ui.heading(game.status_text());
    ui.colored_label(MUTED, "Два игрока за одним компьютером");
    for side in [Color::Black, Color::White] {
        let active = game.engine.turn() == side && game.engine.outcome().is_none();
        egui::Frame::new()
            .fill(if active {
                Color32::from_rgb(61, 57, 50)
            } else {
                Color32::from_rgb(37, 35, 33)
            })
            .corner_radius(5)
            .inner_margin(12)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(34.0), egui::Sense::hover());
                    if let Some(art) = &game.art {
                        art.paint(ui.painter(), PieceKind::K { moved: false }, side, rect, 1.0);
                    }
                    ui.vertical(|ui| {
                        ui.strong(side_name(side));
                        ui.small(if active {
                            "Сейчас ходят"
                        } else {
                            "Второй игрок"
                        });
                    });
                });
            });
    }
    ui.separator();
    if let CalculationStatus::Paused { t, .. } = game.engine.calculation_status() {
        ui.colored_label(REACTION, format!("Отменённый ход на t = {t}"));
        if ui
            .add_sized(
                [ui.available_width(), 36.0],
                egui::Button::new("Продолжить просчёт"),
            )
            .on_hover_text("C · без расходования хода")
            .clicked()
        {
            game.continue_calculation();
        }
    }
    if game.inspect_mode {
        ui.colored_label(INVERTED, "Инспекция");
        ui.small("Выберите фигуру на любой доске.");
    } else if let Some(selection) = &game.selected {
        ui.strong(format!(
            "{} · t = {}",
            piece_name(selection.piece.kind),
            selection.piece.t
        ));
        ui.small("Выберите клетку на этой же доске.");
        let direction = selection.direction;
        ui.horizontal(|ui| {
            ui.colored_label(
                if direction == TimeDirection::Future {
                    FUTURE
                } else {
                    PAST
                },
                direction_name(direction),
            );
            if ui
                .small_button("⇅")
                .on_hover_text("Средняя кнопка мыши — сменить направление")
                .clicked()
            {
                game.toggle_direction();
            }
        });
    } else {
        ui.label("Выберите фигуру");
        ui.colored_label(MUTED, "Возможные ходы появятся на её доске.");
    }
    if game.is_error {
        ui.colored_label(CAPTURE, &game.notice);
    }
    ui.separator();
    if game.inspect_mode {
        inspector(ui, game);
    }
    ui.strong("Ходы партии");
    ui.horizontal(|ui| {
        if ui
            .button("Копировать")
            .on_hover_text("Скопировать полную запись партии в буфер обмена")
            .clicked()
        {
            ui.ctx().copy_text(game.record.to_text());
            game.message("Партия скопирована в буфер обмена.", false);
        }
        if ui.button("Загрузить").clicked() {
            game.clear_selection();
            game.import_open = true;
            game.import_error = None;
        }
    });
    if game.notice.starts_with("Партия скопирована") {
        ui.small(&game.notice);
    }
    if game.log.is_empty() {
        ui.colored_label(MUTED, "Пока нет ходов");
    }
    for (index, entry) in game.log.iter().enumerate().rev().take(10) {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(MUTED, format!("{}.", index + 1));
            ui.label(entry);
        });
    }
    ui.add_space(8.0);
    egui::CollapsingHeader::new("Управление и обозначения").show(ui, |ui| {
        ui.label("ЛКМ — выбрать и ходить на той же доске.\nСКМ — направление.\nПКМ / Esc — снять выделение.\nКолесо — история.");
        ui.colored_label(FUTURE, "● Розовый — в будущее");
        ui.colored_label(Color32::from_rgb(186, 66, 99), "● Карминовый — в прошлое");
        ui.colored_label(ORDINARY, "Контур: ordinary");
        ui.colored_label(INFORMATION, "Контур: новая информация");
        ui.colored_label(REACTION, "Контур: отменённый ход");
        ui.colored_label(CAPTURE, "Красное свечение: доступное взятие");
        ui.colored_label(CAPTURE, "Красный контур короля: видимый шах");
        ui.colored_label(Color32::from_rgb(239, 137, 185), "Розовый контур короля: угроза только из будущего");
        ui.small("Сыгранный ход: синий в будущее, фиолетовый в прошлое.");
        ui.colored_label(INVERTED, "Инверсия — обратные цвета и толстый контур");
        ui.small("Для бесплатного ожидания нажмите ту же клетку на другой доске по направлению фигуры. Кольцо с точкой отмечает доступные доски.");
        ui.small("Фигуры: Cburnett / Lichess · GPL-2.0+");
    });
}

fn inspector(ui: &mut egui::Ui, game: &GameUi) {
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

fn dialogs(ctx: &egui::Context, game: &mut GameUi) {
    if game.import_open {
        egui::Modal::new("import_game".into()).show(ctx, |ui| {
            ui.set_width(560.0_f32.min(ctx.viewport_rect().width() - 60.0));
            ui.heading("Продолжить партию из записи");
            ui.label("Вставьте текст, полученный кнопкой «Копировать» (Ctrl+V).");
            ui.small("Текущая партия заменится только после успешной проверки всех команд.");
            let loading = game.import_job.is_some();
            ui.add_enabled_ui(!loading, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(340.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut game.import_text)
                                .font(egui::TextStyle::Monospace)
                                .desired_width(f32::INFINITY)
                                .desired_rows(13)
                                .hint_text("TEMPORAL-CHESS 1\nmove 0 12:0:0 e2 e4 + normal"),
                        );
                    });
            });
            if let Some(error) = &game.import_error {
                ui.colored_label(CAPTURE, error);
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !loading && !game.import_text.trim().is_empty(),
                        egui::Button::new("Начать из записи"),
                    )
                    .clicked()
                {
                    game.begin_import();
                }
                if ui
                    .add_enabled(!loading, egui::Button::new("Отмена"))
                    .clicked()
                {
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
                            4.0,
                            if response.hovered() {
                                Color32::from_gray(95)
                            } else {
                                Color32::from_gray(65)
                            },
                        );
                        if let Some(art) = &game.art {
                            art.paint(ui.painter(), kind, game.engine.turn(), rect, 1.0);
                        }
                        if response.on_hover_text(piece_name(kind)).clicked() {
                            game.play(mv);
                        }
                    }
                }
            });
            if ui.button("Отмена").clicked() {
                game.promotion.clear();
            }
        });
    }
    if !game.overlapping.is_empty() {
        egui::Modal::new("overlapping".into()).show(ctx, |ui| {
            ui.heading("Выберите воплощение");
            for piece in game.overlapping.clone() {
                if ui
                    .button(format!(
                        "{} · {} · #{} · age={} · loop={}",
                        side_name(piece.color),
                        piece_name(piece.kind),
                        piece.origin,
                        piece.age,
                        piece.loop_turn
                    ))
                    .clicked()
                {
                    game.choose(piece);
                }
            }
            if ui.button("Отмена").clicked() {
                game.overlapping.clear();
            }
        });
    }
    if game.confirm_restart {
        egui::Modal::new("restart".into()).show(ctx, |ui| {
            ui.heading("Начать новую партию?");
            ui.label("Текущая партия будет сброшена.");
            ui.horizontal(|ui| {
                if ui.button("Начать").clicked() {
                    let art = game.art.take();
                    *game = GameUi {
                        art,
                        cell_size: game.cell_size,
                        fonts_ready: game.fonts_ready,
                        ..GameUi::default()
                    };
                }
                if ui.button("Отмена").clicked() {
                    game.confirm_restart = false;
                }
            });
        });
    }
}

pub fn direction_color(direction: TimeDirection) -> Color32 {
    match direction {
        TimeDirection::Future => FUTURE,
        TimeDirection::Past => PAST,
    }
}
fn piece_name(kind: PieceKind) -> &'static str {
    match kind {
        PieceKind::K { .. } => "Король",
        PieceKind::Q => "Ферзь",
        PieceKind::R { .. } => "Ладья",
        PieceKind::B => "Слон",
        PieceKind::N => "Конь",
        PieceKind::P { .. } => "Пешка",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Pos2, Rect};
    use temporal_chess_engine::model::Sq;

    fn frame(ctx: &egui::Context, game: &mut GameUi, events: Vec<egui::Event>) -> egui::FullOutput {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1260.0, 900.0))),
                events,
                ..Default::default()
            },
            |ui| draw(ui.ctx(), game),
        );
        output.textures_delta.clear();
        assert!(
            !ctx.tessellate(output.shapes.clone(), output.pixels_per_point)
                .is_empty()
        );
        output
    }

    #[test]
    fn selection_stays_still_and_a_move_scrolls_to_its_arrival() {
        let ctx = egui::Context::default();
        let mut game = GameUi::default();
        frame(&ctx, &mut game, vec![]);
        frame(&ctx, &mut game, vec![]);
        let before = game.viewport.offset;
        game.click_square(0, Sq(4, 1));
        frame(&ctx, &mut game, vec![]);
        assert_eq!(game.viewport.offset, before);
        game.toggle_direction();
        frame(&ctx, &mut game, vec![]);
        assert_eq!(game.viewport.offset, before);
        game.toggle_direction();
        game.click_square(0, Sq(4, 3));
        frame(&ctx, &mut game, vec![]);
        assert!(game.viewport.offset.abs() < 0.1);
        assert_eq!(game.viewport.times.first(), Some(&1));
        game.inspect_mode = true;
        game.click_square(0, Sq(4, 1));
        frame(&ctx, &mut game, vec![]);
        assert!(game.inspected.is_some());
    }

    #[test]
    fn mouse_buttons_work_over_the_sidebar_without_moving_the_ribbon() {
        let ctx = egui::Context::default();
        let mut game = GameUi::default();
        frame(&ctx, &mut game, vec![]);
        game.click_square(0, Sq(4, 1));
        for button in [egui::PointerButton::Middle, egui::PointerButton::Secondary] {
            let pos = Pos2::new(1200.0, 800.0);
            for pressed in [true, false] {
                frame(
                    &ctx,
                    &mut game,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button,
                            pressed,
                            modifiers: egui::Modifiers::default(),
                        },
                    ],
                );
            }
            if button == egui::PointerButton::Middle {
                assert_eq!(
                    game.selected.as_ref().unwrap().direction,
                    TimeDirection::Past
                );
            }
        }
        assert!(game.selected.is_none());
        assert_eq!(game.times(), [0]);
    }
    fn text_position(output: &egui::FullOutput, label: &str) -> Pos2 {
        output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape
                    && text.galley.text().starts_with(label)
                {
                    Some(text.pos + text.galley.size() * 0.5)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| panic!("missing UI text: {label}"))
    }

    fn click(ctx: &egui::Context, game: &mut GameUi, pos: Pos2) -> egui::FullOutput {
        frame(
            ctx,
            game,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        frame(
            ctx,
            game,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        )
    }

    #[test]
    fn copy_button_emits_the_record_and_import_accepts_paste_without_game_shortcuts() {
        let ctx = egui::Context::default();
        let mut game = GameUi::default();
        game.click_square(0, Sq(4, 1));
        game.click_square(0, Sq(4, 3));
        frame(&ctx, &mut game, vec![]);
        let output = frame(&ctx, &mut game, vec![]);
        let pos = text_position(&output, "Копировать");
        let output = click(&ctx, &mut game, pos);
        let text = game.record.to_text();
        assert!(output.platform_output.commands.iter().any(
            |command| matches!(command, egui::OutputCommand::CopyText(value) if value == &text)
        ));
        let pos = text_position(&output, "Загрузить");
        click(&ctx, &mut game, pos);
        assert!(game.import_open);
        let output = frame(&ctx, &mut game, vec![]);
        let pos = text_position(&output, "TEMPORAL-CHESS 1");
        click(&ctx, &mut game, pos);
        frame(
            &ctx,
            &mut game,
            vec![
                egui::Event::Paste(text.clone()),
                egui::Event::Key {
                    key: egui::Key::I,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                },
            ],
        );
        assert_eq!(game.import_text, text);
        assert!(!game.inspect_mode);
        assert_eq!(game.record.commands().len(), 1);
    }
}
