use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    net::{TcpListener, TcpStream, UdpSocket},
    sync::{
        Arc, Mutex,
        mpsc::{self},
    },
    thread,
    time::{Duration, Instant},
};
use thiserror::Error;
use yandex_practicum_rust_2::{AddressType, StockQuote, Stocks, get_address, make_fn_write};

const TICKERS: &str = include_str!("../assets/tickers.txt");
const PORT_TCP: u16 = 7000;
const PORT_UDP: u16 = 7001;
const DURATION_TICKERS_GENERATE_SEC: u64 = 1;
const DURATION_PING_TIMEOUT_SEC: u64 = 5;

type Clients = Arc<Mutex<HashMap<u16, Vec<String>>>>;

#[derive(Debug, Error)]
enum HandleTcpError {
    #[error("Некорректная команда")]
    InvalidCommand,

    #[error("Некорректный порт")]
    InvalidPort,

    #[error("Порт уже занят")]
    PortInUse,

    #[error("Ошибка Ping/Pong")]
    Ping,

    #[error("Неизвестная ошибка {0}")]
    Unknown(String),
}

fn main() -> anyhow::Result<()> {
    // Канал для слежением за акциями
    let (tickers_tx, tickers_rx) = mpsc::channel::<Stocks>();

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

    // Отслеживание уже занятых портов
    let clients: Clients = Arc::new(Mutex::new(HashMap::new()));

    let tcp = TcpListener::bind(get_address(AddressType::Bind, PORT_TCP))?;
    let udp = UdpSocket::bind(get_address(AddressType::Bind, PORT_UDP))?;

    // Отправка измененых текеров клиентам по портам
    let udp_clone = udp.try_clone()?;
    let clients_clone = clients.clone();
    thread::spawn(move || -> anyhow::Result<()> {
        loop {
            if let Ok(tickers) = tickers_rx.recv() {
                if let Ok(client) = clients_clone.lock() {
                    println!("clients {}", client.iter().count());
                    for (port, tickers_for_watching) in client.iter() {
                        let tickers = tickers
                            .iter()
                            .filter(|t| tickers_for_watching.contains(&t.ticker))
                            .collect::<Vec<_>>();
                        let payload = serde_json::to_vec(&tickers)?;
                        udp_clone.send_to(&payload, get_address(AddressType::Connect, *port))?;
                    }
                }
            }
        }
    });

    // TCP
    for stream in tcp.incoming() {
        let stream_clone = stream?.try_clone()?;
        let udp_clone = udp.try_clone()?;
        let clients_clone = clients.clone();
        thread::spawn(move || -> anyhow::Result<()> {
            let mut write = make_fn_write(&stream_clone)?;
            if let Err(error) = handle_tcp(&stream_clone, &udp_clone, clients_clone) {
                write(&format!("ERR {}", error))?;
            }
            Ok(())
        });
    }

    Ok(())
}

fn handle_tcp(
    stream: &TcpStream,
    udp: &UdpSocket,
    clients: Clients,
) -> anyhow::Result<(), HandleTcpError> {
    let mut write = make_fn_write(stream).map_err(|e| HandleTcpError::Unknown(e.to_string()))?;

    let mut reader = BufReader::new(stream);

    write("Соединение установлено").map_err(|e| HandleTcpError::Unknown(e.to_string()))?;

    let mut client_port: Option<u16> = None;

    // let mut last_ping: Option<Instant> = None;
    // // если не приходил пинг дольше таймаута, то отключаем клиента
    // if let Some(last_ping) = last_ping {
    //     if last_ping.elapsed() >= Duration::from_secs(DURATION_PING_TIMEOUT_SEC) {
    //         return Err(HandleTcpError::Ping);
    //     }
    // }

    loop {
        let mut line = String::new();
        let read_res = reader
            .read_line(&mut line)
            .map_err(|e| HandleTcpError::Unknown(e.to_string()))?;

        // Удаляем порт, если клиент отключился
        if read_res == 0 {
            if let Some(port) = client_port {
                let mut clients = clients
                    .lock()
                    .map_err(|e| HandleTcpError::Unknown(e.to_string()))?;
                clients.remove(&port);
            }
            return Ok(());
        }

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
                // last_ping = Some(Instant::now());
                clients.insert(port, tickers_for_watching);
            } else {
                return Err(HandleTcpError::InvalidPort);
            }
        } else {
            return Err(HandleTcpError::InvalidCommand);
        }
    }
}
