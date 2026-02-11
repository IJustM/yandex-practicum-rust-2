use anyhow::Context;
use clap::Parser;
use std::{
    fs::File,
    io::{BufRead, BufReader},
    net::{TcpStream, UdpSocket},
    thread,
    time::{Duration, Instant},
};
use yandex_practicum_rust_2::{AddressType, StockQuote, get_address, make_fn_write};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(long)]
    server_addr: String,

    #[arg(long)]
    udp_port: u16,

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

    let tickers = match tickers_file {
        Some(path) => {
            println!("path {}", path);
            let mut tickers: Vec<String> = Vec::new();

            let file = File::open(path)?;
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

    let tcp = TcpStream::connect(&server_addr)?;
    let mut write = make_fn_write(&tcp)?;

    let udp = UdpSocket::bind(get_address(AddressType::Bind, udp_port))?;
    let _ = udp.connect(format!(
        "udp://{}",
        format!(
            "{}:{}",
            server_addr
                .split(":")
                .next()
                .context("Некорректный формат --server-addr")?,
            udp_port
        )
    ));

    write(&format!("STREAM {} {}", udp_port, tickers))?;

    // Вывод ответов от сервера
    let tcp_clone = tcp.try_clone()?;
    let join_handle_tcp = thread::spawn(|| {
        let mut reader = BufReader::new(tcp_clone);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    println!("Разрыв соединения с сервером");
                    return;
                }
                Ok(_) => {
                    if !line.is_empty() {
                        if line.starts_with("ERR ") {
                            println!("{}", line.replace("ERR ", ""));
                            return;
                        } else {
                            println!("{}", line);
                        }
                    }
                }
                Err(_) => return,
            };
        }
    });

    // Пинг сервера
    let udp_clone = udp.try_clone()?;
    thread::spawn(move || {
        let mut last_ping = Instant::now();
        loop {
            if last_ping.elapsed() >= Duration::from_secs(1) {
                let _ = udp_clone.send(b"PING");
                last_ping = Instant::now();
            }
        }
    });

    // Вывод тикеров
    thread::spawn(move || {
        let mut buf = [0u8; 65536];
        loop {
            if let Ok(n) = udp.recv(&mut buf) {
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
    });

    let _ = join_handle_tcp.join();

    Ok(())
}
