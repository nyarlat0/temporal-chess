//! Local bot driver. Native search runs on the compute pool; WASM advances the
//! same search in small frame slices. Results are checked against their source
//! position before publishing through the normal controller/journal path.
use crate::controller::GameUi;
use bevy::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, futures::check_ready};
use bevy_egui::{EguiContexts, egui};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tempfish::{Limits, NODE_BUDGETS, Search, SearchResult};
use temporal_chess_engine::{engine::Engine, model::Color, record::Command};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub enabled: bool,
    pub human: Color,
    pub budget: usize,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            human: Color::White,
            budget: 1,
        }
    }
}
#[derive(Default)]
pub struct BotState {
    pub settings: Settings,
    pub paused: bool,
    pub thinking: bool,
    pub nodes: u64,
    pub depth: u8,
    pub move_now: bool,
    pub error: Option<String>,
}
impl BotState {
    pub fn owns_turn(&self, game: &GameUi) -> bool {
        self.settings.enabled && !game.online.enabled && game.engine.turn() != self.settings.human
    }
}

struct Job {
    root: Engine,
    settings: Settings,
    #[cfg(not(target_arch = "wasm32"))]
    task: Task<SearchResult>,
    #[cfg(not(target_arch = "wasm32"))]
    stop: Arc<AtomicBool>,
    #[cfg(not(target_arch = "wasm32"))]
    nodes: Arc<AtomicU64>,
    #[cfg(not(target_arch = "wasm32"))]
    depth: Arc<AtomicU64>,
    #[cfg(target_arch = "wasm32")]
    search: Search,
}
impl Drop for Job {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        self.stop.store(true, Ordering::Relaxed);
    }
}
impl Job {
    fn new(root: Engine, settings: Settings) -> Self {
        let search = Search::new(
            root.clone(),
            Limits {
                nodes: NODE_BUDGETS[settings.budget.min(NODE_BUDGETS.len() - 1)],
                ..Default::default()
            },
        );
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut search = search;
            let stop = Arc::new(AtomicBool::new(false));
            let nodes = Arc::new(AtomicU64::new(0));
            let depth = Arc::new(AtomicU64::new(0));
            let (cancel, count, completed) = (stop.clone(), nodes.clone(), depth.clone());
            let task = AsyncComputeTaskPool::get_or_init(TaskPool::new).spawn(async move {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    while !search.is_done() && !cancel.load(Ordering::Relaxed) {
                        search.step();
                        let result = search.result();
                        count.store(result.nodes, Ordering::Relaxed);
                        completed.store(result.completed_depth as u64, Ordering::Relaxed);
                    }
                    search.result()
                }));
                outcome.unwrap_or_else(|_| SearchResult {
                    first_error: Some("TempFish: engine panicked during search".into()),
                    ..Default::default()
                })
            });
            Self {
                root,
                settings,
                task,
                stop,
                nodes,
                depth,
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            Self {
                root,
                settings,
                search,
            }
        }
    }
    fn poll(&mut self, state: &mut BotState) -> Option<SearchResult> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if state.move_now {
                self.stop.store(true, Ordering::Relaxed);
            }
            state.nodes = self.nodes.load(Ordering::Relaxed);
            state.depth = self.depth.load(Ordering::Relaxed) as u8;
            check_ready(&mut self.task)
        }
        #[cfg(target_arch = "wasm32")]
        {
            if state.move_now {
                self.search.finish_now();
            }
            let start = web_time::Instant::now();
            while !self.search.is_done() && start.elapsed().as_millis() < 6 {
                self.search.step();
            }
            let result = self.search.result();
            state.nodes = result.nodes;
            state.depth = result.completed_depth;
            self.search.is_done().then_some(result)
        }
    }
}
struct Pending {
    root: Engine,
    settings: Settings,
    command: Command,
    focused: bool,
}
#[derive(Resource, Default)]
pub struct Runner {
    job: Option<Job>,
    pending: Option<Pending>,
}

pub fn poll(mut runner: ResMut<Runner>, mut game: ResMut<GameUi>, mut contexts: EguiContexts) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let allowed = game.bot.owns_turn(&game)
        && !game.bot.paused
        && game.engine.outcome().is_none()
        && game.replay_job.is_none()
        && !game.import_open
        && !game.confirm_restart;
    if !allowed {
        runner.job = None;
        if runner.pending.take().is_some() {
            game.reset_playback();
            game.jump_to = None;
            game.viewport.scroll = None;
        }
        game.bot.thinking = false;
        game.bot.move_now = false;
        return;
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(16));
    if runner
        .job
        .as_ref()
        .is_some_and(|job| job.settings != game.bot.settings || job.root != game.engine)
    {
        runner.job = None;
    }
    if let Some(pending) = &mut runner.pending {
        if pending.root != game.engine || pending.settings != game.bot.settings {
            runner.pending = None;
            game.reset_playback();
            game.jump_to = None;
            game.viewport.scroll = None;
            return;
        }
        if let Command::Move { mv, .. } = pending.command {
            if !pending.focused {
                pending.focused = true;
                game.remote_focus = Some(u64::MAX);
                game.playback_turn = Some(game.engine.turn());
                game.viewport.scroll = None;
                game.jump_to = Some(mv.t);
                return;
            }
            if game.jump_to.is_some() || game.viewport.scroll.is_some() {
                return;
            }
        }
        let command = pending.command;
        runner.pending = None;
        game.remote_focus = None;
        if let Err(error) = game.apply_confirmed(command) {
            game.bot.error = Some(error);
            game.bot.paused = true;
        }
        return;
    }
    if game.playback_busy() || game.facing.animating() {
        return;
    }
    if runner.job.is_none() {
        game.bot.error = None;
        game.bot.nodes = 0;
        game.bot.depth = 0;
        game.bot.move_now = false;
        game.clear_selection();
        runner.job = Some(Job::new(game.engine.clone(), game.bot.settings));
    }
    game.bot.thinking = true;
    let result = runner.job.as_mut().unwrap().poll(&mut game.bot);
    if let Some(result) = result {
        let job = runner.job.take().unwrap();
        game.bot.thinking = false;
        game.bot.move_now = false;
        game.bot.nodes = result.nodes;
        game.bot.depth = result.completed_depth;
        if job.root != game.engine {
            return;
        }
        if let Some(command) = result.command {
            runner.pending = Some(Pending {
                root: job.root.clone(),
                settings: job.settings,
                command,
                focused: false,
            });
        } else {
            game.bot.error = result.first_error;
            game.bot.paused = true;
        }
    }
}

