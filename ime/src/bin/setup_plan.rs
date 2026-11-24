//! `pangyi.exe` 怎么分流参数，以及安装目录里哪些文件要留下。
//!
//! 这里不调用 Windows API。Linux 上的 `cargo test` 能跑这些判断；
//! 真正写注册表、建快捷方式、启动进程的代码在 `setup.rs`。

pub const INSTALL_DIR: &str = r"C:\Program Files\Pangyi";
pub const APP_NAME: &str = "pangyi.exe";
pub const SETTINGS_NAME: &str = "pangyi-settings.exe";
pub const UPDATER_NAME: &str = "pangyi-updater.exe";
pub const DLL64_NAME: &str = "pangyi_ime.dll";
pub const DLL32_NAME: &str = "pangyi_ime-x86.dll";
pub const ICO_NAME: &str = "pangyi.ico";
pub const VERSION_FILE: &str = "version.txt";
pub const ACCOUNT_EXAMPLE_NAME: &str = "account.toml.example";
pub const DESKTOP_APP: &str = "旁译.exe";
pub const START_MENU_LINK: &str =
    r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs\旁译.lnk";
pub const LEGACY_SETTINGS_LINK: &str =
    r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs\旁译 设置.lnk";
pub const PUBLIC_DESKTOP_LEGACY_LINK: &str = r"C:\Users\Public\Desktop\旁译 设置.lnk";

pub const ARG_ACCOUNT: &str = "--account";
pub const ARG_UNINSTALL: &str = "--uninstall";
pub const ARG_INSTALL: &str = "--install";
pub const ARG_LEGACY_SETTINGS: &str = "--legacy-settings";

/// 发版脚本只改这个包名在 lock 里的版本行。设置程序源码里的常量要和它对得上。
#[allow(dead_code)]
pub const SETTINGS_PACKAGE: &str = "pangyi-settings";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    /// 账号窗口。比卸载、安装、旧面板都优先，避免带参数的登录被设置窗口吃掉。
    Account,
    Uninstall,
    /// 版本不是当前，或者显式 `--install`。
    Install,
    /// 旧 GDI 面板。不是桌面和开始菜单的入口。
    LegacySettings,
    /// 启动安装目录里的 `pangyi-settings.exe`。
    OpenSettings,
}

/// `installed_current`：`version.txt` 与当前安装包版本一致，且 64 位 DLL 在。
pub fn route<S: AsRef<str>>(args: &[S], installed_current: bool) -> Entry {
    let has = |flag: &str| args.iter().any(|arg| arg.as_ref() == flag);
    if has(ARG_ACCOUNT) {
        return Entry::Account;
    }
    if has(ARG_UNINSTALL) {
        return Entry::Uninstall;
    }
    if has(ARG_INSTALL) {
        return Entry::Install;
    }
    // 显式要求旧面板时不顺便安装，也不提权。
    if has(ARG_LEGACY_SETTINGS) {
        return Entry::LegacySettings;
    }
    if !installed_current {
        return Entry::Install;
    }
    Entry::OpenSettings
}

/// 需要提权时带上的参数。打开设置、账号、旧面板都不提权。
pub fn elevation_arg(entry: Entry) -> Option<&'static str> {
    match entry {
        Entry::Uninstall => Some(ARG_UNINSTALL),
        Entry::Install => Some(ARG_INSTALL),
        Entry::Account | Entry::LegacySettings | Entry::OpenSettings => None,
    }
}

pub fn settings_exe_path() -> String {
    format!(r"{INSTALL_DIR}\{SETTINGS_NAME}")
}

pub fn app_exe_path() -> String {
    format!(r"{INSTALL_DIR}\{APP_NAME}")
}

/// 开始菜单「旁译」直接指向设置程序。
pub fn start_menu_target() -> String {
    settings_exe_path()
}

