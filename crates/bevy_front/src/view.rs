//! Application chrome. Board and piece rendering stay in their own modules.
use crate::{
    controller::GameUi,
    icons::{Icon, icon_button},
    piece_art::PieceArt,
    theme::*,
    tutorial::{self, Anchor, Control},
};
use bevy::prelude::Resource;
use bevy_egui::egui::{self, Vec2};
use temporal_chess_engine::{
    engine::{CalculationStatus, GameStatus},
    model::PieceKind,
};

#[derive(Resource, Clone)]
pub struct ShellState {
    pub sidebar_open: bool,
    pub help_open: bool,
    pub settings_open: bool,
    pub disable_waiting_music: bool,
    pub input_frame: Option<u64>,
    pub zoom_gesture: bool,
    pub suppress_wheel: bool,
    pub ribbon_rect: egui::Rect,
    copied_until: f64,
    initialized: bool,
    pub touch: crate::input::TouchNavigation,
}

impl Default for ShellState {
    fn default() -> Self {
        Self {
            sidebar_open: true,
            help_open: false,
            settings_open: false,
            disable_waiting_music: false,
            input_frame: None,
            zoom_gesture: false,
            suppress_wheel: false,
            ribbon_rect: egui::Rect::NOTHING,
            copied_until: 0.0,
            initialized: false,
            touch: Default::default(),
        }
    }
}

fn initial_sidebar(saved: Option<&str>, mobile: bool) -> bool {
    match saved {
        Some("open") => true,
        Some("closed") => false,
        _ => !mobile,
    }
}

pub(crate) fn stored_setting(key: &str) -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()?
            .local_storage()
            .ok()??
            .get_item(key)
            .ok()?
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = key;
        None
    }
}

pub(crate) fn store_setting(key: &str, value: &str) {
    #[cfg(target_arch = "wasm32")]
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = storage.set_item(key, value);
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (key, value);
}

fn saved_animation_speed(value: &str) -> Option<f32> {
    value
        .parse::<f32>()
        .ok()
        .filter(|v| v.is_finite() && (0.25..=3.0).contains(v))
}

fn mobile_browser() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.document_element())
            .is_some_and(|e| e.get_attribute("data-touch-device").as_deref() == Some("true"))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        false
    }
}

pub fn mobile_layout(ctx: &egui::Context) -> bool {
    let width = ctx.viewport_rect().width();
    mobile_browser()
        || width < 760.0
        || (width < 990.0
            && ctx.data(|d| {
                d.get_temp::<bool>("tutorial_layout".into())
                    .unwrap_or_default()
            }))
}

pub fn draw(ctx: &egui::Context, game: &mut GameUi, shell: &mut ShellState, art: &PieceArt) {
    if !shell.initialized {
        shell.sidebar_open = initial_sidebar(
            stored_setting("temporal-chess.sidebar").as_deref(),
            mobile_browser() || ctx.viewport_rect().width() < 760.0,
        );
        if let Some(flip) =
            stored_setting("temporal-chess.swap-boards").and_then(|v| v.parse::<bool>().ok())
        {
            game.local_auto_flip = flip;
        }
        if let Some(speed) = stored_setting("temporal-chess.animation-speed")
            .as_deref()
            .and_then(saved_animation_speed)
        {
            game.animation_speed = speed;
        }
        shell.disable_waiting_music = stored_setting("temporal-chess.disable-waiting-music")
            .and_then(|v| v.parse::<bool>().ok())
            .unwrap_or(false);
        shell.initialized = true;
        tutorial::on_first_draw(game);
    }
    tutorial::update_scene(game);
    let teaching = game.tutorial.is_some();
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("tutorial_layout"), teaching));
    tutorial::begin_pass(ctx);
    let previous_sidebar = shell.sidebar_open;
    let previous_flip = game.local_auto_flip;
    let previous_speed = game.animation_speed;
    let previous_mute = shell.disable_waiting_music;
    let compact = mobile_layout(ctx);
    crate::input::handle(ctx, game, shell);
    let drawer_duration = 0.45 / (crate::animation::BASE_ANIMATION_SPEED * game.animation_speed);
    if ctx.style_of(egui::Theme::Dark).animation_time != drawer_duration {
        ctx.style_mut_of(egui::Theme::Dark, |style| {
            style.animation_time = drawer_duration
        });
    }
    let viewport = ctx.viewport_rect();
    let mobile_drawer = if compact && !teaching {
        ctx.animate_bool_responsive(egui::Id::new("mobile_drawer_slide"), shell.sidebar_open)
    } else {
        0.0
    };
    let mut root = egui::Ui::new(
        ctx.clone(),
        "viewport".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(viewport),
    );
    let modal = game.import_open
        || game.confirm_restart
        || game.replay_job.is_some()
        || !game.promotion.is_empty()
        || !game.overlapping.is_empty()
        || shell.help_open;
    if modal {
        root.disable();
    }
    if compact {
        ctx.layer_painter(root.layer_id())
            .rect_filled(viewport, 0.0, BACKGROUND);
    }
    let lesson_rect = if teaching {
        let underneath = compact && (viewport.width() < 600.0 || viewport.height() >= 540.0);
        let panel = if underneath {
            egui::Panel::bottom("tutorial_panel").exact_size(
                (viewport.height() * 0.34)
                    .clamp(190.0, 270.0)
                    .min(viewport.height() * 0.5),
            )
        } else {
            egui::Panel::right("tutorial_panel").exact_size(if compact { 270.0 } else { 310.0 })
        };
        panel
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(if compact { 10 } else { 18 }),
            )
            .show(&mut root, |ui| tutorial::panel(ui, game, underneath))
            .response
            .rect
    } else {
        egui::Rect::NOTHING
    };
    if compact {
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(12, 0))
            .show(&mut root, |ui| {
                ui.set_min_width(ui.available_width());
                if shell.settings_open || mobile_drawer > 0.0 {
                    ui.disable();
                }
                ui.add_space(if teaching { 8.0 } else { 58.0 });
                let mut rect = ui.available_rect_before_wrap();
                let width = if has_hud_selection(game)
                    || game.first_deaths().next().is_some()
                    || game.engine.status() != GameStatus::Ongoing
                    || matches!(
                        game.engine.calculation_status(),
                        CalculationStatus::Paused { .. }
                    )
                    || game
                        .engine
                        .turn_streak()
                        .is_some_and(|streak| streak.disqualification_countdown().is_some())
                    || game.engine.can_decline_reactions()
                {
                    420.0
                } else {
                    184.0
                };
                rect = rect.shrink2(Vec2::new((rect.width() - width).max(0.0) * 0.5, 0.0));
                game_hud(ui, game, art, rect, modal);
                ui.allocate_space(Vec2::new(0.0, MOBILE_HUD_HEIGHT));
            });
    }
    let desktop_sidebar_drawn = if compact || teaching {
        false
    } else {
        let mut open = shell.sidebar_open;
        let drawer = egui::Panel::right("game_sidebar")
            .default_size(340.0)
            .min_size(300.0)
            .max_size(400.0)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(18))
            .show_collapsible(&mut root, &mut open, |ui| {
                sidebar_contents(ui, game, shell, art, modal)
            });
        shell.sidebar_open = open && !drawer.as_ref().is_some_and(|r| r.inner);
        drawer.is_some()
    };
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(if compact {
                    egui::Color32::TRANSPARENT
                } else {
                    BACKGROUND
                })
                .inner_margin(12),
        )
        .show(&mut root, |ui| {
            if shell.settings_open || mobile_drawer > 0.0 {
                ui.disable();
            }
            shell.ribbon_rect = if mobile_drawer > 0.0 {
                egui::Rect::NOTHING
            } else {
                ui.available_rect_before_wrap()
            };
            let rect = crate::board_view::ribbon(ui, game, art, &shell.touch);
            if !compact {
                game_hud(ui, game, art, rect, modal);
            }
        });
    if mobile_drawer > 0.0 {
        let position = viewport.min + Vec2::new(viewport.width() * (1.0 - mobile_drawer), 0.0);
        let close = egui::Area::new("mobile_sidebar".into())
            .order(egui::Order::Foreground)
            .fixed_pos(position)
            .constrain_to(viewport)
            .constrain(false)
            .default_size(viewport.size())
            .fade_in(false)
            .show(ctx, |ui| {
                ui.set_min_size(viewport.size());
                ui.set_max_size(viewport.size());
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(ui.min_rect().min, viewport.size()),
                    0.0,
                    PANEL,
                );
                egui::Frame::new()
                    .inner_margin(18)
                    .show(ui, |ui| {
                        if modal {
                            ui.disable();
                        }
                        sidebar_contents(ui, game, shell, art, modal)
                    })
                    .inner
            })
            .inner;
        if close {
            shell.sidebar_open = false;
            ctx.request_repaint();
        }
    } else if !desktop_sidebar_drawn && !teaching {
        menu_button(ctx, game, shell, modal);
    }
    if previous_sidebar != shell.sidebar_open {
        store_setting(
            "temporal-chess.sidebar",
            if shell.sidebar_open { "open" } else { "closed" },
        );
    }
    if previous_flip != game.local_auto_flip {
        store_setting(
            "temporal-chess.swap-boards",
            if game.local_auto_flip {
                "true"
            } else {
                "false"
            },
        );
    }
    if previous_speed != game.animation_speed {
        store_setting(
            "temporal-chess.animation-speed",
            &game.animation_speed.to_string(),
        );
    }
    if previous_mute != shell.disable_waiting_music {
        store_setting(
            "temporal-chess.disable-waiting-music",
            if shell.disable_waiting_music {
                "true"
            } else {
                "false"
            },
        );
    }
    if teaching && game.tutorial.is_some() && !modal {
        tutorial::paint_spotlight(
            ctx,
            lesson_rect,
            game.playback_busy() || game.facing.animating(),
        );
    }
    crate::dialogs::draw(ctx, game, shell, art);
    crate::multiplayer::undo_notification(ctx, game);
    if game.replay_job.is_some() {
        ctx.request_repaint_after(std::time::Duration::from_millis(80));
    }
    if game.playback_busy()
        || game.viewport.scroll.is_some()
        || game.jump_to.is_some()
        || game.pending_square.is_some()
        || game.pending_move.is_some()
    {
        ctx.request_repaint();
    }
}

