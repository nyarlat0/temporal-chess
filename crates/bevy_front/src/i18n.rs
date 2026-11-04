//! UI language is independent of engine state, records and the network protocol.
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Russian,
    English,
}
static LANGUAGE: AtomicU8 = AtomicU8::new(0);

impl Language {
    pub fn current() -> Self {
        if LANGUAGE.load(Ordering::Relaxed) == 0 {
            Self::Russian
        } else {
            Self::English
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn code(self) -> &'static str {
        match self {
            Self::Russian => "ru",
            Self::English => "en",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Russian => "Русский",
            Self::English => "English",
        }
    }

    pub fn select<'a>(self, ru: &'a str, en: &'a str) -> &'a str {
        match self {
            Self::Russian => ru,
            Self::English => en,
        }
    }

    pub fn set(self) {
        LANGUAGE.store(u8::from(self == Self::English), Ordering::Relaxed);
        #[cfg(target_arch = "wasm32")]
        if let Some(window) = web_sys::window() {
            if let Some(root) = window.document().and_then(|d| d.document_element()) {
                let _ = root.set_attribute("lang", self.code());
            }
            if let Ok(Some(storage)) = window.local_storage() {
                let _ = storage.set_item("temporal-chess.language", self.code());
            }
        }
    }
}

pub fn initialize() {
    // The page resolves localStorage and navigator.languages before loading WASM.
    // Do not persist this automatic choice: only an explicit selection is saved.
    #[cfg(target_arch = "wasm32")]
    {
        let code = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.document_element())
            .and_then(|e| e.get_attribute("lang"));
        LANGUAGE.store(u8::from(code.as_deref() != Some("ru")), Ordering::Relaxed);
    }
}

#[macro_export]
macro_rules! tr {
    ($ru:literal, $en:literal) => {
        $crate::i18n::Language::current().select($ru, $en)
    };
}
#[macro_export]
macro_rules! tr_format {
    ($ru:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        match $crate::i18n::Language::current() {
            $crate::i18n::Language::Russian => format!($ru $(, $arg)*),
            $crate::i18n::Language::English => format!($en $(, $arg)*),
        }
    };
}

/// Store both versions so existing notices change language immediately as well.
#[derive(Clone, Debug, Default)]
pub struct Message {
    ru: String,
    en: String,
}
impl Message {
    pub fn new(ru: String, en: String) -> Self {
        Self { ru, en }
    }
    pub fn text(&self) -> &str {
        Language::current().select(&self.ru, &self.en)
    }
    pub fn clear(&mut self) {
        self.ru.clear();
        self.en.clear();
    }
    pub fn is_empty(&self) -> bool {
        self.ru.is_empty() && self.en.is_empty()
    }
}
impl From<String> for Message {
    fn from(text: String) -> Self {
        Self::from(text.as_str())
    }
}
impl From<&str> for Message {
    fn from(text: &str) -> Self {
        error_message(text)
    }
}
#[macro_export]
macro_rules! message {
    ($ru:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        $crate::i18n::Message::new(format!($ru $(, $arg)*), format!($en $(, $arg)*))
    };
}

impl Message {
    pub fn with_error(ru: &str, en: &str, error: impl Into<Message>) -> Self {
        let error = error.into();
        Self::new(format!("{ru}{}", error.ru), format!("{en}{}", error.en))
    }
}

fn error_message(text: &str) -> Message {
    for (ru, en) in [
        ("Партия больше 2 МиБ", "Game record exceeds 2 MiB"),
        ("Пустая запись партии", "Empty game record"),
        ("Не указан режим causal_gaps", "Missing causal_gaps mode"),
        (
            "Ожидалось causal_gaps on или causal_gaps off",
            "Expected causal_gaps on or causal_gaps off",
        ),
        (
            "Ожидался заголовок NOLAN-CHESS 2 или NOLAN-CHESS 1",
            "Expected NOLAN-CHESS 2 or NOLAN-CHESS 1 header",
        ),
        ("Больше 10000 команд", "More than 10000 commands"),
        (
            "Версия фигуры должна иметь вид wid:fut_seen:loop_turn",
            "Piece identity must have the form wid:fut_seen:loop_turn",
        ),
        ("Направление: + или -", "Direction must be + or -"),
        ("Неизвестный вид хода", "Unknown move type"),
        (
            "Неизвестная команда или неверное количество полей",
            "Unknown command or incorrect number of fields",
        ),
        (
            "Не удалось завершить восстановление партии",
            "Could not finish restoring the game",
        ),
        ("Партия уже завершена", "the game has already finished"),
        (
            "Состояние фигуры отсутствует или не может ходить",
            "piece occurrence is absent or cannot act",
        ),
        ("Доска недоступна", "board is unavailable"),
        (
            "Ход недопустим в этой позиции",
            "move is not legal in this position",
        ),
        (
            "Фигура не может ходить в этот ход",
            "piece cannot act on this turn",
        ),
        (
            "Пересчёт истории зациклился",
            "history reconciliation entered a cycle",
        ),
        ("На этом времени нет доски", "no board at this time"),
        ("Нет такой доски.", "No such board."),
        ("Нет такого состояния фигуры.", "No such piece state."),
    ] {
        if text == ru || text == en {
            return Message::new(ru.into(), en.into());
        }
    }
    if let Some(rest) = text
        .strip_prefix("Строка ")
        .or_else(|| text.strip_prefix("Line "))
        && let Some((number, detail)) = rest.split_once(": ")
        && number.parse::<usize>().is_ok()
    {
        let detail = error_message(detail);
        return Message::new(
            format!("Строка {number}: {}", detail.ru),
            format!("Line {number}: {}", detail.en),
        );
    }
    for (ru, en) in [
        ("Команда не принята: ", "Command rejected: "),
        ("Некорректное число: ", "Invalid number: "),
        ("Некорректная клетка: ", "Invalid square: "),
        ("Некорректная история: ", "invalid history: "),
        ("Неподдерживаемый ход: ", "unsupported move: "),
        ("Ошибка движка на этапе ", "engine error at "),
    ] {
        if let Some(detail) = text.strip_prefix(ru).or_else(|| text.strip_prefix(en)) {
            let detail = error_message(detail);
            return Message::new(format!("{ru}{}", detail.ru), format!("{en}{}", detail.en));
        }
    }
    Message::new(text.into(), text.into())
}

