use std::{
    collections::HashMap,
    io::{BufRead, BufReader, ErrorKind},
    net::{TcpListener, TcpStream, UdpSocket},
    sync::{
        Arc, Mutex,
        mpsc::{self},
    },
    thread,
    time::{Duration, Instant},
};
use thiserror::Error;
use yandex_practicum_rust_2::{StockQuote, Stocks, make_fn_write};

const TICKERS: &str = include_str!("../assets/tickers.txt");
const PORT_TCP: u16 = 7000;
const PORT_UDP: u16 = 7001;
const DURATION_TICKERS_GENERATE_SEC: u64 = 1;
const DURATION_PING_TIMEOUT_SEC: u64 = 5;
const DURATION_READ_TIMEOUT_SEC: u64 = 1;
const ADDRESS: &str = "0.0.0.0";

type Clients = Arc<Mutex<HashMap<u16, (Instant, Vec<String>)>>>;

#[derive(Debug, Error)]
enum HandleTcpError {
    #[error("Некорректная команда")]
    InvalidCommand,

    #[error("Некорректный порт")]
    InvalidPort,

    #[error("Порт уже занят")]
    PortInUse,

    #[error("Ошибка Ping/Pong")]
    Ping(Option<u16>),

    #[error("Неизвестная ошибка {0}")]
    Unknown(String),
}

fn main() -> anyhow::Result<()> {
    // Канал для слежением за акциями
    let (tickers_tx, tickers_rx) = mpsc::channel::<Stocks>();

    // Клиенты подписанные на изменение акций
    let clients: Clients = Arc::new(Mutex::new(HashMap::new()));

    // Поток для генерации данных
    thread::spawn(move || -> anyhow::Result<()> {
        let mut tickers: Vec<_> = TICKERS.split("\n").map(StockQuote::new).collect();

        loop {
            for ticker in &mut tickers {
                ticker.generate();
            }
            tickers_tx.send(tickers.clone())?;
            thread::sleep(Duration::from_secs(DURATION_TICKERS_GENERATE_SEC));
        }
    });

    let tcp = TcpListener::bind(format!("{}:{}", ADDRESS, PORT_TCP))?;
    let udp = UdpSocket::bind(format!("{}:{}", ADDRESS, PORT_UDP))?;

    // Отправка измененых текеров клиентам по портам
    let clients_clone = clients.clone();
    thread::spawn(move || -> anyhow::Result<()> {
        loop {
            if let Ok(tickers) = tickers_rx.recv() {
                if let Ok(client) = clients_clone.lock() {
                    println!("clients {}", client.iter().count());
                    for (port, (_, tickers_for_watching)) in client.iter() {
                        let tickers = tickers
                            .iter()
                            .filter(|t| tickers_for_watching.contains(&t.ticker))
                            .collect::<Vec<_>>();
                        let payload = serde_json::to_vec(&tickers)?;
                        let _ = udp.send_to(&payload, format!("127.0.0.1:{}", port));
                    }
                }
            }
        }
    });

    // TCP
    for stream in tcp.incoming() {
        let stream_clone = stream?.try_clone()?;
        let clients_clone = clients.clone();
        thread::spawn(move || -> anyhow::Result<()> {
            let mut write = make_fn_write(&stream_clone)?;
            let res = handle_tcp(&stream_clone, &clients_clone);
            if let Ok(client_port) | Err(HandleTcpError::Ping(client_port)) = res {
                // Удаляем порт, если клиент отключился
                if let Some(port) = client_port {
                    let mut clients = clients_clone
                        .lock()
                        .map_err(|e| HandleTcpError::Unknown(e.to_string()))?;
                    clients.remove(&port);
                }
            }
            if let Err(error) = res {
                write(&format!("ERR {}", error))?;
            }
            Ok(())
        });
    }

    Ok(())
}

fn handle_tcp(
    stream: &TcpStream,
    clients: &Clients,
) -> anyhow::Result<Option<u16>, HandleTcpError> {
    stream
        .set_read_timeout(Some(Duration::from_secs(DURATION_READ_TIMEOUT_SEC)))
        .map_err(|e| HandleTcpError::Unknown(e.to_string()))?;

    let mut write = make_fn_write(stream).map_err(|e| HandleTcpError::Unknown(e.to_string()))?;

    let mut reader = BufReader::new(stream);

    write("Соединение установлено").map_err(|e| HandleTcpError::Unknown(e.to_string()))?;

    let mut client_port: Option<u16> = None;

    loop {
        // Если не приходил пинг дольше таймаута, то отключаем клиента
        {
            let clients = clients
                .lock()
                .map_err(|e| HandleTcpError::Unknown(e.to_string()))?;
            if let Some(port) = client_port {
                if let Some(client) = clients.get(&port) {
                    let &(last_ping, _) = client;
                    if last_ping.elapsed() >= Duration::from_secs(DURATION_PING_TIMEOUT_SEC) {
                        return Err(HandleTcpError::Ping(client_port));
                    }
                }
            }
        }

        // Обработка TCP
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => {
                return Ok(client_port);
            }
            Ok(_) => {
                let input = line.trim();
                if input.is_empty() {
                    continue;
                };

                let mut parts = line.trim().split_whitespace();
                // Обработка команды с клиента
                if let Some("STREAM") = parts.next() {
                    if let Some(port) = parts.next() {
                        let port = port
                            .parse::<u16>()
                            .map_err(|_| HandleTcpError::InvalidPort)?;

                        // Не даем подписаться клиенту на уже существующий порт
                        let mut clients = clients
                            .lock()
                            .map_err(|e| HandleTcpError::Unknown(e.to_string()))?;
                        if clients.contains_key(&port) {
                            return Err(HandleTcpError::PortInUse);
                        }

                        let tickers_all = TICKERS.split("\n").collect::<Vec<_>>().join(",");
                        let tickers_for_watching = parts
                            .next()
                            .unwrap_or(&tickers_all)
                            .split(",")
                            .map(|t| t.to_string())
                            .collect::<Vec<_>>();

                        client_port = Some(port);
                        clients.insert(port, (Instant::now(), tickers_for_watching));
                    } else {
                        return Err(HandleTcpError::InvalidPort);
                    }
                } else {
                    return Err(HandleTcpError::InvalidCommand);
                }
            }
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {}
            Err(e) => {
                return Err(HandleTcpError::Unknown(e.to_string()));
            }
        }
    }
}