pub fn settings(ui: &mut egui::Ui, config: &mut Settings) {
    ui.checkbox(
        &mut config.enabled,
        crate::tr!("Играть против TempFish", "Play against TempFish"),
    );
    if !config.enabled {
        return;
    }
    ui.horizontal(|ui| {
        ui.label(crate::tr!("Вы играете", "You play"));
        ui.selectable_value(
            &mut config.human,
            Color::White,
            crate::tr!("Белыми", "White"),
        );
        ui.selectable_value(
            &mut config.human,
            Color::Black,
            crate::tr!("Чёрными", "Black"),
        );
    });
    ui.add(
        egui::Slider::new(&mut config.budget, 0..=NODE_BUDGETS.len() - 1)
            .integer()
            .show_value(false)
            .text(crate::tr!("Бюджет поиска", "Search budget")),
    );
    ui.label(crate::tr_format!(
        "Узлов: {}",
        "Nodes: {}",
        NODE_BUDGETS[config.budget]
    ));
    ui.small(crate::tr!(
        "Больше узлов — сильнее игра, дольше расчёт.",
        "More nodes: stronger play, longer search."
    ));
}
pub fn panel(ui: &mut egui::Ui, game: &mut GameUi) {
    if game.online.enabled {
        return;
    }
    egui::CollapsingHeader::new("TempFish")
        .default_open(game.bot.settings.enabled)
        .show(ui, |ui| {
            let before = game.bot.settings;
            settings(ui, &mut game.bot.settings);
            if before != game.bot.settings {
                game.bot.paused = false;
                game.clear_selection();
            }
            if !game.bot.settings.enabled {
                return;
            }
            ui.horizontal(|ui| {
                ui.checkbox(&mut game.bot.paused, crate::tr!("Пауза бота", "Pause bot"));
                if game.bot.thinking
                    && ui
                        .add_enabled(
                            game.bot.depth > 0,
                            egui::Button::new(crate::tr!("Ход сейчас", "Move now")),
                        )
                        .clicked()
                {
                    game.bot.move_now = true;
                }
            });
            if game.bot.thinking {
                ui.spinner();
            }
            ui.small(crate::tr_format!(
                "Узлы: {} · глубина: {}",
                "Nodes: {} · depth: {}",
                game.bot.nodes,
                game.bot.depth
            ));
            if let Some(error) = &game.bot.error {
                ui.label(error);
            } else if game.bot.paused && game.bot.owns_turn(game) {
                ui.small(crate::tr!(
                    "Поиск на паузе. Можно увеличить бюджет и продолжить.",
                    "Search paused. You can increase the budget and resume."
                ));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use temporal_chess_engine::record::GameRecord;

    #[test]
    fn bot_turn_blocks_human_commands_and_undo_pauses_the_bot() {
        let mut game = GameUi::default();
        game.bot.settings.enabled = true;
        assert!(game.can_submit());
        game.bot.settings.human = Color::Black;
        assert!(!game.can_submit());
        game.bot.paused = true;
        assert!(
            !game.can_submit(),
            "pausing must not let the human play the bot's pieces"
        );
        game.bot.settings.human = Color::White;
        let command = "move 0 12:0:0 e2 e4 + normal".parse().unwrap();
        game.apply_confirmed(command).unwrap();
        game.reset_playback();
        game.bot.paused = false;
        game.undo_last_move();
        assert!(game.bot.paused);
        assert!(game.undo_in_progress());
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn native_job_returns_a_command_from_its_own_immutable_position() {
        let game = GameUi::default();
        let settings = Settings {
            enabled: true,
            human: Color::Black,
            budget: 0,
        };
        let mut job = Job::new(game.engine.clone(), settings);
        let mut state = BotState::default();
        let start = std::time::Instant::now();
        let result = loop {
            if let Some(result) = job.poll(&mut state) {
                break result;
            }
            assert!(start.elapsed().as_secs() < 30);
            std::thread::yield_now();
        };
        assert_eq!(job.root, game.engine);
        assert!(result.nodes <= NODE_BUDGETS[0]);
        let mut next = game.engine.clone();
        GameRecord::default()
            .apply(&mut next, result.command.unwrap())
            .unwrap();
    }
}
