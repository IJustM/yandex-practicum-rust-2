use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::Duration,
};

use yandex_practicum_rust_2::StockQuote;

const TICKERS: &str = include_str!("../assets/tickers.txt");
const PORT: i32 = 7878;

fn main() {
    let (tickers_tx, tickers_rx) = mpsc::channel::<Vec<StockQuote>>();
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
    let address = &format!("127.0.0.1:{PORT}");
    let listener =
        TcpListener::bind(address).expect(&format!("Ошибка запуска TcpListener на {}", address));
    println!("Сервер запущен на {}", address);

    // Ожидаем TCP соединений
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let tx = client_tx.clone();
                thread::spawn(|| {
                    handle_client(stream, tx);
                });
            }
            Err(e) => eprintln!("Ошибка подключения: {}", e),
        }
    }
}

fn dispatcher(tickers_tx: Receiver<Vec<StockQuote>>, client_tx: Receiver<Sender<Vec<StockQuote>>>) {
    let mut clients: Vec<Sender<Vec<StockQuote>>> = Vec::new();

    loop {
        while let Ok(client) = client_tx.try_recv() {
            clients.push(client);
        }

        match tickers_tx.recv() {
            Ok(tickers) => {
                clients.retain(|client| client.send(tickers.clone()).is_ok());
            }
            Err(_) => {
                break;
            }
        }
    }
}

fn handle_client(stream: TcpStream, dispatcher_tx: Sender<Sender<Vec<StockQuote>>>) {
    let (tx, rx) = mpsc::channel::<Vec<StockQuote>>();

    dispatcher_tx
        .send(tx)
        .expect("Ошибка при добавлении клиента в dispatcher");

    fn send_to_client(mut writer: &TcpStream, text: &str) {
        let _ = writer.write_all(format!("{}\n", text).as_bytes());
        let _ = writer.flush();
    }

    let mut writer = stream.try_clone().expect("Ошибка клонирования stream");
    let mut reader = BufReader::new(stream);

    send_to_client(&writer, "Подлючение установлено!");

    // loop {
    //     match rx.recv() {
    //         Ok(tickers) => {}
    //         Err(_) => {
    //             break;
    //         }
    //     }
    // }

    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => {
                return;
            }
            Ok(_) => {
                let input = line.trim();
                if input.is_empty() {
                    let _ = writer.flush();
                    continue;
                }

                let mut parts = input.split_whitespace();
                let response = match parts.next() {
                    Some("EXIT") => {
                        send_to_client(&writer, "Отключение!");
                        return;
                    }
                    _ => "Ошибка: Неизвестная команда",
                };

                send_to_client(&writer, response);
            }
            Err(_) => {
                send_to_client(&writer, "Ошибка чтения!");
                return;
            }
        }
    }
}