/// 桌面文件仍叫「旁译.exe」，内容是 `pangyi.exe` 的副本。
/// 已是当前版本时，这份副本再去启动设置程序；版本不同时它自己负责安装。
#[allow(dead_code)]
pub fn desktop_file_name() -> &'static str {
    DESKTOP_APP
}

pub fn version_matches(file_text: &str, pkg_version: &str, dll_present: bool) -> bool {
    dll_present && file_text.trim() == pkg_version
}

/// 设置程序不在磁盘上，或进程没有拉起来，就退回旧 GDI 面板。
pub fn use_legacy_settings(settings_present: bool, spawned: bool) -> bool {
    !settings_present || !spawned
}

/// 安装目录升级时按文件名保留。新的 `pangyi-settings.exe` 在这份名单里。
pub fn install_dir_keep_names() -> &'static [&'static str] {
    &[
        DLL64_NAME,
        DLL32_NAME,
        APP_NAME,
        ICO_NAME,
        VERSION_FILE,
        ACCOUNT_EXAMPLE_NAME,
        UPDATER_NAME,
        "account.toml",
        SETTINGS_NAME,
    ]
}

/// 安装目录里的旧 exe，以及文件名以「旁译」开头、又不在保留名单里的文件。
/// 只用于 `C:\Program Files\Pangyi`。桌面清文件用精确名单，不用这个前缀。
pub fn is_obsolete_install_file(name: &str) -> bool {
    let low = name.to_ascii_lowercase();
    let kept = install_dir_keep_names()
        .iter()
        .any(|keep| keep.eq_ignore_ascii_case(name));
    !kept && (low.ends_with(".exe") || name.starts_with("旁译"))
}

/// 桌面上已知的旧版旁译文件名（精确匹配，只删这些）。
///
/// 不能用「旁译」前缀清桌面：用户桌面上可能有「旁译-对话存档」这种
/// 以旁译开头的目录或文件。
pub fn legacy_desktop_names() -> [&'static str; 5] {
    [
        DESKTOP_APP,
        "旁译 设置.lnk",
        "旁译 · 中俄输入.lnk",
        "旁译设置.exe",
        "旁译安装.exe",
    ]
}

/// `%LOCALAPPDATA%\Pangyi` 里的早期用户级副本。
/// 这里的 `pangyi-settings.exe` 是旧位置，不是 `Program Files` 里的新程序。
pub fn is_stale_user_file(name: &str) -> bool {
    let low = name.to_ascii_lowercase();
    (low.starts_with("pangyi_ime") && low.ends_with(".dll"))
        || low == SETTINGS_NAME
        || low.ends_with(".log")
        || legacy_desktop_names().iter().any(|keep| *keep == name)
}

pub fn uninstall_install_dir_files() -> &'static [&'static str] {
    &[
        DLL64_NAME,
        DLL32_NAME,
        ICO_NAME,
        SETTINGS_NAME,
        "pangyi-setup.exe",
        APP_NAME,
        VERSION_FILE,
        ACCOUNT_EXAMPLE_NAME,
        UPDATER_NAME,
    ]
}

pub fn uninstall_links() -> &'static [&'static str] {
    &[
        LEGACY_SETTINGS_LINK,
        START_MENU_LINK,
        PUBLIC_DESKTOP_LEGACY_LINK,
    ]
}

/// 只改每一行行首的 `version = "..."`。依赖行里的 `version =` 不动。
#[cfg(test)]
pub fn rewrite_manifest_version(toml: &str, version: &str) -> String {
    let mut out = String::with_capacity(toml.len() + version.len());
    let mut rest = toml;
    let mut at_line_start = true;
    while !rest.is_empty() {
        if at_line_start && rest.starts_with("version = ") {
            let line_end = rest.find(['\n', '\r']).unwrap_or(rest.len());
            out.push_str("version = \"");
            out.push_str(version);
            out.push('"');
            rest = &rest[line_end..];
            at_line_start = false;
            continue;
        }
        let ch = rest.chars().next().unwrap();
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
        at_line_start = ch == '\n';
    }
    out
}

