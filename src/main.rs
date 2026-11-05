mod config;
mod privilege;
mod runner;
mod tui;
mod update;

use anyhow::Result;
use clap::{Parser, Subcommand};
use config::{Config, Entry};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "tuc",
    version,
    about = "Tool Update Collector - 登録したコマンドをまとめて実行"
)]
struct Cli {
    /// 設定ファイルのパス (省略時は既定の場所。環境変数 TUC_CONFIG でも指定可)
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// TUIを開かずに有効なコマンドをすべて実行
    Run {
        /// 終了前に Enter の入力を待つ (管理者ウィンドウで実行するときに使う)
        #[arg(long, hide = true)]
        pause: bool,
    },
    /// 登録済みコマンドの一覧を表示
    List,
    /// コマンドを登録
    Add {
        /// 実行するコマンド (例: "brew update && brew upgrade")
        command: String,
        #[arg(short, long, default_value = "")]
        name: String,
        /// 対話モード (標準入力を受け付ける)
        #[arg(short, long)]
        interactive: bool,
        /// 最後に実行
        #[arg(short, long)]
        last: bool,
        /// root権限が必要 (未指定時は sudo を含むかで自動判定)
        #[arg(short, long)]
        root: bool,
        /// WSLで実行
        #[cfg(windows)]
        #[arg(short, long)]
        wsl: bool,
    },
    /// 設定ファイルのパスを表示
    ConfigPath,
    /// GitHub Releases から最新版に更新
    Update {
        /// 確認のみ
        #[arg(long)]
        check: bool,
    },
}

fn main() {
    if let Err(e) = real_main() {
        eprintln!("\x1b[31mエラー:\x1b[0m {e:#}");
        std::process::exit(1);
    }
}

fn real_main() -> Result<()> {
    let cli = Cli::parse();
    #[cfg(windows)]
    update::cleanup_old_exe();
    let path = match cli.config {
        Some(p) => p,
        None => config::config_path()?,
    };
    let mut cfg = Config::load(&path)?;

    match cli.cmd {
        None => match tui::run(&mut cfg, &path)? {
            tui::Action::Run => run_and_exit(&cfg, &path, false),
            tui::Action::Quit => Ok(()),
        },
        Some(Cmd::Run { pause }) => run_and_exit(&cfg, &path, pause),
        Some(Cmd::List) => {
            for (i, e) in cfg.commands.iter().enumerate() {
                let on = if e.enabled { "x" } else { " " };
                println!(
                    "{:>2}. [{on}] {:<16} {}  {}",
                    i + 1,
                    e.name,
                    e.command,
                    e.tags().join(",")
                );
            }
            Ok(())
        }
        Some(Cmd::Add {
            command,
            name,
            interactive,
            last,
            root,
            #[cfg(windows)]
            wsl,
        }) => {
            let mut e = Entry::new(name, command);
            e.interactive = interactive;
            e.run_last = last;
            e.requires_root |= root;
            #[cfg(windows)]
            if wsl {
                e.runner = config::Runner::Wsl;
            }
            println!("登録しました: {}", e.name);
            cfg.commands.push(e);
            cfg.save(&path)
        }
        Some(Cmd::ConfigPath) => {
            println!("{}", path.display());
            Ok(())
        }
        Some(Cmd::Update { check }) => update::run(check),
    }
}

fn run_and_exit(cfg: &Config, path: &Path, pause: bool) -> Result<()> {
    #[cfg(windows)]
    if runner::needs_elevation(&cfg.commands) {
        return run_elevated(path);
    }
    #[cfg(not(windows))]
    let _ = path;

    let results = runner::run_all(&cfg.commands);
    if let Ok(results) = &results {
        runner::print_summary(results);
    }
    let failed = results
        .as_ref()
        .map_or(true, |r| r.iter().any(|r| !r.success));
    if pause {
        if let Err(e) = &results {
            eprintln!("\x1b[31mエラー:\x1b[0m {e:#}");
        }
        println!("\nEnter キーで閉じます");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
    results?;
    if failed {
        std::process::exit(1);
    }
    Ok(())
}

/// 管理者権限が必要なコマンドがあるとき、UAC で昇格した tuc に実行させる
#[cfg(windows)]
fn run_elevated(path: &Path) -> Result<()> {
    println!(
        "\x1b[1;33m* 管理者権限が必要なコマンドがあります。UAC で昇格した別ウィンドウで実行します\x1b[0m"
    );
    let args = [
        "--config".to_string(),
        path.to_string_lossy().into_owned(),
        "run".to_string(),
        "--pause".to_string(),
    ];
    match privilege::run_self_elevated(&args)? {
        Some(code) => {
            println!("管理者ウィンドウでの実行が終わりました (終了コード {code})");
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }
        None => anyhow::bail!("UAC で昇格がキャンセルされたため、実行を中止しました"),
    }
}
