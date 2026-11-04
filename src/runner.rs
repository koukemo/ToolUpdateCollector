use anyhow::Result;
use std::process::{Command, Stdio};

use crate::config::Entry;
#[cfg(windows)]
use crate::config::Runner;
use crate::privilege;

pub struct Outcome {
    pub name: String,
    pub success: bool,
    pub detail: String,
}

/// 有効なコマンドを実行順に並べる (通常 → 最後に実行)
pub fn plan(entries: &[Entry]) -> Vec<Entry> {
    let enabled = entries.iter().filter(|e| e.enabled);
    let (last, normal): (Vec<_>, Vec<_>) = enabled.cloned().partition(|e| e.run_last);
    normal.into_iter().chain(last).collect()
}

pub fn run_all(entries: &[Entry]) -> Result<Vec<Outcome>> {
    let plan = plan(entries);
    if plan.is_empty() {
        println!("実行するコマンドがありません。");
        return Ok(vec![]);
    }

    let root = privilege::is_root();
    if root {
        println!("\x1b[1;33m* root権限で実行中: 対話なしで全コマンドを実行します\x1b[0m");
    } else if cfg!(unix) && plan.iter().any(|e| e.requires_root) {
        // 先にsudoの認証を済ませ、途中でパスワード待ちにならないようにする
        println!("\x1b[1;33m* root権限が必要なコマンドがあります。sudo認証を行います\x1b[0m");
        let _ = Command::new("sudo").arg("-v").status();
    }

    let total = plan.len();
    let mut results = Vec::new();
    for (i, e) in plan.iter().enumerate() {
        println!(
            "\n\x1b[1;36m==> [{}/{}] {}\x1b[0m  \x1b[2m{}\x1b[0m",
            i + 1,
            total,
            e.name,
            e.command
        );
        let mut cmd = build_command(e, root);
        // 対話モード以外は標準入力を閉じ、入力待ちで止まらないようにする
        cmd.stdin(if e.interactive || root {
            Stdio::inherit()
        } else {
            Stdio::null()
        });
        let outcome = match cmd.status() {
            Ok(s) if s.success() => Outcome {
                name: e.name.clone(),
                success: true,
                detail: "OK".into(),
            },
            Ok(s) => Outcome {
                name: e.name.clone(),
                success: false,
                detail: s
                    .code()
                    .map(|c| format!("終了コード {c}"))
                    .unwrap_or("シグナルで終了".into()),
            },
            Err(err) => Outcome {
                name: e.name.clone(),
                success: false,
                detail: format!("起動失敗: {err}"),
            },
        };
        results.push(outcome);
    }
    Ok(results)
}

pub fn print_summary(results: &[Outcome]) {
    if results.is_empty() {
        return;
    }
    println!("\n\x1b[1m===== 実行結果 =====\x1b[0m");
    for r in results {
        let mark = if r.success {
            "\x1b[32m✔\x1b[0m"
        } else {
            "\x1b[31m✘\x1b[0m"
        };
        println!(" {mark} {}  ({})", r.name, r.detail);
    }
}

#[cfg(windows)]
fn build_command(e: &Entry, _root: bool) -> Command {
    match e.runner {
        Runner::Wsl => {
            let mut c = Command::new("wsl.exe");
            c.args(["--", "bash", "-lc", &e.command]);
            c
        }
        Runner::Native => {
            let mut c = Command::new("cmd");
            c.args(["/C", &e.command]);
            c
        }
    }
}

#[cfg(not(windows))]
fn build_command(e: &Entry, root: bool) -> Command {
    if root && !e.requires_root {
        // sudo tuc 時、root不要のコマンド (brew 等) は元ユーザーのログイン環境で実行
        if let Some(user) = privilege::sudo_user() {
            let mut c = Command::new("sudo");
            c.args(["-i", "-u", &user, "sh", "-c", &e.command]);
            return c;
        }
    }
    let mut c = Command::new("sh");
    c.args(["-c", &e.command]);
    c
}
