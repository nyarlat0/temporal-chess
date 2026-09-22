# Nolan Chess

Экспериментальный движок шахмат с инверсией фигур в духе «Довода».
История хранится как мировые линии фигур; доска — их срез по времени.
Графического интерфейса пока нет.

Направление времени выбирается для каждого хода вместе с пространственным
перемещением. Игрок может выбрать любой доступный обычный или реакционный ход;
обязательного порядка досок нет. После успешного хода очередь переходит сопернику.
Ход на времени `t` гасит реакции своей стороны на досках `<= t`, сохраняя
обычную доступность вне разыгранного слота хода.

**Состояние реализации:** обычные ходы, взятия, рокировка, en passant,
превращение, отмена зависимых ходов и восстановление фигур проверены тестами.
Можно загружать заранее заданные обратные мировые линии.
Построение новой временной петли при смене направления ещё не реализовано:
`apply_new_mv` возвращает `EngineErr::UnsupportedMove`, сохраняя состояние игры.
Генератор `possible_moves` возвращает и такие кандидаты, чтобы направление
оставалось частью модели хода. Проверка возможности применить команду окончательно
выполняется в `apply_new_mv`.

## Использование

```rust
use nolan_chess::engine::Engine;
use nolan_chess::model::{Color, Sq, TimeDirection};

let mut game = Engine::default(); // Сразу содержит доску и индексы.
let pawn = game.move_sources()
    .find(|piece| piece.t == 0 && piece.sq == Sq(4, 1))
    .expect("white e-pawn");
let mv = game.possible_moves(pawn.origin, pawn.age)?
    .into_iter()
    .find(|mv| mv.sq == Sq(4, 3) && mv.direction == TimeDirection::Future)
    .expect("e2-e4");

game.apply_new_mv(mv)?; // Ход, согласование истории и смена очереди атомарны.
assert_eq!(game.turn(), Color::Black);
assert_eq!(game.board(1).unwrap().get_piece(4, 3).unwrap().origin, pawn.origin);

let restored = Engine::from_history(game.history().clone(), game.turn())?;
assert_eq!(restored, game);
# Ok::<(), nolan_chess::engine::EngineErr>(())
```

`move_sources()` перечисляет доступные состояния фигур на всех досках для
текущего игрока. Для сценариев и тестов есть `State::new` и
`Engine::from_worldlines`; после построения движка история доступна только на чтение.

Подробнее: [модель, инварианты и дальнейшая разработка](docs/architecture.md).

## Проверка

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Внешних зависимостей у библиотеки нет. Пример выше также запускается как doctest.