fn sidebar_contents(
    ui: &mut egui::Ui,
    game: &mut GameUi,
    shell: &mut ShellState,
    art: &PieceArt,
    modal: bool,
) -> bool {
    let close = sidebar_header(ui, game, shell, modal, true);
    let compact = mobile_layout(ui.ctx());
    let history_height =
        (ui.ctx().viewport_rect().height() / 3.0).min((ui.available_height() - 90.0).max(100.0));
    let controls_height = (ui.available_height() - history_height - 16.0).max(60.0);
    let show_board = egui::ScrollArea::vertical()
        .id_salt("sidebar_scroll")
        .max_height(controls_height)
        .auto_shrink([false, compact])
        .show(ui, |ui| sidebar(ui, game, shell, art))
        .inner;
    ui.separator();
    let jumped = details_panel(ui, game, art, history_height);
    close || (compact && (show_board || jumped))
}

fn menu_button(ctx: &egui::Context, game: &mut GameUi, shell: &mut ShellState, modal: bool) {
    egui::Area::new("sidebar_menu".into())
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::RIGHT_TOP, Vec2::new(-18.0, 18.0))
        .show(ctx, |ui| {
            if modal {
                ui.disable();
            }
            ui.visuals_mut().widgets.inactive.weak_bg_fill = PANEL.gamma_multiply(0.55);
            ui.visuals_mut().widgets.inactive.bg_fill = PANEL.gamma_multiply(0.55);
            if sidebar_header(ui, game, shell, modal, false) {
                shell.sidebar_open = true;
                ctx.request_repaint();
            }
        });
}

fn sidebar_header(
    ui: &mut egui::Ui,
    game: &mut GameUi,
    shell: &mut ShellState,
    modal: bool,
    title: bool,
) -> bool {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.set_min_height(38.0);
        if title {
            let width = (ui.available_width() - 3.0 * 38.0 - 12.0).max(0.0);
            ui.add_sized(
                Vec2::new(width, 38.0),
                egui::Label::new(egui::RichText::new("Temporal Chess").size(18.0).strong())
                    .truncate(),
            );
        }
        if icon_button(
            ui,
            Icon::Help,
            crate::tr!("Управление и обозначения", "Controls and legend"),
            shell.help_open,
            !modal,
        )
        .clicked()
        {
            shell.help_open = true;
            shell.settings_open = false;
        }
        let settings = icon_button(
            ui,
            Icon::Settings,
            crate::tr!("Настройки", "Settings"),
            shell.settings_open,
            !modal,
        );
        if settings.clicked() {
            shell.settings_open = !shell.settings_open;
        }
        let mut open = shell.settings_open;
        egui::Popup::from_response(settings.response())
            .id("game_settings_popup".into())
            .open_bool(&mut open)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .width(280.0_f32.min(ui.ctx().viewport_rect().width() - 32.0))
            .show(|ui| {
                ui.strong(crate::tr!("Настройки", "Settings"));
                ui.separator();
                language_header(ui);
                ui.checkbox(
                    &mut game.local_auto_flip,
                    crate::tr!("Переворачивать доски", "Flip boards"),
                )
                .on_hover_text(crate::tr!(
                    "Автоматический поворот при смене хода в локальной игре.",
                    "Automatically flip boards between turns in local play."
                ));
                ui.checkbox(
                    &mut shell.disable_waiting_music,
                    crate::tr!("Выключить музыку ожидания", "Disable waiting music"),
                );
                ui.add(
                    egui::Slider::new(&mut game.animation_speed, 0.25..=3.0)
                        .step_by(0.05)
                        .max_decimals(2)
                        .suffix("×")
                        .text(crate::tr!("Скорость анимаций", "Animation speed")),
                );
            });
        shell.settings_open = open;
        icon_button(
            ui,
            if title && mobile_layout(ui.ctx()) {
                Icon::Close
            } else {
                Icon::Menu
            },
            crate::tr!("Показать или скрыть панель", "Toggle sidebar"),
            false,
            !modal,
        )
        .clicked()
    })
    .inner
}

pub(super) fn timeline_playback_button(ui: &mut egui::Ui, game: &GameUi, idle: bool) -> bool {
    let (icon, label) = if game.history_playback {
        (
            Icon::Stop,
            crate::tr!("Остановить просмотр", "Stop playback"),
        )
    } else {
        (Icon::Play, crate::tr!("Проиграть историю", "Play timeline"))
    };
    icon_button(
        ui,
        icon,
        label,
        game.history_playback,
        game.history_playback
            || (idle && game.engine.history().horizon() > game.engine.history().beginning()),
    )
    .clicked()
}

fn has_hud_selection(game: &GameUi) -> bool {
    game.selected.is_some()
        || ((game.inspect_mode || game.worldline_mode) && game.inspected_piece().is_some())
}

const GAME_HUD_RECT: &str = "game_hud_rect";
const HUD_SECTION_ICON_SIZE: f32 = 22.0;
const MOBILE_HUD_HEIGHT: f32 = 104.0;

pub(crate) fn hud_contains_pointer(ctx: &egui::Context, pos: egui::Pos2) -> bool {
    ctx.data(|d| d.get_temp::<egui::Rect>(egui::Id::new(GAME_HUD_RECT)))
        .is_some_and(|rect| rect.contains(pos))
}

fn game_hud(
    parent: &mut egui::Ui,
    game: &mut GameUi,
    art: &PieceArt,
    rect: egui::Rect,
    modal: bool,
) {
    let compact = mobile_layout(parent.ctx());
    let mut rect = rect;
    rect.max.y = if compact {
        rect.top() + MOBILE_HUD_HEIGHT
    } else {
        parent.clip_rect().bottom() - 12.0
    };

    let mut child = parent.new_child(
        egui::UiBuilder::new()
            .id_salt("game_status_hud")
            .max_rect(rect),
    );
    child.set_clip_rect(parent.clip_rect().intersect(rect));
    let ui = &mut child;
    if modal {
        ui.disable();
    }
    let frame = egui::Frame::new()
        .fill(SURFACE)
        .corner_radius(12)
        .inner_margin(egui::Margin::symmetric(12, if compact { 8 } else { 12 }))
        .show(ui, |ui| {
            let deaths: Vec<_> = game.first_deaths().collect();
            if compact {
                ui.set_width((rect.width() - 24.0).max(0.0));
                mobile_hud(ui, game, art, &deaths);
                return;
            }
            ui.set_max_width((rect.width() - 24.0).clamp(132.0, 172.0));
            ui.spacing_mut().item_spacing.x = 6.0;
            if deaths.is_empty() {
                desktop_hud_sections(ui, game, art);
            } else {
                egui::ScrollArea::vertical()
                    .id_salt("turn_panel")
                    .max_height(ui.available_height().max(0.0))
                    .min_scrolled_height(0.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        desktop_hud_sections(ui, game, art);
                        hud_deaths(ui, game, art, &deaths, false);
                    });
            }
        });
    ui.ctx().data_mut(|d| {
        let bounds = if compact { rect } else { frame.response.rect };
        d.insert_temp(egui::Id::new(GAME_HUD_RECT), bounds);
    });
}

fn mobile_hud(
    ui: &mut egui::Ui,
    game: &mut GameUi,
    art: &PieceArt,
    deaths: &[temporal_chess_engine::model::PieceView],
) {
    ui.spacing_mut().item_spacing.x = 4.0;
    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
    egui::ScrollArea::horizontal()
        .id_salt("mobile_turn_panel")
        .max_height(ui.available_height())
        .auto_shrink([false, false])
        .scroll_source(egui::scroll_area::ScrollSource::ALL)
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| hud_turn(ui, game, art, true));
                hud_turn_actions(ui, game, true);
                if has_hud_selection(game) {
                    ui.separator();
                    ui.vertical(|ui| hud_selection(ui, game, art, true));
                }
                if !deaths.is_empty() {
                    ui.separator();
                    hud_deaths(ui, game, art, deaths, true);
                }
            });
        });
}

fn desktop_hud_sections(ui: &mut egui::Ui, game: &mut GameUi, art: &PieceArt) {
    hud_turn(ui, game, art, false);
    hud_turn_actions(ui, game, false);
    if has_hud_selection(game) {
        ui.separator();
    }
    hud_selection(ui, game, art, false);
}