/// 只改 `name = "<package>"` 下一行的 version。lock 里其它包不动。
#[cfg(test)]
pub fn rewrite_lock_package_version(
    lock: &str,
    package: &str,
    version: &str,
) -> Result<String, String> {
    let needle = format!("name = \"{package}\"");
    let Some(idx) = lock.find(&needle) else {
        return Err(format!("missing package {package}"));
    };
    let after_name = idx + needle.len();
    let rest = &lock[after_name..];
    let nl_len = if rest.starts_with("\r\n") {
        2
    } else if rest.starts_with('\n') {
        1
    } else {
        return Err(format!("expected newline after {package}"));
    };
    let ver_at = after_name + nl_len;
    let prefix = "version = \"";
    if !lock[ver_at..].starts_with(prefix) {
        return Err(format!("expected version line for {package}"));
    }
    let value_start = ver_at + prefix.len();
    let Some(rel_end) = lock[value_start..].find('"') else {
        return Err("unterminated version".to_string());
    };
    let mut out = String::with_capacity(lock.len() + version.len());
    out.push_str(&lock[..value_start]);
    out.push_str(version);
    out.push_str(&lock[value_start + rel_end..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn repo_root() -> PathBuf {
        let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        for _ in 0..5 {
            if dir.join("README.md").is_file() && dir.join(".github").is_dir() {
                return dir;
            }
            dir = dir.parent().expect("repo root").to_path_buf();
        }
        panic!("repo root not found from {}", env!("CARGO_MANIFEST_DIR"));
    }

    #[test]
    fn no_args_opens_settings_only_when_this_version_is_installed() {
        assert_eq!(route(&["pangyi.exe"], true), Entry::OpenSettings);
        assert_eq!(route(&["pangyi.exe"], false), Entry::Install);
        assert_eq!(route::<&str>(&[], true), Entry::OpenSettings);
        assert_eq!(route::<&str>(&[], false), Entry::Install);
    }

    #[test]
    fn account_wins_and_keeps_the_installed_exe_path() {
        assert_eq!(
            route(&["pangyi.exe", "--account", "--uninstall", "--install"], true),
            Entry::Account
        );
        assert_eq!(
            route(&["--account", "--legacy-settings"], false),
            Entry::Account
        );
        assert_eq!(app_exe_path(), r"C:\Program Files\Pangyi\pangyi.exe");
        assert_eq!(ARG_ACCOUNT, "--account");
        assert_eq!(elevation_arg(Entry::Account), None);
    }

    #[test]
    fn uninstall_beats_install_and_legacy() {
        assert_eq!(
            route(&["--uninstall", "--install", "--legacy-settings"], true),
            Entry::Uninstall
        );
        assert_eq!(route(&["--uninstall"], false), Entry::Uninstall);
        assert_eq!(elevation_arg(Entry::Uninstall), Some("--uninstall"));
    }

    #[test]
    fn explicit_install_survives_an_already_current_install() {
        assert_eq!(route(&["--install"], true), Entry::Install);
        assert_eq!(
            route(&["--install", "--legacy-settings"], true),
            Entry::Install
        );
        assert_eq!(elevation_arg(Entry::Install), Some("--install"));
        assert_eq!(elevation_arg(Entry::OpenSettings), None);
        assert_eq!(elevation_arg(Entry::LegacySettings), None);
    }

    #[test]
    fn legacy_flag_opens_the_old_panel_without_installing() {
        assert_eq!(route(&["--legacy-settings"], true), Entry::LegacySettings);
        assert_eq!(route(&["--legacy-settings"], false), Entry::LegacySettings);
        assert_eq!(
            route(&["--install", "--legacy-settings"], false),
            Entry::Install
        );
        assert_ne!(route(&["pangyi.exe"], true), Entry::LegacySettings);
    }

    #[test]
    fn unknown_flag_does_not_change_the_default() {
        assert_eq!(route(&["--Account"], true), Entry::OpenSettings);
        assert_eq!(route(&["--settings"], true), Entry::OpenSettings);
        assert_eq!(route(&["--help"], false), Entry::Install);
    }

    #[test]
    fn version_file_must_match_and_the_dll_must_be_present() {
        assert!(version_matches("4.19.0\n", "4.19.0", true));
        assert!(version_matches("  4.19.0  ", "4.19.0", true));
        assert!(!version_matches("4.19.0", "4.19.0", false));
        assert!(!version_matches("4.18.0", "4.19.0", true));
        assert!(!version_matches("", "4.19.0", true));
    }

    #[test]
    fn new_settings_exe_stays_in_program_files_during_upgrade() {
        assert!(install_dir_keep_names().contains(&SETTINGS_NAME));
        assert!(install_dir_keep_names().contains(&APP_NAME));
        assert!(install_dir_keep_names().contains(&UPDATER_NAME));
        assert!(!is_obsolete_install_file(SETTINGS_NAME));
        assert!(!is_obsolete_install_file("Pangyi-Settings.EXE"));
        assert!(!is_obsolete_install_file(APP_NAME));
        assert!(!is_obsolete_install_file("account.toml"));
        assert!(is_obsolete_install_file("pangyi-setup.exe"));
        assert!(is_obsolete_install_file("旁译安装.exe"));
        assert!(is_obsolete_install_file("旁译.exe"));
        assert!(!is_obsolete_install_file("readme.txt"));
        assert!(!is_obsolete_install_file(DLL64_NAME));
    }

    #[test]
    fn user_dir_still_drops_the_old_settings_exe_copy() {
        assert!(is_stale_user_file("pangyi-settings.exe"));
        assert!(is_stale_user_file("PANGYI-SETTINGS.EXE"));
        assert!(is_stale_user_file("pangyi_ime-old.dll"));
        assert!(is_stale_user_file("debug.log"));
        assert!(is_stale_user_file("旁译.exe"));
        assert!(!is_stale_user_file("settings.txt"));
        assert!(!is_stale_user_file("phrases.txt"));
        assert!(!is_stale_user_file("旁译-对话存档"));
        assert_eq!(
            legacy_desktop_names().len(),
            5,
            "桌面名单变长时要确认没有改成前缀匹配"
        );
    }

    #[test]
    fn entries_point_at_the_new_exe_and_uninstall_removes_it() {
        assert_eq!(
            start_menu_target(),
            r"C:\Program Files\Pangyi\pangyi-settings.exe"
        );
        assert_eq!(settings_exe_path(), start_menu_target());
        assert_eq!(desktop_file_name(), "旁译.exe");
        assert_eq!(START_MENU_LINK.ends_with("旁译.lnk"), true);
        assert!(uninstall_install_dir_files().contains(&SETTINGS_NAME));
        assert!(uninstall_install_dir_files().contains(&APP_NAME));
        assert!(uninstall_install_dir_files().contains(&UPDATER_NAME));
        assert!(uninstall_links().contains(&START_MENU_LINK));
        assert!(uninstall_links().contains(&LEGACY_SETTINGS_LINK));
        assert!(use_legacy_settings(false, false));
        assert!(use_legacy_settings(true, false));
        assert!(use_legacy_settings(false, true));
        assert!(!use_legacy_settings(true, true));
    }

    #[test]
    fn manifest_rewrite_touches_only_the_leading_version_line() {
        let toml = "\
[package]\n\
name = \"pangyi-settings\"\n\
version = \"4.19.0\"\n\
\n\
[dependencies]\n\
serde = { version = \"1\", features = [\"derive\"] }\n\
slint = { version = \"1.19\", default-features = false }\n";
        let rewritten = rewrite_manifest_version(toml, "4.20.0");
        assert!(rewritten.contains("version = \"4.20.0\"\n"));
        assert!(!rewritten.contains("version = \"4.19.0\""));
        assert!(rewritten.contains("serde = { version = \"1\""));
        assert!(rewritten.contains("version = \"1.19\""));
    }

    #[test]
    fn lock_rewrite_touches_only_the_named_package() {
        let lock = "\
[[package]]\n\
name = \"other\"\n\
version = \"4.19.0\"\n\
\n\
[[package]]\n\
name = \"pangyi-settings\"\n\
version = \"4.19.0\"\n\
dependencies = [\n";
        let rewritten = rewrite_lock_package_version(lock, SETTINGS_PACKAGE, "4.20.0").unwrap();
        assert!(rewritten.contains("name = \"other\"\nversion = \"4.19.0\"\n"));
        assert!(rewritten.contains("name = \"pangyi-settings\"\nversion = \"4.20.0\"\n"));
        assert!(rewrite_lock_package_version(lock, "missing", "1.0.0").is_err());
    }

    #[test]
    fn crlf_lock_line_keeps_its_newline() {
        let lock = "name = \"pangyi-settings\"\r\nversion = \"4.19.0\"\r\n";
        let rewritten = rewrite_lock_package_version(lock, SETTINGS_PACKAGE, "9.9.9").unwrap();
        assert_eq!(
            rewritten,
            "name = \"pangyi-settings\"\r\nversion = \"9.9.9\"\r\n"
        );
    }

    #[test]
    fn checked_in_settings_package_accepts_a_version_bump() {
        let root = repo_root();
        let toml = fs::read_to_string(root.join("settings/Cargo.toml")).unwrap();
        let rewritten = rewrite_manifest_version(&toml, "9.9.9");
        assert!(rewritten.contains("version = \"9.9.9\""));
        assert!(rewritten.contains("version = \"1.19\""));
        assert!(!rewritten.contains("version = \"4.19.0\""));
        let lock = fs::read_to_string(root.join("settings/Cargo.lock")).unwrap();
        assert_eq!(lock.matches("name = \"pangyi-settings\"").count(), 1);
        let rewritten = rewrite_lock_package_version(&lock, SETTINGS_PACKAGE, "9.9.9").unwrap();
        let at = rewritten.find("name = \"pangyi-settings\"").unwrap();
        let after = &rewritten[at..];
        assert!(
            after.starts_with("name = \"pangyi-settings\"\nversion = \"9.9.9\"")
                || after.starts_with("name = \"pangyi-settings\"\r\nversion = \"9.9.9\"")
        );
    }

    #[test]
    fn release_script_and_installer_build_wire_the_same_files() {
        let root = repo_root();
        let release = fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let sync = fs::read_to_string(root.join(".github/scripts/sync-settings-version.ps1")).unwrap();
        let build = fs::read_to_string(root.join(".github/scripts/build-windows.ps1")).unwrap();
        assert!(release.contains("sync-settings-version.ps1"));
        assert!(sync.contains("pangyi-settings"));
        assert!(sync.contains("Cargo.toml"));
        assert!(sync.contains("Cargo.lock"));
        let copied = build
            .find("copied pangyi-settings.exe into ime\\payload")
            .expect("payload copy marker");
        let setup = build
            .find("building setup installer")
            .expect("setup build marker");
        let built = build
            .find("building pangyi-settings")
            .expect("settings build marker");
        assert!(built < copied && copied < setup);
        assert!(build.contains("setup.exe size="));
        assert!(build.contains("pangyi-settings.exe"));
    }

    #[test]
    fn settings_program_still_launches_pangyi_exe_account_mode() {
        let src = fs::read_to_string(repo_root().join("settings/src/updater_cfg.rs")).unwrap();
        assert!(src.contains("r\"C:\\Program Files\\Pangyi\\pangyi.exe\""));
        assert!(src.contains("pub const ACCOUNT_ARG: &str = \"--account\""));
        assert!(!src.contains("pangyi-settings.exe --account"));
    }
}
