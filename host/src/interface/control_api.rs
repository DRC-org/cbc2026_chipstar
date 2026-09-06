//! 同一ユーザ専用のUnixソケット。1接続1要求、長さ付きTOMLで交換する。
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub action: String,
    pub token: Option<String>,
    pub axis: Option<String>,
    pub value: Option<f32>,
    pub flag: Option<bool>,
    pub text: Option<String>,
}
impl Request {
    pub fn new(action: &str) -> Self {
        Self {
            action: action.into(),
            ..Default::default()
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    pub message: String,
    pub data: String,
    pub token: Option<String>,
}
impl Reply {
    pub fn accepted() -> Self {
        Self {
            ok: true,
            message: "受付済み。基板の反映はstatusで確認してください".into(),
            data: String::new(),
            token: None,
        }
    }
    pub fn data(data: String) -> Self {
        Self {
            ok: true,
            message: "確認済み".into(),
            data,
            token: None,
        }
    }
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
            data: String::new(),
            token: None,
        }
    }
}
pub fn default_socket() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_RUNTIME_DIR") {
        PathBuf::from(path).join("catchrobo-host.sock")
    } else {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
            .join(".cache/catchrobo/host.sock")
    }
}
pub fn write_frame(stream: &mut UnixStream, value: &impl Serialize) -> Result<()> {
    let data = toml::to_string(value)?;
    if data.len() > 131072 {
        bail!("要求が大きすぎます");
    }
    stream.write_all(&(data.len() as u32).to_be_bytes())?;
    stream.write_all(data.as_bytes())?;
    Ok(())
}
pub fn read_frame<T: for<'de> Deserialize<'de>>(stream: &mut UnixStream) -> Result<T> {
    let mut length = [0; 4];
    stream.read_exact(&mut length)?;
    let size = u32::from_be_bytes(length) as usize;
    if size > 131072 {
        bail!("要求が大きすぎます");
    }
    let mut data = vec![0; size];
    stream.read_exact(&mut data)?;
    Ok(toml::from_str(std::str::from_utf8(&data)?)?)
}
#[allow(dead_code)] // host本体とCLIで通信型を共有する。
pub fn call(socket: &Path, request: &Request) -> Result<Reply> {
    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    write_frame(&mut stream, request)?;
    read_frame(&mut stream)
}
