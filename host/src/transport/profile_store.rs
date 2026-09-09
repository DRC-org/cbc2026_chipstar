//! 機体設定の明示保存。同じディレクトリで書き終えてから置き換える。
use crate::machine::MachineProfile;
use anyhow::{Context, Result};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

pub fn load(path: &Path) -> Result<MachineProfile> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("機体プロファイルを読めません: {}", path.display()))?;
    MachineProfile::parse(&source)
}

pub fn save(path: &Path, profile: &MachineProfile) -> Result<()> {
    let text = toml::to_string_pretty(profile)?;
    save_text(path, &text)
}

pub fn save_text(path: &Path, text: &str) -> Result<()> {
    let temporary = path.with_extension(format!("toml.{}.new", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)?;
    Ok(())
}
