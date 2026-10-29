mod animation;
mod board_view;
mod controller;
mod dialogs;
mod frontend;
mod icons;
mod input;
mod piece_art;
mod theme;
mod view;

use bevy::{
    app::AppExit,
    prelude::*,
    window::{WindowResizeConstraints, WindowResolution},
};

fn main() {
    let smoke = std::env::args().any(|arg| arg == "--smoke-test");
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Temporal Chess · Темпоральные Шахматы".into(),
            resolution: WindowResolution::new(1260, 900),
            resize_constraints: WindowResizeConstraints {
                min_width: 720.0,
                min_height: 520.0,
                ..default()
            },
            ..default()
        }),
        ..default()
    }))
    .add_plugins(frontend::ChessFrontendPlugin);
    if smoke {
        app.insert_resource(bevy::winit::WinitSettings::continuous());
        app.add_systems(Update, smoke_exit);
    }
    app.run();
}

// A bounded real-window render check, also useful on software Vulkan drivers.
fn smoke_exit(mut frames: Local<u32>, mut exit: MessageWriter<AppExit>) {
    *frames += 1;
    if *frames == 180 {
        exit.write(AppExit::Success);
    }
}
