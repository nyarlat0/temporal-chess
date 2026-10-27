mod board_view;
mod controller;
mod piece_art;
mod view;

use bevy::{app::AppExit, prelude::*, window::WindowResolution};
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass};
use controller::GameUi;

fn main() {
    let smoke = std::env::args().any(|arg| arg == "--smoke-test");
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Temporal Chess · Темпоральные Шахматы".into(),
            resolution: WindowResolution::new(1260, 900),
            ..default()
        }),
        ..default()
    }))
    .add_plugins(EguiPlugin::default())
    .init_resource::<GameUi>()
    .add_systems(Startup, |mut commands: Commands| {
        commands.spawn(Camera2d);
    })
    .add_systems(EguiPrimaryContextPass, draw);
    if smoke {
        app.add_systems(Update, smoke_exit);
    }
    app.run();
}

fn draw(mut contexts: EguiContexts, mut game: ResMut<GameUi>) -> Result {
    view::draw(contexts.ctx_mut()?, &mut game);
    Ok(())
}

// A bounded real-window render check, also useful on software Vulkan drivers.
fn smoke_exit(mut frames: Local<u32>, mut exit: MessageWriter<AppExit>) {
    *frames += 1;
    if *frames == 180 {
        exit.write(AppExit::Success);
    }
}
