//! A real browser input for editing fields on touch devices. The canvas
//! controls remain the source of the committed values.
use bevy_egui::egui;
use std::{fmt::Display, ops::RangeInclusive, str::FromStr};

#[derive(Clone, Copy)]
pub enum Keyboard {
    Text,
    Url,
    Numeric,
    Decimal,
}

pub fn available(ctx: &egui::Context) -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        crate::view::mobile_layout(ctx) && web::Editor::get().is_some()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = ctx;
        false
    }
}

pub fn text(
    ui: &mut egui::Ui,
    value: &mut String,
    title: &str,
    hint: &str,
    keyboard: Keyboard,
    max_chars: Option<usize>,
) {
    if available(ui.ctx()) {
        let initial = value.clone();
        let preview = if initial.is_empty() { hint } else { &initial };
        edit(ui, title, preview, &initial, keyboard, max_chars, |input| {
            *value = input
                .chars()
                .take(max_chars.unwrap_or(usize::MAX))
                .collect();
            Ok(())
        });
    } else {
        let mut widget = egui::TextEdit::singleline(value).hint_text(hint);
        if let Some(max_chars) = max_chars {
            widget = widget.char_limit(max_chars);
        }
        ui.add(widget);
    }
}

pub fn integer<T>(
    ui: &mut egui::Ui,
    value: &mut T,
    title: &str,
    range: RangeInclusive<T>,
    speed: f64,
) where
    T: egui::emath::Numeric + Display + FromStr,
{
    if available(ui.ctx()) {
        let initial = value.to_string();
        // Some mobile numeric keyboards omit the minus key.
        let keyboard = if range.start().to_f64() < 0.0 {
            Keyboard::Text
        } else {
            Keyboard::Numeric
        };
        edit(ui, title, &initial, &initial, keyboard, None, |input| {
            *value = parse_number(input, &range).ok_or_else(|| range_error(&range))?;
            Ok(())
        });
    } else {
        ui.add(egui::DragValue::new(value).range(range).speed(speed));
    }
}

pub fn decimal(
    ui: &mut egui::Ui,
    value: &mut f32,
    title: &str,
    range: RangeInclusive<f32>,
    step: f32,
    suffix: &str,
) {
    let initial = format!("{value:.2}");
    let preview = format!("{initial}{suffix}");
    edit(
        ui,
        title,
        &preview,
        &initial,
        Keyboard::Decimal,
        None,
        |input| {
            let input = input.replace(',', ".");
            let number = parse_number(&input, &range).ok_or_else(|| range_error(&range))?;
            let start = range.start();
            *value = (start + ((number - start) / step).round() * step).clamp(*start, *range.end());
            Ok(())
        },
    );
}

fn parse_number<T: FromStr + PartialOrd>(input: &str, range: &RangeInclusive<T>) -> Option<T> {
    let value = input.trim().parse().ok()?;
    range.contains(&value).then_some(value)
}

fn range_error<T: Display>(range: &RangeInclusive<T>) -> String {
    crate::tr_format!(
        "Введите число от {} до {}.",
        "Enter a number from {} to {}.",
        range.start(),
        range.end()
    )
}

fn edit(
    ui: &mut egui::Ui,
    title: &str,
    preview: &str,
    initial: &str,
    keyboard: Keyboard,
    max_chars: Option<usize>,
    apply: impl FnOnce(&str) -> Result<(), String>,
) {
    #[cfg(target_arch = "wasm32")]
    if let Some(editor) = web::Editor::get() {
        let id = ui.make_persistent_id(("native_input", title));
        let field = id.value().to_string();
        if editor.owns(&field) && editor.apply(apply) {
            ui.ctx().request_repaint();
        }
        let button = egui::Button::new(preview).truncate();
        if crate::input::clicked(&ui.add(button).on_hover_text(title)) && !editor.is_open() {
            editor.open(&field, title, initial, keyboard, max_chars);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (ui, title, preview, initial, keyboard, max_chars, apply);
}

#[cfg(target_arch = "wasm32")]
pub(crate) use web::install_wakeup;

#[cfg(target_arch = "wasm32")]
mod web {
    use super::Keyboard;
    use wasm_bindgen::{JsCast, closure::Closure};
    use web_sys::{Document, Element, HtmlDialogElement, HtmlInputElement};

    fn document() -> Option<Document> {
        web_sys::window()?.document()
    }

    pub struct Editor {
        dialog: HtmlDialogElement,
        title: Element,
        input: HtmlInputElement,
        status: Element,
    }

    impl Editor {
        pub fn get() -> Option<Self> {
            let document = document()?;
            Some(Self {
                dialog: document
                    .get_element_by_id("mobile-input")?
                    .dyn_into()
                    .ok()?,
                title: document.get_element_by_id("mobile-input-title")?,
                input: document
                    .get_element_by_id("mobile-input-value")?
                    .dyn_into()
                    .ok()?,
                status: document.get_element_by_id("mobile-input-status")?,
            })
        }

        pub fn is_open(&self) -> bool {
            self.dialog.open()
        }

        pub fn owns(&self, field: &str) -> bool {
            self.is_open() && self.dialog.get_attribute("data-field").as_deref() == Some(field)
        }

        pub fn open(
            &self,
            field: &str,
            title: &str,
            value: &str,
            keyboard: Keyboard,
            max_chars: Option<usize>,
        ) {
            let mode = match keyboard {
                Keyboard::Text => "text",
                Keyboard::Url => "url",
                Keyboard::Numeric => "numeric",
                Keyboard::Decimal => "decimal",
            };
            let _ = self.dialog.set_attribute("data-field", field);
            let _ = self.dialog.remove_attribute("data-action");
            let _ = self.input.set_attribute("inputmode", mode);
            if let Some(max_chars) = max_chars {
                let _ = self
                    .input
                    .set_attribute("maxlength", &max_chars.to_string());
            } else {
                let _ = self.input.remove_attribute("maxlength");
            }
            self.title.set_text_content(Some(title));
            self.input.set_value(value);
            self.status.set_text_content(None);
            let _ = self.dialog.show_modal();
        }

        pub fn apply(&self, apply: impl FnOnce(&str) -> Result<(), String>) -> bool {
            if self.dialog.get_attribute("data-action").as_deref() != Some("submit") {
                return false;
            }
            let _ = self.dialog.remove_attribute("data-action");
            match apply(&self.input.value()) {
                Ok(()) => self.dialog.close(),
                Err(message) => self.status.set_text_content(Some(&message)),
            }
            true
        }
    }

    pub fn install_wakeup(proxy: &bevy::winit::EventLoopProxyWrapper) {
        let Some(document) = document() else { return };
        let proxy = (**proxy).clone();
        let wake = Closure::<dyn FnMut()>::new(move || {
            let _ = proxy.send_event(bevy::winit::WinitUserEvent::WakeUp);
        });
        if document
            .add_event_listener_with_callback(
                "nolan-native-form-action",
                wake.as_ref().unchecked_ref(),
            )
            .is_ok()
        {
            // The listeners and forms live for the lifetime of the page.
            wake.forget();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_fields_accept_signed_board_times_without_losing_precision() {
        assert_eq!(
            parse_number(" -9007199254740993 ", &(i64::MIN..=0)),
            Some(-9007199254740993)
        );
        assert_eq!(parse_number("5.5", &(3..=255)), None);
        assert_eq!(parse_number("256", &(3..=255)), None);
        assert_eq!(parse_number("NaN", &(0.25_f32..=3.0)), None);
    }
}
