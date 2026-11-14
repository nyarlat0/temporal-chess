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
    pub sidebar_open: bool,
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
            sidebar_open: true,
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
    let drawer_duration = 0.45 / (crate::animation::BASE_ANIMATION_SPEED * game.animation_speed);
    if ctx.style_of(egui::Theme::Dark).animation_time != drawer_duration {
        ctx.style_mut_of(egui::Theme::Dark, |style| {
            style.animation_time = drawer_duration
        });
    }
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
    let mut sidebar_open = shell.sidebar_open;
    egui::Panel::right("game_sidebar")
        .default_size(340.0)
        .min_size(300.0)
        .max_size(400.0)
        .frame(egui::Frame::new().fill(PANEL).inner_margin(18))
        .show_collapsible(&mut root, &mut sidebar_open, |ui| {
            language_header(ui);
            let history_height =
                (ctx.viewport_rect().height() * 0.5).min((ui.available_height() - 90.0).max(100.0));
            let controls_height = (ui.available_height() - history_height - 16.0).max(60.0);
            egui::ScrollArea::vertical()
                .id_salt("sidebar_scroll")
                .max_height(controls_height)
                .auto_shrink([false, false])
                .show(ui, |ui| sidebar(ui, game, shell));
            ui.separator();
            details_panel(ui, game, shell, art, history_height);
        });
    shell.sidebar_open = sidebar_open;
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(BACKGROUND).inner_margin(12))
        .show(&mut root, |ui| {
            shell.ribbon_rect = ui.available_rect_before_wrap();
            let rect = crate::board_view::ribbon(ui, game, art);
            game_hud(ui, game, art, rect, modal);
        });
    menu_button(ctx, shell, modal);
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

fn menu_button(ctx: &egui::Context, shell: &mut ShellState, modal: bool) {
    egui::Area::new("sidebar_menu".into())
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::RIGHT_TOP, Vec2::new(-18.0, 18.0))
        .show(ctx, |ui| {
            if modal {
                ui.disable();
            }
            ui.visuals_mut().widgets.inactive.weak_bg_fill = PANEL.gamma_multiply(0.55);
            ui.visuals_mut().widgets.inactive.bg_fill = PANEL.gamma_multiply(0.55);
            if icon_button(
                ui,
                Icon::Menu,
                crate::tr!("Показать или скрыть панель", "Toggle sidebar"),
                false,
                true,
            )
            .clicked()
            {
                shell.sidebar_open = !shell.sidebar_open;
                ctx.request_repaint();
            }
        });
}

fn game_hud(
    parent: &mut egui::Ui,
    game: &mut GameUi,
    art: &PieceArt,
    rect: egui::Rect,
    modal: bool,
) {
    let idle = game.replay_job.is_none() && !game.playback_busy() && !game.facing.animating();
    let mut child = parent.new_child(
        egui::UiBuilder::new()
            .id_salt("game_status_hud")
            .max_rect(rect),
    );
    child.set_clip_rect(parent.clip_rect());
    let ui = &mut child;
    if modal {
        ui.disable();
    }
    egui::Frame::new()
        .fill(SURFACE)
        .corner_radius(12)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_max_width((rect.width() - 24.0).clamp(132.0, 172.0));
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.horizontal(|ui| {
                let side = game.displayed_turn();
                let checked = !game.checked_times_for(side).is_empty();
                let (king, response) =
                    ui.allocate_exact_size(Vec2::splat(45.0), egui::Sense::click());
                art.paint_styled(
                    ui.painter(),
                    PieceKind::K { moved: false },
                    side,
                    king,
                    1.0,
                    crate::piece_art::PieceStyle {
                        outline: if checked {
                            crate::piece_art::Outline::Check
                        } else {
                            crate::piece_art::Outline::None
                        },
                        ..Default::default()
                    },
                );
                if crate::input::clicked(&response) && idle {
                    game.navigate_check();
                }
                response.on_hover_text(crate::tr!("Перейти к шаху", "Go to check"));
                #[cfg(test)]
                ui.ctx()
                    .data_mut(|d| d.insert_temp(egui::Id::new("hud_king"), king));
                ui.allocate_exact_size(Vec2::new(8.0, 32.0), egui::Sense::hover());
                let (badge, _) = ui.allocate_exact_size(Vec2::splat(32.0), egui::Sense::hover());
                crate::board_view::paint_board_badge(ui.painter(), badge, game.displayed_turn());
                ui.painter()
                    .circle_filled(king.center().lerp(badge.center(), 0.5), 2.0, TEXT);
                ui.label(
                    egui::RichText::new(format!(
                        "x {}",
                        game.active_times_for(game.displayed_turn()).len()
                    ))
                    .size(18.0),
                );
            });
            if icon_button(
                ui,
                Icon::Replay,
                crate::tr!("Повторить предыдущий ход", "Replay previous move"),
                game.replaying_move,
                game.can_replay_move(),
            )
            .clicked()
            {
                game.replay_last_move();
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
                ui.horizontal_wrapped(|ui| {
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
                    ui.colored_label(REACTION, crate::tr_format!("Доска {t}", "Board {t}"));
                });
            }
            if !matches!(
                game.engine.status(),
                temporal_chess_engine::engine::GameStatus::Ongoing
            ) {
                ui.label(game.status_text());
            }
            if let Some(selection) = &game.selected {
                let piece = selection.piece;
                let direction = selection.direction;
                ui.separator();
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(36.0), egui::Sense::hover());
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
                    ui.label(format!(
                        "{} · ({}, {})",
                        piece_name(piece.kind),
                        square_name(piece.sq),
                        piece.t
                    ));
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
                        !game.playback_busy(),
                    )
                    .clicked()
                    {
                        game.toggle_direction();
                    }
                    ui.label(crate::controller::direction_name(direction));
                });
            }
        });
}