fn hud_deaths(
    ui: &mut egui::Ui,
    game: &mut GameUi,
    art: &PieceArt,
    deaths: &[temporal_chess_engine::model::PieceView],
    compact: bool,
) {
    use temporal_chess_engine::model::Color;

    if !compact {
        ui.separator();
    }
    ui.horizontal_top(|ui| {
        let (skull, response) =
            ui.allocate_exact_size(Vec2::splat(HUD_SECTION_ICON_SIZE), egui::Sense::hover());
        crate::icons::paint(
            ui.painter(),
            Icon::Skull,
            skull,
            egui::Stroke::new(1.5, TEXT),
        );
        response.on_hover_text(crate::tr!("Погибшие фигуры", "Dead pieces"));
        ui.vertical(|ui| {
            for side in [Color::White, Color::Black] {
                let mut pieces: Vec<_> =
                    deaths.iter().filter(|piece| piece.color == side).collect();
                if pieces.is_empty() {
                    continue;
                }
                pieces.sort_by_key(|piece| (piece.t, piece.origin));
                let layout =
                    egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(!compact);
                let tile_height = if compact { 34.0 } else { 42.0 };
                let icon_size = if compact { 21.0 } else { 27.0 };
                ui.allocate_ui_with_layout(
                    Vec2::new(ui.available_width(), tile_height),
                    layout,
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        ui.spacing_mut().interact_size.y = tile_height;
                        for piece in pieces {
                            let label = format!("({}, {})", square_name(piece.sq), piece.t);
                            let font = egui::FontId::proportional(11.0);
                            let label_width = ui.fonts_mut(|fonts| {
                                fonts
                                    .layout_no_wrap(label.clone(), font.clone(), TEXT)
                                    .size()
                                    .x
                            });
                            let (tile, response) = ui.allocate_exact_size(
                                Vec2::new((label_width + 6.0).max(37.0), tile_height),
                                egui::Sense::click(),
                            );
                            let at = crate::controller::identity(*piece);
                            if game.worldline_state == Some(at) || response.hovered() {
                                ui.painter().rect_filled(
                                    tile,
                                    5.0,
                                    ui.visuals().widgets.hovered.bg_fill,
                                );
                            }
                            art.paint_styled(
                                ui.painter(),
                                piece.kind,
                                side,
                                egui::Rect::from_center_size(
                                    tile.center_top() + Vec2::new(0.0, icon_size * 0.5 + 0.5),
                                    Vec2::splat(icon_size),
                                ),
                                1.0,
                                crate::piece_art::PieceStyle {
                                    inverted: piece.inverted,
                                    ..Default::default()
                                },
                            );
                            ui.painter().text(
                                tile.center_bottom() - Vec2::new(0.0, 1.0),
                                egui::Align2::CENTER_BOTTOM,
                                &label,
                                font,
                                TEXT,
                            );
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label)
                            });
                            if crate::input::clicked(&response)
                                && !game.playback_busy()
                                && !game.facing.animating()
                            {
                                game.inspect_death(*piece);
                            }
                            response.on_hover_text(format!(
                                "{} · {}",
                                piece_name(piece.kind),
                                label
                            ));
                        }
                    },
                );
            }
        });
    });
}

fn hud_turn(ui: &mut egui::Ui, game: &mut GameUi, art: &PieceArt, compact: bool) {
    let idle = game.replay_job.is_none() && !game.playback_busy() && !game.facing.animating();
    ui.horizontal(|ui| {
        let side = game.displayed_turn();
        let checked = !game.checked_times_for(side).is_empty();
        let (king, response) = ui.allocate_exact_size(
            Vec2::splat(if compact { 28.0 } else { 45.0 }),
            egui::Sense::click(),
        );
        tutorial::mark(ui.ctx(), game, Anchor::Turn, king);
        art.paint_styled(
            ui.painter(),
            PieceKind::K { moved: false },
            side,
            king,
            1.0,
            crate::piece_art::PieceStyle {
                outline: if checked {
                    crate::piece_art::Outline::Check
                } else {
                    crate::piece_art::Outline::None
                },
                ..Default::default()
            },
        );
        if crate::input::clicked(&response) && idle && game.tutorial_tool_enabled(Control::Check) {
            game.navigate_check();
            if checked {
                game.tutorial_clicked(Control::Check);
            }
        }
        response.on_hover_text(crate::tr!("Перейти к шаху", "Go to check"));
        #[cfg(test)]
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::new("hud_king"), king));
        ui.allocate_exact_size(
            Vec2::new(if compact { 5.0 } else { 8.0 }, 28.0),
            egui::Sense::hover(),
        );
        let (badge, _) = ui.allocate_exact_size(
            Vec2::splat(if compact { 24.0 } else { 32.0 }),
            egui::Sense::hover(),
        );
        crate::board_view::paint_board_badge(ui.painter(), badge, game.displayed_turn());
        ui.painter()
            .circle_filled(king.center().lerp(badge.center(), 0.5), 2.0, TEXT);
        let count = ui.label(
            egui::RichText::new(format!(
                "x {}",
                game.active_times_for(game.displayed_turn()).len()
            ))
            .size(if compact { 16.0 } else { 18.0 }),
        );
        tutorial::mark(ui.ctx(), game, Anchor::Activity, badge.union(count.rect));
    });
    // Columns justify direct children across their width. A button row keeps
    // the replay control square instead of stretching it with the column.
    ui.horizontal(|ui| {
        let replay = icon_button(
            ui,
            Icon::Replay,
            crate::tr!("Повторить предыдущий ход", "Replay previous move"),
            game.replaying_move,
            game.can_replay_move(),
        );
        tutorial::mark(
            ui.ctx(),
            game,
            Anchor::Control(Control::Replay),
            replay.response().rect,
        );
        if replay.clicked() {
            game.replay_last_move();
            game.tutorial_clicked(Control::Replay);
        }
        let inspect = icon_button(
            ui,
            Icon::Inspect,
            crate::tr!("Инспекция фигуры · I", "Inspect piece · I"),
            game.inspect_mode,
            idle,
        );
        tutorial::mark(
            ui.ctx(),
            game,
            Anchor::Control(Control::Inspect),
            inspect.response().rect,
        );
        if inspect.clicked() {
            game.toggle_inspector();
            game.tutorial_clicked(Control::Inspect);
        }
    });
}

fn hud_turn_actions(ui: &mut egui::Ui, game: &mut GameUi, compact: bool) {
    let idle = game.replay_job.is_none() && !game.playback_busy() && !game.facing.animating();
    if game.engine.can_decline_reactions() {
        let decline = ui
            .add_enabled(
                idle && game.can_submit() && game.tutorial_tool_enabled(Control::Decline),
                egui::Button::new(crate::tr!("Не реагировать", "Decline reactions")),
            )
            .on_hover_text(crate::tr!(
                "Деактивировать все свои реакции и передать ход сопернику.",
                "Deactivate all your reactions and hand the turn to your opponent."
            ));
        tutorial::mark(
            ui.ctx(),
            game,
            Anchor::Control(Control::Decline),
            decline.rect,
        );
        if decline.clicked() {
            game.decline_reactions();
        }
    }
    if let CalculationStatus::Paused { t, .. } = game.engine.calculation_status() {
        let layout = egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(!compact);
        let size = Vec2::new(ui.available_width(), ui.spacing().interact_size.y);
        ui.allocate_ui_with_layout(size, layout, |ui| {
            let resume = icon_button(
                ui,
                Icon::Continue,
                crate::tr!("Продолжить просчёт · C", "Continue calculation · C"),
                false,
                idle && game.can_submit() && game.tutorial_tool_enabled(Control::Continue),
            );
            tutorial::mark(
                ui.ctx(),
                game,
                Anchor::Control(Control::Continue),
                resume.response().rect,
            );
            if resume.clicked() {
                game.continue_calculation();
            }
            ui.colored_label(REACTION, crate::tr_format!("Доска {t}", "Board {t}"));
        });
    }
    if game.lab.is_none()
        && game.engine.outcome().is_none()
        && let Some(streak) = game.engine.turn_streak()
        && let Some(remaining) = streak.disqualification_countdown()
    {
        let countdown = ui.colored_label(
            REACTION,
            egui::RichText::new(crate::tr_format!(
                "{} · до дисквалификации {}",
                "{} · {} until disqualification",
                crate::controller::side_name(streak.side),
                remaining
            ))
            .small(),
        );
        tutorial::mark(ui.ctx(), game, Anchor::Streak, countdown.rect);
    }
    if game.engine.status() != GameStatus::Ongoing {
        ui.label(game.status_text());
    }
}

fn hud_selection(ui: &mut egui::Ui, game: &mut GameUi, art: &PieceArt, compact: bool) {
    if game.inspect_mode || game.worldline_mode {
        if let Some(piece) = game.inspected_piece() {
            let details = ui.scope(|ui| inspection_hud(ui, game, art, piece, compact));
            tutorial::mark(ui.ctx(), game, Anchor::Inspection, details.response.rect);
        }
        return;
    }
    if let Some(selection) = &game.selected {
        let piece = selection.piece;
        let direction = selection.direction;
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(
                Vec2::splat(if compact { 28.0 } else { 36.0 }),
                egui::Sense::hover(),
            );
            #[cfg(test)]
            ui.ctx()
                .data_mut(|d| d.insert_temp(egui::Id::new("hud_selected_piece"), rect));
            art.paint_styled(
                ui.painter(),
                piece.kind,
                piece.color,
                rect,
                1.0,
                crate::piece_art::PieceStyle {
                    inverted: piece.inverted,
                    ..Default::default()
                },
            );
            ui.label(if compact {
                format!("({}, {})", square_name(piece.sq), piece.t)
            } else {
                format!(
                    "{} · ({}, {})",
                    piece_name(piece.kind),
                    square_name(piece.sq),
                    piece.t
                )
            });
        });
        ui.horizontal(|ui| {
            let direction_button = icon_button(
                ui,
                if direction == temporal_chess_engine::model::TimeDirection::Future {
                    Icon::Future
                } else {
                    Icon::Past
                },
                crate::tr!(
                    "Сменить направление · СКМ",
                    "Change direction · middle mouse button"
                ),
                false,
                !game.playback_busy(),
            );
            tutorial::mark(
                ui.ctx(),
                game,
                Anchor::Control(Control::Direction),
                direction_button.response().rect,
            );
            if direction_button.clicked() {
                game.toggle_direction();
                game.tutorial_clicked(Control::Direction);
            }
            ui.label(crate::controller::direction_name(direction));
        });
    }
}