pub fn server_error(code: &str, detail: &str) -> Message {
    let (ru, en) = match code {
        "connection_replaced" => (
            "Подключение заменено другим клиентом.",
            "Connection replaced by another client.",
        ),
        "invalid_player_token" => ("Недействительный токен игрока.", "Invalid player token."),
        "invalid_move" => ("Некорректная команда.", "Invalid command."),
        "history_limit" => (
            "Достигнут предел: 10000 команд.",
            "Limit reached: 10000 commands.",
        ),
        "not_your_turn" => ("Сейчас ход соперника.", "It is your opponent's turn."),
        "invalid_command" => ("Недопустимая команда.", "Invalid command."),
        "illegal_command" => ("Ход не принят:", "Move rejected:"),
        "engine_error" => ("Ошибка движка:", "Engine error:"),
        "replay_failed" => (
            "Не удалось восстановить партию.",
            "Could not restore the game.",
        ),
        "revision_overflow" => (
            "Переполнение номера версии партии.",
            "Game revision overflow.",
        ),
        "game_id_exhausted" => (
            "Не удалось создать код комнаты. Попробуйте снова.",
            "Could not allocate a room code. Try again.",
        ),
        "game_not_found" => ("Комната не найдена.", "Room not found."),
        "game_full" => ("Комната заполнена.", "Room is full."),
        "invalid_game_id" => ("Некорректный код комнаты.", "Invalid room code."),
        "invalid_json" => (
            "Некорректное сообщение протокола.",
            "Invalid protocol message.",
        ),
        "binary_unsupported" => (
            "Поддерживаются только текстовые сообщения JSON.",
            "Only JSON text messages are supported.",
        ),
        "not_authenticated" => (
            "Сначала создайте комнату или присоединитесь к ней.",
            "Create or join a room first.",
        ),
        "protocol_mismatch" => (
            "Версии клиента и сервера несовместимы.",
            "Client and server versions are incompatible.",
        ),
        "engine_revision_mismatch" => (
            "Версии движка не совпадают. Оба игрока должны открыть одну версию игры.",
            "Engine revisions differ. Both players must open the same game version.",
        ),
        "invalid_engine_revision" => (
            "Некорректная версия движка. Обновите клиент.",
            "Invalid engine revision. Update the client.",
        ),
        "opponent_missing" => ("Ожидание второго игрока.", "Waiting for the other player."),
        "proposal_pending" => (
            "Предыдущая команда ещё ожидает подтверждения.",
            "The previous command is still awaiting confirmation.",
        ),
        "already_authenticated" => (
            "Это подключение уже используется в партии.",
            "This connection already belongs to a game.",
        ),
        "server_error" => (
            "Сервер не смог обработать запрос.",
            "The server could not process the request.",
        ),
        _ => {
            return Message::new(
                format!("Ошибка сервера ({code}): {}", error_message(detail).ru),
                format!("Server error ({code}): {}", error_message(detail).en),
            );
        }
    };
    if matches!(
        code,
        "invalid_command" | "illegal_command" | "engine_error" | "replay_failed"
    ) {
        Message::with_error(&format!("{ru} "), &format!("{en} "), detail)
    } else {
        Message::new(ru.into(), en.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn engine_and_unknown_server_errors_preserve_diagnostics_in_both_languages() {
        let detail = "engine error at collision at t=0, pieces 4:4 and 12:0: missing predecessor";
        for code in ["illegal_command", "engine_error", "unexpected_failure"] {
            let message = server_error(code, detail);
            for text in [message.ru, message.en] {
                assert!(text.contains("t=0"));
                assert!(text.contains("4:4 and 12:0"));
                assert!(text.contains("missing predecessor"));
            }
        }
        assert!(
            Message::from(detail)
                .ru
                .starts_with("Ошибка движка на этапе")
        );
    }

    #[test]
    fn stored_messages_keep_both_languages_and_localize_nested_errors() {
        let message = Message::with_error(
            "Импорт: ",
            "Import: ",
            "Строка 3: Команда не принята: move is not legal in this position",
        );
        assert_eq!(
            message.ru,
            "Импорт: Строка 3: Команда не принята: Ход недопустим в этой позиции"
        );
        assert_eq!(
            message.en,
            "Import: Line 3: Command rejected: move is not legal in this position"
        );
        assert_eq!(
            server_error("not_your_turn", "Сейчас ход соперника.").en,
            "It is your opponent's turn."
        );
    }
}
