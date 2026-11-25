use crate::controller::GameUi;
use bevy_egui::egui;
use temporal_chess_engine::{
    engine::{Engine, LabEdit},
    model::{BoardTime, Color, PieceAt, PieceId, PieceKind, Sq, TimeDirection},
    record::GameRecord,
};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Place,
    Remove,
    RemoveState,
    Play,
}

pub struct Editor {
    pub tool: Tool,
    pub kind: PieceKind,
    pub color: Color,
    pub inverted: bool,
    pub boards: i64,
    pub board_to_remove: BoardTime,
    pub prolong_steps: usize,
    pub online: bool,
    pub undo: Vec<(Engine, GameRecord)>,
}
impl Default for Editor {
    fn default() -> Self {
        Self {
            tool: Tool::Place,
            kind: PieceKind::P { moved: false },
            color: Color::White,
            inverted: false,
            boards: 1,
            board_to_remove: 0,
            prolong_steps: 1,
            online: false,
            undo: Vec::new(),
        }
    }
}

impl GameUi {
    pub fn open_network_lab(&mut self) {
        let online = std::mem::take(&mut self.online);
        let mut editor = GameUi::with_start(
            online.rules,
            temporal_chess_engine::model::StartingPosition::Lab,
        );
        editor.cell_size = self.cell_size;
        editor.animation_speed = self.animation_speed;
        editor.local_auto_flip = self.local_auto_flip;
        editor.online = online;
        editor.lab.as_mut().unwrap().online = true;
        *self = editor;
    }
    pub fn lab_edit(&mut self, edit: LabEdit) -> bool {
        if self.lab.is_none() || self.playback_busy() || self.replay_job.is_some() {
            return false;
        }
        let before = self.engine.clone();
        match self.engine.edit_lab(edit) {
            Ok(()) => {
                self.lab
                    .as_mut()
                    .unwrap()
                    .undo
                    .push((before, self.record.clone()));
                self.record = GameRecord::from_lab(&self.engine);
                self.clear_selection();
                self.worldline_selected = None;
                self.worldline_state = None;
                self.worldline_transition = None;
                self.refresh_checks();
                self.last_move = None;
                true
            }
            Err(error) => {
                self.message(error.to_string(), true);
                false
            }
        }
    }
    pub fn lab_click(&mut self, t: BoardTime, square: Sq) -> bool {
        let Some(lab) = &self.lab else {
            return false;
        };
        let edit = match lab.tool {
            Tool::Play => return false,
            Tool::Place => LabEdit::Place {
                t,
                square,
                kind: lab.kind,
                color: lab.color,
                inverted: lab.inverted,
            },
            Tool::Remove | Tool::RemoveState => {
                let Some(piece) = self.pieces(t).into_iter().find(|p| p.sq == square) else {
                    return true;
                };
                if lab.tool == Tool::RemoveState {
                    LabEdit::RemoveState {
                        state: crate::controller::identity(piece),
                    }
                } else {
                    LabEdit::Remove { wid: piece.origin }
                }
            }
        };
        self.lab_edit(edit);
        true
    }
    pub fn start_lab(&mut self) {
        if self.lab.is_none() || self.playback_busy() || self.replay_job.is_some() {
            return;
        }
        let online = self.lab.as_ref().is_some_and(|lab| lab.online);
        self.engine.reset_lab_turn_streak();
        let record = GameRecord::from_lab(&self.engine);
        if online
            && record.lab_setup().unwrap().to_json().len()
                > temporal_chess_protocol::MAX_LAB_SETUP_LEN
        {
            self.message(
                crate::message!(
                    "Сеттинг больше сетевого лимита 16 МиБ.",
                    "The setting exceeds the 16 MiB online limit."
                ),
                true,
            );
            return;
        }
        self.reset_playback();
        self.clear_selection();
        self.inspect_mode = false;
        self.worldline_mode = false;
        self.worldline_selected = None;
        self.worldline_state = None;
        self.worldline_transition = None;
        self.record = record;
        self.lab = None;
        self.last_move = None;
        self.bot = crate::tempfish::BotState {
            settings: if online {
                Default::default()
            } else {
                self.new_game_bot
            },
            ..Default::default()
        };
        if online {
            self.online.create_from_lab(&self.record);
        }
        self.sound_events.push(crate::sound::Event::Reset);
    }
}

