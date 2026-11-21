//! The lesson sequence is data. Board coordinates refer to the actual commands
//! highlighted by the game; waits use the destination board selected in the UI.
use super::{Action, Anchor, Chapter, Control, Position, Scene, Step, Text, at, positions::*};
use temporal_chess_engine::model::{
    BoardTime,
    TimeDirection::{self, Future, Past},
};

use Action::{Button, Inspect, Next, Wait};
use Control::{Continue, Decline, Down, Replay, Up, Worldline};

const fn mv(t: BoardTime, from: &str, to: &str, direction: TimeDirection) -> Action {
    Action::mv(t, from, to, direction)
}

const fn mv_wait(
    t: BoardTime,
    from: &str,
    to: &str,
    direction: TimeDirection,
    until: BoardTime,
) -> Action {
    Action::mv_wait(t, from, to, direction, until)
}

macro_rules! step {
    ($action:expr; $ru:literal, $en:literal) => {
        Step::new(Text($ru, $en), $action)
    };
}

macro_rules! chapter {
    ($ru:literal, $en:literal; $($scene:expr),+ $(,)?) => {
        Chapter { title: Text($ru, $en), scenes: &[$($scene),+] }
    };
}

macro_rules! scene {
    ($setting:expr, $camera:expr; $($step:expr),+ $(,)?) => {
        Scene { position: $setting, camera: $camera, steps: &[$($step),+] }
    };
}

const PAWN_CHAIN: &[Action] = &[
    mv(0, "e2", "e4", Future),
    mv(1, "e7", "e5", Future),
    mv(2, "g1", "f3", Past),
    mv(3, "f8", "b4", Past),
];

const REVENGE: &[Action] = &[
    mv(0, "e2", "e4", Future),
    mv(1, "e7", "e5", Future),
    mv(2, "g1", "f3", Past),
    mv(3, "f8", "a3", Past),
    mv(4, "c2", "c3", Future),
    mv(1, "a3", "b2", Past),
];

const MIDDLE_HISTORY_RECAPTURE: &[Action] = &[
    mv(0, "h2", "h3", Future),
    mv(1, "h7", "h6", Future),
    mv(2, "h3", "h4", Future),
    mv(3, "h6", "h5", Future),
    mv(4, "e2", "e4", Future),
    mv(5, "e7", "e5", Future),
    mv(6, "g1", "f3", Past),
    mv(7, "f8", "a3", Past),
    mv(8, "c2", "c3", Future),
    mv(5, "a3", "b2", Past),
];

const OPEN_KING: &[Action] = &[
    mv(0, "d2", "d4", Future),
    mv(1, "e7", "e5", Future),
    mv(2, "g1", "f3", Future),
    mv(3, "f8", "b4", Future),
];

const PAST_DEFENDER: &[Action] = &[
    mv(0, "d2", "d4", Future),
    mv(1, "e7", "e5", Future),
    mv(2, "g1", "f3", Past),
    mv(3, "f8", "b4", Future),
];

const OPPOSING_TRAVELLER: &[Action] = &[
    mv(0, "g1", "h3", Future),
    mv(1, "g8", "f6", Future),
    mv(2, "a2", "a3", Future),
    mv(3, "f6", "h5", Future),
    mv(4, "b2", "b3", Future),
    mv(5, "h5", "f4", Past),
];

const FRIENDLY_COLLISION: &str = "NOLAN-CHESS 6\nchain_info_activation on\nreact_multiple off\nfree_wait off\nstart standard\nmove 0 12:0:0 e2 e3 + normal\nmove 1 25:1:0 b7 b6 + normal\n";

