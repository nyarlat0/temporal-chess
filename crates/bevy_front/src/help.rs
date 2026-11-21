//! In-game reference. Legend samples use the board's artwork and drawing helpers.
use crate::{
    board_view::{self, DARK, LIGHT},
    icons::{self, Icon, icon_button},
    piece_art::{Outline, PieceArt, PieceStyle},
    theme::*,
};
use bevy_egui::egui::{self, Color32, Rect, Sense, Stroke, StrokeKind, Vec2};
use temporal_chess_engine::model::{Color, GameRules, PieceKind, Sq};

pub fn show(
    ctx: &egui::Context,
    rules: GameRules,
    open: &mut bool,
    art: &PieceArt,
) -> Option<bool> {
    let mut tutorial = None;
    let frame = egui::Frame::popup(&ctx.style_of(egui::Theme::Dark));
    let height = (ctx.viewport_rect().height() - 24.0 - frame.total_margin().sum().y).max(0.0);
    egui::Modal::new("help".into())
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(660.0_f32.min((ctx.viewport_rect().width() - 48.0).max(240.0)));
            ui.set_height(height);
            ui.horizontal(|ui| {
                ui.heading(crate::tr!("Помощь", "Help"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(
                        ui,
                        Icon::Close,
                        crate::tr!("Закрыть справку", "Close help"),
                        false,
                        true,
                    )
                    .clicked()
                    {
                        *open = false;
                    }
                });
            });
            ui.colored_label(
                SUBTLE,
                crate::tr!(
                    "Правила, управление и обозначения Temporal Chess",
                    "Temporal Chess rules, controls and legend"
                ),
            );
            ui.horizontal_wrapped(|ui| {
                if crate::input::clicked(
                    &ui.button(crate::tr!("Пройти обучение заново", "Replay tutorial")),
                ) {
                    tutorial = Some(false);
                }
                if crate::tutorial::can_resume()
                    && crate::input::clicked(
                        &ui.button(crate::tr!("Продолжить обучение", "Resume tutorial")),
                    )
                {
                    tutorial = Some(true);
                }
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("help_contents")
                .max_height(ui.available_height().max(0.0))
                .min_scrolled_height(0.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    section(
                        ui,
                        "rules",
                        crate::tr!("Как играть", "How to play"),
                        false,
                        rules_reference,
                    );
                    section(
                        ui,
                        "controls",
                        crate::tr!("Управление", "Controls"),
                        false,
                        controls,
                    );
                    section(
                        ui,
                        "legend",
                        crate::tr!("Обозначения на досках", "Board legend"),
                        true,
                        |ui| legend(ui, art),
                    );
                    section(
                        ui,
                        "tools",
                        crate::tr!("Кнопки и инспекция", "Tools and inspection"),
                        false,
                        tools,
                    );
                    section(
                        ui,
                        "modes",
                        crate::tr!("Партии и настройки правил", "Games and rule options"),
                        false,
                        |ui| modes(ui, rules),
                    );
                    section(
                        ui,
                        "lab",
                        crate::tr!("Редактор Lab", "Lab editor"),
                        false,
                        lab,
                    );
                    ui.separator();
                    ui.small("Cburnett / Lichess · GPL-2.0+ · Lucide · ISC / MIT");
                });
        });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        *open = false;
    }
    tutorial
}

fn section(
    ui: &mut egui::Ui,
    id: &str,
    title: &str,
    default_open: bool,
    content: impl FnOnce(&mut egui::Ui),
) {
    egui::CollapsingHeader::new(egui::RichText::new(title).strong())
        .id_salt(id)
        .default_open(default_open)
        .show(ui, |ui| {
            content(ui);
            ui.add_space(10.0);
        });
}

fn paragraph(ui: &mut egui::Ui, title: &str, text: &str) {
    ui.strong(title);
    ui.add(egui::Label::new(text).wrap());
    ui.add_space(6.0);
}

