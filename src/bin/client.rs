use std::{
    io::{BufRead, BufReader},
    net::{TcpStream, UdpSocket},
};

use clap::Parser;
use yandex_practicum_rust_2::{StockQuote, send_to};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(long)]
    server_addr: String,

    #[arg(long)]
    udp_port: String,

    #[arg(long)]
    tickers_file: Option<String>, // если не указать, то будут приходить все акции
}

fn main() {
    let args = Args::parse();

    let Args {
        server_addr,
        udp_port,
        tickers_file,
    } = args;

    // TODO
    let tickers = match tickers_file {
        Some(file) => "",
        None => "",
    };

    let stream = TcpStream::connect(&server_addr)
        .expect(&format!("Ошибка подключения к серверу {}", &server_addr));

    let address = server_addr
        .split(":")
        .next()
        .expect("Некорректный формат --server-addr");
    let socket_url = format!("{}:{}", address, udp_port);
    let socket = UdpSocket::bind(&socket_url).expect("Ошибка создание UDP сокета");
    let _ = socket.connect(format!("udp://{}", socket_url));
    println!("Устанавливаем UDP коннект на {}", &socket_url);

    let writer = stream.try_clone().expect("Ошибка клонирования stream");
    let mut reader = BufReader::new(stream);

    send_to(&writer, &format!("STREAM {} {}", udp_port, tickers));

    let mut line = String::new();
    reader
        .read_line(&mut line)
        .expect("Ошибка чтения ответа от сервера");
    println!("{}", line);

    let mut buf = [0u8; 65536];

    loop {
        if let Ok(n) = socket.recv(&mut buf) {
            match serde_json::from_slice::<Vec<StockQuote>>(&buf[..n]) {
                Ok(tickers) => {
                    println!();
                    for ticker in tickers {
                        println!("{}", ticker);
                    }
                }
                Err(e) => {
                    eprintln!("Некорректный json: {}", e);
                    continue;
                }
            }
        }
    }
}
