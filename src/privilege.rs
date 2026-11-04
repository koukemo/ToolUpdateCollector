//! 権限まわり (sudo tuc で起動されたかどうか等)
use std::path::{Path, PathBuf};

#[cfg(unix)]
pub fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

#[cfg(not(unix))]
pub fn is_root() -> bool {
    false
}

/// sudo 経由で起動された場合の元ユーザー名
pub fn sudo_user() -> Option<String> {
    if !is_root() {
        return None;
    }
    std::env::var("SUDO_USER")
        .ok()
        .filter(|u| !u.is_empty() && u != "root")
}

#[cfg(unix)]
pub fn sudo_user_home() -> Option<PathBuf> {
    let user = sudo_user()?;
    let passwd = std::fs::read_to_string("/etc/passwd").ok()?;
    passwd.lines().find_map(|l| {
        let f: Vec<&str> = l.split(':').collect();
        (f.len() >= 6 && f[0] == user).then(|| PathBuf::from(f[5]))
    })
}

#[cfg(not(unix))]
pub fn sudo_user_home() -> Option<PathBuf> {
    None
}

/// sudo tuc で作成したファイルの所有者を元ユーザーに戻す
#[cfg(unix)]
pub fn chown_to_sudo_user(path: &Path) {
    if sudo_user().is_none() {
        return;
    }
    let uid = std::env::var("SUDO_UID").ok().and_then(|v| v.parse().ok());
    let gid = std::env::var("SUDO_GID").ok().and_then(|v| v.parse().ok());
    if let (Some(uid), Some(gid)) = (uid, gid) {
        let _ = std::os::unix::fs::chown(path, Some(uid), Some(gid));
    }
}

#[cfg(not(unix))]
pub fn chown_to_sudo_user(_path: &Path) {}
