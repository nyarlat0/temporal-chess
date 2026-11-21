//! Small, separated positions for cases that are difficult to see on a full
//! starting board. Every subsequent action goes through the normal journal.
use super::{Action, Point, at};
use temporal_chess_engine::{
    engine::{Engine, LabEdit},
    model::{
        BoardTime, Color, GameRules, MoveRights, PieceKind, State,
        TimeDirection::{Future, Past},
    },
    record::GameRecord,
};

pub(super) type Setup = Result<(Engine, GameRecord), String>;
use Color::{Black, White};
use PieceKind::{B, N};
const ROOK: PieceKind = PieceKind::R { moved: true };
const PAWN: PieceKind = PieceKind::P { moved: true };
const KING: PieceKind = PieceKind::K { moved: true };

fn state(kind: PieceKind, color: Color, point: Point, inverted: bool, ordinary: bool) -> State {
    let mut state = State::new(kind, color, point.sq);
    state.t = point.t;
    state.fut_seen = point.t;
    state.inverted = inverted;
    state.rights.ordinary = ordinary;
    state
}

fn line(kind: PieceKind, color: Color, from: Point, to: BoardTime, ordinary: bool) -> Vec<State> {
    let mut states = vec![state(kind, color, from, to < from.t, ordinary)];
    while states.last().unwrap().t != to {
        let mut next = states.last().unwrap().prolongate().unwrap();
        next.rights = MoveRights::default();
        states.push(next);
    }
    states
}

fn setting(lines: Vec<Vec<State>>, side: Color) -> Setup {
    let engine = Engine::from_worldlines(lines, side).map_err(|error| error.to_string())?;
    let engine = engine
        .lab_setup()
        .restore()
        .map_err(|error| error.to_string())?;
    let record = GameRecord::from_lab(&engine);
    Ok((engine, record))
}

fn stage(setup: Setup, actions: &[Action]) -> Setup {
    let (mut engine, mut record) = setup?;
    for &action in actions {
        let command = action.command(&engine)?.ok_or("Expected a setup move")?;
        record
            .apply(&mut engine, command)
            .map_err(|err| err.to_string())?;
    }
    Ok((engine, record))
}

pub(super) fn waiting() -> Setup {
    setting(
        vec![
            vec![state(ROOK, White, at(0, "a1"), false, true)],
            line(N, Black, at(1, "a3"), 6, false),
            vec![state(N, Black, at(1, "g8"), false, true)],
            vec![state(N, Black, at(3, "b1"), false, false)],
        ],
        White,
    )
}

pub(super) fn manual_wait() -> Setup {
    setting(
        vec![
            vec![state(ROOK, White, at(0, "a1"), false, true)],
            vec![state(N, Black, at(1, "g8"), false, true)],
            line(N, Black, at(3, "a1"), 6, false),
        ],
        White,
    )
}

pub(super) fn auto_wait() -> Setup {
    // Lab editor rebuilds the setup without running automatic waiting.
    let build = || -> Result<Engine, temporal_chess_engine::engine::EngineErr> {
        let mut engine = Engine::new_lab(GameRules::default())?;
        engine.edit_lab(LabEdit::Boards {
            beginning: 0,
            horizon: 6,
        })?;
        for (kind, color, point, inverted) in [
            (PAWN, White, at(0, "a8"), false),
            (N, Black, at(3, "a8"), true),
            (N, Black, at(1, "h8"), false),
        ] {
            engine.edit_lab(LabEdit::Place {
                t: point.t,
                square: point.sq,
                kind,
                color,
                inverted,
            })?;
        }
        engine.edit_lab(LabEdit::Turn(Black))?;
        Ok(engine)
    };
    let engine = build().map_err(|error| error.to_string())?;
    let record = GameRecord::from_lab(&engine);
    Ok((engine, record))
}

pub(super) fn sight() -> Setup {
    setting(
        vec![
            line(ROOK, White, at(0, "a1"), 4, false),
            line(N, White, at(0, "h1"), 4, false),
            vec![state(N, Black, at(3, "c5"), false, true)],
        ],
        Black,
    )
}

