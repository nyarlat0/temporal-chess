use super::{Action, CHAPTERS, Leave, select_chapter};
use crate::{controller::GameUi, theme::*};
use bevy_egui::egui::{self, Color32, Rect, Stroke, StrokeKind};

fn rects_id() -> egui::Id {
    egui::Id::new("tutorial_spotlights")
}

pub fn begin_pass(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<Vec<Rect>>(rects_id()));
}

pub(super) fn register(ctx: &egui::Context, rect: Rect) {
    if rect.is_positive() {
        ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<Vec<Rect>>(rects_id())
                .push(rect)
        });
    }
}

// Subtract the union of the spotlight openings from the viewport before
// painting. Overlapping highlights never darken each other.
fn subtract(rect: Rect, opening: Rect) -> impl Iterator<Item = Rect> {
    let middle = rect.intersect(opening);
    let [x0, x1, x2, x3] = [rect.left(), middle.left(), middle.right(), rect.right()];
    let [y0, y1, y2, y3] = [rect.top(), middle.top(), middle.bottom(), rect.bottom()];
    let rect = |x0, y0, x1, y1| Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1));
    [
        rect(x0, y0, x3, y1),
        rect(x0, y2, x3, y3),
        rect(x0, y1, x1, y2),
        rect(x2, y1, x3, y2),
    ]
    .into_iter()
    .filter(Rect::is_positive)
}

pub(super) fn openings(ctx: &egui::Context) -> Vec<Rect> {
    ctx.data(|d| d.get_temp::<Vec<Rect>>(rects_id()))
        .unwrap_or_default()
}

pub fn paint_spotlight(ctx: &egui::Context, panel: Rect, busy: bool) {
    if busy {
        return;
    }
    let viewport = ctx.viewport_rect();
    let openings = openings(ctx);
    if openings.is_empty() {
        return;
    }
    let mut shade = vec![viewport];
    for hole in std::iter::once(panel).chain(openings.iter().map(|r| r.expand(3.0))) {
        shade = shade
            .into_iter()
            .flat_map(|rect| {
                if rect.intersect(hole).is_positive() {
                    subtract(rect, hole).collect()
                } else {
                    vec![rect]
                }
            })
            .collect();
    }
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::new("tutorial_shade"),
    ));
    for rect in shade {
        painter.rect_filled(rect, 0.0, Color32::from_black_alpha(155));
    }
    for opening in openings.iter() {
        painter.rect_stroke(
            opening.expand(2.0),
            5.0,
            Stroke::new(2.0, INVERTED),
            StrokeKind::Outside,
        );
    }
}

