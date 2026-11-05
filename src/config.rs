use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::privilege;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Runner {
    /// OS標準のシェルで実行
    #[default]
    Native,
    /// Windows版のみ: WSL上で実行 (他OSでは Native として扱う)
    #[cfg_attr(not(windows), allow(dead_code))]
    Wsl,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub command: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 対話モード: 標準入力をつなぎ、ユーザーの入力を受け付ける
    #[serde(default)]
    pub interactive: bool,
    /// 最後に実行: 通常のコマンドがすべて終わった後に実行する
    #[serde(default)]
    pub run_last: bool,
    /// root権限が必要 (sudo tuc 時は root のまま、それ以外は元ユーザーで実行)
    /// Windows では管理者権限が必要 (未昇格なら UAC で昇格した tuc で実行)
    #[serde(default)]
    pub requires_root: bool,
    #[serde(default)]
    pub runner: Runner,
}

fn default_true() -> bool {
    true
}

impl Entry {
    pub fn new(name: impl Into<String>, command: impl Into<String>) -> Self {
        let command = command.into();
        let mut name = name.into();
        if name.trim().is_empty() {
            name = command
                .split_whitespace()
                .take(2)
                .collect::<Vec<_>>()
                .join(" ");
        }
        Entry {
            requires_root: looks_like_sudo(&command),
            name,
            command,
            enabled: true,
            interactive: false,
            run_last: false,
            runner: Runner::Native,
        }
    }

    pub fn tags(&self) -> Vec<&'static str> {
        let mut t = Vec::new();
        if self.interactive {
            t.push("対話");
        }
        if self.run_last {
            t.push("最後");
        }
        if self.requires_root {
            t.push(privilege::ROOT_LABEL);
        }
        if cfg!(windows) && self.runner == Runner::Wsl {
            t.push("WSL");
        }
        t
    }
}

pub fn looks_like_sudo(cmd: &str) -> bool {
    cmd.split(|c: char| c.is_whitespace() || c == '&' || c == ';' || c == '|')
        .any(|w| w == "sudo")
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub commands: Vec<Entry>,
}

pub fn config_path() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("TUC_CONFIG") {
        return Ok(PathBuf::from(p));
    }
    // sudo tuc で起動された場合も元ユーザーの設定を使う
    if let Some(home) = privilege::sudo_user_home() {
        return Ok(home.join(".config").join("tuc").join("config.toml"));
    }
    let base = dirs::config_dir().context("設定ディレクトリが見つかりません")?;
    Ok(base.join("tuc").join("config.toml"))
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let s = std::fs::read_to_string(path)
            .with_context(|| format!("{} を読み込めません", path.display()))?;
        toml::from_str(&s).with_context(|| format!("{} の形式が不正です", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
            privilege::chown_to_sudo_user(dir);
        }
        let s = toml::to_string_pretty(self)?;
        std::fs::write(path, s).with_context(|| format!("{} に書き込めません", path.display()))?;
        privilege::chown_to_sudo_user(path);
        Ok(())
    }
}
