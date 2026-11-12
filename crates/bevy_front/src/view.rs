//! Application chrome. Board and piece rendering stay in their own modules.
use crate::{
    controller::GameUi,
    icons::{Icon, icon_button},
    piece_art::PieceArt,
    theme::*,
};
use bevy::prelude::Resource;
use bevy_egui::egui::{self, Vec2};
use temporal_chess_engine::{engine::CalculationStatus, model::PieceKind};

#[derive(Resource, Clone)]
pub struct ShellState {
    pub history_open: bool,
    pub help_open: bool,
    pub input_frame: Option<u64>,
    pub zoom_gesture: bool,
    pub suppress_wheel: bool,
    pub ribbon_rect: egui::Rect,
    copied_until: f64,
}

impl Default for ShellState {
    fn default() -> Self {
        Self {
            history_open: true,
            help_open: false,
            input_frame: None,
            zoom_gesture: false,
            suppress_wheel: false,
            ribbon_rect: egui::Rect::NOTHING,
            copied_until: 0.0,
        }
    }
}

pub fn draw(ctx: &egui::Context, game: &mut GameUi, shell: &mut ShellState, art: &PieceArt) {
    crate::input::handle(ctx, game, shell);
    let mut root = egui::Ui::new(
        ctx.clone(),
        "viewport".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    let modal = game.import_open
        || game.confirm_restart
        || game.replay_job.is_some()
        || !game.promotion.is_empty()
        || !game.overlapping.is_empty()
        || shell.help_open;
    if modal {
        root.disable();
    }
    egui::Panel::right("game_sidebar")
        .default_size(340.0)
        .min_size(300.0)
        .max_size(400.0)
        .frame(egui::Frame::new().fill(PANEL).inner_margin(18))
        .show(&mut root, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("sidebar_scroll")
                .show(ui, |ui| sidebar(ui, game, shell, art));
        });
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(BACKGROUND).inner_margin(12))
        .show(&mut root, |ui| {
            shell.ribbon_rect = ui.available_rect_before_wrap();
            crate::board_view::ribbon(ui, game, art);
        });
    crate::dialogs::draw(ctx, game, shell, art);
    if game.replay_job.is_some() {
        ctx.request_repaint_after(std::time::Duration::from_millis(80));
    }
    if game.playback_busy()
        || game.viewport.scroll.is_some()
        || game.jump_to.is_some()
        || game.pending_square.is_some()
        || game.pending_move.is_some()
    {
        ctx.request_repaint();
    }
}

