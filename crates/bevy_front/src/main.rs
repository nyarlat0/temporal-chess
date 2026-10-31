mod animation;
mod board_view;
mod controller;
mod dialogs;
mod frontend;
mod i18n;
mod icons;
mod input;
mod multiplayer;
mod piece_art;
mod theme;
mod view;

#[cfg(not(target_arch = "wasm32"))]
use bevy::window::WindowResizeConstraints;

use bevy::{app::AppExit, prelude::*, window::WindowResolution};

fn main() {
    i18n::initialize();
    #[cfg(not(target_arch = "wasm32"))]
    let smoke = std::env::args().any(|arg| arg == "--smoke-test");
    #[cfg(target_arch = "wasm32")]
    let smoke = false;
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: crate::tr!("Temporal Chess · Темпоральные Шахматы", "Temporal Chess").into(),
            resolution: WindowResolution::new(1260, 900),
            #[cfg(target_arch = "wasm32")]
            canvas: Some("#game".into()),
            #[cfg(target_arch = "wasm32")]
            fit_canvas_to_parent: true,
            // Let browser clipboard shortcuts produce copy/cut/paste events.
            // The page suppresses other canvas shortcuts selectively.
            #[cfg(target_arch = "wasm32")]
            prevent_default_event_handling: false,
            #[cfg(not(target_arch = "wasm32"))]
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