fn inspection_hud(
    ui: &mut egui::Ui,
    game: &GameUi,
    art: &PieceArt,
    piece: temporal_chess_engine::model::PieceView,
    compact: bool,
) {
    let Some(state) = game.engine.piece_state(crate::controller::identity(piece)) else {
        return;
    };
    ui.scope(|ui| {
        let show_clock = piece.fut_seen != piece.t || piece.loop_turn != 0;
        let layout = egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(!compact);
        let size = Vec2::new(ui.available_width(), ui.spacing().interact_size.y);
        ui.allocate_ui_with_layout(size, layout, |ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(32.0), egui::Sense::hover());
            art.paint_styled(
                ui.painter(),
                piece.kind,
                piece.color,
                rect,
                1.0,
                crate::piece_art::PieceStyle {
                    inverted: piece.inverted,
                    ..Default::default()
                },
            );
            ui.label(format!(
                "{} · ({}, {})",
                piece_name(piece.kind),
                square_name(piece.sq),
                piece.t
            ));
            if compact && show_clock {
                inspection_clock(ui, piece);
            }
        });
        if !compact && show_clock {
            ui.horizontal(|ui| inspection_clock(ui, piece));
        }
        let visible: Vec<_> = state
            .informed
            .iter()
            .map(|o| (*o, !state.acknowledged_observations.contains(o)))
            .collect();
        if !visible.is_empty() {
            let remaining = (!compact).then(|| {
                let height = (ui.max_rect().bottom() - ui.cursor().min.y - 22.0).max(64.0);
                ui.separator();
                height
            });
            observation_block(ui, art, piece.color.opposite(), &visible, remaining);
        }
    });
}

fn inspection_clock(ui: &mut egui::Ui, piece: temporal_chess_engine::model::PieceView) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::splat(HUD_SECTION_ICON_SIZE), egui::Sense::hover());
    crate::icons::paint(
        ui.painter(),
        Icon::PastClock,
        rect.shrink(2.0),
        egui::Stroke::new(1.5, TEXT),
    );
    response.on_hover_text(format!(
        "fut_seen: {} · loop_turn: {}",
        piece.fut_seen, piece.loop_turn
    ));
    ui.label(piece.fut_seen.to_string());
}

fn observation_block(
    ui: &mut egui::Ui,
    art: &PieceArt,
    side: temporal_chess_engine::model::Color,
    observations: &[(temporal_chess_engine::model::Observation, bool)],
    height: Option<f32>,
) {
    let (size, capacity) = height.map_or((22.0, observations.len()), |height| {
        observation_grid(ui.available_width(), height, observations.len())
    });
    let cell = Vec2::new((size + 2.0).max(HUD_SECTION_ICON_SIZE), size + 9.0);
    let shown = if observations.len() > capacity {
        capacity.saturating_sub(1)
    } else {
        observations.len()
    };
    let layout = egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(height.is_some());
    ui.allocate_ui_with_layout(Vec2::new(ui.available_width(), cell.y), layout, |ui| {
        ui.spacing_mut().interact_size.y = cell.y;
        ui.spacing_mut().item_spacing = Vec2::splat(4.0);
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(HUD_SECTION_ICON_SIZE, cell.y),
            egui::Sense::hover(),
        );
        crate::icons::paint(
            ui.painter(),
            Icon::Information,
            egui::Rect::from_center_size(
                rect.center() + Vec2::X,
                Vec2::splat(HUD_SECTION_ICON_SIZE),
            ),
            egui::Stroke::new(1.5, TEXT),
        );
        response.on_hover_text(crate::tr!("Видимая информация", "Visible information"));
        for (observation, new) in observations.iter().take(shown) {
            let (rect, response) = ui.allocate_exact_size(cell, egui::Sense::hover());
            let image = egui::Rect::from_min_size(rect.min, Vec2::splat(size));
            art.paint_styled(
                ui.painter(),
                observation.kind,
                side,
                image,
                1.0,
                crate::piece_art::PieceStyle {
                    inverted: observation.inverted,
                    outline: if *new {
                        crate::piece_art::Outline::Information
                    } else {
                        crate::piece_art::Outline::None
                    },
                    ..Default::default()
                },
            );
            let square = square_name(observation.square);
            ui.painter().text(
                rect.right_bottom(),
                egui::Align2::RIGHT_BOTTOM,
                &square,
                egui::FontId::proportional(9.5),
                TEXT,
            );
            response.on_hover_text(format!("{} · {square}", piece_name(observation.kind)));
        }
        if shown < observations.len() {
            let (rect, response) = ui.allocate_exact_size(cell, egui::Sense::hover());
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("+{}", observations.len() - shown),
                egui::FontId::proportional(10.0),
                TEXT,
            );
            response.on_hover_text(
                observations[shown..]
                    .iter()
                    .map(|(o, _)| format!("{} · {}", piece_name(o.kind), square_name(o.square)))
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
    });
}

fn observation_grid(width: f32, height: f32, count: usize) -> (f32, usize) {
    let mut result = (18.0, 0);
    for size in [34.0_f32, 30.0, 26.0, 22.0, 18.0] {
        let stride = (size + 2.0).max(HUD_SECTION_ICON_SIZE) + 4.0;
        let columns = ((width + 4.0) / stride).floor().max(1.0) as usize;
        let first_row = ((width - HUD_SECTION_ICON_SIZE) / stride).floor().max(0.0) as usize;
        let rows = ((height + 4.0) / (size + 13.0)).floor().max(1.0) as usize;
        result = (size, first_row + columns * (rows - 1));
        if count <= result.1 {
            break;
        }
    }
    result
}

pub(crate) fn language_header(ui: &mut egui::Ui) {
    let mut language = crate::i18n::Language::current();
    ui.horizontal(|ui| {
        ui.set_min_height(38.0);
        ui.label(crate::tr!("Язык", "Language"));
        egui::ComboBox::from_id_salt("interface_language")
            .selected_text(language.name())
            .show_ui(ui, |ui| {
                for choice in [
                    crate::i18n::Language::Russian,
                    crate::i18n::Language::English,
                ] {
                    ui.selectable_value(&mut language, choice, choice.name());
                }
            });
    });
    if language != crate::i18n::Language::current() {
        language.set();
        ui.ctx().request_repaint();
    }
}

fn sidebar(ui: &mut egui::Ui, game: &mut GameUi, shell: &mut ShellState, art: &PieceArt) -> bool {
    let idle = game.replay_job.is_none() && !game.playback_busy() && !game.facing.animating();
    let rules = game.engine.history().rules();
    if rules.wait_before_move {
        ui.small(crate::tr!("Свободное ожидание", "Free wait"));
    }
    if !rules.chain_info_activation {
        ui.small(crate::tr!(
            "Инфоактивация с обучением",
            "Learn info-activation"
        ));
    }
    if !rules.global_reaction_reset {
        ui.small(crate::tr!("Реагировать многократно", "React multiple"));
    }
    crate::lab::panel(ui, game, art);
    if game.lab.is_none() {
        crate::tempfish::panel(ui, game);
    }
    ui.horizontal(|ui| {
        if icon_button(
            ui,
            Icon::Undo,
            if game.online.enabled {
                crate::tr!("Запросить отмену хода · Ctrl+Z", "Request undo · Ctrl+Z")
            } else {
                crate::tr!("Отменить последний ход · Ctrl+Z", "Undo last move · Ctrl+Z")
            },
            false,
            game.can_undo(),
        )
        .clicked()
        {
            game.undo_last_move();
        }
        let copied = ui.input(|i| i.time) < shell.copied_until;
        if icon_button(
            ui,
            if copied { Icon::Check } else { Icon::Copy },
            crate::tr!("Копировать партию", "Copy game"),
            copied,
            true,
        )
        .clicked()
        {
            ui.ctx().copy_text(game.record.to_text());
            shell.copied_until = ui.input(|i| i.time) + 2.0;
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs(2));
        }
        if icon_button(
            ui,
            Icon::Import,
            crate::tr!("Загрузить партию из записи", "Import game record"),
            false,
            idle && !game.online.enabled,
        )
        .clicked()
        {
            game.clear_selection();
            game.import_open = true;
            game.import_error = None;
        }
        if icon_button(
            ui,
            Icon::New,
            crate::tr!("Новая партия", "New game"),
            false,
            idle,
        )
        .clicked()
        {
            game.new_game_bot = game.bot.settings;
            game.new_game_online = game.online.enabled;
            game.confirm_restart = true;
        }
    });
    if game.is_error {
        ui.horizontal_wrapped(|ui| {
            if icon_button(
                ui,
                Icon::Close,
                crate::tr!("Скрыть сообщение", "Dismiss message"),
                false,
                true,
            )
            .clicked()
            {
                game.is_error = false;
            }
            ui.colored_label(CAPTURE, game.notice.text());
        });
    }
    ui.separator();
    ui.horizontal(|ui| {
        ui.strong(
            if game
                .engine
                .history()
                .starting_position()
                .initial_end()
                .is_some()
            {
                "Time War"
            } else {
                crate::tr!("Лента времени", "Timeline")
            },
        );
        ui.colored_label(
            SUBTLE,
            format!(
                "{} — {}",
                game.engine.history().beginning(),
                game.engine.history().horizon()
            ),
        );
    });
    let mut show_board = false;
    ui.horizontal(|ui| {
        if timeline_playback_button(ui, game, idle) {
            game.toggle_history_playback();
            show_board = game.history_playback;
        }
        if icon_button(
            ui,
            Icon::First,
            crate::tr!("Последняя доска", "Latest board"),
            false,
            idle,
        )
        .clicked()
        {
            game.jump_to = game.times().first().copied();
            show_board = true;
        }
        if icon_button(
            ui,
            Icon::Last,
            crate::tr!("Начало истории", "Beginning of history"),
            false,
            idle,
        )
        .clicked()
        {
            game.jump_to = Some(game.engine.history().beginning());
            show_board = true;
        }
        if icon_button(
            ui,
            Icon::ResetZoom,
            crate::tr!("Сбросить масштаб", "Reset zoom"),
            false,
            idle,
        )
        .clicked()
        {
            game.cell_size = 64.0;
        }
        ui.label(format!("{:.0}%", game.cell_size / 64.0 * 100.0));
    });
    ui.colored_label(
        SUBTLE,
        egui::RichText::new(if mobile_layout(ui.ctx()) {
            crate::tr!(
                "Масштаб · сведите или разведите два пальца",
                "Pinch with two fingers to zoom"
            )
        } else {
            crate::tr!(
                "Масштаб: Shift или ПКМ + колесо",
                "Zoom: Shift or right mouse button + wheel"
            )
        })
        .small(),
    );
    show_board
}