fn sidebar(ui: &mut egui::Ui, game: &mut GameUi, shell: &mut ShellState, art: &PieceArt) {
    let idle = game.replay_job.is_none() && !game.playback_busy() && !game.facing.animating();
    let mut language = crate::i18n::Language::current();
    ui.horizontal(|ui| {
        ui.label(crate::tr!("Язык", "Language"));
        egui::ComboBox::from_id_salt("interface_language")
            .selected_text(language.name())
            .show_ui(ui, |ui| {
                for choice in [
                    crate::i18n::Language::Russian,
                    crate::i18n::Language::English,
                ] {
                    ui.selectable_value(&mut language, choice, choice.name());
                }
            });
    });
    if language != crate::i18n::Language::current() {
        language.set();
        ui.ctx().request_repaint();
    }
    ui.horizontal(|ui| {
        ui.heading("Temporal Chess");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icon_button(
                ui,
                Icon::Help,
                crate::tr!("Управление и обозначения", "Controls and legend"),
                shell.help_open,
                true,
            )
            .clicked()
            {
                shell.help_open = true;
            }
        });
    });
    egui::Frame::new()
        .fill(SURFACE)
        .corner_radius(10)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(32.0), egui::Sense::hover());
                art.paint(
                    ui.painter(),
                    PieceKind::K { moved: false },
                    game.displayed_turn(),
                    rect,
                    1.0,
                );
                ui.vertical(|ui| {
                    ui.strong(game.status_text());
                    ui.colored_label(
                        SUBTLE,
                        crate::tr_format!(
                            "Активных досок: {}",
                            "Active boards: {}",
                            game.active_times().len()
                        ),
                    );
                });
            });
        });
    let rules = game.engine.history().rules();
    if rules.wait_before_move {
        ui.small(crate::tr!("Свободное ожидание", "Free wait"));
    }
    if rules.chain_info_activation {
        ui.small(crate::tr!("Цепная инфоактивация", "Chain info-activation"));
    }
    if !rules.global_reaction_reset {
        ui.small(crate::tr!("Реагировать многократно", "React multiple"));
    }
    crate::tempfish::panel(ui, game);
    crate::multiplayer::panel(ui, game);
    ui.horizontal(|ui| {
        if icon_button(
            ui,
            Icon::Undo,
            crate::tr!("Отменить последний ход · Ctrl+Z", "Undo last move · Ctrl+Z"),
            false,
            game.can_undo(),
        )
        .clicked()
        {
            game.undo_last_move();
        }
        let copied = ui.input(|i| i.time) < shell.copied_until;
        if icon_button(
            ui,
            if copied { Icon::Check } else { Icon::Copy },
            crate::tr!("Копировать партию", "Copy game"),
            copied,
            true,
        )
        .clicked()
        {
            ui.ctx().copy_text(game.record.to_text());
            shell.copied_until = ui.input(|i| i.time) + 2.0;
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs(2));
        }
        if icon_button(
            ui,
            Icon::Import,
            crate::tr!("Загрузить партию из записи", "Import game record"),
            false,
            idle && !game.online.enabled,
        )
        .clicked()
        {
            game.clear_selection();
            game.import_open = true;
            game.import_error = None;
        }
        if icon_button(
            ui,
            Icon::New,
            crate::tr!("Новая партия", "New game"),
            false,
            idle && !game.online.enabled,
        )
        .clicked()
        {
            game.new_game_bot = game.bot.settings;
            game.confirm_restart = true;
        }
    });
    if game.is_error {
        ui.horizontal_wrapped(|ui| {
            if icon_button(
                ui,
                Icon::Close,
                crate::tr!("Скрыть сообщение", "Dismiss message"),
                false,
                true,
            )
            .clicked()
            {
                game.is_error = false;
            }
            ui.colored_label(CAPTURE, game.notice.text());
        });
    }
    ui.separator();
    ui.horizontal(|ui| {
        ui.strong(
            if game
                .engine
                .history()
                .starting_position()
                .fixed_end()
                .is_some()
            {
                "Time War"
            } else {
                crate::tr!("Лента времени", "Timeline")
            },
        );
        ui.colored_label(SUBTLE, format!("0 — {}", game.engine.history().horizon()));
    });
    ui.horizontal(|ui| {
        if icon_button(
            ui,
            Icon::First,
            crate::tr!("Последняя доска", "Latest board"),
            false,
            idle,
        )
        .clicked()
        {
            game.jump_to = game.times().first().copied();
        }
        if icon_button(
            ui,
            Icon::Last,
            crate::tr!("Начало истории", "Beginning of history"),
            false,
            idle,
        )
        .clicked()
        {
            game.jump_to = Some(0);
        }
        if icon_button(
            ui,
            Icon::ResetZoom,
            crate::tr!("Сбросить масштаб", "Reset zoom"),
            false,
            idle,
        )
        .clicked()
        {
            game.cell_size = 64.0;
        }
        ui.label(format!("{:.0}%", game.cell_size / 64.0 * 100.0));
    });
    ui.colored_label(
        SUBTLE,
        egui::RichText::new(crate::tr!(
            "Масштаб: Shift или ПКМ + колесо",
            "Zoom: Shift or right mouse button + wheel"
        ))
        .small(),
    );
    if !game.online.enabled && game.bot.settings.human_side().is_none() {
        ui.add_enabled(
            idle,
            egui::Checkbox::new(
                &mut game.local_auto_flip,
                crate::tr!("Переворачивать доски", "Flip boards"),
            ),
        )
        .on_hover_text(crate::tr!(
            "Если выключено, доски сохраняют текущую ориентацию.",
            "When disabled, boards keep their current orientation."
        ));
    }
    if game.engine.can_decline_reactions()
        && ui
            .add_enabled(
                idle && game.can_submit(),
                egui::Button::new(crate::tr!("Не реагировать", "Decline reactions")),
            )
            .on_hover_text(crate::tr!(
                "Деактивировать все свои реакции и передать ход сопернику.",
                "Deactivate all your reactions and hand the turn to your opponent."
            ))
            .clicked()
    {
        game.decline_reactions();
    }
    if let CalculationStatus::Paused { t, .. } = game.engine.calculation_status() {
        ui.horizontal(|ui| {
            if icon_button(
                ui,
                Icon::Continue,
                crate::tr!("Продолжить просчёт · C", "Continue calculation · C"),
                false,
                idle && game.can_submit(),
            )
            .clicked()
            {
                game.continue_calculation();
            }
            ui.colored_label(
                REACTION,
                crate::tr_format!("Отменённый ход на доске {t}", "Cancelled move on board {t}"),
            );
        });
        ui.small(crate::tr!(
            "Продолжение просчёта не расходует ход.",
            "Continuing calculation does not use a turn."
        ));
    }
    ui.separator();
    if let Some(selection) = &game.selected {
        let piece = selection.piece;
        let direction = selection.direction;
        let moves = selection
            .moves
            .iter()
            .filter(|mv| mv.direction == direction)
            .map(|mv| (mv.sq.0, mv.sq.1))
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        let captures = selection
            .captures
            .iter()
            .filter(|(mv, _)| mv.direction == direction)
            .map(|(mv, _)| (mv.sq.0, mv.sq.1))
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        let fatal = selection
            .fatal_captures
            .iter()
            .filter(|(mv, _)| mv.direction == direction)
            .map(|(mv, _)| (mv.sq.0, mv.sq.1))
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        let skips = if direction
            == temporal_chess_engine::model::TimeDirection::from_inverted(piece.inverted)
        {
            selection.skips.len()
        } else {
            0
        };
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(42.0), egui::Sense::hover());
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
                ui.strong(format!(
                    "{} · {}",
                    piece_name(piece.kind),
                    square_name(piece.sq)
                ));
                ui.colored_label(
                    SUBTLE,
                    crate::tr_format!(
                        "Доска {}{}",
                        "Board {}{}",
                        piece.t,
                        if piece.inverted {
                            crate::tr!(" · инверсия", " · inverted")
                        } else {
                            ""
                        }
                    ),
                );
            });
        });
        ui.horizontal(|ui| {
            if icon_button(
                ui,
                if direction == temporal_chess_engine::model::TimeDirection::Future {
                    Icon::Future
                } else {
                    Icon::Past
                },
                crate::tr!(
                    "Сменить направление · СКМ",
                    "Change direction · middle mouse button"
                ),
                false,
                idle,
            )
            .clicked()
            {
                game.toggle_direction();
            }
            ui.label(crate::controller::direction_name(direction));
        });
        ui.label(crate::tr_format!(
            "Клеток для хода: {moves} · взятий: {captures}",
            "Move destinations: {moves} · captures: {captures}"
        ));
        if skips > 0 {
            ui.colored_label(
                SUBTLE,
                crate::tr_format!(
                    "Ожидание доступно на {skips} досках",
                    "Waiting available on {skips} boards"
                ),
            );
        }
        if fatal > 0 {
            ui.horizontal_wrapped(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(18.0), egui::Sense::hover());
                crate::icons::paint(
                    ui.painter(),
                    Icon::Skull,
                    rect,
                    egui::Stroke::new(1.5, CAPTURE),
                );
                ui.colored_label(
                    CAPTURE,
                    crate::tr_format!(
                        "Опасных клеток: {fatal} — ваша фигура погибнет",
                        "Deadly destinations: {fatal} — your piece will die"
                    ),
                );
            });
        }
    } else {
        ui.colored_label(
            SUBTLE,
            if game.inspect_mode {
                crate::tr!("Выберите фигуру для инспекции", "Select a piece to inspect")
            } else {
                crate::tr!(
                    "Выберите свою активную фигуру",
                    "Select one of your active pieces"
                )
            },
        );
    }
    ui.separator();
    details_panel(ui, game, shell, art);
}