fn rules_reference(ui: &mut egui::Ui) {
    for (title, text) in [
        (
            crate::tr!(
                "Ход в пространстве и времени",
                "Moving through space and time"
            ),
            crate::tr!(
                "Каждый пространственный ход переносит фигуру на соседнюю доску: t + 1 или t − 1. Направление можно выбрать заново при каждом ходе. Возможные клетки показаны на исходной доске. Белые действуют с чётных досок, чёрные — с нечётных, в том числе при отрицательном времени.",
                "Each spatial move takes a piece to the adjacent board: t + 1 or t − 1. You may choose either direction for each move. Destinations are shown on the source board. White acts from even-numbered boards and Black from odd-numbered boards, including negative times."
            ),
        ),
        (
            crate::tr!("Кто может ходить", "Who can act"),
            crate::tr!(
                "Выбирайте активное состояние своей фигуры на доске своего цвета. Живое последнее состояние мировой линии на такой доске имеет обычную активность. Более ранние состояния могут реагировать на новую информацию или отмену хода. Ход передаёт очередь сопернику; реакции необязательны. Если остались только реакции, можно нажать «Не реагировать».",
                "Select an active state of your piece on a board of your color. A living final state of a worldline on such a board has ordinary activity. Earlier states may react to new information or a cancelled move. A move passes the turn to your opponent; reactions are optional. When only reactions remain, you can choose Decline reactions."
            ),
        ),
        (
            crate::tr!("Переписывание истории", "Rewriting history"),
            crate::tr!(
                "Изменение прошлого может отменить или заменить уже сделанные ходы, вызвать столкновения и изменить последующие доски. Даже ход, который исчез из пересчитанной позиции, остаётся в журнале партии и экспорте. У временной границы пространственный ход создаёт новые доски; отскока больше нет. Ожидание и пассивное продолжение сами по себе историю не расширяют.",
                "Changing the past can cancel or replace existing moves, cause collisions and alter later boards. Even a move that disappears from the reconciled position remains in the game journal and export. At a time boundary, a spatial move creates new boards; there is no bounce. Waiting and passive continuation do not extend the timeline by themselves."
            ),
        ),
        (
            crate::tr!("Ожидание", "Waiting"),
            crate::tr!(
                "Выберите фигуру, затем ту же клетку на другой доступной доске по её направлению времени. Можно также выбрать ход и конечную доску ожидания после него: движок сначала применит ход и затем проверит ожидание. Ручное ожидание блокируется первой встреченной фигурой любого цвета и заканчивается на доске своего цвета. По умолчанию отдельное ожидание передаёт ход. Заблокированный конец мировой линии может автоматически продлеваться внутри существующей истории.",
                "Select a piece, then the same square on another available board along its time direction. You can also choose a move and a later waiting destination: the engine applies the move before checking the wait. Manual waiting is blocked by the first piece of either color and ends on a board of your color. Standalone waiting spends a turn by default. A blocked worldline endpoint may automatically continue within the existing history."
            ),
        ),
        (
            crate::tr!("Видимая информация", "Visible information"),
            crate::tr!(
                "Фигура видит доступные цели на своей доске. Наблюдение определяется клеткой, типом фигуры и её ориентацией. Исчезнувшая цель перестаёт поддерживать инфоактивацию. Если она снова появится в поле зрения, фигура сможет отреагировать заново. Ход гасит реакции на те цели, которые фигуры видят к началу хода, а ожидание само по себе не активирует ждущую фигуру. Состояние с записанным ходом не получает инфоактивацию.",
                "A piece sees targets on its own board. An observation is defined by the target’s square, type and orientation. When a target leaves sight, its information reaction goes away. Reappearing targets can trigger a new reaction. Taking a turn clears reactions to targets already visible at its start, and waiting does not itself activate the waiting piece. A state with a recorded move does not gain an information reaction."
            ),
        ),
        (
            crate::tr!("Взятия и столкновения", "Captures and collisions"),
            crate::tr!(
                "При встречном столкновении фигур разных цветов всегда выживает инвертированная. При одинаковой временной ориентации обычное взятие убивает цель, но пешка может взять только своим атакующим ходом. Столкновение со своей фигурой тоже влияет на историю, а не считается обычным взятием. Красный череп предупреждает, что выбранная фигура проиграет столкновение.",
                "When opposing colors collide with opposite time orientations, the inverted piece always survives. With the same time orientation, a regular capture kills the target, but a pawn must use its capturing movement. A collision with a friendly piece also affects history rather than acting as a regular capture. A red skull warns that your selected piece will lose the collision."
            ),
        ),
        (
            crate::tr!("Шах и завершение партии", "Check and game endings"),
            crate::tr!(
                "Шах не ограничивает выбор хода: можно пытаться изменить историю. Победа за мат наступает при фактическом взятии короля на любой доске. Парадокс бесконечного пересчёта приводит к поражению вызвавшего его игрока. Пат без обычного шаха наступает, если есть обычные активные фигуры, но нет доступных ходов и ожиданий. Пат откладывается, пока у соперника есть незавершённые обычные ходы в прошлом. Если активных фигур нет, очередь передаётся сопернику.",
                "Check does not restrict your choice of move: you may try to change history. Checkmate is won by actually capturing the king on any board. An endless reconciliation paradox loses the game for the player who caused it. Without ordinary check, stalemate requires ordinary active pieces and no available moves or waits. Stalemate is deferred while the opponent has outstanding ordinary moves in the past. A side with no active pieces passes the turn."
            ),
        ),
        (
            crate::tr!("Поражение за свинство", "Loss for turn hogging"),
            crate::tr!(
                "Сторона, которая сделала десять ходов подряд, дисквалифицируется. После пятого хода в блоке хода появляется отсчёт. Платное ожидание и отказ от реакций считаются ходами. Бесплатное ожидание и продолжение просчёта сохраняют текущий счёт.",
                "A side that takes ten consecutive turns is disqualified for turn hogging. The turn panel shows a countdown after the fifth turn. Paid waiting and declining reactions count as turns. Free waiting and continuing calculation leave the count unchanged."
            ),
        ),
        (
            crate::tr!("Особые ходы", "Special moves"),
            crate::tr!(
                "Поддерживаются превращение пешки, обычное и инвертированное взятие на проходе, рокировка в обоих направлениях времени. Для рокировки король и ладья должны иметь одинаковую временную ориентацию и сохранять право на рокировку. Как и другие ходы, рокировка может отмениться при изменении истории.",
                "Pawn promotion, regular and inverted en passant, and castling in either time direction are supported. The king and rook must share the same time orientation and retain their castling rights. Like any other move, castling may be cancelled when history changes."
            ),
        ),
    ] {
        paragraph(ui, title, text);
    }
}

