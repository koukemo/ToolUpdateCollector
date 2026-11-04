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

#[cfg(not(target_os = "linux"))]
fn install(release: &Release) -> Result<()> {
    println!("このOS向けの自動更新は未対応です。以下からダウンロードしてください:");
    println!("  {}", release.html_url);
    Ok(())
}