pub fn panel(ui: &mut egui::Ui, game: &mut GameUi, compact: bool) {
    let session = game.tutorial.as_ref().unwrap();
    let chapter = &CHAPTERS[session.cursor.chapter];
    let step = session.step();
    let busy = game.playback_busy() || game.facing.animating() || game.replay_job.is_some();
    let completed = session.completed;
    let chapter_steps: usize = chapter.scenes.iter().map(|scene| scene.steps.len()).sum();
    let steps_before_scene: usize = chapter.scenes[..session.cursor.scene]
        .iter()
        .map(|scene| scene.steps.len())
        .sum();
    let steps_done = steps_before_scene + session.cursor.step;
    let mut leave = None;
    let mut selected_chapter = session.cursor.chapter;
    let mut restart = false;
    let mut advance = false;
    let mut playback = false;
    let bounds = ui.max_rect().intersect(ui.clip_rect());
    let mut footer = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("tutorial_actions")
            .max_rect(bounds)
            .layout(egui::Layout::bottom_up(egui::Align::Min)),
    );
    footer.set_clip_rect(bounds);
    if !compact {
        crate::view::language_header(&mut footer);
        footer.separator();
    }
    footer.horizontal(|ui| {
        if completed {
            if crate::input::clicked(&ui.button(crate::tr!("К игре", "Back to game"))) {
                leave = Some(Leave::Pause);
            }
        } else {
            if step.action == Action::Next {
                advance = crate::input::clicked(
                    &ui.add_enabled(!busy, egui::Button::new(crate::tr!("Далее", "Next"))),
                );
            }
            restart = crate::input::clicked(
                &ui.button(crate::tr!("Заново", "Restart"))
                    .on_hover_text(crate::tr!("Перезапустить главу", "Restart this chapter")),
            );
            playback = crate::view::timeline_playback_button(ui, game, !busy);
        }
    });

    // Measure the controls first so scrolling text never takes their space.
    let body_rect = Rect::from_min_max(
        bounds.min,
        egui::pos2(
            bounds.right(),
            (footer.min_rect().top() - ui.spacing().item_spacing.y).max(bounds.top()),
        ),
    );
    let mut body = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("tutorial_body")
            .max_rect(body_rect),
    );
    body.set_clip_rect(body_rect);
    if compact {
        body.spacing_mut().item_spacing.y = 4.0;
        body.spacing_mut().interact_size.y = 26.0;
    }
    body.horizontal_wrapped(|ui| {
        ui.strong(crate::tr!("Обучение", "Tutorial"));
        ui.colored_label(
            SUBTLE,
            format!("{}/{}", session.cursor.chapter + 1, CHAPTERS.len()),
        );
        let skip = crate::tr!("Пропустить обучение", "Skip tutorial");
        if !completed
            && crate::input::clicked(
                &ui.small_button(crate::tr!("Пропустить", "Skip"))
                    .on_hover_text(skip),
            )
        {
            leave = Some(Leave::Skip);
        }
    });
    if completed {
        body.heading(crate::tr!("Обучение пройдено", "Tutorial complete"));
    } else {
        let width = body.available_width();
        egui::ComboBox::from_id_salt("tutorial_chapters")
            .width(width)
            .wrap()
            .selected_text(chapter.title.get())
            .show_ui(&mut body, |ui| {
                ui.set_max_width(ui.available_width().min(width));
                for (index, entry) in CHAPTERS.iter().enumerate() {
                    if crate::input::clicked(
                        &ui.add(
                            egui::Button::selectable(index == selected_chapter, entry.title.get())
                                .wrap(),
                        ),
                    ) {
                        selected_chapter = index;
                    }
                }
            });
    }
    body.add(
        egui::ProgressBar::new(if completed {
            1.0
        } else {
            steps_done as f32 / chapter_steps as f32
        })
        .desired_height(3.0),
    );
    egui::ScrollArea::vertical()
        .id_salt((
            "tutorial_text",
            session.cursor.chapter,
            session.cursor.scene,
            session.cursor.step,
            completed,
        ))
        .max_height(body.available_height())
        .min_scrolled_height(0.0)
        .auto_shrink([false, false])
        .show(&mut body, |ui| {
            ui.add(
                egui::Label::new(if completed {
                    crate::tr!(
                        "Теперь можно начать свою партию. Обучение всегда доступно в помощи.",
                        "You can now start your own game. The tutorial is always available in Help."
                    )
                } else {
                    step.text.get()
                })
                .wrap(),
            );
            if game.is_error {
                ui.add(
                    egui::Label::new(egui::RichText::new(game.notice.text()).color(CAPTURE)).wrap(),
                );
            }
        });

    if let Some(leave) = leave {
        game.tutorial.as_mut().unwrap().leave = Some(leave);
        ui.ctx().request_repaint();
    } else if restart || selected_chapter != session.cursor.chapter {
        select_chapter(game, selected_chapter);
        ui.ctx().request_repaint();
    } else if advance {
        game.tutorial.as_mut().unwrap().advance();
        ui.ctx().request_repaint();
    } else if playback {
        game.toggle_history_playback();
        ui.ctx().request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spotlight_preserves_exact_opening() {
        let rect = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(10.0, 10.0));
        let hole = Rect::from_min_max(egui::pos2(3.0, 3.0), egui::pos2(8.0, 8.0));
        let pieces = subtract(rect, hole).collect::<Vec<_>>();
        assert_eq!(pieces.iter().map(Rect::area).sum::<f32>(), 75.0);
        assert!(pieces.iter().all(|r| !r.intersect(hole).is_positive()));
    }
}