pub fn panel(ui: &mut egui::Ui, game: &mut GameUi, art: &crate::piece_art::PieceArt) {
    let Some(mut lab) = game.lab.take() else {
        return;
    };
    let previous_tool = lab.tool;
    let mut edit = None;
    let mut start = false;
    ui.heading("Lab");
    ui.label(crate::tr!(
        "Редактор стартового сеттинга",
        "Starting setting editor"
    ));
    let idle = !game.playback_busy() && game.replay_job.is_none();
    ui.add_enabled_ui(idle, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut lab.tool, Tool::Place, crate::tr!("Ставить", "Place"));
            ui.selectable_value(
                &mut lab.tool,
                Tool::Remove,
                crate::tr!("Убрать фигуру", "Erase piece"),
            );
            ui.selectable_value(
                &mut lab.tool,
                Tool::RemoveState,
                crate::tr!("Убрать состояние", "Erase state"),
            );
            ui.selectable_value(&mut lab.tool, Tool::Play, crate::tr!("Ходить", "Play"));
        });
        if lab.tool == Tool::Place {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut lab.color, Color::White, crate::tr!("Белые", "White"));
                ui.selectable_value(&mut lab.color, Color::Black, crate::tr!("Чёрные", "Black"));
            });
            ui.horizontal_wrapped(|ui| {
                for kind in [
                    PieceKind::P { moved: false },
                    PieceKind::N,
                    PieceKind::B,
                    PieceKind::R { moved: false },
                    PieceKind::Q,
                    PieceKind::K { moved: false },
                ] {
                    let response = ui
                        .add(
                            egui::Button::new("")
                                .selected(lab.kind == kind)
                                .min_size(egui::Vec2::splat(36.0)),
                        )
                        .on_hover_text(crate::view::piece_name(kind));
                    art.paint_styled(
                        ui.painter(),
                        kind,
                        lab.color,
                        response.rect.shrink(3.0),
                        1.0,
                        crate::piece_art::PieceStyle {
                            inverted: lab.inverted,
                            hovered: response.hovered(),
                            ..Default::default()
                        },
                    );
                    if response.clicked() {
                        lab.kind = kind;
                    }
                }
            });
            ui.checkbox(&mut lab.inverted, crate::tr!("Инвертированная", "Inverted"));
        }
        ui.small(match lab.tool {
            Tool::Place => crate::tr!(
                "Нажмите на пустую клетку. Каждая фигура — новая мировая линия.",
                "Click an empty cell. Each piece starts a new worldline."
            ),
            Tool::Remove => crate::tr!(
                "Нажмите на фигуру, чтобы удалить всю её мировую линию.",
                "Click a piece to erase its entire worldline."
            ),
            Tool::RemoveState => crate::tr!(
                "Нажмите на состояние: оно и его причинное продолжение удалятся.",
                "Click a state to erase it and its causal continuation."
            ),
            Tool::Play => crate::tr!(
                "Выбирайте фигуры любой стороны. Ходы и ожидание проверяются движком.",
                "Select either side's pieces. Moves and waits follow the engine rules."
            ),
        });
        ui.separator();
        ui.horizontal(|ui| {
            let label = crate::tr!("Добавить доски", "Add boards");
            ui.label(label);
            crate::mobile_input::integer(ui, &mut lab.boards, label, 1..=i64::MAX, 1.0);
        });
        ui.horizontal(|ui| {
            if ui
                .button(crate::tr!("В прошлое", "Into the past"))
                .clicked()
                && let Some(beginning) = game.engine.history().beginning().checked_sub(lab.boards)
            {
                edit = Some(LabEdit::Boards {
                    beginning,
                    horizon: game.engine.history().horizon(),
                });
                game.jump_to = Some(beginning);
            }
            if ui
                .button(crate::tr!("В будущее", "Into the future"))
                .clicked()
                && let Some(horizon) = game.engine.history().horizon().checked_add(lab.boards)
            {
                edit = Some(LabEdit::Boards {
                    beginning: game.engine.history().beginning(),
                    horizon,
                });
                game.jump_to = Some(horizon);
            }
        });
        ui.horizontal(|ui| {
            let label = crate::tr!("Удалить от доски", "Delete from board");
            ui.label(label);
            let beginning = game.engine.history().beginning();
            let horizon = game.engine.history().horizon();
            lab.board_to_remove = lab.board_to_remove.clamp(beginning, horizon);
            crate::mobile_input::integer(
                ui,
                &mut lab.board_to_remove,
                label,
                beginning..=horizon,
                1.0,
            );
        });
        let delete_label = if lab.board_to_remove < 0 {
            crate::tr!("Удалить эту и более старые", "Delete this and older boards")
        } else {
            crate::tr!("Удалить эту и более новые", "Delete this and newer boards")
        };
        if ui
            .add_enabled(lab.board_to_remove != 0, egui::Button::new(delete_label))
            .on_hover_text(crate::tr!(
                "Нулевая доска остаётся. Время остальных досок не меняется.",
                "Board zero stays. Surviving board times do not change."
            ))
            .clicked()
        {
            edit = Some(LabEdit::RemoveBoard {
                t: lab.board_to_remove,
            });
            game.jump_to = Some(0);
        }
        ui.horizontal(|ui| {
            let label = crate::tr!("Шагов продления линии", "Worldline extension steps");
            ui.label(label);
            crate::mobile_input::integer(ui, &mut lab.prolong_steps, label, 1..=usize::MAX, 1.0);
        });
        ui.horizontal(|ui| {
            ui.label(crate::tr!("Первый ход", "Side to move"));
            for side in [Color::White, Color::Black] {
                if ui
                    .selectable_label(
                        game.engine.turn() == side,
                        crate::controller::side_name(side),
                    )
                    .clicked()
                {
                    edit = Some(LabEdit::Turn(side));
                }
            }
        });
        egui::CollapsingHeader::new(crate::tr!("Все мировые линии", "All worldlines")).show(
            ui,
            |ui| {
                for (wid, line) in game.engine.history().worldlines().iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "#{} · {} · {}",
                            wid,
                            crate::view::piece_name(line[0].kind),
                            line.len()
                        ));
                        for (direction, label) in
                            [(TimeDirection::Past, "−t"), (TimeDirection::Future, "+t")]
                        {
                            let available = game
                                .engine
                                .lab_prolongation_source(wid, direction)
                                .is_some();
                            if ui
                                .add_enabled(available, egui::Button::new(label))
                                .on_hover_text(if direction == TimeDirection::Past {
                                    crate::tr!("Пролонгировать в прошлое", "Prolong into the past")
                                } else {
                                    crate::tr!(
                                        "Пролонгировать в будущее",
                                        "Prolong into the future"
                                    )
                                })
                                .clicked()
                            {
                                edit = Some(LabEdit::Prolong {
                                    wid,
                                    direction,
                                    count: lab.prolong_steps,
                                });
                            }
                        }
                        if ui
                            .small_button("×")
                            .on_hover_text(crate::tr!("Удалить мировую линию", "Delete worldline"))
                            .clicked()
                        {
                            edit = Some(LabEdit::Remove { wid });
                        }
                    });
                    egui::CollapsingHeader::new(crate::tr!("Состояния", "States"))
                        .id_salt(("lab_states", wid))
                        .show(ui, |ui| {
                            for state in line.iter() {
                                ui.horizontal(|ui| {
                                    ui.label(format!(
                                        "{} · {}{}{}",
                                        state.t,
                                        (b'a' + state.x as u8) as char,
                                        state.y + 1,
                                        if state.alive { "" } else { " · ×" }
                                    ));
                                    if ui
                                        .small_button("×")
                                        .on_hover_text(crate::tr!(
                                            "Удалить отсюда до причинного конца линии",
                                            "Delete from here to the causal end of the line"
                                        ))
                                        .clicked()
                                    {
                                        edit = Some(LabEdit::RemoveState {
                                            state: PieceAt {
                                                piece: PieceId::from_state(state, wid),
                                                t: state.t,
                                            },
                                        });
                                    }
                                });
                            }
                        });
                }
            },
        );
        ui.separator();
        crate::tempfish::creation_settings(ui, &mut game.new_game_bot, &mut lab.online);
        if lab.online {
            crate::multiplayer::lab_controls(ui, &mut game.online);
        }
        start = ui
            .button(crate::tr!("Начать партию", "Start game"))
            .clicked();
    });
    game.lab = Some(lab);
    if game.lab.as_ref().unwrap().tool != previous_tool {
        game.clear_selection();
    }
    if let Some(edit) = edit {
        game.lab_edit(edit);
    }
    if start {
        game.start_lab();
    }
    ui.separator();
}