fn language_header(ui: &mut egui::Ui) {
    let mut language = crate::i18n::Language::current();
    ui.horizontal(|ui| {
        ui.set_min_height(38.0);
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
}

fn sidebar(ui: &mut egui::Ui, game: &mut GameUi, shell: &mut ShellState) {
    let idle = game.replay_job.is_none() && !game.playback_busy() && !game.facing.animating();
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
            if game.history_playback {
                Icon::Stop
            } else {
                Icon::Play
            },
            if game.history_playback {
                crate::tr!("Остановить просмотр", "Stop playback")
            } else {
                crate::tr!("Проиграть историю", "Play timeline")
            },
            game.history_playback,
            game.history_playback || (idle && game.engine.history().horizon() > 0),
        )
        .clicked()
        {
            if game.history_playback {
                game.reset_playback();
                game.viewport.scroll = None;
                game.jump_to = None;
            } else {
                game.play_history();
            }
        }
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
    ui.add(
        egui::Slider::new(&mut game.animation_speed, 0.25..=3.0)
            .step_by(0.05)
            .max_decimals(2)
            .suffix("×")
            .text(crate::tr!("Скорость анимаций", "Animation speed")),
    );
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
}

fn details_panel(
    ui: &mut egui::Ui,
    game: &mut GameUi,
    shell: &mut ShellState,
    art: &PieceArt,
    height: f32,
) {
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
            .max_height((height - 34.0).max(80.0))
            .min_scrolled_height((height - 34.0).max(80.0))
            .auto_shrink([false, false])
            .show(ui, |ui| crate::dialogs::inspector(ui, game));
    } else if game.record.commands().is_empty() {
        ui.allocate_ui(
            Vec2::new(ui.available_width(), (height - 34.0).max(80.0)),
            |ui| {
                ui.colored_label(
                    SUBTLE,
                    crate::tr!("Здесь появятся ходы партии", "Moves will appear here"),
                );
            },
        );
    } else {
        let playback_busy = game.playback_busy();
        let commands = game.record.commands();
        egui::ScrollArea::both()
            .id_salt("history_scroll")
            .max_height((height - 34.0).max(80.0))
            .min_scrolled_height((height - 34.0).max(80.0))
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
    fn live_waiting_focuses_source_before_starting_its_first_transition() {
        for spatial in [false, true] {
            let ctx = egui::Context::default();
            let mut game = GameUi::with_start(
                Default::default(),
                temporal_chess_engine::model::StartingPosition::TimeWar { separation: 9 },
            );
            frame(&ctx, &mut game, vec![]);
            frame(&ctx, &mut game, vec![]);
            // The player has scrolled to the chosen waiting destination.
            let row = game.viewport.times.iter().position(|&t| t == 6).unwrap();
            game.viewport.offset = row as f32 * game.viewport.stride;
            game.viewport.scroll = None;
            game.jump_to = None;
            game.click_square(0, Sq(4, 1));
            game.click_square(6, if spatial { Sq(4, 3) } else { Sq(4, 1) });
            assert_eq!(game.jump_to, Some(0));
            assert!(game.move_focus_pending && game.transition.is_none());
            let first = game.transition_queue.front().unwrap();
            assert_eq!((first.from_t, first.to_t), (0, 1));
            assert_eq!(first.tracks.iter().any(|p| p.from.sq != p.to.sq), spatial);
            for _ in 0..3 {
                frame(&ctx, &mut game, vec![]);
                assert_eq!(game.viewport.scroll.as_ref().unwrap().target, 0);
                assert!(game.transition.is_none());
                assert!(game.transition_queue.iter().all(|m| m.started.is_none()));
            }
            // Complete camera navigation without spending wall time in a test.
            game.viewport.scroll = None;
            let row = game.viewport.times.iter().position(|&t| t == 0).unwrap();
            game.viewport.offset = row as f32 * game.viewport.stride;
            frame(&ctx, &mut game, vec![]);
            let first = game.transition.as_ref().unwrap();
            assert_eq!((first.from_t, first.to_t), (0, 1));
            assert_eq!(game.viewport.motion_progress, 0.0);
            assert!(first.started.is_none());
            frame(&ctx, &mut game, vec![]);
            assert!(game.transition.as_ref().unwrap().started.is_some());
        }
    }

    #[test]
    fn wheel_scroll_settles_monotonically_and_stays_fixed_after_idle_frames() {
        let ctx = egui::Context::default();
        let mut game = GameUi::with_start(
            Default::default(),
            temporal_chess_engine::model::StartingPosition::TimeWar { separation: 9 },
        );
        game.jump_to = None;
        frame(&ctx, &mut game, vec![]);
        frame(&ctx, &mut game, vec![]);
        frame(
            &ctx,
            &mut game,
            vec![
                egui::Event::PointerMoved(Pos2::new(420.0, 300.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    phase: egui::TouchPhase::Move,
                    delta: Vec2::new(0.0, -500.37),
                    modifiers: Default::default(),
                },
            ],
        );
        let mut previous = game.viewport.offset;
        for _ in 0..150 {
            frame(&ctx, &mut game, vec![]);
            assert!(
                game.viewport.offset >= previous,
                "camera reversed at the end of a wheel scroll: {previous} -> {}",
                game.viewport.offset
            );
            previous = game.viewport.offset;
        }
        assert!(!game.viewport.wheel.active());
        assert!((previous - 500.37).abs() < 0.02);
        for _ in 0..20 {
            frame(&ctx, &mut game, vec![]);
            assert_eq!(game.viewport.offset, previous);
        }
    }

    #[test]
    fn sidebar_menu_reclaims_ribbon_space_without_changing_selection_or_game() {
        let ctx = egui::Context::default();
        let mut game = GameUi::default();
        game.click_square(0, Sq(4, 1));
        let engine = game.engine.clone();
        frame(&ctx, &mut game, vec![]);
        frame(&ctx, &mut game, vec![]);
        let key = egui::Id::new("test_shell");
        let before = ctx.data_mut(|d| d.get_temp::<ShellState>(key)).unwrap();
        let button = icon_position(
            &ctx,
            crate::tr!("Показать или скрыть панель", "Toggle sidebar"),
        );
        click(&ctx, &mut game, button);
        for _ in 0..40 {
            frame(&ctx, &mut game, vec![]);
        }
        let after = ctx.data_mut(|d| d.get_temp::<ShellState>(key)).unwrap();
        assert!(!after.sidebar_open);
        assert!(after.ribbon_rect.width() > before.ribbon_rect.width() + 250.0);
        assert!(game.selected.is_some());
        assert_eq!(game.engine, engine);
        click(
            &ctx,
            &mut game,
            icon_position(
                &ctx,
                crate::tr!("Показать или скрыть панель", "Toggle sidebar"),
            ),
        );
        frame(&ctx, &mut game, vec![]);
        assert!(
            ctx.data_mut(|d| d.get_temp::<ShellState>(key))
                .unwrap()
                .sidebar_open
        );
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
        game.reset_playback();
        game.local_auto_flip = false;
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