fn details_panel(ui: &mut egui::Ui, game: &mut GameUi, shell: &mut ShellState, art: &PieceArt) {
    ui.horizontal(|ui| {
        let history = ui.selectable_label(
            !game.inspect_mode,
            crate::tr!("Ходы партии", "Move history"),
        );
        if crate::input::clicked(&history) {
            game.inspect_mode = false;
            shell.history_open = true;
        }
        let inspect = ui
            .selectable_label(game.inspect_mode, crate::tr!("Инспекция", "Inspect"))
            .on_hover_text(crate::tr!(
                "I · вся информация движка о фигуре",
                "I · all engine data for a piece"
            ));
        if crate::input::clicked(&inspect) && !game.playback_busy() {
            game.clear_selection();
            game.inspect_mode = true;
            game.worldline_mode = false;
            game.worldline_selected = None;
        }
    });
    if game.inspect_mode {
        egui::ScrollArea::both()
            .id_salt("inspector_scroll")
            .max_height(360.0)
            .show(ui, |ui| crate::dialogs::inspector(ui, game));
    } else if game.record.commands().is_empty() {
        ui.add_space(20.0);
        ui.colored_label(
            SUBTLE,
            crate::tr!("Здесь появятся ходы партии", "Moves will appear here"),
        );
    } else {
        let playback_busy = game.playback_busy();
        let commands = game.record.commands();
        egui::ScrollArea::both()
            .id_salt("history_scroll")
            .max_height(320.0)
            .auto_shrink([false, false])
            .show_rows(ui, 30.0, commands.len(), |ui, range| {
                for row in range {
                    let index = commands.len() - 1 - row;
                    let (text, t) = match commands[index] {
                        temporal_chess_engine::record::Command::Move { mv, from }
                        | temporal_chess_engine::record::Command::MoveAndWait {
                            mv, from, ..
                        } => (
                            format!(
                                "({}, {}) -> ({}, {})",
                                square_name(from),
                                mv.t,
                                square_name(mv.sq),
                                commands[index]
                                    .wait_until()
                                    .or_else(|| mv.direction.step(mv.t))
                                    .unwrap_or(mv.t)
                            ),
                            mv.t,
                        ),
                        temporal_chess_engine::record::Command::Advance { source, from, to_t } => (
                            format!(
                                "({}, {}) -> ({}, {})",
                                square_name(from),
                                source.t,
                                square_name(from),
                                to_t
                            ),
                            source.t,
                        ),
                        temporal_chess_engine::record::Command::Continue => (
                            crate::tr!("Продолжение просчёта", "Continue calculation").into(),
                            game.engine.history().horizon(),
                        ),
                        temporal_chess_engine::record::Command::DeclineReactions => (
                            crate::tr!("Не реагировать", "Decline reactions").into(),
                            game.engine.history().horizon(),
                        ),
                    };
                    let response = ui
                        .horizontal(|ui| {
                            ui.set_min_height(30.0);
                            ui.label(format!("{}.", index + 1));
                            let (rect, _) =
                                ui.allocate_exact_size(Vec2::splat(24.0), egui::Sense::hover());
                            if let Some(piece) = game.record.piece(index) {
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
                                if let Some(mv) = commands[index].spatial_move()
                                    && mv.to_past() != piece.inverted
                                {
                                    let (arrow, response) = ui.allocate_exact_size(
                                        Vec2::splat(18.0),
                                        egui::Sense::hover(),
                                    );
                                    crate::icons::paint(
                                        ui.painter(),
                                        if mv.to_past() {
                                            Icon::Past
                                        } else {
                                            Icon::Future
                                        },
                                        arrow.shrink(2.0),
                                        egui::Stroke::new(1.5, TEXT),
                                    );
                                    response.on_hover_text(if mv.to_past() {
                                        crate::tr!("Поворот в прошлое", "Turn towards the past")
                                    } else {
                                        crate::tr!("Поворот в будущее", "Turn towards the future")
                                    });
                                }
                            } else {
                                crate::icons::paint(
                                    ui.painter(),
                                    Icon::Continue,
                                    rect.shrink(3.0),
                                    egui::Stroke::new(1.5, TEXT),
                                );
                            }
                            ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Extend));
                        })
                        .response;
                    let response = ui.interact(
                        response.rect,
                        ui.make_persistent_id(("record_row", index)),
                        egui::Sense::click(),
                    );
                    if crate::input::clicked(&response.on_hover_text(crate::tr!(
                        "Перейти к исходной доске",
                        "Go to the source board"
                    ))) && !playback_busy
                    {
                        game.jump_to = Some(t);
                    }
                }
            });
    }
}

