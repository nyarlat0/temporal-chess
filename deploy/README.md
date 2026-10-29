# Браузерная сборка и размещение

## Сборка

Нужны Rust с `wasm32-unknown-unknown`, `wasm-bindgen-cli` **точно версии из Cargo.lock**,
Binaryen (`wasm-opt`), Brotli, gzip. Пользовательские Cargo-инструменты должны быть в PATH:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
rustup target add wasm32-unknown-unknown
# Версия должна совпадать с пакетом wasm-bindgen в Cargo.lock:
cargo install --locked wasm-bindgen-cli --version 0.2.129
tools/build-web.sh
```

Результат: **`dist/web/`**, включая `index.html`, модуль JS, оптимизированный WASM,
`.br` и `.gz`. Профиль `web-release`: оптимизация размера `s`, ThinLTO, одна единица
кодогенерации, `panic=abort`, удалённая отладочная информация. Затем `wasm-bindgen`
удаляет служебные секции, `wasm-opt -Os` оптимизирует модуль, Brotli-11/gzip-9 сжимают
артефакты. Нативные оконные бэкенды и многопоточность выключены в браузерной сборке.
Рендер — WebGL2, без требования WebGPU или SharedArrayBuffer. Шрифты и SVG встроены.

Параметры скрипта: `NOLAN_CARGO`, `NOLAN_WASM_BINDGEN`, `NOLAN_WASM_OPT`,
`NOLAN_BROTLI`, `CARGO_TARGET_DIR`. Сборка использует `--locked`.

JS и WASM адресуются хешем содержимого, entrypoint публикуется последним. Старые
`releases/<hash>` сохраняются, чтобы открытые страницы могли закончить загрузку.
При очистке размещения удаляйте старые релизы отдельно, после окончания их использования.

## Запуск

Поднимите сервер и PostgreSQL по [инструкции сервера](../crates/server/README.md).
Из корня репозитория:

```sh
NOLAN_SITE=http://localhost:8081 caddy run --config deploy/Caddyfile
```

Откройте `http://localhost:8081`. Caddy раздаёт `dist/web`, проксирует `/ws` и `/health`
в `127.0.0.1:8080`, выбирает Brotli/gzip по поддержке браузера. HTML не кешируется
надолго; файлы с хешем кешируются как immutable. Порт 8081 принадлежит Caddy,
8080 — Rust-серверу. Для внешнего сайта задайте `NOLAN_SITE=chess.example.org` и
абсолютный `NOLAN_WEB_ROOT`; Caddy обслуживает TLS. Сайт и WebSocket работают на одном
origin, браузер сам выбирает `ws`/`wss`.

Без сервера можно открыть только локальную игру:

```sh
python3 -m http.server 8081 --directory dist/web
```

Это не поддерживает `/ws`; используйте Caddy для мультиплеера. Открытие `index.html`
через `file://` не подходит для загрузки WASM. Буфер обмена браузера требует HTTPS
или localhost.

При отдельном сетевом сервере задайте `NOLAN_WS_URL=wss://server.example.org/ws`
**при сборке** или измените адрес в разделе «Играть по сети → Сервер».
Для нативного клиента `NOLAN_WS_URL` читается при запуске, по умолчанию
`ws://127.0.0.1:8080/ws`. Ссылка `https://example.org/?room=КОД` предзаполняет код комнаты.

Размещайте клиент и сервер одной версии: изменения правил движка требуют синхронного
обновления. Сервер не запускался автоматически и приложение не публикуется скриптом.