pub(super) static CHAPTERS: &[Chapter] = &[
    chapter!("Ходы I · Ходы и время", "Moves I · Moves and time";
        scene!(Position::Standard, 0;
            step!(mv(0, "e2", "e4", Future);
                "Белые ходят с чётных досок на нечётные, чёрные — с нечётных на чётные. Выберите белую пешку e2 на доске 0 и нажмите e4 на этой же доске.",
                "White moves from even boards to odd boards; Black moves from odd boards to even boards. Select the white pawn on e2 at board 0, then click e4 on that same board."),
            step!(Next;
                "Король в блоке хода показывает, чья очередь. Сейчас ходят чёрные: белая пешка перешла на доску 1.",
                "The king icon in the turn panel shows whose turn it is. It is now Black’s turn: the white pawn arrived on board 1.").focus(Anchor::Turn),
            step!(mv(1, "e7", "e5", Future);
                "Сделайте ход за чёрных: e7 → e5 на доске 1. После хода откроется новая доска 2.",
                "Play Black’s e7 → e5 on board 1. This move opens a new board, board 2."),
            step!(Next;
                "Розовая стрелка остаётся на доске, с которой фигура сходила в будущее. Новая верхняя доска появляется только от хода с текущей последней доски стандартной партии.",
                "The pink arrow stays on the board the piece moved from. In a standard game, only a move from the latest board adds a new board at the top.").focus(Anchor::Board(1)).camera(1),
        ),
    ),
    chapter!("Ходы II · Инверсия", "Moves II · Inversion";
        scene!(Position::Standard, 0;
            step!(mv(0, "g1", "f3", Past);
                "Выберите коня g1. Нажмите стрелку направления рядом с выбранной фигурой или СКМ. Сходите на f3: конь инвертируется и прибудет на доску −1.",
                "Select the knight on g1. Click the direction arrow beside the selection or middle-click. Move to f3: the knight inverts and arrives on board −1."),
            step!(Next;
                "Карминовая стрелка на доске 0 отмечает ход в прошлое. При ходе за нижнюю границу появляются отрицательные доски, в этом случае −1 и −2.",
                "The carmine arrow on board 0 marks a move into the past. Moving beyond the lower boundary creates negative boards, here −1 and −2.").focus(Anchor::Board(0)).camera(0),
            step!(Button(Replay);
                "Нажмите кнопку повтора последнего хода. Проследите, как конь уходит в прошлое.",
                "Click Replay previous move and watch the knight travel into the past."),
            step!(mv(1, "b8", "c6", Future);
                "Теперь сходите за чёрных с b8 на c6 в будущее на доске 1.",
                "Now play Black’s b8 → c6 into the future on board 1."),
            step!(Next;
                "Рамки отмечают доски с активными белыми фигурами. Счётчик показывает их число. Белые могут ходить и обычной армией на доске 2, и путешественником на −2.",
                "Frames mark boards with active white pieces. The counter shows how many there are. White can move the army on board 2 or the traveller on −2.").focus(Anchor::Activity).also_focus(Anchor::Board(2)).camera(2),
            step!(Button(Down);
                "Нажмите нижнюю стрелку у ленты, чтобы перейти к другой активной доске.",
                "Click the lower arrow beside the timeline to cycle to the other active board."),
            step!(Button(Up);
                "Нажмите верхнюю стрелку, чтобы пролистать активные доски в обратном порядке.",
                "Click the upper arrow to cycle active boards in the opposite order."),
            step!(Button(Worldline);
                "Включите инспекцию мировых линий кнопкой с пешкой и стрелкой.",
                "Turn on worldline inspection with the pawn-and-arrow button."),
            step!(Inspect(at(-2, "f3"));
                "Нажмите на инвертированного коня f3 на доске −2. Кривая соединит его состояния в порядке, в котором он их прожил.",
                "Click the inverted knight on f3 at board −2. The curve connects his states in the order he experienced them.").camera(-2),
            step!(Button(Up);
                "При выбранной мировой линии верхняя стрелка листает к следующему состоянию фигуры.",
                "With a worldline selected, the upper arrow goes to the next state on that line."),
            step!(Button(Down);
                "Нижняя возвращает к предыдущему состоянию. Вернитесь к инвертированному коню.",
                "The lower arrow goes to the previous state. Return to the inverted knight."),
            step!(Next;
                "Часы в инспекции показывают, что фигура является путешественником во времени и может ходить независимо от других фигур, пока не перейдёт за время, с которого она инвертировалась. Это время показано числом рядом с часами. Если конь перейдёт за него, он вернётся к общему порядку ходов.",
                "The clock in the inspector marks a time traveller. The piece can move independently until it goes beyond the time at which it inverted, shown next to the clock. Once the knight crosses that point, it rejoins ordinary turn order.").focus(Anchor::Inspection),
            step!(Button(Worldline);
                "Закройте инспекцию той же кнопкой.",
                "Close worldline inspection with the same button."),
            step!(mv(-2, "f3", "h4", Past);
                "Продолжите маршрут путешественника: с f3 на h4 на доске −2. Направление уже выбрано в прошлое.",
                "Continue the traveller’s route: play f3 → h4 on board −2. The direction already points into the past."),
            step!(mv(2, "e2", "e4", Future);
                "Сыграйте белыми e2 → e4 с доски 2. Пока конь в прошлом, белые могут играть и на последней доске.",
                "Play White’s e2 → e4 from board 2. While the knight is in the past, White can also play on the latest board."),
            step!(mv(3, "d7", "d5", Future);
                "Сыграйте чёрными d7 → d5 с доски 3.",
                "Play Black’s d7 → d5 from board 3."),
            step!(mv(4, "a2", "a3", Future);
                "Займите ещё один обычный ход белых: a2 → a3 с доски 4.",
                "Use another ordinary White turn: a2 → a3 from board 4."),
            step!(mv(5, "a7", "a6", Future);
                "Ответьте чёрными a7 → a6 с доски 5.",
                "Reply with Black’s a7 → a6 from board 5."),
            step!(mv(-4, "h4", "f5", Future);
                "Теперь разверните инвертированного коня в будущее. Выберите h4 на доске −4, смените направление и походите на f5.",
                "Now turn the inverted knight toward the future. Select h4 on board −4, switch direction, and move to f5."),
            step!(mv(-2, "f5", "h4", Future);
                "Продвиньтесь к точке инверсии: f5 → h4 с доски −2 в будущее.",
                "Head toward the inversion point: play f5 → h4 into the future from board −2."),
            step!(mv(0, "h4", "f3", Future);
                "Перейдите за точку инверсии: сыграйте h4 → f3 в будущее на доске 0. Выберите клетку f3 прямо на доске 0.",
                "Cross the inversion point: play h4 → f3 into the future on board 0. Click f3 on board 0 itself."),
            step!(Next;
                "Конь вернулся к общему порядку ходов. На досках 2 и 4 белые уже сходили, поэтому конь автоматически прождал на f3 до доски 6.",
                "The knight has rejoined ordinary turn order. White already moved from boards 2 and 4, so it automatically waited on f3 until board 6.").focus(Anchor::Square(6, at(6, "f3").sq)).camera(6),
        ),
    ),
    chapter!("Ходы III · Ожидание", "Moves III · Waiting";
        scene!(Position::Scenario(waiting), 0;
            step!(mv_wait(0, "a1", "a3", Future, 6);
                "Выберите ладью a1 на доске 0, затем прокрутите до доски 6 и нажмите a3 там. Ладья съест коня при прибытии на доску 1 и прождёт до 6 в рамках одного хода.",
                "Select the rook on a1 at board 0, scroll to board 6 and click a3 there. The rook takes the knight on arrival at board 1 and waits until 6 as part of the same turn."),
            step!(Next;
                "Взятый конь исчез из последующих досок, поэтому он не мешал ожиданию.",
                "The captured knight disappeared from subsequent boards, so it did not block the wait.").focus(Anchor::Board(6)).camera(6),
        ),
        scene!(Position::Scenario(manual_wait), 0;
            step!(Wait { from: at(0, "a1"), until: 2 };
                "Чтобы ждать без пространственного хода, выберите ладью a1 на доске 0 и нажмите a1 на доске 2. Конь на доске 3 блокирует ожидание до 4 и 6. Вручную можно ждать только по своему направлению, до доски своего цвета и в пределах существующей истории.",
                "To wait without moving spatially, select the rook on a1 at board 0, then click a1 on board 2. The knight on board 3 blocks waiting to 4 and 6. Manual waits follow the piece’s orientation, end on its color, and stay within existing history."),
            step!(Next;
                "В обычном режиме такое ожидание передаёт ход. В режиме «Свободное ожидание» оно не тратит ход.",
                "In the standard rules, this wait passes the turn. With Free wait enabled, it does not cost a turn.").focus(Anchor::Turn),
        ),
    ),
    chapter!("Ходы IV · Авто-ожидание", "Moves IV · Automatic waiting";
        scene!(Position::Scenario(auto_wait), 1;
            step!(mv(1, "h8", "f7", Future);
                "Белая пешка на a8 заперта и не может походить. Сделайте чёрными ход конём h8 → f7 с доски 1. При согласовании пешка автоматически продвинется по существующим доскам.",
                "The white pawn on a8 has no move. Play Black’s h8 → f7 from board 1. During reconciliation, the pawn automatically advances along existing boards."),
            step!(Next;
                "Авто-ожидание остановится на доступном ходе или у границы истории. Оно не создаёт досок, зато может вызвать столкновение: здесь пешка встретилась с инвертированным конём на a8.",
                "Automatic waiting stops at an available move or at the timeline boundary. It cannot create boards, but it can cause a collision: here the pawn met the inverted knight on a8.").focus(Anchor::Board(3)).camera(3),
        ),
    ),
    chapter!("Реакция I · Зрение и инфо-активация", "Reaction I · Sight and information";
        scene!(Position::Scenario(sight), 3;
            step!(Button(Control::Inspect);
                "Фигура видит доступные ей вражеские цели на своей доске. Откройте инспекцию фигуры кнопкой информации в блоке хода.",
                "A piece sees enemy targets it could capture on its current board. Open piece inspection with the information button in the turn panel."),
            step!(Inspect(at(2, "a1"));
                "Нажмите на белую ладью a1 на доске 2. В её поле зрения пока никого нет.",
                "Click the white rook on a1 at board 2. It has no enemy in sight yet.").camera(2),
            step!(Button(Control::Inspect);
                "Нажмите кнопку информации снова, чтобы выйти из инспекции.",
                "Click the information button again to leave inspection."),
            step!(mv(3, "c5", "a4", Past);
                "Инвертируйте чёрного коня c5 → a4 с доски 3. Он появится в поле зрения ладьи на доске 2.",
                "Invert Black’s knight c5 → a4 from board 3. It enters the rook’s sight on board 2."),
            step!(Button(Control::Inspect);
                "Значок глаза отмечает новую информацию и право на реакцию. Снова откройте инспекцию.",
                "The eye marks new information and the right to react. Open inspection again."),
            step!(Inspect(at(2, "a1"));
                "Нажмите на ладью a1 на доске 2.",
                "Click the rook on a1 at board 2.").camera(2),
            step!(Next;
                "Жёлтый контур коня в разделе с глазом отмечает новую цель на текущей доске. Если конь исчезнет из поля зрения и появится снова, ладья сможет снова отреагировать.",
                "The yellow outline on the knight marks a new target on the current board. If it leaves sight and reappears, the rook can react again.").focus(Anchor::Inspection),
            step!(Button(Control::Inspect);
                "Закройте инспекцию и приготовьтесь реагировать.",
                "Close inspection to make a reaction move."),
            step!(mv(2, "a1", "a4", Past);
                "Ладья может отреагировать даже на старой доске. Её ход a1 → a4 в прошлое с доски 2 снимет угрозу.",
                "The rook can react on an older board. Play a1 → a4 into the past from board 2 to remove the threat."),
            step!(Next;
                "Ладья ответила ходом в прошлое и взяла путешественника.",
                "The rook used a move into the past to take the traveller."),
        ),
        scene!(Position::Scenario(replace_move), 0;
            step!(mv(0, "a1", "b1", Future);
                "На доске 0 уже есть ход белого коня. У ладьи появилась инфо-реакция в том же временном порядке. Выберите a1 и обратите внимание на перечёркнутую стрелку, затем сыграйте a1 → b1 в будущее.",
                "Board 0 already contains White’s knight move. The rook gained an information reaction in the same temporal order. Select a1, notice the crossed-out arrow, then play a1 → b1 into the future."),
            step!(Next;
                "Ладья заняла слот коня. Его прежний ход исчез с доски, и история перестроилась с учётом хода ладьи.",
                "The rook took over the knight’s move slot. Its old arrow disappeared and history was rebuilt around the rook’s new move.").focus(Anchor::Board(0)).camera(0),
        ),
        scene!(Position::Opening(REVENGE), 0;
            step!(mv(0, "c1", "b2", Past);
                "Инвертированный чёрный слон съел белую пешку b2. Отомстите: инвертируйте белого слона с c1 на b2 с доски 0.",
                "The inverted black bishop captured White’s b2 pawn. Retaliate by inverting the white bishop from c1 to b2 on board 0."),
            step!(Next;
                "Ответное взятие произошло в прошлом. Стрелки и анимации отмен показывают, какие ходы пришлось переписать.",
                "The recapture took place in the past. Move arrows and cancellation animations show which moves had to be rewritten."),
        ),
    ),
    chapter!("Реакция II · Отмена хода", "Reaction II · Move cancellation";
        scene!(Position::Opening(PAWN_CHAIN), 0;
            step!(mv(0, "f3", "e5", Future);
                "Верните белого коня в будущее ходом f3 → e5 с доски 0. Он окажется на пути чёрной пешки.",
                "Turn the white knight forward with f3 → e5 from board 0. It will appear on the black pawn’s route."),
            step!(Next;
                "Фигура, которая уже сделала ход с этой доски, не становится инфо-активной от новой цели. Пешка e7 активировалась из-за отмены своего хода. Её значок — перечёркнутая стрелка.",
                "A piece that already moved from a board does not gain an information reaction there. The e7 pawn became active because its move was cancelled. The crossed-out arrow marks this reaction.").focus(Anchor::Square(1, at(1, "e7").sq)).camera(1),
        ),
    ),
    chapter!("Реакция III · Виды активаций", "Reaction III · Types of activation";
        scene!(Position::Scenario(reactions), 6;
            step!(Next;
                "Зелёная клетка отмечает обычную активацию. Глаз означает новую информацию, перечёркнутая стрелка — отмену хода. Обычное право связано с концом мировой линии на доске своего цвета.",
                "A green square marks ordinary activation. An eye means new information; a crossed arrow means a cancelled move. Ordinary rights belong to the end of the worldline on its own color.").focus(Anchor::Board(6)),
            step!(Next;
                "На доске 0 глаз над ладьёй, которая увидела коня. На доске 2 над белым конём перечёркнутая стрелка.",
                "On board 0 the rook has an eye because it saw the knight. On board 2 the white knight has a crossed-out arrow.").focus(Anchor::Board(0)).camera(0),
            step!(mv(2, "h1", "f2", Future);
                "Сделайте реакционный ход конём h1 → f2 с доски 2. Посмотрите, какие белые активации останутся.",
                "React with the knight h1 → f2 from board 2. See which White activations remain."),
            step!(Next;
                "По умолчанию ход сбрасывает реакции этой стороны на всех досках. То, что фигура уже видела, не активирует её повторно, пока остаётся в поле зрения. Обычная активация не расходуется этим сбросом.",
                "By default a turn clears that side’s reactions across all boards. Already visible targets do not reactivate a piece while they remain in sight. Ordinary activation survives this reset."),
        ),
    ),
    chapter!("Реакция IV · Не реагировать", "Reaction IV · Decline reactions";
        scene!(Position::Scenario(decline), 0;
            step!(Button(Decline);
                "У белых есть только реакции. Нажмите «Не реагировать» в блоке хода, чтобы принять изменившуюся историю и передать очередь.",
                "White has only reactive moves. Click Decline reactions in the turn panel to accept the rewritten history and pass the turn."),
            step!(Next;
                "Отклонённая реакция погасла. Очередь у чёрных; при наличии обычной активации отказ от реакции был бы недоступен.",
                "The declined reaction is now cleared. It is Black’s turn. Declining would be unavailable if White still had ordinary activation.").focus(Anchor::Turn),
        ),
    ),
    chapter!("История I · Смерть и воскрешение", "History I · Death and resurrection";
        scene!(Position::Scenario(revival), 0;
            step!(mv(0, "a1", "a6", Future);
                "Съешьте ладьёй a1 коня a6 с доски 0. Взятие на доске 1 уберёт все будущие состояния коня.",
                "Capture the a6 knight with the rook from a1 at board 0. A capture on board 1 removes all of the knight’s later states."),
            step!(Next;
                "На доске 6 больше нет коня. Чтобы вернуть его, нужно убрать причину смерти.",
                "The knight is gone on board 6. To bring it back, remove the cause of its death.").focus(Anchor::Board(6)).camera(6),
            step!(mv(1, "c2", "a1", Past);
                "Отмените причину взятия ходом чёрного инвертированного коня c2 → a1 с доски 1.",
                "Undo the cause of the capture by playing the inverted black knight c2 → a1 from board 1."),
            step!(Next;
                "Когда причина смерти исчезает, движок возвращает состояния жертвы и восстанавливает её ходы, если они всё ещё допустимы в изменённой истории.",
                "When the cause of death disappears, the engine restores the victim’s states and brings back its moves if they are still valid in the changed history.").focus(Anchor::Board(6)).camera(6),
        ),
    ),
    chapter!("История II · Повороты", "History II · Temporal turns";
        scene!(Position::Opening(PAWN_CHAIN), 0;
            step!(mv(0, "f3", "e5", Future);
                "Разверните белого коня с f3 на e5 в будущее. Этим вы отмените ход чёрной пешки.",
                "Turn the white knight forward from f3 to e5. This cancels the black pawn’s move."),
            step!(Button(Continue);
                "Продолжите просчёт. Чёрный слон, начавший инверсию с f8, потеряет маршрут: на e7 теперь стоит пешка.",
                "Continue calculation. The black bishop that started its inversion from f8 now has its route blocked by the pawn on e7."),
            step!(Next;
                "Отмена начала инверсии убрала состояния слона на b4. Если отменить обратный поворот, исчезает его новая ветвь в будущее, а фигура остаётся на инвертированном конце линии.",
                "Cancelling the start of inversion removes the bishop’s states on b4. Cancelling the turn back removes the new future route and leaves the piece at its inverted endpoint.").focus(Anchor::Board(2)).camera(2),
        ),
        scene!(Position::Scenario(cancel_return), 2;
            step!(mv(2, "g4", "g5", Future);
                "Разверните инвертированную пешку g4 → g5 в будущее с доски 2.",
                "Turn the inverted pawn g4 → g5 toward the future from board 2."),
            step!(mv(1, "f6", "g5", Future);
                "Чёрными сыграйте слоном f6 → g5 с доски 1. Слон окажется на маршруте пешки раньше неё.",
                "Play Black’s bishop f6 → g5 from board 1. The bishop will reach the pawn’s route before it does."),
            step!(Next;
                "Пешка не может съесть слона ходом прямо. Отмена поворота убрала её новые состояния на g5 и оставила пешку инвертированной на g4.",
                "The pawn cannot take a bishop by moving straight. Cancelling the turn removes the new states on g5 and leaves the pawn inverted on g4.").focus(Anchor::Board(2)).camera(2),
        ),
        scene!(Position::Scenario(reconciliation), 0;
            step!(mv(0, "a1", "a3", Past);
                "Ладья увидела инвертированного коня на доске 0. Отреагируйте ходом a1 → a3 в прошлое. До реакции у ладьи был ход в будущее с доски 2.",
                "The rook saw an inverted knight on board 0. React with a1 → a3 into the past. Before reacting, the rook had a later move from board 2."),
            step!(Next;
                "Создав путешественника из старого состояния, вы удалили прежнее будущее ладьи, включая её поздний ход.",
                "Turning an earlier state into a traveller removed the rook’s old future, including its later move.").focus(Anchor::Board(2)).camera(2),
        ),
    ),
    chapter!("История III · Согласование позиции", "History III · Reconciliation";
        scene!(Position::Scenario(reconciliation), 0;
            step!(mv(0, "a1", "a3", Future);
                "Отреагируйте ладьёй a1 → a3 в будущее с доски 0. У ладьи уже есть ход на a4 с доски 2.",
                "React with the rook a1 → a3 into the future from board 0. It already has a move to a4 from board 2."),
            step!(Next;
                "Ладья перенесла свою новую позицию вперёд. Стрелка её следующего хода начинается с новой клетки, если цель по-прежнему достижима.",
                "The rook’s new position propagates forward. Its later arrow starts on the new square if the destination is still reachable.").focus(Anchor::Board(2)).camera(2),
        ),
    ),
    chapter!("История IV · Предоски", "History IV · Preboards";
        scene!(Position::Opening(PAWN_CHAIN), 0;
            step!(mv(0, "f3", "e5", Future);
                "Разверните коня в будущее ходом f3 → e5 с доски 0. Отмена хода пешки задержит согласование на доске 1.",
                "Turn the knight toward the future with f3 → e5 from board 0. Cancelling the pawn’s move will pause reconciliation on board 1."),
            step!(Next;
                "За точкой остановки видны полупрозрачные предоски. Движок ещё не решил, какие состояния и ходы там сохранятся.",
                "Translucent preboards follow the pause. The engine has yet to resolve which states and moves remain there.").focus(Anchor::Board(2)).camera(2),
            step!(mv(1, "e7", "e6", Future);
                "Замените отменённый ход пешки ходом e7 → e6 с доски 1. Освободив e7, вы сохраните путь слону.",
                "Replace the cancelled pawn move with e7 → e6 from board 1. Clearing e7 preserves the bishop’s route."),
            step!(Next;
                "Слон сохранил ход f8 → b4, потому что пешка больше не перекрывает диагональ. Если просто продолжить просчёт, этот ход отменится.",
                "The bishop keeps f8 → b4 because the pawn no longer blocks the diagonal. If you simply continue calculation, the bishop’s move is cancelled.").focus(Anchor::Board(3)).camera(3),
        ),
    ),
    chapter!("Тактика I · Съедение", "Tactics I · Capture";
        scene!(Position::Scenario(capture_inverted), 2;
            step!(mv(2, "a1", "a4", Future);
                "Проверьте красный череп: ход обычной ладьи a1 → a4 в будущее с доски 2 ведёт к столкновению с инвертированным конём. Сделайте этот ход и посмотрите, кто выживет.",
                "Look for the red skull. The ordinary rook’s a1 → a4 into the future from board 2 meets an inverted knight. Play it and see who survives."),
            step!(Next;
                "При встречном столкновении разных цветов выживает инвертированная фигура.",
                "When opposing pieces meet in opposite time orientations, the inverted piece survives."),
        ),
        scene!(Position::Scenario(capture_inverted), 2;
            step!(mv(2, "a1", "a4", Past);
                "Попробуйте то же взятие в прошлое. Теперь ладья тоже инвертируется и сможет съесть коня.",
                "Try the same capture into the past. This time the rook inverts too and can take the knight."),
            step!(Next;
                "Инвертированную фигуру можно взять ходом в прошлое. Обычная фигура уязвима для взятий в обе стороны.",
                "An inverted piece can be taken with a move into the past. An ordinary piece is vulnerable in both directions."),
        ),
        scene!(Position::Scenario(capture_ordinary), 2;
            step!(mv(2, "a1", "a4", Past);
                "Возьмите обычного коня отправленной в прошлое ладьёй a1 → a4 с доски 2.",
                "Take the ordinary knight by sending the rook a1 → a4 into the past from board 2."),
            step!(Next;
                "Обычный конь проиграл встречное столкновение.",
                "The ordinary knight lost the oncoming collision."),
        ),
    ),
    chapter!("Тактика II · Фигуры в прошлом", "Tactics II · Keeping pieces in the past";
        scene!(Position::Opening(OPEN_KING), 4;
            step!(Button(Control::Check);
                "После d4, …e5, Кf3 и …Сb4 белый король под шахом по диагонали b4–e1. Нажмите на короля в блоке хода, чтобы перейти к шаху.",
                "After d4, …e5, Nf3 and …Bb4, White’s king is in check along the b4–e1 diagonal. Click the king in the turn panel to jump to the check."),
            step!(mv(4, "c2", "c3", Future);
                "Попробуйте закрыться как в обычных шахматах. Сыграйте c2 → c3 в будущее с доски 4.",
                "Try blocking the check as in ordinary chess. Play c2 → c3 into the future from board 4."),
            step!(mv(5, "b4", "e1", Past);
                "На доске 5 пешка закрыла диагональ, но на доске 4 король всё ещё открыт. Чёрные отвечают ходом слона b4 → e1 в прошлое.",
                "The pawn blocks the diagonal on board 5, but the king is still exposed on board 4. Black replies by moving the bishop b4 → e1 into the past.").automatic(),
            step!(Next;
                "Чёрные взяли короля на доске 4 и выиграли. Заслон, поставленный после шаха, защитил только более позднее состояние короля.",
                "Black captured the king on board 4 and won. The block played after the check only protected the king’s later state.").focus(Anchor::Board(4)).camera(4),
        ),
        scene!(Position::Opening(PAST_DEFENDER), 4;
            step!(Next;
                "Тот же дебют, но белые заранее отправили коня g1 в прошлое. Теперь он стоит на f3 на доске 0 и может вмешаться до шаха.",
                "The same opening, but White sent the g1 knight into the past. It is now on f3 at board 0 and can intervene before the check.").focus(Anchor::Board(4)).camera(4),
            step!(mv_wait(0, "f3", "d2", Future, 4);
                "Выберите коня f3 на доске 0, смените направление на будущее и нажмите d2 на доске 4. Конь встанет на диагональ до появления слона и прождёт там до шаха.",
                "Select the knight on f3 at board 0, switch to the future and click d2 on board 4. The knight will occupy the diagonal before the bishop arrives and wait there."),
            step!(Next;
                "Конь уже стоит на d2 на доске 4, поэтому слон не может взять короля ходом в прошлое. Полезно держать доступные фигуры в прошлом: иногда только они могут вмешаться до шаха.",
                "The knight is already on d2 at board 4, so the bishop cannot capture the king with a move into the past. Keep playable pieces in the past: sometimes only they can intervene before the check.").focus(Anchor::Board(4)).camera(4),
        ),
    ),
    chapter!("Тактика III · Поражение за свинство", "Tactics III · Turn hogging";
        scene!(Position::Opening(OPPOSING_TRAVELLER), 4;
            step!(mv(4, "h3", "f4", Past);
                "У чёрных остался один доступный путешественник — конь на f4. Инвертируйте белого коня h3 → f4 с доски 4 и возьмите чёрного коня на доске 3.",
                "Black has one playable traveller left, the knight on f4. Invert White’s knight h3 → f4 from board 4 to capture it on board 3."),
            step!(mv(2, "f4", "d5", Past);
                "У чёрных больше нет доступных фигур, и очередь вернулась белым. Продолжите конём f4 → d5 в прошлое с доски 2.",
                "Black has no playable pieces left, so the turn returned to White. Continue with f4 → d5 into the past from board 2."),
            step!(mv(0, "d5", "f4", Past);
                "Сыграйте d5 → f4 в прошлое с доски 0.",
                "Play d5 → f4 into the past from board 0."),
            step!(mv(-2, "f4", "d5", Past);
                "Сыграйте f4 → d5 в прошлое с доски −2.",
                "Play f4 → d5 into the past from board −2."),
            step!(mv(-4, "d5", "f4", Past);
                "Сделайте пятый ход подряд: d5 → f4 в прошлое с доски −4.",
                "Take your fifth consecutive turn: d5 → f4 into the past from board −4."),
            step!(Next;
                "Вы сделали пять ходов подряд. Будущее осталось на доске 6, поэтому чёрные так и не получили активную доску. В блоке хода появился отсчёт: десятый ход подряд приведёт к дисквалификации за свинство.",
                "You took five turns in a row. The future stayed at board 6, leaving Black without an active board. The countdown is now visible: ten consecutive turns lead to disqualification for turn hogging.").focus(Anchor::Streak).also_focus(Anchor::Board(6)).camera(6),
        ),
    ),
    chapter!("Тактика IV · Пассивные столкновения", "Tactics IV · Passive collisions";
        scene!(Position::Scenario(passive), 5;
            step!(mv(5, "b3", "a1", Past); "Отправьте чёрного коня b3 → a1 с доски 5. На доске 4 клетка a1 пуста.", "Send the black knight b3 → a1 from board 5. Square a1 is empty on board 4."),
            step!(Next;
                "При пассивном продвижении на доску 3 конь встретил белую ладью и убил её.",
                "When the knight continued passively to board 3, it met the white rook and killed it.").focus(Anchor::Board(3)).camera(3),
        ),
        scene!(Position::Record(FRIENDLY_COLLISION), 2;
            step!(mv(2, "e1", "e2", Past); "Инвертируйте белого короля e1 → e2 с доски 2. На доске 1 пешка уже покинула e2.", "Invert the white king e1 → e2 from board 2. The pawn has already left e2 on board 1."),
            step!(Next;
                "На доске 0 король пассивно встретил свою пешку и убил её. Инвертированная фигура может убить обычную фигуру своего цвета на доске своего цвета.",
                "On board 0 the king passively ran into and killed its own pawn. An inverted piece can kill an ordinary ally on a board of their own color.").focus(Anchor::Board(0)).camera(0),
        ),
        scene!(Position::Scenario(paradox), 6;
            step!(mv(6, "c3", "c1", Past);
                "Ладья пришла с a1 на c3 двумя ходами в будущее. Теперь разверните её: c3 → c1 в прошлое с доски 6.",
                "The rook reached c3 from a1 with two moves into the future. Now invert it: play c3 → c1 into the past from board 6."),
            step!(mv(4, "c1", "a1", Past); "Сыграйте c1 → a1 с доски 4. Теперь ладья столкнётся со своим прошлым.", "Play c1 → a1 from board 4. The rook will run into its own past."),
            step!(Next;
                "Ладья уничтожила причину собственного хода. Если история зацикливается при согласовании, движок засчитывает поражение тому, кто вызвал парадокс.",
                "The rook destroyed the cause of its own move. If reconciliation loops indefinitely, the player who caused the paradox loses."),
        ),
        scene!(Position::Opening(MIDDLE_HISTORY_RECAPTURE), 2;
            step!(Next;
                "Посмотрите на белую пешку b2 на доске 2. Здесь она ещё жива. На доске 3 чёрный инвертированный слон уже занял её клетку.",
                "Look at White’s b2 pawn on board 2. It is still alive there. On board 3, the inverted black bishop has already taken its square.").focus(Anchor::Square(2, at(2, "b2").sq)),
            step!(mv(4, "c1", "b2", Past);
                "Перевозьмите чёрного слона: сыграйте c1 → b2 в прошлое с доски 4. Проследите, что случится с пешкой на доске 2.",
                "Recapture the black bishop with c1 → b2 into the past from board 4. Watch what happens to the pawn on board 2."),
            step!(Next;
                "Белый слон взял чёрного на доске 3 и пассивно дошёл до доски 2. Там он столкнулся со своей ещё живой пешкой и убил её. Пешка погибла раньше, чем при взятии чёрным слоном.",
                "The white bishop took the black bishop on board 3 and continued passively to board 2. There it ran into and killed its own pawn while it was still alive. The pawn died a board earlier than in Black’s capture.").focus(Anchor::Square(2, at(2, "b2").sq)).camera(2),
        ),
    ),
];