fn controls(ui: &mut egui::Ui) {
    for (key, text) in [
        (
            crate::tr!("ЛКМ / касание", "Left click / tap"),
            crate::tr!(
                "Выбрать активную свою фигуру, затем клетку хода. Клик по недоступной клетке снимает выбор, по другой активной своей фигуре — выбирает её.",
                "Select your active piece, then a destination. An unavailable square clears selection; another active friendly piece switches selection."
            ),
        ),
        (
            crate::tr!("СКМ", "Middle click"),
            crate::tr!(
                "Выбрать фигуру с обратным направлением времени. Повторный клик по выбранной фигуре или клик по недоступной фигуре переключает выбранное направление.",
                "Select a piece with the opposite time direction. Clicking it again, or clicking an unselectable piece, toggles the selected direction."
            ),
        ),
        (
            crate::tr!("ПКМ / Esc", "Right click / Esc"),
            crate::tr!(
                "Снять выбор; выйти из инспекции фигуры или мировой линии. Esc также останавливает просмотр анимаций.",
                "Clear selection; exit piece or worldline inspection. Esc also stops animation playback."
            ),
        ),
        (
            crate::tr!("Колесо / свайп", "Wheel / swipe"),
            crate::tr!(
                "Прокрутить ленту досок. Новые доски расположены выше старых.",
                "Scroll the board ribbon. Newer boards are above older ones."
            ),
        ),
        (
            crate::tr!("Shift / ПКМ + колесо", "Shift / right button + wheel"),
            crate::tr!(
                "Изменить масштаб от 15%. На смартфоне — свести или развести два пальца.",
                "Zoom down to 15%. On a phone, pinch with two fingers."
            ),
        ),
        (
            "Ctrl+Z / ⌘Z",
            crate::tr!(
                "Отменить ход с повторным применением оставшейся записи. В мультиплеере — отправить сопернику запрос на отмену.",
                "Undo by replaying the remaining journal. In multiplayer, request your opponent's consent."
            ),
        ),
        (
            "I",
            crate::tr!(
                "Включить инспекцию фигуры. Можно выбирать чужие и неактивные состояния.",
                "Toggle piece inspection. Opposing and inactive states can also be selected."
            ),
        ),
        (
            "C",
            crate::tr!(
                "Продолжить остановленный просчёт досок. Не тратит ход.",
                "Continue paused board calculation without spending a turn."
            ),
        ),
    ] {
        paragraph(ui, key, text);
    }
    ui.small(crate::tr!("На сенсорном экране направление меняется кнопкой со стрелкой в блоке выбранной фигуры. Подсказки кнопок появляются при неподвижном наведении.", "On touchscreens, use the direction arrow in the selected-piece panel. Button tooltips appear after hovering without moving."));
}

#[derive(Clone, Copy)]
enum Sample {
    Move(bool),
    Wait,
    Ordinary,
    Reaction(bool),
    Capture,
    Fatal,
    Check(bool),
    Inverted,
    Board,
    Preboard,
    Arrow(bool, bool),
    Inspect,
    Information,
}

