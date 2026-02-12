pub mod constants;

use serde::{Deserialize, Serialize};
use std::{
    fmt,
    io::Write,
    net::TcpStream,
    time::{SystemTime, UNIX_EPOCH},
};

pub type Stocks = Vec<StockQuote>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockQuote {
    pub ticker: String,
    pub price: f64,
    pub volume: u32,
    pub timestamp: u64,
}

impl StockQuote {
    pub fn new(ticker: &str) -> Self {
        let mut stock_quote = Self {
            ticker: ticker.to_string(),
            price: 0.0,
            volume: 0,
            timestamp: 0,
        };
        stock_quote.generate();
        stock_quote
    }

    pub fn generate(&mut self) {
        self.price = if self.price == 0.0 {
            rand::random::<f64>() * 1000.0
        } else {
            let diff = self.price * rand::random::<f64>() / 1000.0;
            if rand::random::<bool>() {
                self.price + diff
            } else {
                self.price - diff
            }
        };
        self.volume = match self.ticker.as_str() {
            "AAPL" | "MSFT" | "TSLA" => 1000 + (rand::random::<f64>() * 5000.0) as u32,
            _ => 100 + (rand::random::<f64>() * 1000.0) as u32,
        };
        self.timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }
}

impl fmt::Display for StockQuote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}|{}|{}|{}",
            self.ticker, self.price, self.volume, self.timestamp
        )
    }
}

pub fn make_fn_write(stream: &TcpStream) -> anyhow::Result<impl FnMut(&str) -> anyhow::Result<()>> {
    let mut writer = stream.try_clone()?;

    let write = move |text: &str| -> anyhow::Result<()> {
        writer.write_all(format!("{}\n", text).as_bytes())?;
        writer.flush()?;
        Ok(())
    };

    Ok(write)
}
