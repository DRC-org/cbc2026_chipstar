//! cctl への USB-CDC シリアル送信。
//!
//! 元の C++ ブリッジと同様、ポートは遅延オープンし、書き込み失敗時は
//! ポートを閉じて次回の送信で再オープンを試みる。

use std::io::Read;
use std::time::Duration;

use anyhow::{Context, Result};
use serialport::SerialPort;

/// cctl へ 1 行ずつ送信するシリアルリンク。
pub struct SerialLink {
    device: String,
    baud: u32,
    port: Option<Box<dyn SerialPort>>,
    /// 改行までの受信途中のバイト列。
    rx: Vec<u8>,
}

/// 未完の行を溜め込みすぎないための上限。これを超えたら捨てる。
const RX_LIMIT: usize = 4096;

impl SerialLink {
    /// 送信先デバイスとボーレートを指定して生成する（この時点では未オープン）。
    pub fn new(device: String, baud: u32) -> Self {
        Self {
            device,
            baud,
            port: None,
            rx: Vec::new(),
        }
    }

    /// ポートが未オープンなら開く。
    fn ensure_open(&mut self) -> Result<()> {
        if self.port.is_some() {
            return Ok(());
        }

        let port = serialport::new(&self.device, self.baud)
            .timeout(Duration::from_millis(50))
            .open()
            .with_context(|| format!("failed to open {}", self.device))?;
        self.port = Some(port);
        Ok(())
    }

    /// 受信済みの完全な行を取り出す。ポートが開いていなければ空を返す。
    /// 読めるバイト数だけを読むので、制御周期を待たせない。
    pub fn read_lines(&mut self) -> Vec<String> {
        let Some(port) = self.port.as_mut() else {
            return Vec::new();
        };

        let available = port.bytes_to_read().unwrap_or(0) as usize;
        if available > 0 {
            let mut chunk = vec![0u8; available];
            match port.read_exact(&mut chunk) {
                Ok(()) => self.rx.extend_from_slice(&chunk),
                Err(_) => {
                    // 次回の送信で開き直す。
                    self.port = None;
                    self.rx.clear();
                    return Vec::new();
                }
            }
        }

        let mut lines = Vec::new();
        while let Some(index) = self.rx.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.rx.drain(..=index).collect();
            let text = String::from_utf8_lossy(&line[..index]);
            let text = text.trim_end_matches('\r').trim();
            if !text.is_empty() {
                lines.push(text.to_owned());
            }
        }

        if self.rx.len() > RX_LIMIT {
            self.rx.clear();
        }

        lines
    }

    /// 1 行を送信する（改行を付与）。失敗時はポートを閉じてエラーを返す。
    pub fn write_line(&mut self, line: &str) -> Result<()> {
        self.ensure_open()?;
        let port = self.port.as_mut().expect("port opened above");

        let result = port
            .write_all(line.as_bytes())
            .and_then(|()| port.write_all(b"\n"));

        match result {
            Ok(()) => Ok(()),
            Err(err) => {
                self.port = None;
                self.rx.clear();
                Err(err).with_context(|| format!("failed to write to {}", self.device))
            }
        }
    }
}