fn legend(ui: &mut egui::Ui, art: &PieceArt) {
    for (sample, title, text) in [
        (
            Sample::Move(false),
            crate::tr!("Ход в будущее", "Move into the future"),
            crate::tr!(
                "Розовая точка на доступной клетке. При наведении отметка усиливается.",
                "A pink dot marks an available square. Hovering emphasizes it."
            ),
        ),
        (
            Sample::Move(true),
            crate::tr!("Ход в прошлое", "Move into the past"),
            crate::tr!(
                "Тёмно-карминовая точка. Цвет показывает направление выбранного хода, а не цвет фигуры.",
                "A dark carmine dot. Color indicates the selected move's time direction, not the piece's color."
            ),
        ),
        (
            Sample::Wait,
            crate::tr!("Куда можно ждать", "Waiting destination"),
            crate::tr!(
                "Кольцо с точкой на той же клетке другой доски; цвет соответствует направлению времени.",
                "A ring and dot on the same square of another board; its color follows the time direction."
            ),
        ),
        (
            Sample::Capture,
            crate::tr!("Доступное взятие", "Available capture"),
            crate::tr!(
                "Красное размытое свечение вокруг жертвы.",
                "A soft red glow around the victim."
            ),
        ),
        (
            Sample::Fatal,
            crate::tr!("Проигрыш столкновения", "Losing collision"),
            crate::tr!(
                "Красный череп сверху справа: противник переживёт столкновение, ваша фигура погибнет.",
                "A red skull at the upper right: the opponent survives the collision and your piece dies."
            ),
        ),
        (
            Sample::Ordinary,
            crate::tr!("Обычная активность", "Ordinary activity"),
            crate::tr!(
                "Зелёная клетка. Не показывается на самой новой доске и когда все фигуры стороны на доске активны.",
                "A green square. Hidden on the newest board and when all pieces of the side on that board are active."
            ),
        ),
        (
            Sample::Reaction(false),
            crate::tr!("Реакция на информацию", "Information reaction"),
            crate::tr!(
                "Жёлтый глаз над фигурой: новая цель в поле зрения даёт право на реакцию.",
                "A yellow eye above the piece: a new target in sight grants a reaction."
            ),
        ),
        (
            Sample::Reaction(true),
            crate::tr!("Реакция на отмену хода", "Cancelled-move reaction"),
            crate::tr!(
                "Оранжевая перечёркнутая стрелка над фигурой: её прежний ход отменился. Значки реакций скрыты на самой новой доске.",
                "An orange crossed-out arrow above the piece: its previous move was cancelled. Reaction badges are hidden on the newest board."
            ),
        ),
        (
            Sample::Check(false),
            crate::tr!("Шах", "Check"),
            crate::tr!(
                "Красный контур короля. Нажатие на короля в блоке очереди листает доски с шахом.",
                "A red king outline. Click the king in the turn panel to cycle through checked boards."
            ),
        ),
        (
            Sample::Check(true),
            crate::tr!("Угроза из будущего", "Threat from the future"),
            crate::tr!(
                "Розоватый контур короля: чисто инвертированная угроза. Атакующая фигура может прийти с будущей доски.",
                "A pink king outline: a purely inverted threat. The attacker can come from a future board."
            ),
        ),
        (
            Sample::Inverted,
            crate::tr!("Инвертированные фигуры", "Inverted pieces"),
            crate::tr!(
                "Светлая или тёмная фигура с аметистовым контуром и свечением: её текущая ориентация направлена в прошлое.",
                "A light or dark piece with an amethyst outline and glow: its current time orientation points into the past."
            ),
        ),
        (
            Sample::Board,
            crate::tr!("Активная доска", "Active board"),
            crate::tr!(
                "Утолщённая скруглённая рамка цвета стороны этой доски. Значок доски × число в блоке очереди показывает количество активных досок.",
                "A thick rounded frame in the board side's color. The board icon × count in the turn panel shows the number of active boards."
            ),
        ),
        (
            Sample::Preboard,
            crate::tr!("Предоска", "Preboard"),
            crate::tr!(
                "Полупрозрачные клетки и фигуры. История здесь ещё не согласована; на клетке могут накладываться несколько фигур.",
                "Translucent squares and pieces. History here is not yet reconciled; several pieces may overlap on a square."
            ),
        ),
        (
            Sample::Arrow(false, false),
            crate::tr!("Сыгранный ход в будущее", "Played forward move"),
            crate::tr!(
                "Розовая стрелка на исходной доске. Показываются сохранившиеся ходы живых на ней фигур.",
                "A pink arrow on the source board. Only retained moves of pieces alive on that board are shown."
            ),
        ),
        (
            Sample::Arrow(true, false),
            crate::tr!("Сыгранный ход в прошлое", "Played backward move"),
            crate::tr!(
                "Карминовая стрелка. На конечной доске хода отдельной стрелки нет.",
                "A carmine arrow. No separate arrow is placed on the destination board."
            ),
        ),
        (
            Sample::Arrow(false, true),
            crate::tr!("Предстоящая замена хода", "Pending move replacement"),
            crate::tr!(
                "Красная перечёркнутая стрелка с красным свечением: выбранный новый ход заменит этот старый ход.",
                "A crossed-out red arrow with a red glow: the selected new move will replace this old move."
            ),
        ),
        (
            Sample::Arrow(true, true),
            crate::tr!("Замена хода в прошлое", "Replacing a backward move"),
            crate::tr!(
                "Более тёмная красная стрелка с аметистовым свечением, как у инвертированных фигур.",
                "A darker red arrow with the same amethyst glow as inverted pieces."
            ),
        ),
        (
            Sample::Inspect,
            crate::tr!("Выбранное состояние", "Inspected state"),
            crate::tr!(
                "Синяя рамка клетки и увеличенная фигура в обоих режимах инспекции.",
                "A blue square frame and an enlarged piece in either inspection mode."
            ),
        ),
        (
            Sample::Information,
            crate::tr!("Видимая информация", "Visible information"),
            crate::tr!(
                "Глаз в блоке инспекции. Новые наблюдения отмечены жёлтым контуром иконки фигуры; рядом указана клетка.",
                "The eye in the inspection panel. Newly observed pieces have yellow icon outlines and square labels."
            ),
        ),
    ] {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(82.0, 62.0), Sense::hover());
            sample_paint(ui.painter(), rect, sample, art);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                ui.add(egui::Label::new(egui::RichText::new(title).strong()).wrap());
                ui.add(egui::Label::new(text).wrap());
            });
        });
        ui.add_space(4.0);
    }
}

