//! cctl への USB-CDC シリアル送信。
//!
//! 元の C++ ブリッジと同様、ポートは遅延オープンし、書き込み失敗時は
//! ポートを閉じて次回の送信で再オープンを試みる。

use std::time::Duration;

use anyhow::{Context, Result};
use serialport::SerialPort;

/// cctl へ 1 行ずつ送信するシリアルリンク。
pub struct SerialLink {
    device: String,
    baud: u32,
    port: Option<Box<dyn SerialPort>>,
}

impl SerialLink {
    /// 送信先デバイスとボーレートを指定して生成する（この時点では未オープン）。
    pub fn new(device: String, baud: u32) -> Self {
        Self {
            device,
            baud,
            port: None,
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
                Err(err).with_context(|| format!("failed to write to {}", self.device))
            }
        }
    }
}
