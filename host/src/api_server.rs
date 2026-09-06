use crate::{
    app_state::Shared,
    control_api::{Reply, Request, read_frame, write_frame},
};
use anyhow::{Context, Result, bail};
use std::{
    fs,
    io::ErrorKind,
    os::unix::{
        fs::{FileTypeExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::Duration,
};

pub struct Server {
    listener: UnixListener,
    path: PathBuf,
}
impl Server {
    pub fn bind(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if let Ok(meta) = fs::symlink_metadata(path) {
            if !meta.file_type().is_socket() {
                bail!("ソケット以外のファイルがあります: {}", path.display());
            }
            if UnixStream::connect(path).is_ok() {
                bail!("hostは既に起動しています: {}", path.display());
            }
            fs::remove_file(path).context("古いソケットを削除できません")?;
        }
        let listener = UnixListener::bind(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            path: path.into(),
        })
    }
    pub fn run(self, shared: Arc<Shared>) {
        while shared.is_running() {
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
                    let _ = stream.set_write_timeout(Some(Duration::from_millis(200)));
                    let reply = match read_frame::<Request>(&mut stream) {
                        Ok(request) => shared.submit(request, false),
                        Err(error) => Reply::error(error.to_string()),
                    };
                    let _ = write_frame(&mut stream, &reply);
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10))
                }
                Err(error) => {
                    shared.log(format!("API: {error}"));
                    break;
                }
            }
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