pub fn piece_name(kind: PieceKind) -> &'static str {
    match kind {
        PieceKind::K { .. } => crate::tr!("Король", "King"),
        PieceKind::Q => crate::tr!("Ферзь", "Queen"),
        PieceKind::R { .. } => crate::tr!("Ладья", "Rook"),
        PieceKind::B => crate::tr!("Слон", "Bishop"),
        PieceKind::N => crate::tr!("Конь", "Knight"),
        PieceKind::P { .. } => crate::tr!("Пешка", "Pawn"),
    }
}

fn square_name(sq: temporal_chess_engine::model::Sq) -> String {
    format!("{}{}", char::from(b'a' + sq.0 as u8), sq.1 + 1)
}

pub fn starting_position_controls(
    ui: &mut egui::Ui,
    position: &mut temporal_chess_engine::model::StartingPosition,
) {
    use temporal_chess_engine::model::{MAX_TIME_WAR_SEPARATION, StartingPosition};
    egui::ComboBox::from_label(crate::tr!("Стартовая позиция", "Starting position"))
        .selected_text(match position {
            StartingPosition::Standard => crate::tr!("Обычная", "Standard"),
            StartingPosition::TimeWar { .. } => "Time War",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(
                position,
                StartingPosition::Standard,
                crate::tr!("Обычная", "Standard"),
            );
            let time_war = match *position {
                StartingPosition::TimeWar { .. } => *position,
                _ => StartingPosition::TimeWar { separation: 9 },
            };
            ui.selectable_value(position, time_war, "Time War");
        });
    if let StartingPosition::TimeWar { separation } = position {
        ui.horizontal(|ui| {
            ui.label(crate::tr!("Разнос по времени", "Time separation"));
            ui.add(
                egui::DragValue::new(separation)
                    .range(3..=MAX_TIME_WAR_SEPARATION)
                    .speed(2.0),
            );
        });
        ui.small(crate::tr!("Нечётное число от 3 до 255. Белые — на 0, чёрные — на N. Границы истории фиксированы.", "Odd number from 3 to 255. White starts at 0, Black at N. The timeline has fixed bounds."));
        if *separation % 2 == 0 {
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                crate::tr!("Введите нечётное число", "Enter an odd number"),
            );
        }
    }
}

