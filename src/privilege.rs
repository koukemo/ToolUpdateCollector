//! 権限まわり (sudo tuc / 管理者として実行 で起動されたかどうか等)
use std::path::{Path, PathBuf};

/// root (Unix) または管理者として昇格済み (Windows) か
#[cfg(unix)]
pub fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

#[cfg(windows)]
pub fn is_root() -> bool {
    use std::sync::OnceLock;
    static ELEVATED: OnceLock<bool> = OnceLock::new();
    *ELEVATED.get_or_init(win::is_elevated)
}

#[cfg(not(any(unix, windows)))]
pub fn is_root() -> bool {
    false
}

/// 権限名 (表示用)
pub const ROOT_LABEL: &str = if cfg!(windows) { "管理者" } else { "root" };

/// sudo 経由で起動された場合の元ユーザー名
#[cfg(unix)]
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

/// 自分自身を管理者として (UAC経由で) 起動し、終了を待って終了コードを返す。
/// UAC でキャンセルされたときは None。
#[cfg(windows)]
pub fn run_self_elevated(args: &[String]) -> anyhow::Result<Option<i32>> {
    win::run_self_elevated(args)
}

#[cfg(windows)]
mod win {
    use anyhow::{bail, Result};
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_CANCELLED, HANDLE};
    use windows_sys::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetExitCodeProcess, OpenProcessToken, WaitForSingleObject, INFINITE,
    };
    use windows_sys::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    pub fn is_elevated() -> bool {
        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return false;
            }
            let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
            let mut len = 0u32;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                &mut elevation as *mut _ as *mut _,
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut len,
            );
            CloseHandle(token);
            ok != 0 && elevation.TokenIsElevated != 0
        }
    }

    fn wide(s: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
        s.as_ref().encode_wide().chain(Some(0)).collect()
    }

    /// CommandLineToArgvW の規則で1引数をクォートする
    fn quote(arg: &str) -> String {
        if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
            return arg.to_string();
        }
        let mut out = String::from("\"");
        let mut backslashes = 0;
        for c in arg.chars() {
            match c {
                '\\' => backslashes += 1,
                '"' => {
                    out.push_str(&"\\".repeat(backslashes * 2 + 1));
                    out.push('"');
                    backslashes = 0;
                }
                c => {
                    out.push_str(&"\\".repeat(backslashes));
                    out.push(c);
                    backslashes = 0;
                }
            }
        }
        out.push_str(&"\\".repeat(backslashes * 2));
        out.push('"');
        out
    }

    pub fn run_self_elevated(args: &[String]) -> Result<Option<i32>> {
        let exe = std::env::current_exe()?;
        let params = args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ");
        let (verb, file, params) = (wide("runas"), wide(&exe), wide(params));
        unsafe {
            let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
            info.fMask = SEE_MASK_NOCLOSEPROCESS;
            info.lpVerb = verb.as_ptr();
            info.lpFile = file.as_ptr();
            info.lpParameters = params.as_ptr();
            info.nShow = SW_SHOWNORMAL;
            if ShellExecuteExW(&mut info) == 0 {
                let err = GetLastError();
                if err == ERROR_CANCELLED {
                    return Ok(None);
                }
                bail!("管理者として起動できません (エラーコード {err})");
            }
            if info.hProcess.is_null() {
                bail!("管理者として起動したプロセスを取得できません");
            }
            WaitForSingleObject(info.hProcess, INFINITE);
            let mut code = 1u32;
            GetExitCodeProcess(info.hProcess, &mut code);
            CloseHandle(info.hProcess);
            Ok(Some(code as i32))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::quote;

        #[test]
        fn quotes_args() {
            assert_eq!(quote("run"), "run");
            assert_eq!(quote(r"C:\a b\c.toml"), r#""C:\a b\c.toml""#);
            assert_eq!(quote(r"C:\a b\"), r#""C:\a b\\""#);
            assert_eq!(quote(r#"a"b"#), r#""a\"b""#);
            assert_eq!(quote(""), r#""""#);
        }
    }
}
