//! tuc update: GitHub Releases から最新版を取得して更新する
use anyhow::{bail, Context, Result};
use serde::Deserialize;

pub const REPO: &str = match option_env!("TUC_REPO") {
    Some(r) => r,
    None => "koukemo/ToolUpdateCollector",
};

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

fn parse_ver(s: &str) -> Result<semver::Version> {
    semver::Version::parse(s.trim_start_matches('v'))
        .with_context(|| format!("不正なバージョン: {s}"))
}

pub fn run(check_only: bool) -> Result<()> {
    let current = parse_ver(env!("CARGO_PKG_VERSION"))?;
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let release: Release = ureq::get(&url)
        .set("User-Agent", concat!("tuc/", env!("CARGO_PKG_VERSION")))
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(404, _) => {
                anyhow::anyhow!("{REPO} に公開済みのリリースがありません")
            }
            e => anyhow::Error::new(e).context("最新リリース情報を取得できません"),
        })?
        .into_json()?;
    let latest = parse_ver(&release.tag_name)?;

    println!("現在のバージョン: {current}");
    println!("最新のバージョン: {latest}");
    if latest <= current {
        println!("最新版です。");
        return Ok(());
    }
    if check_only {
        println!("新しいバージョンがあります: tuc update で更新できます");
        return Ok(());
    }
    install(&release)
}

#[cfg(target_os = "linux")]
fn install(release: &Release) -> Result<()> {
    use std::process::Command;
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        a => a,
    };
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.ends_with(&format!("_{arch}.deb")))
        .with_context(|| {
            format!(
                "{arch} 向けの .deb がリリースにありません: {}",
                release.html_url
            )
        })?;

    let dir = std::env::temp_dir().join(format!("tuc-update-{}", release.tag_name));
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(&asset.name);
    println!("ダウンロード中: {}", asset.name);
    let resp = ureq::get(&asset.browser_download_url)
        .set("User-Agent", concat!("tuc/", env!("CARGO_PKG_VERSION")))
        .call()?;
    let mut file = std::fs::File::create(&path)?;
    std::io::copy(&mut resp.into_reader(), &mut file)?;
    drop(file);

    println!("インストール中...");
    let mut cmd = if crate::privilege::is_root() {
        Command::new("dpkg")
    } else {
        let mut c = Command::new("sudo");
        c.arg("dpkg");
        c
    };
    let status = cmd.arg("-i").arg(&path).status()?;
    let _ = std::fs::remove_dir_all(&dir);
    if !status.success() {
        bail!("dpkg -i に失敗しました");
    }
    println!("更新しました: {}", release.tag_name);
    Ok(())
}

/// Windows版のリリース成果物名: tuc_<ver>_windows_<arch>.zip
#[cfg(windows)]
fn install(release: &Release) -> Result<()> {
    use std::process::Command;
    let suffix = format!("_windows_{}.zip", std::env::consts::ARCH);
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.ends_with(&suffix))
        .with_context(|| {
            format!(
                "Windows ({}) 向けの zip がリリースにありません: {}",
                std::env::consts::ARCH,
                release.html_url
            )
        })?;

    let dir = std::env::temp_dir().join(format!("tuc-update-{}", release.tag_name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let zip = dir.join(&asset.name);
    println!("ダウンロード中: {}", asset.name);
    let resp = ureq::get(&asset.browser_download_url)
        .set("User-Agent", concat!("tuc/", env!("CARGO_PKG_VERSION")))
        .call()?;
    let mut file = std::fs::File::create(&zip)?;
    std::io::copy(&mut resp.into_reader(), &mut file)?;
    drop(file);

    // Windows 10 以降に標準で入っている tar.exe (bsdtar) で zip を展開する
    let extracted = dir.join("extracted");
    std::fs::create_dir_all(&extracted)?;
    // PATH 上の GNU tar (Git for Windows 等) は zip を扱えないので、System32 のものを使う
    let system_root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let tar = std::path::Path::new(&system_root)
        .join("System32")
        .join("tar.exe");
    let status = Command::new(tar)
        .arg("-xf")
        .arg(&zip)
        .arg("-C")
        .arg(&extracted)
        .status()
        .context("tar.exe を起動できません")?;
    if !status.success() {
        bail!("{} を展開できません", asset.name);
    }
    let new_exe = extracted.join("tuc.exe");
    if !new_exe.is_file() {
        bail!("{} に tuc.exe がありません", asset.name);
    }

    println!("インストール中...");
    let result = replace_exe(&new_exe);
    let _ = std::fs::remove_dir_all(&dir);
    result?;
    println!("更新しました: {}", release.tag_name);
    Ok(())
}

/// 実行中の exe はリネームはできるが上書きはできないので、
/// tuc.exe → tuc.exe.old に退避してから新しい exe を置く
#[cfg(windows)]
fn replace_exe(new_exe: &std::path::Path) -> Result<()> {
    let current = std::env::current_exe()?;
    let old = old_exe_path(&current);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&current, &old).map_err(|e| {
        let hint = if e.kind() == std::io::ErrorKind::PermissionDenied {
            "\n管理者として実行したターミナルで tuc update を実行してください"
        } else {
            ""
        };
        anyhow::anyhow!("{} を置き換えられません: {e}{hint}", current.display())
    })?;
    if let Err(e) = std::fs::copy(new_exe, &current) {
        let _ = std::fs::rename(&old, &current);
        bail!("新しい tuc.exe を配置できません: {e}");
    }
    Ok(())
}

#[cfg(windows)]
fn old_exe_path(current: &std::path::Path) -> std::path::PathBuf {
    let mut s = current.as_os_str().to_owned();
    s.push(".old");
    s.into()
}

/// 前回の tuc update で退避した古い exe を消す (失敗しても無視)
#[cfg(windows)]
pub fn cleanup_old_exe() {
    if let Ok(current) = std::env::current_exe() {
        let _ = std::fs::remove_file(old_exe_path(&current));
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
fn install(release: &Release) -> Result<()> {
    println!("このOS向けの自動更新は未対応です。以下からダウンロードしてください:");
    println!("  {}", release.html_url);
    Ok(())
}