fn sample_paint(painter: &egui::Painter, rect: Rect, sample: Sample, art: &PieceArt) {
    let cell = Rect::from_center_size(rect.center(), Vec2::splat(46.0));
    let piece = |side, inverted, outline| {
        art.paint_styled(
            painter,
            PieceKind::N,
            side,
            cell,
            1.0,
            PieceStyle {
                inverted,
                outline,
                ..Default::default()
            },
        );
    };
    painter.rect_filled(rect, 8.0, SURFACE);
    match sample {
        Sample::Arrow(past, replacing) => {
            let cell_size = 28.0;
            let board = Rect::from_min_size(
                rect.center() - Vec2::new(42.0, 42.0),
                Vec2::splat(8.0 * cell_size),
            );
            let from = Sq(0, 6);
            let to = Sq(2, 6);
            let start = board_view::square_rect(board, cell_size, from, false).center();
            let end = board_view::square_rect(board, cell_size, to, false).center();
            let color = match (past, replacing) {
                (true, true) => Color32::from_rgb(205, 30, 48),
                (false, true) => Color32::from_rgb(255, 35, 55),
                (true, false) => PAST,
                (false, false) => FUTURE,
            };
            board_view::paint_move_arrow(
                painter,
                board,
                cell_size,
                from,
                to,
                false,
                color,
                replacing.then(|| {
                    if past {
                        crate::piece_art::inverted_outline_color(Color::White)
                    } else {
                        Color32::from_rgb(255, 40, 60)
                    }
                }),
            );
            if replacing {
                let delta = Vec2::splat(cell_size * 0.25 * std::f32::consts::FRAC_1_SQRT_2);
                board_view::paint_round_line(
                    painter,
                    start.lerp(end, 0.5) - delta,
                    start.lerp(end, 0.5) + delta,
                    cell_size * 0.058,
                    color,
                    true,
                );
            }
        }
        Sample::Board => {
            for (x, side) in [(-19.0, Color::White), (19.0, Color::Black)] {
                let board =
                    Rect::from_center_size(rect.center() + Vec2::new(x, 0.0), Vec2::splat(22.0));
                let (outer, inner) = board_view::active_board_colors(side);
                board_view::paint_frame(painter, board, 11.0, outer, inner, 1.0, 1.0);
                squares(painter, board, 1.0);
            }
        }
        Sample::Preboard => {
            squares(painter, cell, 0.4);
            for offset in [-5.0, 5.0] {
                art.paint(
                    painter,
                    PieceKind::N,
                    Color::White,
                    cell.shrink(3.0).translate(Vec2::new(offset, -offset)),
                    0.4,
                );
            }
        }
        Sample::Inverted => {
            for (x, side) in [(-18.0, Color::White), (18.0, Color::Black)] {
                art.paint_styled(
                    painter,
                    PieceKind::B,
                    side,
                    Rect::from_center_size(rect.center() + Vec2::new(x, 0.0), Vec2::splat(38.0)),
                    1.0,
                    PieceStyle {
                        inverted: true,
                        ..Default::default()
                    },
                );
            }
        }
        Sample::Information => {
            icons::paint(
                painter,
                Icon::Information,
                Rect::from_center_size(cell.center() - Vec2::new(22.0, 0.0), Vec2::splat(20.0)),
                Stroke::new(1.8, TEXT),
            );
            art.paint_styled(
                painter,
                PieceKind::N,
                Color::Black,
                cell.translate(Vec2::new(12.0, 0.0)),
                1.0,
                PieceStyle {
                    outline: Outline::Information,
                    ..Default::default()
                },
            );
        }
        Sample::Check(inverted) => art.paint_styled(
            painter,
            PieceKind::K { moved: true },
            Color::White,
            cell,
            1.0,
            PieceStyle {
                outline: if inverted {
                    Outline::InvertedCheck
                } else {
                    Outline::Check
                },
                ..Default::default()
            },
        ),
        _ => {
            painter.rect_filled(cell, 0.0, LIGHT);
            match sample {
                Sample::Move(past) => {
                    let color = if past { PAST } else { FUTURE };
                    painter.rect_filled(cell, 0.0, color.gamma_multiply(0.08));
                    painter.circle_filled(
                        cell.center(),
                        cell.width() * 0.145,
                        color.gamma_multiply(0.78),
                    );
                }
                Sample::Wait => {
                    painter.circle_stroke(
                        cell.center(),
                        cell.width() * 0.20,
                        Stroke::new(2.5, FUTURE.gamma_multiply(0.85)),
                    );
                    painter.circle_filled(cell.center(), cell.width() * 0.065, FUTURE);
                }
                Sample::Ordinary => {
                    painter.rect_filled(cell, 0.0, ORDINARY.gamma_multiply(0.75));
                    piece(Color::White, false, Outline::None);
                }
                Sample::Reaction(cancelled) => {
                    piece(Color::White, false, Outline::None);
                    let badge = Rect::from_center_size(
                        cell.center_top() + Vec2::new(0.0, 2.0),
                        Vec2::splat(15.0),
                    );
                    painter.rect_filled(badge.expand(2.0), 4.5, BACKGROUND.gamma_multiply(0.95));
                    icons::paint(
                        painter,
                        if cancelled {
                            Icon::CancelledMove
                        } else {
                            Icon::Information
                        },
                        badge.shrink(1.0),
                        Stroke::new(2.0, if cancelled { REACTION } else { INFORMATION }),
                    );
                }
                Sample::Capture => piece(Color::Black, false, Outline::Capture),
                Sample::Fatal => {
                    piece(Color::Black, true, Outline::None);
                    board_view::paint_skull(painter, cell, 1.0, false);
                }
                Sample::Inspect => {
                    piece(Color::White, false, Outline::None);
                    painter.rect_stroke(
                        cell.expand(1.5),
                        3.0,
                        Stroke::new(3.0, Color32::from_rgb(92, 157, 255)),
                        StrokeKind::Inside,
                    );
                }
                _ => unreachable!(),
            }
        }
    }
}

