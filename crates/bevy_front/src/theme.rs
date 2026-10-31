use bevy_egui::egui::{self, Color32, FontId, Vec2};
use temporal_chess_engine::model::TimeDirection;

pub const FUTURE: Color32 = Color32::from_rgb(241, 108, 169);
pub const PAST: Color32 = Color32::from_rgb(128, 21, 52);
pub const REACTION: Color32 = Color32::from_rgb(240, 125, 32);
pub const INFORMATION: Color32 = Color32::from_rgb(245, 205, 46);
pub const ORDINARY: Color32 = Color32::from_rgb(83, 207, 105);
pub const CAPTURE: Color32 = Color32::from_rgb(235, 48, 61);
pub const INVERTED: Color32 = Color32::from_rgb(150, 134, 190);
pub const MUTED: Color32 = Color32::from_rgb(158, 154, 146);

pub const BACKGROUND: Color32 = Color32::from_rgb(22, 24, 29);
pub const PANEL: Color32 = Color32::from_rgb(30, 33, 40);
pub const SURFACE: Color32 = Color32::from_rgb(40, 44, 53);
pub const BORDER: Color32 = Color32::from_rgb(55, 60, 71);
pub const TEXT: Color32 = Color32::from_rgb(231, 234, 241);
pub const SUBTLE: Color32 = Color32::from_rgb(155, 163, 180);
pub const ACCENT: Color32 = Color32::from_rgb(156, 178, 239);

pub fn configure(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "text".into(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/DejaVuSans.ttf")).into(),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "text".into());
    ctx.set_fonts(fonts);
    ctx.tessellation_options_mut(|options| {
        options.feathering = true;
        options.feathering_size_in_pixels = 1.0;
    });
    ctx.set_theme(egui::Theme::Dark);
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = PANEL;
    style.visuals.window_fill = PANEL;
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.selection.bg_fill = Color32::from_rgb(63, 76, 110);
    style.visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT);
    style.visuals.window_corner_radius = egui::CornerRadius::same(14);
    style.visuals.window_stroke = egui::Stroke::new(1.0, BORDER);
    style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, BORDER);
    for widget in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        widget.corner_radius = egui::CornerRadius::same(10);
        widget.bg_stroke = egui::Stroke::NONE;
        widget.fg_stroke = egui::Stroke::new(1.5, TEXT);
    }
    style.visuals.widgets.inactive.bg_fill = SURFACE;
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(55, 62, 77);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(67, 80, 111);
    style.spacing.item_spacing = Vec2::new(8.0, 10.0);
    style.spacing.button_padding = Vec2::new(10.0, 8.0);
    style.spacing.interact_size.y = 38.0;
    style.spacing.tooltip_width = 300.0;
    style.interaction.tooltip_delay = 0.7;
    style.interaction.tooltip_grace_time = 0.0;
    style.interaction.show_tooltips_only_when_still = true;
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, FontId::proportional(19.0));
    style
        .text_styles
        .insert(egui::TextStyle::Small, FontId::proportional(12.0));
    ctx.set_style_of(egui::Theme::Dark, style);
}

pub fn direction_color(direction: TimeDirection) -> Color32 {
    match direction {
        TimeDirection::Future => FUTURE,
        TimeDirection::Past => PAST,
    }
}
