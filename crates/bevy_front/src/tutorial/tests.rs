use super::*;
use temporal_chess_engine::{
    engine::{CalculationStatus, GameOutcome, WinReason},
    model::{Color, State},
};

fn game(chapter: usize, scene: usize, step: usize) -> GameUi {
    build_scene(Cursor {
        chapter,
        scene,
        step,
    })
    .unwrap()
}

fn last(chapter: usize, scene: usize) -> GameUi {
    game(
        chapter,
        scene,
        CHAPTERS[chapter].scenes[scene].steps.len() - 1,
    )
}

fn state(game: &GameUi, wid: usize, t: BoardTime) -> &State {
    game.engine.history().worldlines()[wid]
        .iter()
        .find(|state| state.t == t)
        .unwrap()
}

#[test]
fn every_checkpoint_replays_and_all_required_actions_are_available() {
    let mut failures = Vec::new();
    for (chapter, lesson) in CHAPTERS.iter().enumerate() {
        for (scene, setting) in lesson.scenes.iter().enumerate() {
            for (step, instruction) in setting.steps.iter().enumerate() {
                let cursor = Cursor {
                    chapter,
                    scene,
                    step,
                };
                let result = (|| -> Result<(), String> {
                    let mut game = build_scene(cursor)?;
                    if let Some(command) = instruction.action.command(&game.engine)? {
                        if command == Command::Continue
                            && game.engine.calculation_status() == CalculationStatus::Complete
                        {
                            return Err("Calculation is not paused".into());
                        }
                        game.apply_confirmed(command)?;
                    }
                    match instruction.action {
                        Action::Button(Control::Replay) if !game.can_replay_move() => {
                            Err("Replay is unavailable".into())
                        }
                        Action::Button(Control::Check)
                            if game.checked_times_for(game.engine.turn()).is_empty() =>
                        {
                            Err("No check to navigate to".into())
                        }
                        Action::Inspect(point)
                            if !(game.inspect_mode || game.worldline_mode)
                                || !game
                                    .pieces(point.t)
                                    .iter()
                                    .any(|piece| piece.sq == point.sq) =>
                        {
                            Err("Inspection is disabled or the target is missing".into())
                        }
                        _ => Ok(()),
                    }
                })();
                if let Err(error) = result {
                    failures.push(format!(
                        "{cursor:?} · {} · {:?}: {error}",
                        lesson.title.1, instruction.action
                    ));
                    break;
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn prior_checkpoints_resume_in_the_updated_lessons() {
    for (old, expected) in [
        (
            Cursor {
                chapter: 1,
                scene: 1,
                step: 2,
            },
            Cursor {
                chapter: 1,
                scene: 0,
                step: 14,
            },
        ),
        (
            Cursor {
                chapter: 5,
                scene: 0,
                step: 3,
            },
            Cursor {
                chapter: 5,
                scene: 0,
                step: 0,
            },
        ),
        (
            Cursor {
                chapter: 15,
                scene: 1,
                step: 5,
            },
            Cursor {
                chapter: 14,
                scene: 0,
                step: 0,
            },
        ),
    ] {
        assert_eq!(
            migrate_checkpoint(Checkpoint::Playing(old), 1),
            Checkpoint::Playing(expected)
        );
        assert_eq!(
            migrate_checkpoint(Checkpoint::Paused(old), 1),
            Checkpoint::Paused(expected)
        );
        build_scene(expected).unwrap();
    }
    assert_eq!(
        migrate_checkpoint(Checkpoint::Completed, 1),
        Checkpoint::Completed
    );
    let former_hogging = Cursor {
        chapter: 14,
        scene: 1,
        step: 5,
    };
    assert_eq!(
        migrate_checkpoint(Checkpoint::Paused(former_hogging), 3),
        Checkpoint::Paused(Cursor::chapter_start(14)),
    );
    let unchanged = Cursor {
        chapter: 13,
        scene: 1,
        step: 1,
    };
    assert_eq!(
        migrate_checkpoint(Checkpoint::Playing(unchanged), 3),
        Checkpoint::Playing(unchanged),
    );
    let former_recapture = Cursor {
        chapter: 15,
        scene: 3,
        step: 1,
    };
    assert_eq!(
        migrate_checkpoint(Checkpoint::Paused(former_recapture), 4),
        Checkpoint::Paused(Cursor {
            step: 0,
            ..former_recapture
        }),
    );
    let former_paradox = Cursor {
        chapter: 15,
        scene: 2,
        step: 3,
    };
    assert_eq!(
        migrate_checkpoint(Checkpoint::Playing(former_paradox), 4),
        Checkpoint::Playing(Cursor {
            step: 0,
            ..former_paradox
        }),
    );
    assert_eq!(
        migrate_checkpoint(Checkpoint::Playing(former_paradox), 5),
        Checkpoint::Playing(Cursor {
            step: 0,
            ..former_paradox
        }),
    );
    assert_eq!(
        migrate_checkpoint(Checkpoint::Paused(former_recapture), 5),
        Checkpoint::Paused(former_recapture),
    );
    assert_eq!(
        migrate_checkpoint(
            Checkpoint::Playing(Cursor {
                chapter: 13,
                scene: 2,
                step: 1
            }),
            2
        ),
        Checkpoint::Playing(Cursor::chapter_start(13)),
    );
}

#[test]
fn changing_chapters_and_skipping_return_to_the_suspended_game() {
    let mut live = GameUi::default();
    live.click_square(0, at(0, "e2").sq);
    live.click_square(0, at(0, "e4").sq);
    let suspended = live.engine.clone();
    start(&mut live, Cursor::FIRST);
    select_chapter(&mut live, 13);
    update_scene(&mut live);
    assert_eq!(
        live.tutorial.as_ref().unwrap().cursor,
        Cursor::chapter_start(13)
    );
    assert_eq!(live.engine.history().horizon(), 4);

    live.tutorial.as_mut().unwrap().leave = Some(Leave::Skip);
    update_scene(&mut live);
    assert!(live.tutorial.is_none());
    assert_eq!(live.engine, suspended);
}

#[test]
fn the_tactical_reply_plays_automatically_once_the_player_move_finishes() {
    let mut live = GameUi::default();
    let reply = Cursor {
        chapter: 13,
        scene: 0,
        step: 2,
    };
    start(&mut live, reply);
    assert!(live.engine.outcome().is_none());
    let command = live
        .tutorial
        .as_ref()
        .unwrap()
        .step()
        .action
        .command(&live.engine)
        .unwrap()
        .unwrap();
    assert!(!live.tutorial.as_ref().unwrap().accepts_input(command));
    update_scene(&mut live);
    assert_eq!(
        live.tutorial.as_ref().unwrap().cursor,
        reply.next().unwrap()
    );
    assert_eq!(
        live.engine.outcome(),
        Some(GameOutcome::Victory {
            winner: Color::Black,
            reason: WinReason::KingCaptured,
        })
    );
}

#[test]
fn lesson_commands_are_gated_and_replaying_a_checkpoint_rebuilds_the_position() {
    let mut live = GameUi::default();
    start(&mut live, Cursor::FIRST);
    live.click_square(0, at(0, "d2").sq);
    live.click_square(0, at(0, "d4").sq);
    assert!(live.record.commands().is_empty());
    assert_eq!(live.tutorial.as_ref().unwrap().cursor, Cursor::FIRST);
    live.click_square(0, at(0, "e2").sq);
    live.click_square(0, at(0, "e4").sq);
    let session = live.tutorial.as_ref().unwrap();
    assert_eq!(session.cursor.step, 1);
    let stored = serde_json::to_string(&Checkpoint::Playing(session.cursor)).unwrap();
    let Checkpoint::Playing(cursor) = serde_json::from_str(&stored).unwrap() else {
        panic!("Expected a resumable checkpoint");
    };
    assert_eq!(build_scene(cursor).unwrap().engine, live.engine);

    exit(&mut live, Leave::Pause);
    assert!(live.tutorial.is_none());
    assert!(live.record.commands().is_empty());
}

#[test]
fn spotlight_cells_accept_clicks_with_desktop_and_phone_layouts() {
    use bevy_egui::egui::{Event, PointerButton, Pos2, RawInput, Rect, Vec2};

    for size in [Vec2::new(1260.0, 900.0), Vec2::new(390.0, 720.0)] {
        let mut game = GameUi::default();
        let mut shell = crate::view::ShellState::default();
        let ctx = egui::Context::default();
        crate::theme::configure(&ctx);
        let art = crate::piece_art::PieceArt::new(&ctx);
        start(&mut game, Cursor::FIRST);
        let mut frame = |game: &mut GameUi, events| {
            game.apply_pending_input();
            let mut output = ctx.run_ui(
                RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
                    events,
                    ..Default::default()
                },
                |ui| crate::view::draw(ui.ctx(), game, &mut shell, &art),
            );
            output.textures_delta.clear();
        };

        for _ in 0..4 {
            frame(&mut game, Vec::new());
        }
        for _ in 0..2 {
            let openings = panel::openings(&ctx);
            assert_eq!(openings.len(), 1, "viewport {size:?}");
            let pos = openings[0].center();
            for pressed in [true, false] {
                frame(
                    &mut game,
                    vec![
                        Event::PointerMoved(pos),
                        Event::PointerButton {
                            pos,
                            button: PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ],
                );
            }
            frame(&mut game, Vec::new());
        }
        assert_eq!(game.record.commands().len(), 1, "viewport {size:?}");
        assert_eq!(game.tutorial.as_ref().unwrap().cursor.step, 1);
    }
}

#[test]
fn lesson_text_wraps_and_scrolls_inside_the_panel_with_controls_visible() {
    use bevy_egui::egui::{Event, MouseWheelUnit, Pos2, RawInput, Rect, Shape, Vec2};

    let cursor = Cursor {
        chapter: 14,
        scene: 0,
        step: CHAPTERS[14].scenes[0].steps.len() - 1,
    };
    let chapter = &CHAPTERS[cursor.chapter];
    let instruction = chapter.scenes[cursor.scene].steps[cursor.step].text;
    let find_text = |output: &egui::FullOutput, labels: [&str; 2]| {
        output
            .shapes
            .iter()
            .find_map(|clipped| {
                if let Shape::Text(text) = &clipped.shape
                    && labels.contains(&text.galley.job.text.as_str())
                {
                    Some((text.visual_bounding_rect(), clipped.clip_rect))
                } else {
                    None
                }
            })
            .expect("tutorial text is present")
    };

    let mut scrolled_long_text = false;
    for size in [
        Vec2::new(320.0, 420.0),
        Vec2::new(390.0, 720.0),
        Vec2::new(844.0, 390.0),
        Vec2::new(1100.0, 600.0),
    ] {
        let mut game = GameUi::default();
        let mut shell = crate::view::ShellState::default();
        let ctx = egui::Context::default();
        crate::theme::configure(&ctx);
        let art = crate::piece_art::PieceArt::new(&ctx);
        start(&mut game, cursor);
        let viewport = Rect::from_min_size(Pos2::ZERO, size);
        let mut frame = |events| {
            let mut output = ctx.run_ui(
                RawInput {
                    screen_rect: Some(viewport),
                    events,
                    ..Default::default()
                },
                |ui| crate::view::draw(ui.ctx(), &mut game, &mut shell, &art),
            );
            output.textures_delta.clear();
            output
        };
        frame(Vec::new());
        let output = frame(Vec::new());
        let (index, panel_rect) = output
            .shapes
            .iter()
            .enumerate()
            .find_map(|(i, clipped)| match &clipped.shape {
                Shape::Rect(rect) if rect.fill == crate::theme::PANEL => Some((i, rect.rect)),
                _ => None,
            })
            .expect("tutorial panel");
        assert!(
            viewport.expand(1.0).contains_rect(panel_rect),
            "{size:?}: {panel_rect:?}"
        );
        assert!(
            !output.shapes[index + 1..].iter().any(|clipped| {
                matches!(&clipped.shape, Shape::Rect(rect)
                if rect.fill == crate::theme::BACKGROUND && rect.rect == viewport)
            }),
            "{size:?}: a background layer covers the lesson panel"
        );

        let (title, title_clip) = find_text(&output, [chapter.title.0, chapter.title.1]);
        let (instructions, instructions_clip) = find_text(&output, [instruction.0, instruction.1]);
        let (next, next_clip) = find_text(&output, ["Далее", "Next"]);
        assert!(
            title_clip.expand(1.0).contains_rect(title),
            "{size:?}: {title:?} clipped by {title_clip:?}"
        );
        assert!(
            next_clip.expand(1.0).contains_rect(next),
            "{size:?}: {next:?} clipped by {next_clip:?}"
        );
        assert!(panel_rect.contains_rect(instructions_clip));
        assert!(instructions_clip.bottom() <= next.top());
        assert!(instructions.left() >= instructions_clip.left() - 1.0);
        assert!(instructions.right() <= instructions_clip.right() + 1.0);

        if instructions.bottom() > instructions_clip.bottom() + 1.0 {
            scrolled_long_text = true;
            let pointer = instructions_clip.center();
            frame(vec![
                Event::PointerMoved(pointer),
                Event::MouseWheel {
                    unit: MouseWheelUnit::Point,
                    phase: egui::TouchPhase::Move,
                    delta: Vec2::new(0.0, -400.0),
                    modifiers: Default::default(),
                },
            ]);
            let output = (0..30).map(|_| frame(Vec::new())).last().unwrap();
            let (scrolled, clip) = find_text(&output, [instruction.0, instruction.1]);
            assert!(scrolled.top() < instructions.top());
            assert!(scrolled.bottom() <= clip.bottom() + 2.0);
        }
    }
    assert!(scrolled_long_text);
}

#[test]
fn waiting_examples_cover_capture_blocking_and_automatic_collision() {
    let travelled = last(2, 0);
    assert_eq!(travelled.engine.turn(), Color::Black);
    assert!(state(&travelled, 0, 6).alive);
    assert!(
        travelled.engine.history().worldlines()[1]
            .iter()
            .all(|s| !s.alive)
    );

    let manual = game(2, 1, 0);
    let source = manual
        .engine
        .move_sources()
        .find(|p| p.origin == 0)
        .unwrap();
    assert_eq!(
        manual
            .engine
            .possible_skips(crate::controller::identity(source)),
        [2]
    );

    let auto = last(3, 0);
    assert_eq!(auto.engine.history().horizon(), 6);
    assert!(!state(&auto, 0, 3).alive);
    assert!(state(&auto, 1, 3).inverted);
}

#[test]
fn inversion_lesson_expands_the_past_and_crossing_a_pivot_closes_the_loop() {
    let first_inversion = game(1, 0, 1);
    assert_eq!(first_inversion.engine.history().beginning(), -2);
    assert!(
        first_inversion.engine.history().worldlines()[6]
            .iter()
            .any(|s| s.t == -2 && s.inverted)
    );
    let closed = last(1, 0);
    let knight = state(&closed, 6, 6);
    assert_eq!(Sq(knight.x, knight.y), at(6, "f3").sq);
    assert!(knight.rights.ordinary);
    assert!(
        closed
            .engine
            .history()
            .moves()
            .values()
            .any(|mv| mv.t == 2 && mv.pid.wid == 12)
    );
    assert!(
        closed
            .engine
            .history()
            .moves()
            .values()
            .any(|mv| mv.t == 4 && mv.pid.wid == 8)
    );
    let closing_move = closed.record.commands().last().unwrap();
    assert!(matches!(closing_move, Command::Move { mv, .. } if mv.t == 0 && mv.pid.wid == 6));
    assert_eq!(knight.loop_turn, 0);
    assert!(!knight.inverted);
}

#[test]
fn reactions_replace_a_move_and_reset_without_consuming_ordinary_rights() {
    let sight = game(4, 0, 4);
    assert!(state(&sight, 0, 2).rights.new_information);

    let replacement = last(4, 1);
    let slot = replacement
        .engine
        .history()
        .moves()
        .values()
        .find(|mv| mv.t == 0)
        .unwrap();
    assert_eq!(slot.pid.wid, 0);

    let after = last(6, 0);
    assert!(state(&after, 0, 6).rights.ordinary);
    assert!(!state(&after, 1, 2).rights.cancelled_move);
    assert!(!state(&after, 0, 0).rights.new_information);
    assert!(!state(&after, 0, 0).acknowledged_observations.is_empty());
    let decline = last(7, 0);
    assert_eq!(decline.engine.turn(), Color::Black);
    assert!(!state(&decline, 0, 0).rights.is_reaction());
}

#[test]
fn resurrecting_the_victim_restores_its_later_move() {
    let captured = game(8, 0, 1);
    assert!(!state(&captured, 1, 6).alive);
    let restored = last(8, 0);
    let endpoint = state(&restored, 1, 6);
    assert!(endpoint.alive);
    assert_eq!(Sq(endpoint.x, endpoint.y), at(6, "b4").sq);
}

#[test]
fn prepending_moves_cancels_turns_or_preserves_a_valid_later_move() {
    let paused = game(11, 0, 1);
    assert_eq!(
        paused.engine.calculation_status(),
        CalculationStatus::Paused {
            t: 1,
            side: Color::Black
        }
    );
    assert!(paused.engine.board(2).is_none() && paused.engine.preboard(2).is_some());
    let pawn = state(&paused, 28, 1);
    assert!(pawn.rights.cancelled_move && !pawn.rights.new_information);
    let bishop_moves = |g: &GameUi| {
        g.engine
            .history()
            .moves()
            .values()
            .any(|mv| mv.pid.wid == 21)
    };
    assert!(!bishop_moves(&last(9, 0)));
    assert!(bishop_moves(&last(11, 0)));

    let return_cancelled = last(9, 1);
    assert!(state(&return_cancelled, 0, 2).inverted);
    assert_eq!(return_cancelled.engine.history().worldlines()[0].len(), 1);

    let turned = last(9, 2);
    assert!(
        turned.engine.history().worldlines()[0]
            .iter()
            .all(|s| s.t <= 0)
    );
    let shifted = last(10, 0);
    let move_at_two = shifted
        .engine
        .history()
        .moves()
        .values()
        .find(|mv| mv.t == 2)
        .unwrap();
    let source = shifted.engine.piece_state(move_at_two.source()).unwrap();
    assert_eq!(Sq(source.x, source.y), at(2, "a3").sq);
    assert_eq!(move_at_two.sq, at(2, "a4").sq);
}

#[test]
fn tactical_examples_live_up_to_their_results() {
    assert!(!state(&last(12, 0), 0, 3).alive);
    assert!(!state(&last(12, 1), 1, 1).alive);
    assert!(!state(&last(12, 2), 1, 1).alive);

    assert_eq!(
        last(13, 0).engine.outcome(),
        Some(GameOutcome::Victory {
            winner: Color::Black,
            reason: WinReason::KingCaptured
        })
    );
    let undefended = game(13, 0, 2);
    assert_eq!(undefended.engine.outcome(), None);
    assert!(state(&undefended, 4, 4).alive);
    assert!(undefended.engine.board(4).unwrap().is_empty(at(4, "c3").sq));
    assert_eq!(state(&undefended, 10, 5).y, 2);

    let defended = last(13, 1);
    let blocker = defended.engine.board(4).unwrap().get_piece(3, 1).unwrap();
    assert_eq!(blocker.origin, 6);
    assert!(defended.engine.outcome().is_none());
    let checks = defended.engine.checks(Color::White).unwrap();
    assert!(
        checks
            .ordinary
            .iter()
            .chain(&checks.inverted)
            .all(|check| check.t != 4)
    );

    assert!(!state(&last(15, 0), 0, 3).alive);
    assert!(!state(&last(15, 1), 12, 0).alive);
    let paradox = last(15, 2);
    assert_eq!(
        paradox.record.commands().len() - game(15, 2, 0).record.commands().len(),
        3
    );
    assert_eq!(
        CHAPTERS[15].scenes[2]
            .steps
            .iter()
            .filter(|step| matches!(step.action, Action::Move { .. }) && !step.automatic)
            .count(),
        2
    );
    assert_eq!(
        paradox.engine.outcome(),
        Some(GameOutcome::Victory {
            winner: Color::Black,
            reason: WinReason::Paradox
        })
    );
}

#[test]
fn an_inverted_recapture_kills_a_friendly_pawn_that_was_alive_on_an_earlier_board() {
    let mut live = GameUi::default();
    start(
        &mut live,
        Cursor {
            chapter: 15,
            scene: 3,
            step: 0,
        },
    );
    assert_eq!(live.jump_to, Some(2));
    assert!(live.engine.history().beginning() < 2 && live.engine.history().horizon() > 4);
    let pawn = live
        .engine
        .board(2)
        .unwrap()
        .get_piece(1, 1)
        .copied()
        .unwrap();
    assert_eq!((pawn.origin, pawn.color), (9, Color::White));
    let attacker = live
        .engine
        .board(3)
        .unwrap()
        .get_piece(1, 1)
        .copied()
        .unwrap();
    assert_eq!(
        (attacker.origin, attacker.color, attacker.inverted),
        (21, Color::Black, true)
    );

    live.tutorial.as_mut().unwrap().advance();
    live.click_square(4, at(4, "c1").sq);
    live.toggle_direction();
    live.click_square(4, at(4, "b2").sq);

    let recapturer = live.engine.board(2).unwrap().get_piece(1, 1).unwrap();
    assert_eq!(
        (recapturer.origin, recapturer.color, recapturer.inverted),
        (2, Color::White, true)
    );
    assert!(
        live.engine
            .piece_state(crate::controller::identity(attacker))
            .is_some_and(|state| !state.alive)
    );
    assert!(
        live.engine
            .piece_state(crate::controller::identity(pawn))
            .is_some_and(|state| !state.alive)
    );
    assert!(state(&live, pawn.origin, 1).alive);
    let collision = live
        .transition_queue
        .iter()
        .find(|motion| (motion.from_t, motion.to_t) == (3, 2))
        .unwrap();
    assert!(
        collision
            .tracks
            .iter()
            .any(|track| track.dying && track.to.origin == pawn.origin)
    );

    let deaths: Vec<_> = live.first_deaths().collect();
    let inspection = deaths
        .iter()
        .find(|death| death.origin == pawn.origin)
        .unwrap();
    assert_eq!(
        (inspection.t, inspection.sq, inspection.color),
        (2, pawn.sq, Color::White)
    );
    let black_death = deaths
        .iter()
        .find(|death| death.origin == attacker.origin)
        .unwrap();
    assert_eq!(
        (black_death.t, black_death.sq, black_death.color),
        (3, attacker.sq, Color::Black)
    );
    live.reset_playback();
    live.inspect_death(*inspection);
    live.update_worldline_transition(&egui::Context::default());
    assert!(live.worldline_mode);
    assert_eq!(
        live.worldline_state,
        Some(crate::controller::identity(*inspection))
    );
    assert_eq!(live.jump_to, Some(2));
}

#[test]
fn inspection_and_replay_are_available_without_advancing_a_lesson() {
    let mut live = GameUi::default();
    start(&mut live, Cursor::FIRST);
    let cursor = live.tutorial.as_ref().unwrap().cursor;
    assert!(spotlight_square(&live, 0).is_some());
    live.toggle_inspector();
    live.tutorial_clicked(Control::Inspect);
    assert_eq!(spotlight_square(&live, 0), None);
    live.click_square(0, at(0, "e7").sq);
    assert_eq!(live.inspected_piece().unwrap().color, Color::Black);
    live.toggle_worldline();
    live.tutorial_clicked(Control::Worldline);
    live.click_square(0, at(0, "d2").sq);
    assert_eq!(live.worldline_selected, Some(11));
    assert_eq!(live.tutorial.as_ref().unwrap().cursor, cursor);
    live.toggle_worldline();

    let Action::Move { from, to, .. } = live.tutorial.as_ref().unwrap().step().action else {
        panic!("expected the first lesson to start with a move");
    };
    live.click_square(from.t, from.sq);
    live.click_square(from.t, to);
    live.reset_playback();
    let cursor = live.tutorial.as_ref().unwrap().cursor;
    let journal = live.record.to_text();
    live.replay_last_move();
    live.tutorial_clicked(Control::Replay);
    assert!(live.replaying_move);
    assert_eq!(live.tutorial.as_ref().unwrap().cursor, cursor);
    assert_eq!(live.record.to_text(), journal);
    live.reset_playback();
    live.toggle_history_playback();
    assert!(live.history_playback);
    live.toggle_history_playback();
    assert!(!live.playback_busy());
    assert_eq!(live.tutorial.as_ref().unwrap().cursor, cursor);
    assert_eq!(live.record.to_text(), journal);
}

#[test]
fn capturing_the_last_traveller_starts_a_player_driven_streak_in_the_past() {
    use temporal_chess_engine::{
        engine::TurnStreak,
        model::{PieceKind, StartingPosition},
    };

    let mut live = GameUi::default();
    start(&mut live, Cursor::chapter_start(14));
    assert_eq!(live.record.starting_position(), StartingPosition::Standard);
    assert_eq!(
        live.engine.turn_streak(),
        Some(TurnStreak {
            side: Color::Black,
            count: 1
        })
    );
    let attackers: Vec<_> = live.engine.active_occurrences(Color::Black).collect();
    assert_eq!(attackers.len(), 1);
    let victim = attackers[0];
    assert_eq!((victim.t, victim.sq), (3, at(3, "f4").sq));
    assert!(victim.inverted);

    for count in 1..=5 {
        let session = live.tutorial.as_ref().unwrap();
        let step = session.step();
        let Command::Move { mv, from } = step.action.command(&live.engine).unwrap().unwrap() else {
            panic!("expected a spatial move");
        };
        assert_eq!(mv.direction, TimeDirection::Past);
        assert!(session.accepts_input(Command::Move { mv, from }));
        live.click_square(mv.t, from);
        if !live.selected.as_ref().unwrap().direction.is_inverted() {
            live.toggle_direction();
        }
        live.click_square(mv.t, mv.sq);
        assert_eq!(live.engine.turn(), Color::White);
        assert_eq!(live.engine.history().horizon(), 6);
        assert!(
            live.engine
                .active_occurrences(Color::Black)
                .next()
                .is_none()
        );
        assert!(live.engine.outcome().is_none());
        assert_eq!(
            live.engine.turn_streak(),
            Some(TurnStreak {
                side: Color::White,
                count
            })
        );
        live.reset_playback();
    }
    assert!(
        live.engine
            .piece_state(crate::controller::identity(victim))
            .is_some_and(|s| !s.alive)
    );
    assert_eq!(
        live.engine
            .turn_streak()
            .unwrap()
            .disqualification_countdown(),
        Some(5)
    );
    let latest_board = live.engine.board(6).unwrap();
    assert!(latest_board.pieces().len() >= 30);
    assert_eq!(
        latest_board
            .pieces()
            .iter()
            .filter(|piece| matches!(piece.kind, PieceKind::K { .. }))
            .count(),
        2
    );
    assert_eq!(live.tutorial.as_ref().unwrap().step().action, Action::Next);
}

#[test]
fn passive_knight_collision_animates_the_rooks_death_on_arrival() {
    let mut live = GameUi::default();
    start(&mut live, Cursor::chapter_start(15));
    live.click_square(5, at(5, "b3").sq);
    live.click_square(5, at(5, "a1").sq);

    let check_clip = |game: &GameUi| {
        let first = &game.transition_queue[0];
        assert_eq!((first.from_t, first.to_t), (5, 4));
        assert!(first.tracks.iter().all(|track| !track.dying));

        let collision = &game.transition_queue[1];
        assert_eq!((collision.from_t, collision.to_t), (4, 3));
        let mut casualties = collision.tracks.iter().filter(|track| track.dying);
        let rook = casualties.next().unwrap();
        assert_eq!((rook.to.origin, rook.to.sq), (0, at(3, "a1").sq));
        assert!(!rook.visible_from && !rook.visible_to);
        assert!(casualties.next().is_none());
        assert!(collision.tracks.iter().any(|track| {
            track.to.origin == 1 && track.to.sq == rook.to.sq && track.visible_to
        }));
        assert!(collision.travel_fraction() < first.travel_fraction());
    };
    check_clip(&live);
    live.reset_playback();
    live.replay_last_move();
    check_clip(&live);
}