fn squares(painter: &egui::Painter, rect: Rect, alpha: f32) {
    for x in 0..2 {
        for y in 0..2 {
            painter.rect_filled(
                Rect::from_min_size(
                    rect.min + Vec2::new(x as f32, y as f32) * rect.width() * 0.5,
                    Vec2::splat(rect.width() * 0.5),
                ),
                0.0,
                (if (x + y) % 2 == 0 { DARK } else { LIGHT }).gamma_multiply(alpha),
            );
        }
    }
}

fn tools(ui: &mut egui::Ui) {
    for (icon, text) in [
        (
            Icon::Up,
            crate::tr!(
                "Стрелки у линии времени циклически листают активные доски. ПКМ по стрелке — доски противника. В инспекции мировой линии: верхняя — следующее состояние по причинному порядку, нижняя — предыдущее. Shift + верхняя стрелка — самая новая доска или последнее состояние мировой линии; Shift + нижняя — самая старая доска или первое состояние. Для досок это края всей истории, включая неактивные доски, а для состояний — причинный порядок, независимо от их t.",
                "The timeline arrows cycle through active boards. Right-click an arrow for your opponent's boards. In worldline inspection, the upper arrow selects the next causal state and the lower arrow the previous state. Shift + upper arrow goes to the newest board or last worldline state; Shift + lower arrow goes to the oldest board or first state. Board jumps use the entire history, including inactive boards; state jumps follow causal order regardless of t."
            ),
        ),
        (
            Icon::Worldline,
            crate::tr!(
                "Инспекция мировой линии: выберите любую фигуру, остальные затемнятся. Кривая связывает её состояния со стрелками направления: зелёная — обычные живые, аметистовая — инвертированные, красная — мёртвые обычные, карминовая — мёртвые инвертированные. Пустая клетка сбрасывает выбор; ПКМ или повторное нажатие выключает режим.",
                "Worldline inspection: select any piece to dim the others. A curve links its states with direction arrows: green for living ordinary states, amethyst for inverted states, red for dead ordinary states and carmine for dead inverted states. An empty square clears selection; right-click or press the tool again to exit."
            ),
        ),
        (
            Icon::Inspect,
            crate::tr!(
                "Инспекция фигуры / I: показывает выбранное состояние, его видимые наблюдения. Часы со стрелкой назад показывают fut_seen, когда временная информация отличается от времени доски или есть петля.",
                "Piece inspection / I shows the selected state, its visible observations. The backward clock displays fut_seen when temporal information differs from board time or the state belongs to a loop."
            ),
        ),
        (
            Icon::Replay,
            crate::tr!(
                "Повтор последнего действия в блоке очереди: перемещение, ожидание, взятия и отмены ходов. Позиция и журнал не меняются.",
                "Replay the last action from the turn panel, including movement, waiting, captures and cancellations. The position and journal stay unchanged."
            ),
        ),
        (
            Icon::Continue,
            crate::tr!(
                "Продолжить просчёт / C: доступно при остановке из-за влияющей на историю отмены хода. Не тратит ход; может снова остановиться на другой такой отмене.",
                "Continue calculation / C is available when a cancellation affecting history pauses calculation. It costs no turn and may stop at another such cancellation."
            ),
        ),
        (
            Icon::Play,
            crate::tr!(
                "Проигрывание всей истории в правой панели: показывает доски от самой ранней до самой новой со звуками ходов. Повторное нажатие или Esc останавливает просмотр.",
                "Full-history playback in the sidebar shows boards from earliest to latest with move sounds. Press again or use Esc to stop."
            ),
        ),
        (
            Icon::Undo,
            crate::tr!(
                "Отмена последнего хода / Ctrl+Z: позиция восстанавливается из начального сеттинга по оставшемуся журналу. В сети требуется согласие обеих сторон.",
                "Undo / Ctrl+Z rebuilds the position from the starting setup and remaining journal. Online undo needs both players' consent."
            ),
        ),
        (
            Icon::Copy,
            crate::tr!(
                "Копирует полный журнал партии, включая настройки и стартовый сеттинг. Кнопка импорта рядом позволяет вставить такую запись и продолжить игру.",
                "Copies the complete game journal, including rules and starting setup. The adjacent import button loads that record so you can continue playing."
            ),
        ),
        (
            Icon::Settings,
            crate::tr!(
                "Шестерёнка: язык, поворот досок в локальной игре, скорость анимаций и отключение музыки ожидания. Выбор сохраняется в браузере. Помощь и настройки доступны и при свёрнутой правой панели.",
                "The gear opens language, local board flipping, animation speed and waiting-music settings. Choices are saved in the browser. Help and settings remain accessible with the sidebar collapsed."
            ),
        ),
    ] {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::hover());
            icons::paint(ui.painter(), icon, rect.shrink(6.0), Stroke::new(1.8, TEXT));
            ui.add(egui::Label::new(text).wrap());
        });
        ui.add_space(6.0);
    }
}