#[cfg(test)]
mod tests {
    use super::*;
    use temporal_chess_engine::model::StartingPosition;

    #[test]
    fn editor_can_place_erase_undo_and_freeze_trial_moves_for_bot_play() {
        let mut game = GameUi::with_start(Default::default(), StartingPosition::Lab);
        game.lab_edit(LabEdit::Boards {
            beginning: -2,
            horizon: 6,
        });
        game.lab.as_mut().unwrap().kind = PieceKind::N;
        game.click_square(0, Sq(1, 0));
        assert_eq!(game.engine.history().worldlines().len(), 1);
        game.lab.as_mut().unwrap().tool = Tool::Remove;
        game.click_square(0, Sq(1, 0));
        assert!(game.engine.history().worldlines().is_empty());
        game.undo_last_move();
        assert_eq!(game.engine.history().worldlines().len(), 1);
        game.lab.as_mut().unwrap().tool = Tool::Play;
        game.click_square(0, Sq(1, 0));
        assert!(game.selected.is_some());
        game.click_square(0, Sq(2, 2));
        assert_eq!(game.record.commands().len(), 1);
        game.reset_playback();
        game.new_game_bot.enabled = true;
        game.new_game_bot.self_play = true;
        game.start_lab();
        assert!(game.lab.is_none());
        assert!(game.bot.settings.enabled && game.bot.settings.self_play);
        assert!(game.record.commands().is_empty());
        assert_eq!(game.engine.history().moves().len(), 1);
        assert_eq!(
            GameRecord::load(&game.record.to_text()).unwrap().0,
            game.engine
        );
    }

