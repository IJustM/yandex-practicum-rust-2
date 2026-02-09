# Проектная работа rust 2

Егоров Дмитрий

## Настройка

Включение pre-commit `pre-commit install`

## CLI

### Server

Пример запуска `cargo run --bin server`

Пример команды для запроса акций `STREAM 34254 AAPL,TSLA`

### Client

Пример запуска `cargo run --bin client`

Аргументы:
- `--server-addr <SERVER_ADDR>` Адрес сервера
- `--udp-port <UDP_PORT>` Порт для UDP соединения
- `--tickers-file <TICKERS_FILE>` (Опционально) Путь до файла с акциями
