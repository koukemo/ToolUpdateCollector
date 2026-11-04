mod config;
mod privilege;
mod runner;
mod tui;
mod update;

use anyhow::Result;
use clap::{Parser, Subcommand};
use config::{Config, Entry, Runner};

#[derive(Parser)]
#[command(
    name = "tuc",
    version,
    about = "Tool Update Collector - 登録したコマンドをまとめて実行"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// TUIを開かずに有効なコマンドをすべて実行
    Run,
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
        /// WSLで実行 (Windows版のみ)
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
    let path = config::config_path()?;
    let mut cfg = Config::load(&path)?;

    match cli.cmd {
        None => match tui::run(&mut cfg, &path)? {
            tui::Action::Run => run_and_exit(&cfg),
            tui::Action::Quit => Ok(()),
        },
        Some(Cmd::Run) => run_and_exit(&cfg),
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
            wsl,
        }) => {
            let mut e = Entry::new(name, command);
            e.interactive = interactive;
            e.run_last = last;
            e.requires_root |= root;
            if wsl {
                e.runner = Runner::Wsl;
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

fn run_and_exit(cfg: &Config) -> Result<()> {
    let results = runner::run_all(&cfg.commands)?;
    runner::print_summary(&results);
    if results.iter().any(|r| !r.success) {
        std::process::exit(1);
    }
    Ok(())
}
