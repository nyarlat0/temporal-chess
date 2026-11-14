//! Presentation audio. Only confirmed live commands produce move sounds.
use crate::controller::GameUi;
use bevy::prelude::*;
use temporal_chess_engine::{
    engine::{Engine, GameOutcome, GameStatus, WinReason},
    model::{Color, TimeDirection},
};

pub enum Event {
    Reset,
    ReplayMove {
        direction: TimeDirection,
    },
    Action {
        waiting: u8,
        direction: Option<TimeDirection>,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Music {
    Silent,
    Waiting(u8),
    Finale(&'static str),
}
#[derive(Resource)]
pub struct Director {
    music: Music,
    waiting: u8,
    direction: TimeDirection,
    #[cfg(not(target_arch = "wasm32"))]
    sources: std::collections::HashMap<&'static str, Handle<AudioSource>>,
    #[cfg(not(target_arch = "wasm32"))]
    entity: Option<Entity>,
    #[cfg(not(target_arch = "wasm32"))]
    reversed: bool,
}
impl Default for Director {
    fn default() -> Self {
        Self {
            music: Music::Silent,
            waiting: 0,
            direction: TimeDirection::Future,
            #[cfg(not(target_arch = "wasm32"))]
            sources: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            entity: None,
            #[cfg(not(target_arch = "wasm32"))]
            reversed: false,
        }
    }
}
pub struct SoundPlugin;
impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Director>();
        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(Startup, load);
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn load(mut audio: ResMut<Director>, mut assets: ResMut<Assets<AudioSource>>) {
    macro_rules! sound {
        ($name:literal) => {
            audio.sources.insert(
                $name,
                assets.add(AudioSource {
                    bytes: std::sync::Arc::from(
                        include_bytes!(concat!("../assets/sounds/", $name, ".ogg")).as_slice(),
                    ),
                }),
            );
        };
    }
    sound!("move_01");
    sound!("move_02");
    sound!("move_03");
    sound!("move_04");
    sound!("move_reversed_01");
    sound!("move_reversed_02");
    sound!("move_reversed_03");
    sound!("move_reversed_04");
    sound!("victory");
    sound!("victory_reversed");
    sound!("defeat");
    sound!("defeat_reversed");
    sound!("no_future");
    sound!("no_future_reversed");
}
#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen]
    unsafe extern "C" {
        #[wasm_bindgen(js_namespace=globalThis,js_name=nolanMoveSound)]
        pub fn movement(reversed: bool);
        #[wasm_bindgen(js_namespace=globalThis,js_name=nolanMusic)]
        pub fn music(name: &str);
    }
}
fn bit(side: Color) -> u8 {
    if side == Color::White { 1 } else { 2 }
}
fn finale(outcome: GameOutcome, viewer: Option<Color>, direction: TimeDirection) -> &'static str {
    let won = match outcome {
        GameOutcome::Victory { winner, .. } => viewer.is_none_or(|s| s == winner),
        GameOutcome::Stalemate => false,
    };
    let direction = match outcome {
        GameOutcome::Victory {
            reason: WinReason::KingCaptured,
            ..
        } if viewer.is_some() => direction,
        _ => TimeDirection::Future,
    };
    match (won, direction) {
        (true, TimeDirection::Future) => "victory",
        (true, TimeDirection::Past) => "victory_reversed",
        (false, TimeDirection::Future) => "defeat",
        (false, TimeDirection::Past) => "defeat_reversed",
    }
}

pub(super) fn after_action(engine: &Engine, waiting: u8, actor: Color, spent: bool) -> u8 {
    if spent
        && engine.turn() == actor
        && engine.outcome().is_none()
        && engine.active_occurrences(actor.opposite()).next().is_none()
    {
        waiting | bit(actor.opposite())
    } else if !spent
        && engine.turn() != actor
        && engine.outcome().is_none()
        && engine.active_occurrences(actor).next().is_none()
    {
        waiting | bit(actor)
    } else {
        waiting
    }
}
fn refresh_waiting(engine: &Engine, mut waiting: u8, started: bool) -> u8 {
    // Absence on the non-moving side during an ordinary turn is not a pass.
    if let GameStatus::Waiting { player, .. } = engine.status()
        && started
        && engine.active_occurrences(player).next().is_none()
    {
        waiting |= bit(player);
    }
    for side in [Color::White, Color::Black] {
        if engine.active_occurrences(side).next().is_some() {
            waiting &= !bit(side);
        }
    }
    waiting
}

pub fn update(
    #[cfg(not(target_arch = "wasm32"))] mut commands: Commands,
    mut game: ResMut<GameUi>,
    mut audio: ResMut<Director>,
    #[cfg(not(target_arch = "wasm32"))] sinks: Query<&AudioSink>,
    #[cfg(not(target_arch = "wasm32"))] mut contexts: bevy_egui::EguiContexts,
) {
    for event in std::mem::take(&mut game.sound_events) {
        let movement = match event {
            Event::Reset => {
                audio.waiting = 0;
                audio.music = Music::Silent;
                audio.direction = game
                    .record
                    .commands()
                    .iter()
                    .rev()
                    .find_map(|c| c.spatial_move())
                    .map_or(TimeDirection::Future, |m| m.direction);
                #[cfg(not(target_arch = "wasm32"))]
                stop(&mut audio, &mut commands, &sinks);
                #[cfg(target_arch = "wasm32")]
                web::music("");
                None
            }
            Event::Action { waiting, direction } => {
                audio.waiting |= waiting;
                if let Some(direction) = direction {
                    audio.direction = direction;
                }
                direction
            }
            Event::ReplayMove { direction } => Some(direction),
        };
        if let Some(direction) = movement {
            let reversed = direction == TimeDirection::Past;
            #[cfg(not(target_arch = "wasm32"))]
            {
                let options = if reversed {
                    [
                        "move_reversed_01",
                        "move_reversed_02",
                        "move_reversed_03",
                        "move_reversed_04",
                    ]
                } else {
                    ["move_01", "move_02", "move_03", "move_04"]
                };
                let name = options[fastrand::usize(..options.len())];
                commands.spawn((
                    AudioPlayer::new(audio.sources[name].clone()),
                    PlaybackSettings::DESPAWN,
                ));
            }
            #[cfg(target_arch = "wasm32")]
            web::movement(reversed);
        }
    }
    audio.waiting = refresh_waiting(
        &game.engine,
        audio.waiting,
        !game.record.commands().is_empty(),
    );
    let viewer = if game.online.enabled {
        game.online.side
    } else {
        game.bot.settings.human_side()
    };
    let desired = if let Some(outcome) = game.engine.outcome() {
        // Draws use the regular defeat track; local/self-play victories use victory.
        let direction = if outcome == GameOutcome::Stalemate {
            TimeDirection::Future
        } else {
            audio.direction
        };
        Music::Finale(finale(outcome, viewer, direction))
    } else if audio.waiting != 0 {
        Music::Waiting(audio.waiting)
    } else {
        Music::Silent
    };
    if desired != audio.music {
        #[cfg(not(target_arch = "wasm32"))]
        {
            stop(&mut audio, &mut commands, &sinks);
            audio.reversed = false;
            let name = match desired {
                Music::Silent => None,
                Music::Waiting(_) => Some("no_future"),
                Music::Finale(name) => Some(name),
            };
            if let Some(name) = name {
                audio.entity = Some(
                    commands
                        .spawn((
                            AudioPlayer::new(audio.sources[name].clone()),
                            PlaybackSettings::ONCE,
                        ))
                        .id(),
                );
            }
        }
        #[cfg(target_arch = "wasm32")]
        web::music(match desired {
            Music::Silent => "",
            Music::Waiting(_) => "no_future",
            Music::Finale(name) => name,
        });
        audio.music = desired;
    }
    #[cfg(not(target_arch = "wasm32"))]
    if matches!(audio.music, Music::Waiting(_))
        && let Ok(ctx) = contexts.ctx_mut()
    {
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
    #[cfg(not(target_arch = "wasm32"))]
    if matches!(audio.music, Music::Waiting(_))
        && let Some(entity) = audio.entity
        && sinks.get(entity).is_ok_and(AudioSinkPlayback::empty)
    {
        commands.entity(entity).despawn();
        audio.reversed = !audio.reversed;
        let name = if audio.reversed {
            "no_future_reversed"
        } else {
            "no_future"
        };
        audio.entity = Some(
            commands
                .spawn((
                    AudioPlayer::new(audio.sources[name].clone()),
                    PlaybackSettings::ONCE,
                ))
                .id(),
        );
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn stop(audio: &mut Director, commands: &mut Commands, sinks: &Query<&AudioSink>) {
    if let Some(entity) = audio.entity.take() {
        if let Ok(sink) = sinks.get(entity) {
            sink.stop();
        }
        commands.entity(entity).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use temporal_chess_engine::engine::WinReason;
    #[test]
    fn endings_follow_perspective_and_temporal_direction() {
        let result = GameOutcome::Victory {
            winner: Color::Black,
            reason: WinReason::KingCaptured,
        };
        assert_eq!(
            finale(result, Some(Color::Black), TimeDirection::Future),
            "victory"
        );
        assert_eq!(
            finale(result, Some(Color::White), TimeDirection::Past),
            "defeat_reversed"
        );
        assert_eq!(finale(result, None, TimeDirection::Past), "victory");
        assert_eq!(
            finale(GameOutcome::Stalemate, None, TimeDirection::Future),
            "defeat"
        );
    }
    #[test]
    fn waiting_music_requires_a_forced_pass_and_stops_when_reserves_return() {
        use temporal_chess_engine::model::{PieceKind, Sq, State};
        let mut white = State::new(PieceKind::K { moved: true }, Color::White, Sq(0, 0));
        white.rights = Default::default();
        let mut tail = white.prolongate().unwrap();
        tail.rights = Default::default();
        let mut black = State::new(PieceKind::K { moved: true }, Color::Black, Sq(7, 7));
        black.t = 1;
        black.fut_seen = 1;
        black.rights.ordinary = true;
        let passed =
            Engine::from_worldlines(vec![vec![white, tail], vec![black]], Color::White).unwrap();
        assert_eq!(passed.turn(), Color::Black);
        assert_eq!(
            after_action(&passed, 0, Color::Black, true),
            bit(Color::White)
        );
        assert_eq!(after_action(&passed, 0, Color::Black, false), 0);
        let initial = Engine::default();
        assert_eq!(refresh_waiting(&initial, 0, false), 0);
        assert_eq!(refresh_waiting(&initial, bit(Color::White), true), 0);
        let mut game = GameUi::default();
        game.sound_events.clear();
        game.apply_confirmed("move 0 12:0:0 e2 e4 + normal".parse().unwrap())
            .unwrap();
        assert_eq!(
            refresh_waiting(
                &game.engine,
                after_action(&game.engine, 0, Color::White, true),
                true
            ),
            0
        );
        assert!(matches!(
            game.sound_events.as_slice(),
            [Event::Action {
                direction: Some(TimeDirection::Future),
                ..
            }]
        ));
        game.sound_events.clear();
        assert!(
            game.apply_confirmed("move 1 28:1:0 e7 e4 + normal".parse().unwrap())
                .is_err()
        );
        assert!(game.sound_events.is_empty());
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn every_embedded_ogg_decodes_without_an_audio_device() {
        use bevy::audio::Decodable;
        let mut app = App::new();
        app.init_resource::<Director>()
            .init_resource::<Assets<AudioSource>>()
            .add_systems(Startup, load);
        app.update();
        let assets = app.world().resource::<Assets<AudioSource>>();
        let sounds = app.world().resource::<Director>();
        assert_eq!(sounds.sources.len(), 14);
        for (name, handle) in &sounds.sources {
            assert!(
                assets.get(handle).unwrap().decoder().next().is_some(),
                "{name}"
            );
        }
    }
}