fn details_panel(ui: &mut egui::Ui, game: &mut GameUi, art: &PieceArt, height: f32) -> bool {
    ui.strong(crate::tr!("Ходы партии", "Move history"));
    let mut jumped = false;
    if game.record.commands().is_empty() {
        ui.allocate_ui(
            Vec2::new(ui.available_width(), (height - 34.0).max(80.0)),
            |ui| {
                ui.colored_label(
                    SUBTLE,
                    crate::tr!("Здесь появятся ходы партии", "Moves will appear here"),
                );
            },
        );
    } else {
        let playback_busy = game.playback_busy();
        let commands = game.record.commands();
        egui::ScrollArea::both()
            .id_salt("history_scroll")
            .max_height((height - 34.0).max(80.0))
            .min_scrolled_height((height - 34.0).max(80.0))
            .auto_shrink([false, false])
            .show_rows(ui, 30.0, commands.len(), |ui, range| {
                for row in range {
                    let index = commands.len() - 1 - row;
                    let (text, t) = match commands[index] {
                        temporal_chess_engine::record::Command::Move { mv, from }
                        | temporal_chess_engine::record::Command::MoveAndWait {
                            mv, from, ..
                        } => (
                            format!(
                                "({}, {}) -> ({}, {})",
                                square_name(from),
                                mv.t,
                                square_name(mv.sq),
                                commands[index]
                                    .wait_until()
                                    .or_else(|| mv.direction.step(mv.t))
                                    .unwrap_or(mv.t)
                            ),
                            mv.t,
                        ),
                        temporal_chess_engine::record::Command::Advance { source, from, to_t } => (
                            format!(
                                "({}, {}) -> ({}, {})",
                                square_name(from),
                                source.t,
                                square_name(from),
                                to_t
                            ),
                            source.t,
                        ),
                        temporal_chess_engine::record::Command::Continue => (
                            crate::tr!("Продолжение просчёта", "Continue calculation").into(),
                            game.engine.history().horizon(),
                        ),
                        temporal_chess_engine::record::Command::DeclineReactions => (
                            crate::tr!("Не реагировать", "Decline reactions").into(),
                            game.engine.history().horizon(),
                        ),
                    };
                    let response = ui
                        .horizontal(|ui| {
                            ui.set_min_height(30.0);
                            ui.label(format!("{}.", index + 1));
                            let (rect, _) =
                                ui.allocate_exact_size(Vec2::splat(24.0), egui::Sense::hover());
                            if let Some(piece) = game.record.piece(index) {
                                art.paint_styled(
                                    ui.painter(),
                                    piece.kind,
                                    piece.color,
                                    rect,
                                    1.0,
                                    crate::piece_art::PieceStyle {
                                        inverted: piece.inverted,
                                        ..Default::default()
                                    },
                                );
                                if let Some(mv) = commands[index].spatial_move()
                                    && mv.to_past() != piece.inverted
                                {
                                    let (arrow, response) = ui.allocate_exact_size(
                                        Vec2::splat(18.0),
                                        egui::Sense::hover(),
                                    );
                                    crate::icons::paint(
                                        ui.painter(),
                                        if mv.to_past() {
                                            Icon::Past
                                        } else {
                                            Icon::Future
                                        },
                                        arrow.shrink(2.0),
                                        egui::Stroke::new(1.5, TEXT),
                                    );
                                    response.on_hover_text(if mv.to_past() {
                                        crate::tr!("Поворот в прошлое", "Turn towards the past")
                                    } else {
                                        crate::tr!("Поворот в будущее", "Turn towards the future")
                                    });
                                }
                            } else {
                                crate::icons::paint(
                                    ui.painter(),
                                    Icon::Continue,
                                    rect.shrink(3.0),
                                    egui::Stroke::new(1.5, TEXT),
                                );
                            }
                            ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Extend));
                        })
                        .response;
                    let response = ui.interact(
                        response.rect,
                        ui.make_persistent_id(("record_row", index)),
                        egui::Sense::click(),
                    );
                    if crate::input::clicked(&response.on_hover_text(crate::tr!(
                        "Перейти к исходной доске",
                        "Go to the source board"
                    ))) && !playback_busy
                    {
                        game.jump_to = Some(t);
                        jumped = true;
                    }
                }
            });
    }
    jumped
}

pub fn piece_name(kind: PieceKind) -> &'static str {
    match kind {
        PieceKind::K { .. } => crate::tr!("Король", "King"),
        PieceKind::Q => crate::tr!("Ферзь", "Queen"),
        PieceKind::R { .. } => crate::tr!("Ладья", "Rook"),
        PieceKind::B => crate::tr!("Слон", "Bishop"),
        PieceKind::N => crate::tr!("Конь", "Knight"),
        PieceKind::P { .. } => crate::tr!("Пешка", "Pawn"),
    }
}

fn square_name(sq: temporal_chess_engine::model::Sq) -> String {
    format!("{}{}", char::from(b'a' + sq.0 as u8), sq.1 + 1)
}

pub fn starting_position_controls(
    ui: &mut egui::Ui,
    position: &mut temporal_chess_engine::model::StartingPosition,
) {
    use temporal_chess_engine::model::{MAX_TIME_WAR_SEPARATION, StartingPosition};
    egui::ComboBox::from_label(crate::tr!("Стартовая позиция", "Starting position"))
        .selected_text(match position {
            StartingPosition::Standard => crate::tr!("Обычная", "Standard"),
            StartingPosition::TimeWar { .. } => "Time War",
            StartingPosition::Lab => "Lab",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(
                position,
                StartingPosition::Standard,
                crate::tr!("Обычная", "Standard"),
            );
            let time_war = match *position {
                StartingPosition::TimeWar { .. } => *position,
                _ => StartingPosition::TimeWar { separation: 9 },
            };
            ui.selectable_value(position, time_war, "Time War");
            ui.selectable_value(position, StartingPosition::Lab, "Lab");
        });
    if *position == StartingPosition::Lab {
        ui.small(crate::tr!(
            "Сначала откроется редактор сеттинга. Режим игры выберете после расстановки.",
            "Opens the setting editor first. Choose how to play after building the position."
        ));
    }
    if let StartingPosition::TimeWar { separation } = position {
        ui.horizontal(|ui| {
            ui.label(crate::tr!("Разнос по времени", "Time separation"));
            ui.add(
                egui::DragValue::new(separation)
                    .range(3..=MAX_TIME_WAR_SEPARATION)
                    .speed(2.0),
            );
        });
        ui.small(crate::tr!("Нечётное число от 3 до 255. Белые — на 0, чёрные — на N. Ходы могут расширять историю в обе стороны.", "Odd number from 3 to 255. White starts at 0, Black at N. Moves can extend the timeline in both directions."));
        if *separation % 2 == 0 {
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                crate::tr!("Введите нечётное число", "Enter an odd number"),
            );
        }
    }
}