pub fn rule_controls(ui: &mut egui::Ui, rules: &mut temporal_chess_engine::model::GameRules) {
    ui.checkbox(
        &mut rules.wait_before_move,
        crate::tr!("Свободное ожидание", "Free wait"),
    )
    .on_hover_text(crate::tr!(
        "Ожидание не передаёт ход. Авто-ожидание заблокированных фигур выключается; выход из петли остаётся автоматическим.",
        "Waiting does not spend a turn. Disables automatic waits for blocked pieces; loop exits still wait automatically."
    ));
    ui.checkbox(
        &mut rules.chain_info_activation,
        crate::tr!("Цепная инфоактивация", "Chain info-activation"),
    )
    .on_hover_text(crate::tr!(
        "Новая информация активирует каждое увидевшее её состояние мировой линии.",
        "New information activates every observing state of a worldline."
    ));
    let mut multiple = !rules.global_reaction_reset;
    ui.checkbox(&mut multiple,
        crate::tr!("Реагировать многократно", "React multiple"))
        .on_hover_text(crate::tr!(
            "Сохраняет реакции на досках позже совершённого хода. По умолчанию ход сбрасывает все реакции своей стороны.",
            "Keeps reactions on boards later than the move. By default, each turn clears all reactions of its side."
        ));
    rules.global_reaction_reset = !multiple;
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Pos2, Rect};
    use temporal_chess_engine::model::{Sq, TimeDirection};

    fn frame(ctx: &egui::Context, game: &mut GameUi, events: Vec<egui::Event>) -> egui::FullOutput {
        let key = egui::Id::new("test_art");
        let art = ctx
            .data_mut(|data| data.get_temp::<std::sync::Arc<PieceArt>>(key))
            .unwrap_or_else(|| {
                crate::theme::configure(ctx);
                let art = std::sync::Arc::new(PieceArt::new(ctx));
                ctx.data_mut(|data| data.insert_temp(key, art.clone()));
                art
            });
        let shell_key = egui::Id::new("test_shell");
        let mut shell = ctx
            .data_mut(|data| data.get_temp::<ShellState>(shell_key))
            .unwrap_or_default();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1260.0, 900.0))),
                events,
                ..Default::default()
            },
            |ui| draw(ui.ctx(), game, &mut shell, &art),
        );
        ctx.data_mut(|data| data.insert_temp(shell_key, shell));
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
        assert!(game.transition.is_some());
        game.reset_playback();
        game.finish_travel(1);
        frame(&ctx, &mut game, vec![]);
        assert_eq!(game.viewport.focused, Some(1));
        assert_eq!(game.viewport.scroll.as_ref().unwrap().target, 1);
        assert_eq!(game.viewport.times.first(), Some(&1));
        game.reset_playback();
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

    fn icon_position(ctx: &egui::Context, label: &str) -> Pos2 {
        ctx.data_mut(|data| data.get_temp::<Rect>(egui::Id::new(("icon", label))))
            .expect("icon button")
            .center()
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
        game.transition = None;
        frame(&ctx, &mut game, vec![]);
        let _output = frame(&ctx, &mut game, vec![]);
        let pos = icon_position(&ctx, "Копировать партию");
        let output = click(&ctx, &mut game, pos);
        let text = game.record.to_text();
        assert!(output.platform_output.commands.iter().any(
            |command| matches!(command, egui::OutputCommand::CopyText(value) if value == &text)
        ));
        let pos = icon_position(&ctx, "Загрузить партию из записи");
        click(&ctx, &mut game, pos);
        assert!(game.import_open);
        let output = frame(&ctx, &mut game, vec![]);
        let pos = text_position(&output, "NOLAN-CHESS 1");
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