fn modes(ui: &mut egui::Ui, rules: GameRules) {
    paragraph(
        ui,
        crate::tr!("Новая партия", "New game"),
        crate::tr!(
            "В правой панели: «Локальный», «Против TempFish», «TempFish vs TempFish» или «Мультиплеер». В сетевой игре создайте комнату и передайте код сопернику либо введите его код. Бюджет TempFish задаётся числом узлов; два бота могут играть самостоятельно.",
            "In the sidebar, choose Local, Against TempFish, TempFish vs TempFish or Multiplayer. For online play, create a room and share its code, or join using your opponent's code. TempFish strength uses a node budget; two bots can play unattended."
        ),
    );
    paragraph(
        ui,
        crate::tr!("Стартовая позиция", "Starting position"),
        crate::tr!(
            "«Обычная» — стандартная расстановка. Time War — белые на доске 0, инвертированные чёрные на доске N; разнос задаётся нечётным числом. Обе позиции позволяют активными ходами создавать новые доски в обе стороны. Lab открывает редактор собственного сеттинга.",
            "Standard uses the usual setup. Time War starts White on board 0 and inverted Black on board N, with an odd-numbered separation. Both allow active moves to create new boards in either direction. Lab opens a custom setup editor."
        ),
    );
    ui.separator();
    ui.strong(crate::tr!("Настройки текущей партии", "Current game rules"));
    for (name, enabled, text) in [
        (
            crate::tr!("Инфоактивация с обучением", "Learn info-activation"),
            !rules.chain_info_activation,
            crate::tr!(
                "Включена: на каждое новое наблюдение реагирует только самое раннее увидевшее состояние каждой мировой линии, по её причинному порядку. Выключена по умолчанию: реагируют все подходящие состояния.",
                "On: only the first observing state of each worldline reacts to each new observation, in causal order. Off by default: all eligible states react."
            ),
        ),
        (
            crate::tr!("Реагировать многократно", "React multiple"),
            !rules.global_reaction_reset,
            crate::tr!(
                "Включено: после хода сохраняются реакции на досках позже исходной. Выключено по умолчанию: сбрасываются все реакции своей стороны. Новые цели после применения хода могут дать новые реакции.",
                "On: reactions on boards later than the source board remain after a move. Off by default: all your side's reactions are dismissed. Targets that appear after applying the command can trigger new reactions."
            ),
        ),
        (
            crate::tr!("Свободное ожидание", "Free wait"),
            rules.wait_before_move,
            crate::tr!(
                "Включено: отдельное ожидание не тратит ход, авто-ожидание заблокированных фигур выключено. Выключено по умолчанию: отдельное ожидание передаёт ход, авто-ожидание работает.",
                "On: standalone waiting is free, and automatic waits for blocked pieces are disabled. Off by default: standalone waiting spends a turn, and automatic waiting is enabled."
            ),
        ),
    ] {
        ui.strong(format!(
            "{} · {}",
            name,
            if enabled {
                crate::tr!("вкл", "on")
            } else {
                crate::tr!("выкл", "off")
            }
        ));
        ui.add(egui::Label::new(text).wrap());
        ui.add_space(6.0);
    }
    ui.small(crate::tr!(
        "Эти правила выбираются при создании партии; переключатели независимы друг от друга.",
        "Choose these rules when creating a game; the switches are independent."
    ));
}