pub(super) fn replace_move() -> Setup {
    stage(
        setting(
            vec![
                line(ROOK, White, at(0, "a1"), 4, false),
                line(N, White, at(0, "h1"), 4, true),
                vec![state(N, Black, at(1, "c5"), false, true)],
            ],
            White,
        ),
        &[
            Action::mv(0, "h1", "f2", Future),
            Action::mv(1, "c5", "a4", Past),
        ],
    )
}

pub(super) fn reactions() -> Setup {
    let mut observer = line(ROOK, White, at(0, "a1"), 6, false);
    observer.last_mut().unwrap().rights.ordinary = true;
    let mut cancelled = line(N, White, at(0, "h1"), 5, false);
    cancelled[2].rights.cancelled_move = true;
    setting(
        vec![
            observer,
            cancelled,
            line(N, Black, at(6, "a5"), 0, false),
            line(N, White, at(0, "g8"), 7, false),
        ],
        White,
    )
}

pub(super) fn decline() -> Setup {
    setting(
        vec![
            line(ROOK, White, at(0, "a1"), 3, false),
            line(N, Black, at(0, "a5"), 3, false),
            vec![state(N, Black, at(3, "h8"), false, true)],
        ],
        White,
    )
}

pub(super) fn revival() -> Setup {
    let mut victim = line(N, Black, at(1, "a6"), 6, false);
    victim[2].rights.ordinary = true;
    let mut assassin = line(N, Black, at(3, "c2"), 1, false);
    assassin.last_mut().unwrap().rights.ordinary = true;
    stage(
        setting(
            vec![
                vec![state(ROOK, White, at(0, "a1"), false, true)],
                victim,
                assassin,
            ],
            Black,
        ),
        &[Action::mv(3, "a6", "b4", Future)],
    )
}

pub(super) fn capture_demo(inverted: bool) -> Setup {
    let victim = if inverted {
        line(N, Black, at(3, "a4"), 1, false)
    } else {
        line(N, Black, at(1, "a4"), 3, false)
    };
    setting(
        vec![vec![state(ROOK, White, at(2, "a1"), false, true)], victim],
        White,
    )
}

pub(super) fn capture_inverted() -> Setup {
    capture_demo(true)
}

pub(super) fn capture_ordinary() -> Setup {
    capture_demo(false)
}

pub(super) fn paradox() -> Setup {
    let mut rook = line(ROOK, White, at(0, "a1"), 8, false);
    for t in [0, 2, 4, 6, 8] {
        rook[t].rights.ordinary = true;
    }
    stage(
        setting(
            vec![
                rook,
                line(KING, White, at(0, "h1"), 8, false),
                vec![state(KING, Black, at(0, "h8"), true, false)],
            ],
            White,
        ),
        &[
            Action::mv(2, "a1", "a3", Future),
            Action::mv(4, "a3", "c3", Future),
        ],
    )
}

pub(super) fn reconciliation() -> Setup {
    let mut rook = line(ROOK, White, at(0, "a1"), 4, false);
    rook[2].rights.ordinary = true;
    stage(
        setting(
            vec![rook, vec![state(N, Black, at(1, "c5"), false, true)]],
            White,
        ),
        &[
            Action::mv(2, "a1", "a4", Future),
            Action::mv(1, "c5", "a4", Past),
        ],
    )
}

pub(super) fn cancel_return() -> Setup {
    setting(
        vec![
            vec![state(PAWN, White, at(2, "g4"), true, true)],
            vec![state(B, Black, at(1, "f6"), false, true)],
            line(N, Black, at(1, "h8"), 6, false),
        ],
        White,
    )
}

pub(super) fn passive() -> Setup {
    setting(
        vec![
            line(ROOK, White, at(0, "a1"), 3, false),
            vec![state(N, Black, at(5, "b3"), true, true)],
        ],
        Black,
    )
}
