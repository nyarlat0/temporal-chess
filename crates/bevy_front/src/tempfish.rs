//! Local bot driver. Native root workers and browser Web Workers search
//! concurrently; an incremental browser fallback remains available. Results are checked against their source
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
#[cfg(target_arch = "wasm32")]
use tempfish::Search;
use tempfish::{Limits, NODE_BUDGETS, SearchResult};
use temporal_chess_engine::{engine::Engine, model::Color, record::Command};

#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen]
    unsafe extern "C" {
        #[wasm_bindgen(catch,js_namespace=globalThis,js_name=tempfishStart)]
        pub fn start(record: &str, nodes: u32, threads: u32) -> Result<u32, JsValue>;
        #[wasm_bindgen(catch,js_namespace=globalThis,js_name=tempfishPoll)]
        pub fn poll(id: u32) -> Result<String, JsValue>;
        #[wasm_bindgen(js_namespace=globalThis,js_name=tempfishStop)]
        pub fn stop(id: u32);
        #[wasm_bindgen(js_namespace=globalThis,js_name=tempfishCancel)]
        pub fn cancel(id: u32);
    }
}
#[cfg(target_arch = "wasm32")]
#[derive(serde::Deserialize)]
struct WebResult {
    done: bool,
    command: Option<String>,
    score: i32,
    depth: u8,
    nodes: u64,
    cache_hits: u64,
    rejected: u64,
    error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub enabled: bool,
    pub human: Color,
    pub budget: usize,
    pub self_play: bool,
    pub white_budget: usize,
    pub black_budget: usize,
    /// Zero selects the available CPU count with a reserve for the interface.
    pub threads: usize,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            human: Color::White,
            budget: 1,
            self_play: false,
            white_budget: 1,
            black_budget: 1,
            threads: 0,
        }
    }
}
impl Settings {
    pub fn human_side(self) -> Option<Color> {
        (self.enabled && !self.self_play).then_some(self.human)
    }
    fn nodes(self, side: Color) -> u64 {
        let budget = if self.self_play {
            if side == Color::White {
                self.white_budget
            } else {
                self.black_budget
            }
        } else {
            self.budget
        };
        NODE_BUDGETS[budget.min(NODE_BUDGETS.len() - 1)]
    }
    fn worker_count(self) -> usize {
        if self.threads == 0 {
            automatic_threads()
        } else {
            self.threads.clamp(1, tempfish::parallel::MAX_THREADS)
        }
    }
}
fn automatic_threads() -> usize {
    #[cfg(not(target_arch = "wasm32"))]
    let available = std::thread::available_parallelism().map_or(1, |n| n.get().saturating_sub(1));
    #[cfg(target_arch = "wasm32")]
    let available = web_sys::window().map_or(1, |w| {
        (w.navigator().hardware_concurrency() as usize).saturating_sub(2)
    });
    available.clamp(1, tempfish::parallel::MAX_THREADS)
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
    pub started: Option<web_time::Instant>,
    pub elapsed: f64,
}
impl BotState {
    pub fn owns_turn(&self, game: &GameUi) -> bool {
        self.settings.enabled
            && !game.online.enabled
            && self.settings.human_side() != Some(game.engine.turn())
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
    search: Option<Search>,
    #[cfg(target_arch = "wasm32")]
    web_job: Option<u32>,
}
impl Drop for Job {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        self.stop.store(true, Ordering::Relaxed);
        #[cfg(target_arch = "wasm32")]
        if let Some(id) = self.web_job {
            web::cancel(id);
        }
    }
}
impl Job {
    fn new(
        root: Engine,
        settings: Settings,
        record: &temporal_chess_engine::record::GameRecord,
    ) -> Self {
        let limits = Limits {
            nodes: settings.nodes(root.turn()),
            ..Default::default()
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = record;
            let position = root.clone();
            let stop = Arc::new(AtomicBool::new(false));
            let nodes = Arc::new(AtomicU64::new(0));
            let depth = Arc::new(AtomicU64::new(0));
            let (cancel, count, completed) = (stop.clone(), nodes.clone(), depth.clone());
            let task = AsyncComputeTaskPool::get_or_init(TaskPool::new).spawn(async move {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let threads = settings.worker_count();
                    tempfish::parallel::run(position, limits, threads, &cancel, |result| {
                        count.store(result.nodes, Ordering::Relaxed);
                        completed.store(result.completed_depth as u64, Ordering::Relaxed);
                    })
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
            let web_job = web::start(
                &record.to_text(),
                limits.nodes as u32,
                settings.worker_count() as u32,
            )
            .ok();
            let search = web_job.is_none().then(|| Search::new(root.clone(), limits));
            Self {
                root,
                settings,
                search,
                web_job,
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
            if let Some(id) = self.web_job {
                if state.move_now {
                    web::stop(id);
                }
                match web::poll(id).and_then(|text| {
                    serde_json::from_str::<WebResult>(&text)
                        .map_err(|e| wasm_bindgen::JsValue::from_str(&e.to_string()))
                }) {
                    Ok(report) => {
                        state.nodes = report.nodes;
                        state.depth = report.depth;
                        if report.done {
                            return Some(SearchResult {
                                command: report.command.and_then(|s| s.parse().ok()),
                                score: report.score,
                                completed_depth: report.depth,
                                nodes: report.nodes,
                                cache_hits: report.cache_hits,
                                rejected: report.rejected,
                                first_error: report.error,
                            });
                        }
                        return None;
                    }
                    Err(_) => {
                        web::cancel(id);
                        self.web_job = None;
                        self.search = Some(Search::new(
                            self.root.clone(),
                            Limits {
                                nodes: self.settings.nodes(self.root.turn()),
                                ..Default::default()
                            },
                        ));
                    }
                }
            }
            let search = self.search.as_mut().expect("browser fallback");
            if state.move_now {
                search.finish_now();
            }
            let start = web_time::Instant::now();
            while !search.is_done() && start.elapsed().as_millis() < 6 {
                search.step();
            }
            state.nodes = search.nodes();
            state.depth = search.completed_depth();
            search.is_done().then(|| search.result())
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
        if let Some(started) = game.bot.started.take() {
            game.bot.elapsed = started.elapsed().as_secs_f64();
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
        if let Some(mv) = pending.command.spatial_move() {
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
        game.bot.started = Some(web_time::Instant::now());
        game.bot.elapsed = 0.;
        game.bot.nodes = 0;
        game.bot.depth = 0;
        game.bot.move_now = false;
        game.clear_selection();
        runner.job = Some(Job::new(
            game.engine.clone(),
            game.bot.settings,
            &game.record,
        ));
    }
    game.bot.thinking = true;
    if let Some(started) = game.bot.started {
        game.bot.elapsed = started.elapsed().as_secs_f64();
    }
    let result = runner.job.as_mut().unwrap().poll(&mut game.bot);
    if let Some(result) = result {
        let job = runner.job.take().unwrap();
        game.bot.thinking = false;
        if let Some(started) = game.bot.started.take() {
            game.bot.elapsed = started.elapsed().as_secs_f64();
        }
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
            game.bot.error = Some(result.first_error.unwrap_or_else(|| {
                crate::tr!(
                    "TempFish не нашёл применимого действия. Поиск остановлен.",
                    "TempFish found no applicable action. Search paused."
                )
                .into()
            }));
            game.bot.paused = true;
        }
    }
}

fn budget_control(ui: &mut egui::Ui, budget: &mut usize, label: &str) {
    ui.add(
        egui::Slider::new(budget, 0..=NODE_BUDGETS.len() - 1)
            .integer()
            .show_value(false)
            .text(label),
    );
    ui.label(crate::tr_format!(
        "Узлов: {}",
        "Nodes: {}",
        NODE_BUDGETS[(*budget).min(NODE_BUDGETS.len() - 1)]
    ));
}
pub fn settings(ui: &mut egui::Ui, config: &mut Settings) {
    let mut mode = if !config.enabled {
        0
    } else if config.self_play {
        2
    } else {
        1
    };
    let names = [
        crate::tr!("Без бота", "No bot"),
        crate::tr!("Против TempFish", "Against TempFish"),
        "TempFish vs TempFish",
    ];
    egui::ComboBox::from_label(crate::tr!("Режим игры", "Game mode"))
        .selected_text(names[mode])
        .show_ui(ui, |ui| {
            for (value, label) in names.into_iter().enumerate() {
                ui.selectable_value(&mut mode, value, label);
            }
        });
    config.enabled = mode != 0;
    config.self_play = mode == 2;
    if !config.enabled {
        return;
    }
    if config.self_play {
        ui.push_id("white_bot", |ui| {
            budget_control(ui, &mut config.white_budget, crate::tr!("Белые", "White"))
        });
        ui.push_id("black_bot", |ui| {
            budget_control(ui, &mut config.black_budget, crate::tr!("Чёрные", "Black"))
        });
    } else {
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
        budget_control(
            ui,
            &mut config.budget,
            crate::tr!("Бюджет поиска", "Search budget"),
        );
    }
    let mut automatic = config.threads == 0;
    if ui
        .checkbox(
            &mut automatic,
            crate::tr_format!(
                "Потоки автоматически: {}",
                "Automatic workers: {}",
                automatic_threads()
            ),
        )
        .changed()
    {
        config.threads = if automatic { 0 } else { automatic_threads() };
    }
    if !automatic {
        ui.add(
            egui::Slider::new(&mut config.threads, 1..=tempfish::parallel::MAX_THREADS)
                .integer()
                .text(crate::tr!("Потоки поиска", "Search workers")),
        );
    }
    ui.small(crate::tr!(
        "Больше узлов — сильнее игра, дольше расчёт. Каждый поток использует собственный кэш.",
        "More nodes: stronger play, longer search. Each worker uses its own cache."
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
                ui.checkbox(&mut game.bot.paused, crate::tr!("Пауза", "Pause"));
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
            if game.bot.settings.self_play {
                ui.label(crate::tr_format!(
                    "TempFish играет за {}",
                    "TempFish plays {}",
                    crate::controller::side_name(game.displayed_turn())
                ));
            }
            if game.bot.thinking {
                ui.spinner();
            }
            ui.small(crate::tr_format!(
                "Узлы: {} · глубина: {}",
                "Nodes: {} · depth: {}",
                game.bot.nodes,
                game.bot.depth
            ));
            if game.bot.elapsed > 0. {
                ui.small(crate::tr_format!(
                    "{:.1} с · {:.0} узлов/с",
                    "{:.1} s · {:.0} nodes/s",
                    game.bot.elapsed,
                    game.bot.nodes as f64 / game.bot.elapsed
                ));
            }
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
            threads: 2,
            ..Default::default()
        };
        let mut job = Job::new(game.engine.clone(), settings, &game.record);
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
    #[test]
    fn self_play_owns_both_colors_with_independent_budgets_and_pause_on_undo() {
        let mut game = GameUi::default();
        game.bot.settings = Settings {
            enabled: true,
            self_play: true,
            white_budget: 0,
            black_budget: 2,
            threads: 12,
            ..Default::default()
        };
        assert_eq!(game.bot.settings.worker_count(), 12);
        assert_eq!(game.bot.settings.nodes(Color::White), 50);
        assert_eq!(game.bot.settings.nodes(Color::Black), 800);
        assert!(game.bot.owns_turn(&game));
        assert!(!game.can_submit());
        game.apply_confirmed("move 0 12:0:0 e2 e4 + normal".parse().unwrap())
            .unwrap();
        game.reset_playback();
        assert!(game.bot.owns_turn(&game));
        assert!(!game.can_submit());
        assert_eq!(game.bot.settings.human_side(), None);
        game.undo_last_move();
        assert!(game.bot.paused);
        game.online.enabled = true;
        assert!(!game.bot.owns_turn(&game));
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn both_sides_can_run_from_the_same_driver_and_export_the_game() {
        let mut game = GameUi::default();
        let settings = Settings {
            enabled: true,
            self_play: true,
            white_budget: 0,
            black_budget: 0,
            threads: 2,
            ..Default::default()
        };
        game.bot.settings = settings;
        let mut sides = [false; 2];
        for _ in 0..8 {
            assert!(game.bot.owns_turn(&game));
            sides[usize::from(game.engine.turn() == Color::Black)] = true;
            let mut job = Job::new(game.engine.clone(), settings, &game.record);
            let mut state = BotState::default();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            let result = loop {
                if let Some(result) = job.poll(&mut state) {
                    break result;
                }
                assert!(std::time::Instant::now() < deadline);
                std::thread::yield_now();
            };
            game.apply_confirmed(result.command.unwrap()).unwrap();
            game.reset_playback();
            if game.engine.outcome().is_some() {
                break;
            }
        }
        assert!(sides.into_iter().all(|played| played));
        assert_eq!(
            GameRecord::load(&game.record.to_text()).unwrap().0,
            game.engine
        );
    }
}
