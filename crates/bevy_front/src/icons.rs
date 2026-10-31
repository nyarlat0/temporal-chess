//! Small vector icons share geometry, hit targets, focus and tooltip behavior.
use crate::theme::{ACCENT, SUBTLE, TEXT};
use bevy_egui::egui::{self, Pos2, Rect, Response, Stroke, StrokeKind, Vec2};

#[derive(Clone, Copy)]
pub enum Icon {
    New,
    Undo,
    Copy,
    Import,
    Help,
    Up,
    Down,
    First,
    Last,
    Future,
    Past,
    Continue,
    Close,
    Check,
    ResetZoom,
    Skull,
    Information,
    CancelledMove,
}

pub struct IconResponse(Response);
impl IconResponse {
    pub fn clicked(&self) -> bool {
        crate::input::clicked(&self.0)
    }
}

pub fn icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    selected: bool,
    enabled: bool,
) -> IconResponse {
    let response = ui
        .push_id(label, |ui| {
            ui.add_enabled(
                enabled,
                egui::Button::new("")
                    .min_size(Vec2::splat(38.0))
                    .selected(selected),
            )
        })
        .inner;
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, selected, label)
    });
    let color = if !enabled {
        SUBTLE.gamma_multiply(0.35)
    } else if selected {
        ACCENT
    } else {
        TEXT
    };
    paint(
        ui.painter(),
        icon,
        response.rect.shrink(10.0),
        Stroke::new(1.65, color),
    );
    #[cfg(test)]
    ui.ctx()
        .data_mut(|data| data.insert_temp(egui::Id::new(("icon", label)), response.rect));
    IconResponse(response.on_hover_text(label).on_disabled_hover_text(label))
}

pub fn paint(painter: &egui::Painter, icon: Icon, rect: Rect, stroke: Stroke) {
    let p = |x: f32, y: f32| {
        Pos2::new(
            rect.left() + x * rect.width(),
            rect.top() + y * rect.height(),
        )
    };
    let line = |points: &[(f32, f32)]| {
        painter.add(egui::Shape::line(
            points.iter().map(|&(x, y)| p(x, y)).collect(),
            stroke,
        ));
    };
    let box_at = |a, b| {
        painter.rect_stroke(Rect::from_min_max(a, b), 2.0, stroke, StrokeKind::Inside);
    };
    match icon {
        Icon::Information => {
            line(&[
                (0.05, 0.5),
                (0.25, 0.28),
                (0.5, 0.2),
                (0.75, 0.28),
                (0.95, 0.5),
                (0.75, 0.72),
                (0.5, 0.8),
                (0.25, 0.72),
                (0.05, 0.5),
            ]);
            painter.circle_stroke(p(0.5, 0.5), rect.width() * 0.16, stroke);
        }
        Icon::CancelledMove => {
            line(&[(0.5, 0.9), (0.5, 0.1)]);
            line(&[(0.15, 0.45), (0.5, 0.1), (0.85, 0.45)]);
            line(&[(0.08, 0.85), (0.92, 0.15)]);
        }
        Icon::Skull => {
            let dark = crate::theme::BACKGROUND;
            painter.circle_filled(p(0.5, 0.37), rect.width() * 0.36, stroke.color);
            painter.rect_filled(
                Rect::from_min_max(p(0.29, 0.49), p(0.71, 0.92)),
                rect.width() * 0.06,
                stroke.color,
            );
            for x in [0.35, 0.65] {
                painter.circle_filled(p(x, 0.39), rect.width() * 0.09, dark);
            }
            painter.add(egui::Shape::convex_polygon(
                vec![p(0.5, 0.52), p(0.43, 0.65), p(0.57, 0.65)],
                dark,
                Stroke::NONE,
            ));
            for x in [0.42, 0.58] {
                painter.line_segment(
                    [p(x, 0.78), p(x, 0.94)],
                    Stroke::new((rect.width() * 0.055).max(1.0), dark),
                );
            }
        }
        Icon::New => {
            line(&[(0.5, 0.1), (0.5, 0.9)]);
            line(&[(0.1, 0.5), (0.9, 0.5)]);
        }
        Icon::Close => {
            line(&[(0.2, 0.2), (0.8, 0.8)]);
            line(&[(0.2, 0.8), (0.8, 0.2)]);
        }
        Icon::Check => line(&[(0.1, 0.5), (0.4, 0.8), (0.9, 0.2)]),
        Icon::Undo => {
            line(&[(0.3, 0.1), (0.05, 0.35), (0.3, 0.6)]);
            line(&[
                (0.05, 0.35),
                (0.65, 0.35),
                (0.85, 0.5),
                (0.85, 0.75),
                (0.7, 0.9),
                (0.4, 0.9),
            ]);
        }
        Icon::Copy => {
            box_at(p(0.3, 0.3), p(0.95, 0.95));
            line(&[
                (0.65, 0.2),
                (0.65, 0.05),
                (0.05, 0.05),
                (0.05, 0.65),
                (0.2, 0.65),
            ]);
        }
        Icon::Import => {
            line(&[(0.5, 0.05), (0.5, 0.65)]);
            line(&[(0.25, 0.4), (0.5, 0.65), (0.75, 0.4)]);
            line(&[(0.1, 0.65), (0.1, 0.95), (0.9, 0.95), (0.9, 0.65)]);
        }
        Icon::Help => {
            painter.circle_stroke(p(0.5, 0.5), rect.width() * 0.46, stroke);
            line(&[
                (0.35, 0.33),
                (0.42, 0.24),
                (0.58, 0.24),
                (0.66, 0.33),
                (0.66, 0.42),
                (0.5, 0.53),
                (0.5, 0.6),
            ]);
            painter.circle_filled(p(0.5, 0.76), 1.1, stroke.color);
        }
        Icon::Up | Icon::First => {
            line(&[(0.2, 0.6), (0.5, 0.3), (0.8, 0.6)]);
            if matches!(icon, Icon::First) {
                line(&[(0.15, 0.1), (0.85, 0.1)]);
            }
        }
        Icon::Down | Icon::Last => {
            line(&[(0.2, 0.4), (0.5, 0.7), (0.8, 0.4)]);
            if matches!(icon, Icon::Last) {
                line(&[(0.15, 0.9), (0.85, 0.9)]);
            }
        }
        Icon::Future | Icon::Past => {
            if matches!(icon, Icon::Future) {
                line(&[(0.5, 0.9), (0.5, 0.1)]);
                line(&[(0.15, 0.45), (0.5, 0.1), (0.85, 0.45)]);
            } else {
                line(&[(0.5, 0.1), (0.5, 0.9)]);
                line(&[(0.15, 0.55), (0.5, 0.9), (0.85, 0.55)]);
            }
        }
        Icon::Continue => {
            line(&[(0.15, 0.15), (0.55, 0.5), (0.15, 0.85)]);
            line(&[(0.5, 0.15), (0.9, 0.5), (0.5, 0.85)]);
        }
        Icon::ResetZoom => {
            line(&[(0.35, 0.05), (0.05, 0.05), (0.05, 0.35)]);
            line(&[(0.65, 0.05), (0.95, 0.05), (0.95, 0.35)]);
            line(&[(0.35, 0.95), (0.05, 0.95), (0.05, 0.65)]);
            line(&[(0.65, 0.95), (0.95, 0.95), (0.95, 0.65)]);
            painter.circle_stroke(p(0.5, 0.5), rect.width() * 0.15, stroke);
        }
    }
}
