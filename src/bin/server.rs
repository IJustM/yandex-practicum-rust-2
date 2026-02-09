use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream, UdpSocket},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Duration,
};

use yandex_practicum_rust_2::StockQuote;

const TICKERS: &str = include_str!("../assets/tickers.txt");
const PORT_TCP: i32 = 7878;
const PORT_UDP: i32 = 5000;

fn main() {
    // Канал для слежением за акциями
    let (tickers_tx, tickers_rx) = mpsc::channel::<Vec<StockQuote>>();
    // Канал для слежением за клиентами
    let (client_tx, client_rx) = mpsc::channel::<Sender<Vec<StockQuote>>>();

    // Запускаем поток для генерации данных
    thread::spawn(move || {
        let mut tickers: Vec<_> = TICKERS
            .split("\n")
            .map(|ticker| StockQuote::new(&ticker))
            .collect();

        loop {
            for ticker in &mut tickers {
                ticker.generate();
            }
            tickers_tx
                .send(tickers.clone())
                .expect("Ошибка отправки mpsc");
            thread::sleep(Duration::from_secs(1));
        }
    });

    // Запускаем диспатчер для подписки на клиентов
    thread::spawn(|| {
        dispatcher(tickers_rx, client_rx);
    });

    // Запускает TCP сервер
    let address_tcp = &format!("127.0.0.1:{PORT_TCP}");
    let listener = TcpListener::bind(address_tcp)
        .expect(&format!("Ошибка запуска TcpListener на {}", address_tcp));

    println!("Сервер запущен на {}", address_tcp);

    // Создаем UDP сокет
    let address_udp = &format!("127.0.0.1:{PORT_UDP}");
    let socket = UdpSocket::bind(&address_udp)
        .expect(&format!("Ошибка создания UdpSocket на {}", address_udp));

    // Ожидаем TCP соединений
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let client_tx = client_tx.clone();
                let socket = socket.try_clone().expect("Ошибка клонирования socket");
                thread::spawn(|| {
                    handle_tcp(stream, socket, client_tx);
                });
            }
            Err(e) => println!("Ошибка подключения: {}", e),
        }
    }
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

fn send_to_client(mut writer: &TcpStream, text: &str) {
    let _ = writer.write_all(format!("{}\n", text).as_bytes());
    let _ = writer.flush();
}

fn handle_tcp(stream: TcpStream, socket: UdpSocket, client_tx: Sender<Sender<Vec<StockQuote>>>) {
    let mut writer = stream.try_clone().expect("Ошибка клонирования stream");
    let mut reader = BufReader::new(stream);

    send_to_client(&writer, "Подлючение установлено!");

    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(_) => {
                let input = line.trim();
                if input.is_empty() {
                    let _ = writer.flush();
                    continue;
                }

                let mut parts = input.split_whitespace();
                let response = match parts.next() {
                    Some("STREAM") => {
                        let address = match parts.next() {
                            Some(url) => url.to_string(),
                            None => {
                                send_to_client(&writer, "Ошибка: Неоходимо указать UDP адрес");
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

                        send_to_client(
                            &writer,
                            &format!(
                                "Запускаем UDP соединение на {} для акций {}",
                                address,
                                tickers_for_watching.join(",")
                            ),
                        );

                        let client_tx = client_tx.clone();
                        let socket = socket.try_clone().expect("Ошибка клонирования socket");
                        thread::spawn(|| {
                            handle_udp(client_tx, socket, address, tickers_for_watching);
                        });
                        continue;
                    }
                    Some("EXIT") => {
                        send_to_client(&writer, "Отключение!");
                        return;
                    }
                    _ => "Ошибка: Неизвестная команда",
                };

                send_to_client(&writer, response);
            }
            Err(e) => {
                send_to_client(&writer, &format!("Ошибка чтения: {}", e));
                return;
            }
        }
    }
}

fn handle_udp(
    client_tx: Sender<Sender<Vec<StockQuote>>>,
    socket: UdpSocket,
    address: String,
    tickers_for_watching: Vec<String>,
) {
    let (tx, rx) = mpsc::channel::<Vec<StockQuote>>();

    client_tx
        .send(tx)
        .expect("Ошибка при добавлении клиента в client_tx");

    loop {
        while let Ok(tickers) = rx.try_recv() {
            let tickers = tickers
                .iter()
                .filter(|t| tickers_for_watching.contains(&t.ticker))
                .collect::<Vec<_>>();
            let payload = serde_json::to_vec(&tickers).expect("Ошибка сериализации");
            if socket.send_to(&payload, &address).is_err() {
                break;
            }
        }
    }
}
