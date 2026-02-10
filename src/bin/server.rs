use anyhow::Context;
use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream, UdpSocket},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Duration,
};
use yandex_practicum_rust_2::{StockQuote, send_to};

const TICKERS: &str = include_str!("../assets/tickers.txt");
const ADDRESS: &str = "127.0.0.1";
const PORT_TCP: i32 = 7878;
const PORT_UDP_SOCKET: i32 = 5000;
const DURATION_TICKERS_GENERATE_SEC: u64 = 1;

fn main() -> anyhow::Result<()> {
    // Канал для слежением за акциями
    let (tickers_tx, tickers_rx) = mpsc::channel::<Vec<StockQuote>>();
    // Канал для слежением за клиентами
    let (client_tx, client_rx) = mpsc::channel::<Sender<Vec<StockQuote>>>();

    // Запускаем поток для генерации данных
    thread::spawn(move || {
        let mut tickers: Vec<_> = TICKERS.split("\n").map(StockQuote::new).collect();

        loop {
            for ticker in &mut tickers {
                ticker.generate();
            }
            let _ = tickers_tx.send(tickers.clone());
            thread::sleep(Duration::from_secs(DURATION_TICKERS_GENERATE_SEC));
        }
    });

    // Запускаем диспатчер для подписки на клиентов
    thread::spawn(|| {
        dispatcher(tickers_rx, client_rx);
    });

    // Запускает TCP сервер
    let address_tcp = &format!("{ADDRESS}:{PORT_TCP}");
    let listener = TcpListener::bind(address_tcp)
        .with_context(|| format!("Ошибка запуска TcpListener на {}", address_tcp))?;

    println!("Сервер запущен на {}", address_tcp);

    // Создаем UDP сокет
    let address_udp = &format!("{ADDRESS}:{PORT_UDP_SOCKET}");
    let socket = UdpSocket::bind(address_udp)
        .with_context(|| format!("Ошибка создания UdpSocket на {}", address_udp))?;

    // Ожидаем TCP соединений
    for stream in listener.incoming() {
        let stream = stream.context("Ошибка подключения")?;

        let client_tx = client_tx.clone();
        let socket = socket.try_clone().context("Ошибка клонирования socket")?;
        thread::spawn(|| {
            let _ = handle_tcp(stream, socket, client_tx);
        });
    }

    Ok(())
}

fn dispatcher(tickers_tx: Receiver<Vec<StockQuote>>, client_tx: Receiver<Sender<Vec<StockQuote>>>) {
    let mut clients: Vec<Sender<Vec<StockQuote>>> = Vec::new();

    loop {
        while let Ok(client) = client_tx.try_recv() {
            clients.push(client);
        }

        while let Ok(tickers) = tickers_tx.try_recv() {
            clients.retain(|client| client.send(tickers.clone()).is_ok());
        }
    }
}

fn handle_tcp(
    stream: TcpStream,
    socket: UdpSocket,
    client_tx: Sender<Sender<Vec<StockQuote>>>,
) -> anyhow::Result<()> {
    let mut writer = stream.try_clone().context("Ошибка клонирования stream")?;
    let mut reader = BufReader::new(stream);

    send_to(&writer, "Подлючение установлено!");

    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(_) => {
                let input = line.trim();
                if input.is_empty() {
                    let _ = writer.flush();
                    continue;
                }

                println!("input = {}", input);

                let mut parts = input.split_whitespace();
                let response = match parts.next() {
                    Some("STREAM") => {
                        let port = match parts.next() {
                            Some(url) => url.to_string(),
                            None => {
                                send_to(&writer, "Ошибка: Неоходимо указать UDP порт");
                                continue;
                            }
                        };

                        let tickers_all = TICKERS.split("\n").collect::<Vec<_>>().join(",");
                        let tickers_for_watching = parts
                            .next()
                            .unwrap_or(&tickers_all)
                            .split(",")
                            .map(|t| t.to_string())
                            .collect::<Vec<_>>();

                        send_to(
                            &writer,
                            &format!(
                                "Запускаем UDP соединение на {}:{} для акций {}",
                                ADDRESS,
                                port,
                                tickers_for_watching.join(",")
                            ),
                        );

                        let client_tx = client_tx.clone();
                        let socket = socket.try_clone().context("Ошибка клонирования socket")?;
                        thread::spawn(|| {
                            let _ = handle_udp(client_tx, socket, port, tickers_for_watching);
                        });
                        continue;
                    }
                    Some("EXIT") => {
                        send_to(&writer, "Отключение!");
                        return Ok(());
                    }
                    _ => "Ошибка: Неизвестная команда",
                };

                send_to(&writer, response);
            }
            Err(e) => {
                send_to(&writer, &format!("Ошибка чтения: {}", e));
                return Ok(());
            }
        }
    }
}

fn handle_udp(
    client_tx: Sender<Sender<Vec<StockQuote>>>,
    socket: UdpSocket,
    port: String,
    tickers_for_watching: Vec<String>,
) -> anyhow::Result<()> {
    let (tx, rx) = mpsc::channel::<Vec<StockQuote>>();

    client_tx
        .send(tx)
        .context("Ошибка при добавлении клиента в client_tx")?;

    loop {
        while let Ok(tickers) = rx.try_recv() {
            let tickers = tickers
                .iter()
                .filter(|t| tickers_for_watching.contains(&t.ticker))
                .collect::<Vec<_>>();
            let payload = serde_json::to_vec(&tickers).context("Ошибка сериализации")?;
            if socket
                .send_to(&payload, format!("{}:{}", ADDRESS, port))
                .is_err()
            {
                break;
            }
        }
    }
}