pub fn rule_controls(ui: &mut egui::Ui, rules: &mut temporal_chess_engine::model::GameRules) {
    ui.checkbox(
        &mut rules.wait_before_move,
        crate::tr!("Свободное ожидание", "Free wait"),
    )
    .on_hover_text(crate::tr!(
        "Ожидание не передаёт ход. Авто-ожидание заблокированных фигур выключается.",
        "Waiting does not spend a turn. Disables automatic waits for blocked pieces."
    ));
    let mut learn = !rules.chain_info_activation;
    ui.checkbox(&mut learn,
        crate::tr!("Инфоактивация с обучением", "Learn info-activation"))
        .on_hover_text(crate::tr!(
            "Включено: реагирует только первое увидевшее состояние каждой мировой линии. Выключено: реагируют все увидевшие состояния (по умолчанию).",
            "On: only the first observing state of each worldline reacts. Off: all observing states react (default)."
        ));
    rules.chain_info_activation = !learn;
    let mut multiple = !rules.global_reaction_reset;
    ui.checkbox(&mut multiple,
        crate::tr!("Реагировать многократно", "React multiple"))
        .on_hover_text(crate::tr!(
            "Сохраняет реакции на досках позже совершённого хода. По умолчанию ход сбрасывает все реакции своей стороны.",
            "Keeps reactions on boards later than the move. By default, each turn clears all reactions of its side."
        ));
    rules.global_reaction_reset = !multiple;
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Pos2, Rect};
    use temporal_chess_engine::model::{Sq, TimeDirection};

    fn frame(ctx: &egui::Context, game: &mut GameUi, events: Vec<egui::Event>) -> egui::FullOutput {
        frame_at(ctx, game, Vec2::new(1260.0, 900.0), events)
    }

    fn frame_at(
        ctx: &egui::Context,
        game: &mut GameUi,
        size: Vec2,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let key = egui::Id::new("test_art");
        let art = ctx
            .data_mut(|data| data.get_temp::<std::sync::Arc<PieceArt>>(key))
            .unwrap_or_else(|| {
                crate::theme::configure(ctx);
                let art = std::sync::Arc::new(PieceArt::new(ctx));
                ctx.data_mut(|data| data.insert_temp(key, art.clone()));
                art
            });
        let shell_key = egui::Id::new("test_shell");
        let mut shell = ctx
            .data_mut(|data| data.get_temp::<ShellState>(shell_key))
            .unwrap_or_default();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
                events,
                ..Default::default()
            },
            |ui| draw(ui.ctx(), game, &mut shell, &art),
        );
        ctx.data_mut(|data| data.insert_temp(shell_key, shell));
        output.textures_delta.clear();
        assert!(
            !ctx.tessellate(output.shapes.clone(), output.pixels_per_point)
                .is_empty()
        );
        output
    }

    #[test]
    fn phone_menu_covers_the_viewport_and_returns_to_the_centered_game() {
        let ctx = egui::Context::default();
        let mut game = GameUi::default();
        let size = Vec2::new(430.0, 812.0);
        let viewport = Rect::from_min_size(Pos2::ZERO, size);
        let toggle = crate::tr!("Показать или скрыть панель", "Toggle sidebar");
        let shell_key = egui::Id::new("test_shell");
        let mut render = |events| frame_at(&ctx, &mut game, size, events);
        render(vec![]);
        let output = render(vec![]);
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect)
            if rect.fill == BACKGROUND && rect.rect == viewport && shape.clip_rect.contains_rect(viewport))));
        assert!(
            output
                .shapes
                .iter()
                .any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect)
            if rect.fill == SURFACE && (rect.rect.center().x - viewport.center().x).abs() < 1.0))
        );

        let mut click = |label: &str| {
            let pos = icon_position(&ctx, label);
            for pressed in [true, false] {
                render(vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ]);
            }
            (0..48).map(|_| render(vec![])).last().unwrap()
        };
        let output = click(toggle);
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect)
            if rect.fill == PANEL && rect.rect == viewport && shape.clip_rect.contains_rect(viewport))));
        let shell = ctx.data(|d| d.get_temp::<ShellState>(shell_key)).unwrap();
        assert!(shell.sidebar_open && shell.ribbon_rect == Rect::NOTHING);

        click(crate::tr!("Последняя доска", "Latest board"));
        let shell = ctx.data(|d| d.get_temp::<ShellState>(shell_key)).unwrap();
        assert!(!shell.sidebar_open && shell.ribbon_rect.width() > 400.0);
    }

    #[test]
    fn phone_hud_scrolls_to_inspection_details_without_taking_space_from_the_ribbon() {
        use temporal_chess_engine::{
            engine::Engine,
            model::{Color, State},
        };
        let mut lines = vec![vec![State::new(PieceKind::Q, Color::White, Sq(3, 3))]];
        for x in 2..=4 {
            for y in 2..=4 {
                if (x, y) != (3, 3) {
                    lines.push(vec![State::new(PieceKind::N, Color::Black, Sq(x, y))]);
                }
            }
        }
        let engine = Engine::from_worldlines(lines, Color::White).unwrap();
        let mut game = GameUi {
            engine,
            ..Default::default()
        };
        let ctx = egui::Context::default();
        let size = Vec2::new(320.0, 600.0);
        let positions = || {
            ctx.data(|d| {
                (
                    d.get_temp::<Rect>(egui::Id::new(GAME_HUD_RECT)).unwrap(),
                    d.get_temp::<ShellState>(egui::Id::new("test_shell"))
                        .unwrap()
                        .ribbon_rect,
                )
            })
        };
        frame_at(&ctx, &mut game, size, vec![]);
        let (before, ribbon) = positions();
        game.toggle_inspector();
        game.click_square(0, Sq(3, 3));
        frame_at(&ctx, &mut game, size, vec![]);
        let before_scroll = frame_at(&ctx, &mut game, size, vec![]);
        let (hud, ribbon_with_selection) = positions();
        assert_eq!(hud.height(), before.height());
        assert_eq!(ribbon_with_selection, ribbon);
        let last = text_position(&before_scroll, "e5");
        assert!(last.x > hud.right());

        let start = egui::pos2(hud.right() - 28.0, hud.center().y);
        let end = egui::pos2(hud.left() + 28.0, hud.center().y);
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame_at(
            &ctx,
            &mut game,
            size,
            vec![egui::Event::PointerMoved(start), button(start, true)],
        );
        frame_at(&ctx, &mut game, size, vec![egui::Event::PointerMoved(end)]);
        frame_at(&ctx, &mut game, size, vec![button(end, false)]);
        let after_scroll = frame_at(&ctx, &mut game, size, vec![]);
        assert!(hud.contains(text_position(&after_scroll, "e5")));
        assert_eq!(positions(), (hud, ribbon));
    }

    #[test]
    fn both_inspectors_show_current_information() {
        use temporal_chess_engine::{
            engine::Engine,
            model::{Color, Observation, State},
        };
        let mut queen = State::new(PieceKind::Q, Color::White, Sq(3, 3));
        let knight = State::new(PieceKind::N, Color::Black, Sq(5, 5));
        let bishop = State::new(PieceKind::B, Color::Black, Sq(3, 6));
        queen
            .acknowledged_observations
            .insert(Observation::from_state(&knight));
        let engine =
            Engine::from_worldlines(vec![vec![queen], vec![knight], vec![bishop]], Color::White)
                .unwrap();
        let mut game = GameUi {
            engine,
            inspect_mode: true,
            ..Default::default()
        };
        game.click_square(0, Sq(3, 3));
        let ctx = egui::Context::default();
        let output = frame(&ctx, &mut game, vec![]);
        text_position(&output, "f6");
        text_position(&output, "d7");
        game.inspect_mode = false;
        game.worldline_mode = true;
        game.click_square(0, Sq(3, 3));
        assert_eq!(game.inspected_piece().unwrap().sq, Sq(3, 3));
        let worldline_output = frame(&ctx, &mut game, vec![]);
        text_position(&worldline_output, "f6");
        text_position(&worldline_output, "d7");
    }

    #[test]
    fn learn_info_activation_checkbox_inverts_chain_and_starts_unchecked() {
        let ctx = egui::Context::default();
        let mut game = GameUi {
            confirm_restart: true,
            ..Default::default()
        };
        let before = game.new_game_rules;
        assert!(before.chain_info_activation);
        frame(&ctx, &mut game, vec![]);
        let output = frame(&ctx, &mut game, vec![]);
        let position = text_position(
            &output,
            crate::tr!("Инфоактивация с обучением", "Learn info-activation"),
        );
        click(&ctx, &mut game, position);
        assert!(!game.new_game_rules.chain_info_activation);
        assert_eq!(
            game.new_game_rules.global_reaction_reset,
            before.global_reaction_reset
        );
        assert_eq!(
            game.new_game_rules.wait_before_move,
            before.wait_before_move
        );
        click(&ctx, &mut game, position);
        assert_eq!(game.new_game_rules, before);
    }

    #[test]
    fn empty_information_does_not_add_a_section() {
        let ctx = egui::Context::default();
        crate::theme::configure(&ctx);
        let art = PieceArt::new(&ctx);
        let mut game = GameUi {
            inspect_mode: true,
            ..Default::default()
        };
        game.click_square(0, Sq(4, 1));
        let piece = game.inspected_piece().unwrap();
        let state = game
            .engine
            .piece_state(crate::controller::identity(piece))
            .unwrap();
        assert!(state.informed.is_empty());
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.set_max_width(300.0);
            inspection_hud(ui, &game, &art, piece, false);
            assert!(ui.min_rect().height() <= 40.0);
        });
        output.textures_delta.clear();
    }

    #[test]
    fn hud_inspector_button_selects_inactive_opponents_without_playing() {
        let ctx = egui::Context::default();
        let mut game = GameUi::default();
        let before = game.engine.clone();
        game.click_square(0, Sq(4, 1));
        frame(&ctx, &mut game, vec![]);
        let replay = icon_position(
            &ctx,
            crate::tr!("Повторить предыдущий ход", "Replay previous move"),
        );
        let inspect = icon_position(
            &ctx,
            crate::tr!("Инспекция фигуры · I", "Inspect piece · I"),
        );
        assert!(inspect.x > replay.x && (inspect.y - replay.y).abs() < 1.0);
        click(&ctx, &mut game, inspect);
        assert!(game.inspect_mode && game.selected.is_none());
        assert_eq!(game.inspected_piece().unwrap().sq, Sq(4, 1));
        game.click_square(0, Sq(0, 6));
        let enemy = game.inspected_piece().unwrap();
        assert_eq!(enemy.color, temporal_chess_engine::model::Color::Black);
        assert!(!game.selectable(enemy));
        frame(&ctx, &mut game, vec![]);
        game.click_square(0, Sq(3, 3));
        assert!(game.inspected.is_none());
        assert_eq!(game.engine, before);
        assert!(game.record.commands().is_empty());
    }

    #[test]
    fn sidebar_preference_overrides_device_default() {
        assert!(!initial_sidebar(None, true));
        assert!(initial_sidebar(None, false));
        for mobile in [false, true] {
            assert!(initial_sidebar(Some("open"), mobile));
            assert!(!initial_sidebar(Some("closed"), mobile));
        }
    }

    #[test]
    fn phone_hud_places_selection_beside_turn_and_desktop_keeps_it_below() {
        for width in [320.0, 375.0, 430.0, 1260.0] {
            let ctx = egui::Context::default();
            crate::theme::configure(&ctx);
            let art = PieceArt::new(&ctx);
            let mut shell = ShellState::default();
            let mut game = GameUi::default();
            game.click_square(0, Sq(4, 1));
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 812.0))),
                    ..Default::default()
                },
                |ui| draw(ui.ctx(), &mut game, &mut shell, &art),
            );
            output.textures_delta.clear();
            let (king, piece) = ctx.data(|d| {
                (
                    d.get_temp::<Rect>(egui::Id::new("hud_king")).unwrap(),
                    d.get_temp::<Rect>(egui::Id::new("hud_selected_piece"))
                        .unwrap(),
                )
            });
            let replay = ctx
                .data(|d| {
                    d.get_temp::<Rect>(egui::Id::new((
                        "icon",
                        crate::tr!("Повторить предыдущий ход", "Replay previous move"),
                    )))
                })
                .unwrap();
            assert_eq!(
                replay.size(),
                Vec2::splat(38.0),
                "replay must stay square at width {width}"
            );
            if width < 760.0 {
                assert!(piece.left() > king.right());
                assert!((piece.top() - king.top()).abs() < 1.0);
                assert!(piece.right() < width - 50.0);
            } else {
                assert!(piece.top() > king.bottom());
                assert!((piece.left() - king.left()).abs() < 1.0);
            }
        }
    }

    #[test]
    fn phone_layout_keeps_ribbon_inside_screen_with_closed_sidebar() {
        let ctx = egui::Context::default();
        crate::theme::configure(&ctx);
        let art = PieceArt::new(&ctx);
        let mut shell = ShellState::default();
        let mut game = GameUi::with_start(
            Default::default(),
            temporal_chess_engine::model::StartingPosition::TimeWar { separation: 9 },
        );
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(375.0, 812.0))),
                ..Default::default()
            },
            |ui| draw(ui.ctx(), &mut game, &mut shell, &art),
        );
        output.textures_delta.clear();
        assert!(!shell.sidebar_open);
        assert!(shell.ribbon_rect.width() > 300.0);
        assert!(shell.ribbon_rect.top() < 300.0);
        assert!(game.viewport.stride < 400.0);
        let king = ctx
            .data(|d| d.get_temp::<Rect>(egui::Id::new("hud_king")))
            .unwrap();
        assert!(
            output.shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Mesh(mesh)
                    if mesh.texture_id != egui::TextureId::default()
                        && mesh.calc_bounds().contains_rect(king)
                        && shape.clip_rect.contains_rect(king))
            }),
            "the turn king must be painted inside the visible clip"
        );
        let help = ctx
            .data(|d| {
                d.get_temp::<Rect>(egui::Id::new((
                    "icon",
                    crate::tr!("Управление и обозначения", "Controls and legend"),
                )))
            })
            .unwrap();
        assert!(king.top() < 90.0, "turn status lives at the screen top");
        assert!(
            king.top() >= help.bottom(),
            "phone toolbar must not overlap the turn status"
        );
        assert!(shell.ribbon_rect.top() > king.bottom());
        let pos = shell.ribbon_rect.center();
        let offset = game.viewport.offset;
        let touch = |phase, x, y| egui::Event::Touch {
            device_id: egui::TouchDeviceId(0),
            id: egui::TouchId(0),
            phase,
            pos: egui::pos2(x, y),
            force: None,
        };
        for event in [
            touch(egui::TouchPhase::Start, pos.x, pos.y),
            touch(egui::TouchPhase::Move, pos.x, pos.y + 40.0),
        ] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(375.0, 812.0))),
                    events: vec![event],
                    ..Default::default()
                },
                |ui| draw(ui.ctx(), &mut game, &mut shell, &art),
            );
            output.textures_delta.clear();
        }
        assert!(
            game.viewport.offset < offset - 40.0,
            "fast swipes accelerate"
        );
        assert!(game.pending_square.is_none());
    }

    #[test]
    fn live_waiting_focuses_source_before_starting_its_first_transition() {
        for spatial in [false, true] {
            let ctx = egui::Context::default();
            let mut game = GameUi::with_start(
                Default::default(),
                temporal_chess_engine::model::StartingPosition::TimeWar { separation: 9 },
            );
            frame(&ctx, &mut game, vec![]);
            frame(&ctx, &mut game, vec![]);
            // The player has scrolled to the chosen waiting destination.
            let row = game.viewport.times.iter().position(|&t| t == 6).unwrap();
            game.viewport.offset = row as f32 * game.viewport.stride;
            game.viewport.scroll = None;
            game.jump_to = None;
            game.click_square(0, Sq(4, 1));
            game.click_square(6, if spatial { Sq(4, 3) } else { Sq(4, 1) });
            assert_eq!(game.jump_to, Some(0));
            assert!(game.move_focus_pending && game.transition.is_none());
            let first = game.transition_queue.front().unwrap();
            assert_eq!((first.from_t, first.to_t), (0, 1));
            assert_eq!(first.tracks.iter().any(|p| p.from.sq != p.to.sq), spatial);
            for _ in 0..3 {
                frame(&ctx, &mut game, vec![]);
                assert_eq!(game.viewport.scroll.as_ref().unwrap().target, 0);
                assert!(game.transition.is_none());
                assert!(game.transition_queue.iter().all(|m| m.started.is_none()));
            }
            // Complete camera navigation without spending wall time in a test.
            game.viewport.scroll = None;
            let row = game.viewport.times.iter().position(|&t| t == 0).unwrap();
            game.viewport.offset = row as f32 * game.viewport.stride;
            frame(&ctx, &mut game, vec![]);
            let first = game.transition.as_ref().unwrap();
            assert_eq!((first.from_t, first.to_t), (0, 1));
            assert_eq!(game.viewport.motion_progress, 0.0);
            assert!(first.started.is_none());
            frame(&ctx, &mut game, vec![]);
            assert!(game.transition.as_ref().unwrap().started.is_some());
        }
    }

    #[test]
    fn wheel_scroll_settles_monotonically_and_stays_fixed_after_idle_frames() {
        let ctx = egui::Context::default();
        let mut game = GameUi::with_start(
            Default::default(),
            temporal_chess_engine::model::StartingPosition::TimeWar { separation: 9 },
        );
        game.jump_to = None;
        frame(&ctx, &mut game, vec![]);
        frame(&ctx, &mut game, vec![]);
        frame(
            &ctx,
            &mut game,
            vec![
                egui::Event::PointerMoved(Pos2::new(420.0, 300.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    phase: egui::TouchPhase::Move,
                    delta: Vec2::new(0.0, -500.37),
                    modifiers: Default::default(),
                },
            ],
        );
        let mut previous = game.viewport.offset;
        for _ in 0..150 {
            frame(&ctx, &mut game, vec![]);
            assert!(
                game.viewport.offset >= previous,
                "camera reversed at the end of a wheel scroll: {previous} -> {}",
                game.viewport.offset
            );
            previous = game.viewport.offset;
        }
        assert!(!game.viewport.wheel.active());
        assert!((previous - 500.37).abs() < 0.02);
        for _ in 0..20 {
            frame(&ctx, &mut game, vec![]);
            assert_eq!(game.viewport.offset, previous);
        }
    }

    #[test]
    fn sidebar_menu_reclaims_ribbon_space_without_changing_selection_or_game() {
        let ctx = egui::Context::default();
        let mut game = GameUi::default();
        game.click_square(0, Sq(4, 1));
        let engine = game.engine.clone();
        frame(&ctx, &mut game, vec![]);
        frame(&ctx, &mut game, vec![]);
        let key = egui::Id::new("test_shell");
        let before = ctx.data_mut(|d| d.get_temp::<ShellState>(key)).unwrap();
        let button = icon_position(
            &ctx,
            crate::tr!("Показать или скрыть панель", "Toggle sidebar"),
        );
        click(&ctx, &mut game, button);
        for _ in 0..40 {
            frame(&ctx, &mut game, vec![]);
        }
        let after = ctx.data_mut(|d| d.get_temp::<ShellState>(key)).unwrap();
        assert!(!after.sidebar_open);
        assert!(after.ribbon_rect.width() > before.ribbon_rect.width() + 250.0);
        assert!(game.selected.is_some());
        assert_eq!(game.engine, engine);
        click(
            &ctx,
            &mut game,
            icon_position(
                &ctx,
                crate::tr!("Показать или скрыть панель", "Toggle sidebar"),
            ),
        );
        frame(&ctx, &mut game, vec![]);
        assert!(
            ctx.data_mut(|d| d.get_temp::<ShellState>(key))
                .unwrap()
                .sidebar_open
        );
    }

    #[test]
    fn settings_popup_works_with_the_sidebar_open_and_closed() {
        let ctx = egui::Context::default();
        let mut game = GameUi::default();
        game.click_square(0, Sq(4, 1));
        let engine = game.engine.clone();
        let key = egui::Id::new("test_shell");
        for expanded in [true, false] {
            let mut shell = ShellState {
                sidebar_open: expanded,
                initialized: true,
                ..Default::default()
            };
            ctx.data_mut(|d| d.insert_temp(key, shell.clone()));
            for _ in 0..40 {
                frame(&ctx, &mut game, vec![]);
            }
            let gear = icon_position(&ctx, crate::tr!("Настройки", "Settings"));
            let toggle = icon_position(
                &ctx,
                crate::tr!("Показать или скрыть панель", "Toggle sidebar"),
            );
            assert!(gear.x < toggle.x);
            assert!(
                icon_position(
                    &ctx,
                    crate::tr!("Управление и обозначения", "Controls and legend")
                )
                .x < gear.x
            );
            click(&ctx, &mut game, gear);
            let output = frame(&ctx, &mut game, vec![]);
            shell = ctx.data_mut(|d| d.get_temp::<ShellState>(key)).unwrap();
            assert!(shell.settings_open);
            let mute = text_position(
                &output,
                crate::tr!("Выключить музыку ожидания", "Disable waiting music"),
            );
            text_position(&output, crate::tr!("Скорость анимаций", "Animation speed"));
            text_position(&output, crate::tr!("Переворачивать доски", "Flip boards"));
            click(&ctx, &mut game, mute);
            shell = ctx.data_mut(|d| d.get_temp::<ShellState>(key)).unwrap();
            assert!(shell.settings_open && shell.disable_waiting_music);
            click(&ctx, &mut game, gear);
            assert!(
                !ctx.data_mut(|d| d.get_temp::<ShellState>(key))
                    .unwrap()
                    .settings_open
            );
            assert!(game.selected.is_some());
            assert_eq!(game.engine, engine);
            click(
                &ctx,
                &mut game,
                icon_position(
                    &ctx,
                    crate::tr!("Управление и обозначения", "Controls and legend"),
                ),
            );
            assert!(
                ctx.data_mut(|d| d.get_temp::<ShellState>(key))
                    .unwrap()
                    .help_open
            );
            frame(&ctx, &mut game, vec![]);
            click(
                &ctx,
                &mut game,
                icon_position(&ctx, crate::tr!("Закрыть справку", "Close help")),
            );
            assert!(
                !ctx.data_mut(|d| d.get_temp::<ShellState>(key))
                    .unwrap()
                    .help_open
            );
        }
    }

    #[test]
    fn multiplayer_is_a_new_game_mode_and_history_stays_visible_during_inspection() {
        let ctx = egui::Context::default();
        let mut game = GameUi {
            inspect_mode: true,
            ..Default::default()
        };
        frame(&ctx, &mut game, vec![]);
        let output = frame(&ctx, &mut game, vec![]);
        text_position(&output, crate::tr!("Ходы партии", "Move history"));
        assert!(!output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == crate::tr!("Создать новую комнату", "Create a new room"))));
        click(
            &ctx,
            &mut game,
            icon_position(&ctx, crate::tr!("Новая партия", "New game")),
        );
        let output = frame(&ctx, &mut game, vec![]);
        text_position(&output, crate::tr!("Локальный", "Local"));
        let mode = text_position(&output, crate::tr!("Локальный", "Local"));
        click(&ctx, &mut game, mode);
        let output = frame(&ctx, &mut game, vec![]);
        let online = text_position(&output, crate::tr!("Мультиплеер", "Multiplayer"));
        click(&ctx, &mut game, online);
        frame(&ctx, &mut game, vec![]);
        let output = frame(&ctx, &mut game, vec![]);
        assert!(game.new_game_online);
        assert!(!game.online.enabled);
        text_position(&output, crate::tr!("Создать комнату", "Create room"));
        text_position(&output, crate::tr!("Присоединиться", "Join"));
    }

    #[test]
    fn selection_stays_still_and_a_move_scrolls_to_its_arrival() {
        let ctx = egui::Context::default();
        let mut game = GameUi::default();
        frame(&ctx, &mut game, vec![]);
        frame(&ctx, &mut game, vec![]);
        let before = game.viewport.offset;
        game.click_square(0, Sq(4, 1));
        frame(&ctx, &mut game, vec![]);
        assert_eq!(game.viewport.offset, before);
        game.toggle_direction();
        frame(&ctx, &mut game, vec![]);
        assert_eq!(game.viewport.offset, before);
        game.toggle_direction();
        game.click_square(0, Sq(4, 3));
        frame(&ctx, &mut game, vec![]);
        assert!(game.transition.is_some());
        game.reset_playback();
        game.finish_travel(1);
        frame(&ctx, &mut game, vec![]);
        assert_eq!(game.viewport.focused, Some(1));
        assert_eq!(game.viewport.scroll.as_ref().unwrap().target, 1);
        assert_eq!(game.viewport.times.first(), Some(&1));
        game.reset_playback();
        game.inspect_mode = true;
        game.click_square(0, Sq(4, 1));
        frame(&ctx, &mut game, vec![]);
        assert!(game.inspected.is_some());
    }

    #[test]
    fn mouse_buttons_work_over_the_sidebar_without_moving_the_ribbon() {
        let ctx = egui::Context::default();
        let mut game = GameUi::default();
        frame(&ctx, &mut game, vec![]);
        game.click_square(0, Sq(4, 1));
        for button in [egui::PointerButton::Middle, egui::PointerButton::Secondary] {
            let pos = Pos2::new(1200.0, 800.0);
            for pressed in [true, false] {
                frame(
                    &ctx,
                    &mut game,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button,
                            pressed,
                            modifiers: egui::Modifiers::default(),
                        },
                    ],
                );
            }
            if button == egui::PointerButton::Middle {
                assert_eq!(
                    game.selected.as_ref().unwrap().direction,
                    TimeDirection::Past
                );
            }
        }
        assert!(game.selected.is_none());
        assert_eq!(game.times(), [0]);
    }
    fn text_position(output: &egui::FullOutput, label: &str) -> Pos2 {
        output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape
                    && text.galley.text().starts_with(label)
                {
                    Some(text.pos + text.galley.size() * 0.5)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| panic!("missing UI text: {label}"))
    }

    fn icon_position(ctx: &egui::Context, label: &str) -> Pos2 {
        ctx.data_mut(|data| data.get_temp::<Rect>(egui::Id::new(("icon", label))))
            .expect("icon button")
            .center()
    }

    fn click(ctx: &egui::Context, game: &mut GameUi, pos: Pos2) -> egui::FullOutput {
        frame(
            ctx,
            game,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        frame(
            ctx,
            game,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        )
    }

    #[test]
    fn death_rows_list_white_before_black_and_open_worldline_inspection() {
        let ctx = egui::Context::default();
        let mut game = GameUi {
            local_auto_flip: false,
            ..Default::default()
        };
        for (t, from, to) in [
            (0, Sq(4, 1), Sq(4, 3)),
            (1, Sq(3, 6), Sq(3, 4)),
            (2, Sq(4, 3), Sq(3, 4)),
            (3, Sq(3, 7), Sq(3, 4)),
        ] {
            game.click_square(t, from);
            game.click_square(t, to);
            game.reset_playback();
        }
        frame(&ctx, &mut game, vec![]);
        let output = frame(&ctx, &mut game, vec![]);
        let white = text_position(&output, "(d5, 4)");
        let black = text_position(&output, "(d5, 3)");
        assert!(white.y < black.y);
        click(&ctx, &mut game, black);
        assert!(game.worldline_mode);
        let target = game.worldline_state.unwrap();
        assert_eq!(target.t, 3);
        assert!(
            game.engine
                .piece_state(target)
                .is_some_and(|state| !state.alive)
        );
    }

    #[test]
    fn death_tiles_wrap_on_desktop_and_stay_in_two_rows_on_phones() {
        use temporal_chess_engine::model::{Color, PieceView, State};

        let mut game = GameUi::default();
        let deaths: Vec<_> = (0..7)
            .map(|file| {
                let color = if file < 5 { Color::White } else { Color::Black };
                let mut state = State::new(PieceKind::N, color, Sq(file, 4));
                state.t = 2;
                state.alive = false;
                PieceView::from_state(&state, file, 0)
            })
            .collect();
        for compact in [false, true] {
            let ctx = egui::Context::default();
            let art = PieceArt::new(&ctx);
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(160.0, 350.0))),
                    ..Default::default()
                },
                |ui| hud_deaths(ui, &mut game, &art, &deaths, compact),
            );
            output.textures_delta.clear();
            let first = text_position(&output, "(a5, 2)");
            let last_white = text_position(&output, "(e5, 2)");
            let black = text_position(&output, "(f5, 2)");
            assert!(last_white.y < black.y);
            assert!((first.x - black.x).abs() < 2.0);
            if compact {
                assert_eq!(first.y, last_white.y);
                assert!(last_white.x >= 160.0);
            } else {
                assert!(first.y < last_white.y);
                for file in 'a'..='g' {
                    assert!(text_position(&output, &format!("({file}5, 2)")).x < 160.0);
                }
            }
        }
    }

    #[test]
    fn copy_button_emits_the_record_and_import_accepts_paste_without_game_shortcuts() {
        let ctx = egui::Context::default();
        let mut game = GameUi::default();
        game.click_square(0, Sq(4, 1));
        game.click_square(0, Sq(4, 3));
        game.reset_playback();
        game.local_auto_flip = false;
        frame(&ctx, &mut game, vec![]);
        let _output = frame(&ctx, &mut game, vec![]);
        let pos = icon_position(&ctx, "Копировать партию");
        let output = click(&ctx, &mut game, pos);
        let text = game.record.to_text();
        assert!(output.platform_output.commands.iter().any(
            |command| matches!(command, egui::OutputCommand::CopyText(value) if value == &text)
        ));
        let pos = icon_position(&ctx, "Загрузить партию из записи");
        click(&ctx, &mut game, pos);
        assert!(game.import_open);
        let output = frame(&ctx, &mut game, vec![]);
        let pos = text_position(&output, "NOLAN-CHESS 1");
        click(&ctx, &mut game, pos);
        frame(
            &ctx,
            &mut game,
            vec![
                egui::Event::Paste(text.clone()),
                egui::Event::Key {
                    key: egui::Key::I,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                },
            ],
        );
        assert_eq!(game.import_text, text);
        assert!(!game.inspect_mode);
        assert_eq!(game.record.commands().len(), 1);
    }
}
