//! Bevy integration: initialize UI once, poll replay once per update, paint in
//! the egui schedule so layout can use multiple passes.
use crate::{
    controller::GameUi,
    piece_art::PieceArt,
    theme,
    view::{self, ShellState},
};
use bevy::{prelude::*, winit::WinitSettings};
use bevy_egui::{
    EguiContexts, EguiPlugin, EguiPrimaryContextPass, EguiStartupSet, PrimaryEguiContext,
};

pub struct ChessFrontendPlugin;

impl Plugin for ChessFrontendPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(EguiPlugin::default())
            .insert_resource(WinitSettings::desktop_app())
            .init_resource::<GameUi>()
            .init_resource::<ShellState>()
            .add_systems(
                PreStartup,
                setup_camera.before(EguiStartupSet::InitContexts),
            )
            .add_systems(Startup, configure_ui)
            .add_systems(Update, update_game)
            .add_systems(EguiPrimaryContextPass, draw);
    }
}

fn setup_camera(mut commands: Commands) {
    commands.spawn((Camera2d, PrimaryEguiContext));
}

fn configure_ui(mut contexts: EguiContexts, mut commands: Commands) -> Result {
    let ctx = contexts.ctx_mut()?;
    theme::configure(ctx);
    commands.insert_resource(PieceArt::new(ctx));
    Ok(())
}

fn update_game(mut game: ResMut<GameUi>) {
    game.apply_pending_input();
    if game.replay_job.is_some() {
        game.poll_replay();
    }
}

fn draw(
    mut contexts: EguiContexts,
    mut game: ResMut<GameUi>,
    mut shell: ResMut<ShellState>,
    art: Res<PieceArt>,
) -> Result {
    view::draw(contexts.ctx_mut()?, &mut game, &mut shell, &art);
    Ok(())
}