    #[test]
    fn editor_can_prolong_cut_a_state_and_cut_boards_with_undo() {
        let mut game = GameUi::with_start(Default::default(), StartingPosition::Lab);
        assert!(game.lab_edit(LabEdit::Boards {
            beginning: -2,
            horizon: 6
        }));
        game.lab.as_mut().unwrap().kind = PieceKind::N;
        game.click_square(0, Sq(1, 0));
        assert!(game.lab_edit(LabEdit::Prolong {
            wid: 0,
            direction: TimeDirection::Future,
            count: 4
        }));
        let before = game.engine.clone();
        game.lab.as_mut().unwrap().tool = Tool::RemoveState;
        game.click_square(2, Sq(1, 0));
        assert_eq!(game.engine.history().worldlines()[0].len(), 2);
        game.undo_last_move();
        assert_eq!(game.engine, before);
        assert!(game.lab_edit(LabEdit::RemoveBoard { t: 2 }));
        assert_eq!(game.engine.history().horizon(), 1);
        game.undo_last_move();
        assert_eq!(game.engine, before);
        assert!(!game.lab_edit(LabEdit::RemoveBoard { t: 0 }));
        assert_eq!(
            GameRecord::load(&game.record.to_text()).unwrap().0,
            game.engine
        );
    }

    #[test]
    fn online_start_uses_the_built_setting_and_locks_local_commands_immediately() {
        let mut game = GameUi::with_start(Default::default(), StartingPosition::Lab);
        game.lab_edit(LabEdit::Place {
            t: 0,
            square: Sq(4, 0),
            kind: PieceKind::K { moved: false },
            color: Color::White,
            inverted: true,
        });
        game.lab.as_mut().unwrap().online = true;
        game.start_lab();
        assert!(game.online.enabled && !game.can_submit());
        assert!(game.online.lab_setup.is_some());
        assert_eq!(
            GameRecord::load(&game.online.initial_record().to_text())
                .unwrap()
                .0,
            game.engine
        );
    }

    #[test]
    fn importing_into_the_editor_keeps_editing_and_folds_the_position_into_its_setting() {
        let mut game = GameUi::with_start(Default::default(), StartingPosition::Lab);
        game.import_text = GameRecord::default().to_text();
        game.begin_import();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while game.replay_job.is_some() {
            game.poll_replay();
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(game.lab.is_some());
        assert_eq!(
            game.engine.history().starting_position(),
            StartingPosition::Lab
        );
        assert_eq!(game.engine.history().worldlines().len(), 32);
        assert!(game.record.commands().is_empty());
        assert_eq!(
            GameRecord::load(&game.record.to_text()).unwrap().0,
            game.engine
        );
        game.undo_last_move();
        assert!(game.engine.history().worldlines().is_empty());
    }
}
