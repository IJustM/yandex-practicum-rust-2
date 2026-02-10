use anyhow::Context;
use clap::Parser;
use std::{
    fs::File,
    io::{BufRead, BufReader},
    net::{TcpStream, UdpSocket},
};
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

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let Args {
        server_addr,
        udp_port,
        tickers_file,
    } = args;

    // TODO
    let tickers = match tickers_file {
        Some(path) => {
            println!("path {}", path);
            let mut tickers: Vec<String> = Vec::new();

            let file = File::open(path).context("Ошибка открытия файлы с тикерами")?;
            let reader = BufReader::new(file);

            for line in reader.lines() {
                if let Ok(line) = line
                    && !line.is_empty()
                {
                    tickers.push(line);
                }
            }

            tickers.join(",")
        }
        None => "".to_string(),
    };

    let stream = TcpStream::connect(&server_addr)
        .with_context(|| format!("Ошибка подключения к серверу {}", server_addr))?;

    let address = server_addr
        .split(":")
        .next()
        .context("Некорректный формат --server-addr")?;
    let socket_url = format!("{}:{}", address, udp_port);
    let socket = UdpSocket::bind(&socket_url).context("Ошибка создание UDP сокета")?;
    let _ = socket.connect(format!("udp://{}", socket_url));

    println!("Устанавливаем UDP коннект на {}", &socket_url);

    let writer = stream.try_clone().context("Ошибка клонирования stream")?;
    let mut reader = BufReader::new(stream);

    send_to(&writer, &format!("STREAM {} {}", udp_port, tickers));

    let mut line = String::new();
    reader
        .read_line(&mut line)
        .context("Ошибка чтения ответа от сервера")?;
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