fn lab(ui: &mut egui::Ui) {
    ui.label(crate::tr!(
        "Создавайте доски в прошлом и будущем, расставляйте обычные и инвертированные фигуры любого цвета. Каждая поставленная фигура начинает отдельную мировую линию. Новые крайние доски автоматически продолжают линии с прежней границы; отдельную линию можно продлить кнопками −t / +t. Режим «Ходить» использует игровые правила.",
        "Create past and future boards and place ordinary or inverted pieces of either color. Each placed piece starts its own worldline. New boundary boards automatically extend lines from the previous boundary; use −t / +t to extend a single line. The Move tool uses the game rules."));
    ui.label(crate::tr!(
        "«Убрать фигуру» удаляет всю мировую линию. «Убрать состояние» удаляет выбранное состояние и его причинное продолжение. Удаление положительной доски убирает её и все более новые, отрицательной — её и все более старые. Нулевую доску удалить нельзя. Король, ладья и пешка на своей обычной стартовой клетке считаются ещё не ходившими.",
        "Remove piece deletes a whole worldline. Remove state deletes that state and its causal continuation. Deleting a positive board also removes all newer boards; deleting a negative one also removes all older boards. Board zero cannot be deleted. Kings, rooks and pawns on their usual starting squares are treated as unmoved."));
    ui.label(crate::tr!(
        "Когда сеттинг готов, выберите локальную игру, бота, двух ботов или мультиплеер и начните партию. Экспорт сохраняет собранный сеттинг перед журналом игровых действий.",
        "When your setup is ready, choose local play, a bot, two bots or multiplayer and start the game. Export stores your custom setup before the gameplay journal."));
}
