//! 旁译安装程序，也是桌面上的「旁译.exe」。
//!
//! 无参数：已经装的是当前版本，就启动 `pangyi-settings.exe`；否则安装。
//! `--account` 打开账号窗口。`--uninstall` 卸载。`--install` 强制重装。
//! `--legacy-settings` 打开旧的 GDI 设置面板。
//! 设置程序不在，或启动失败时，也会退回旧面板。
//! 装完会出现在「设置 → 应用」里。
#![windows_subsystem = "windows"]
#![allow(non_snake_case)]

use std::ffi::c_void;
use std::path::Path;
use windows::core::{PCWSTR, GUID};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::ShellExecuteW;

#[path = "setup/auth.rs"]
mod auth;
#[path = "setup_plan.rs"]
mod plan;
use windows::Win32::UI::WindowsAndMessaging::*;

// 组件直接打进安装程序里（构建前由脚本放到 ime/payload/）。
// 设置程序和更新程序一样嵌进来，安装时写到安装目录，不单独再发一个下载文件。
// 设计说明在 GPLv3 小节里建议不要嵌，以免安装包本身变成 GPL 程序。
// 本仓库按免版税协议使用 Slint，更新程序也只下载并运行这一个安装包。
const DLL64: &[u8] = include_bytes!("../../payload/pangyi_ime.dll");
const DLL32: &[u8] = include_bytes!("../../payload/pangyi_ime-x86.dll");
const ICO: &[u8] = include_bytes!("../../payload/pangyi.ico");
const UPDATER: &[u8] = include_bytes!("../../payload/pangyi-updater.exe");
const SETTINGS_EXE: &[u8] = include_bytes!("../../payload/pangyi-settings.exe");
const ACCOUNT_EXAMPLE: &[u8] = include_bytes!("../../../account/account.toml.example");
const ACCOUNT_EXAMPLE_NAME: &str = plan::ACCOUNT_EXAMPLE_NAME;

const DIR: &str = plan::INSTALL_DIR;
const DLL64_NAME: &str = plan::DLL64_NAME;
const DLL32_NAME: &str = plan::DLL32_NAME;
const ICO_NAME: &str = plan::ICO_NAME;
const UPDATER_NAME: &str = plan::UPDATER_NAME;
/// 开机自启注册表位置（当前用户，不需要管理员权限）
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const SETTINGS_NAME: &str = plan::SETTINGS_NAME;
/// 安装、卸载、账号窗口仍是这个 exe。日常设置改由 pangyi-settings.exe 打开。
const APP_NAME: &str = plan::APP_NAME;
/// 桌面上的那个软件名。文件内容仍是本 exe 的副本，无参时再去启动设置程序。
const DESKTOP_APP: &str = plan::DESKTOP_APP;
/// 版本标记：装完后写进安装目录，用来判断"已经是当前版本了"
const VERSION_FILE: &str = plan::VERSION_FILE;
const CLSID_STR: &str = "{B4E8F2A1-7C3D-4E5F-9A21-8D6C4B3E1F07}";
const PROFILE_STR: &str = "{5A7C9E12-3D4B-4F8A-B6E1-2C9D7A5E8F34}";
const CLSID: GUID = GUID::from_u128(0xB4E8F2A1_7C3D_4E5F_9A21_8D6C4B3E1F07);

const CATS: [&str; 8] = [
    "{046B8C80-1647-40F7-9B21-B93B81AABC1B}",
    "{13A016DF-560B-46CD-947A-4C3AF1E0E35D}",
    "{25504FB4-7BAB-4BC1-9C69-CF81890F0EF5}",
    "{34745C63-B2F0-4784-8B67-5E12C8701A31}",
    "{364215D9-75BC-11D7-A6EF-00065B84435C}",
    "{49D2F9CE-1F5E-11D7-A6D3-00065B84435C}",
    "{49D2F9CF-1F5E-11D7-A6D3-00065B84435C}",
    "{CCF05DD7-4A87-11D7-A6E2-00065B84435C}",
];

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn msg(title: &str, text: &str) {
    unsafe {
        let t = wide(text);
        let c = wide(title);
        let _ = MessageBoxW(HWND(0), PCWSTR(t.as_ptr()), PCWSTR(c.as_ptr()), MB_OK | MB_ICONINFORMATION);
    }
}

unsafe fn elevated() -> bool {
    // 试着往 Program Files 里写一下：写不进去就说明权限不够
    let probe = format!("{DIR}\\.writetest");
    if std::fs::create_dir_all(DIR).is_err() {
        return false;
    }
    match std::fs::write(&probe, b"x") {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

unsafe fn relaunch_elevated(extra_args: &str) {
    let exe = std::env::current_exe().unwrap_or_default();
    let exe_w = wide(&exe.to_string_lossy());
    let args = wide(extra_args);
    let verb = wide("runas");
    ShellExecuteW(
        HWND(0),
        PCWSTR(verb.as_ptr()),
        PCWSTR(exe_w.as_ptr()),
        PCWSTR(args.as_ptr()),
        PCWSTR::null(),
        SW_SHOWNORMAL,
    );
}

use windows::Win32::System::Registry::*;

fn reg_set(root: HKEY, sub: &str, name: &str, value: &str) {
    unsafe {
        let mut h = HKEY::default();
        let sub_w = wide(sub);
        if RegCreateKeyExW(
            root,
            PCWSTR(sub_w.as_ptr()),
            0,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE | KEY_WOW64_64KEY,
            None,
            &mut h,
            None,
        )
        .is_err()
        {
            return;
        }
        let name_w = wide(name);
        let val: Vec<u16> = value.encode_utf16().collect();
        let bytes = std::slice::from_raw_parts(val.as_ptr() as *const u8, val.len() * 2);
        let _ = RegSetValueExW(h, PCWSTR(name_w.as_ptr()), 0, REG_SZ, Some(bytes));
        let _ = RegCloseKey(h);
    }
}

fn reg_set_dword(root: HKEY, sub: &str, name: &str, v: u32) {
    unsafe {
        let mut h = HKEY::default();
        let sub_w = wide(sub);
        if RegCreateKeyExW(
            root,
            PCWSTR(sub_w.as_ptr()),
            0,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE | KEY_WOW64_64KEY,
            None,
            &mut h,
            None,
        )
        .is_err()
        {
            return;
        }
        let name_w = wide(name);
        let bytes = std::slice::from_raw_parts(&v as *const u32 as *const u8, 4);
        let _ = RegSetValueExW(h, PCWSTR(name_w.as_ptr()), 0, REG_DWORD, Some(bytes));
        let _ = RegCloseKey(h);
    }
}

fn reg_set_32(root: HKEY, sub: &str, name: &str, value: &str) {
    unsafe {
        let mut h = HKEY::default();
        let sub_w = wide(sub);
        if RegCreateKeyExW(
            root,
            PCWSTR(sub_w.as_ptr()),
            0,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE | KEY_WOW64_32KEY,
            None,
            &mut h,
            None,
        )
        .is_err()
        {
            return;
        }
        let name_w = wide(name);
        let val: Vec<u16> = value.encode_utf16().collect();
        let bytes = std::slice::from_raw_parts(val.as_ptr() as *const u8, val.len() * 2);
        let _ = RegSetValueExW(h, PCWSTR(name_w.as_ptr()), 0, REG_SZ, Some(bytes));
        let _ = RegCloseKey(h);
    }
}

fn reg_del_tree(root: HKEY, sub: &str) {
    unsafe {
        let sub_w = wide(sub);
        let _ = RegDeleteTreeW(root, PCWSTR(sub_w.as_ptr()));
    }
}

fn reg_del_value(root: HKEY, sub: &str, name: &str) {
    unsafe {
        let mut h = HKEY::default();
        let sub_w = wide(sub);
        if RegOpenKeyExW(
            root,
            PCWSTR(sub_w.as_ptr()),
            0,
            KEY_SET_VALUE | KEY_WOW64_64KEY,
            &mut h,
        )
        .is_err()
        {
            return;
        }
        let name_w = wide(name);
        let _ = RegDeleteValueW(h, PCWSTR(name_w.as_ptr()));
        let _ = RegCloseKey(h);
    }
}

fn icacls_grant(dir: &str) {
    // 给打包应用(UWP)读+执行权限，否则新版记事本这类应用加载不了输入法
    let mut cmd = std::process::Command::new("icacls.exe");
    cmd.args([dir, "/grant", "*S-1-15-2-1:(OI)(CI)RX", "/T", "/C", "/Q"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let _ = cmd.status();
}

/// 建快捷方式（开始菜单 + 公共桌面）
unsafe fn make_shortcut(lnk: &str, target: &str, workdir: &str) {
    // 用 WScript.Shell 建快捷方式，省得折腾 COM
    let script = format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}'); $s.TargetPath='{}'; $s.WorkingDirectory='{}'; $s.Description='旁译 · 中文→俄语输入法'; $s.Save()",
        lnk, target, workdir
    );
    let mut cmd = std::process::Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &script]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let _ = cmd.status();
}

unsafe fn shortcuts() {
    // 开始菜单只留【一个】入口，直接打开设置程序。
    // 旧的「旁译 设置.lnk」不再创建。账号和卸载仍走 pangyi.exe 的参数。
    let _ = std::fs::remove_file(plan::LEGACY_SETTINGS_LINK);
    make_shortcut(plan::START_MENU_LINK, &plan::start_menu_target(), DIR);
}

/// 写组件；如果目标文件被正在运行的程序占用，就换个带时间戳的名字写，
/// 再把注册表指过去（否则升级会"装了个寂寞"）。
fn write_or_versioned(dir: &Path, name: &str, data: &[u8]) -> String {
    let p = dir.join(name);
    if std::fs::write(&p, data).is_ok() {
        return p.to_string_lossy().to_string();
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let stem = name.trim_end_matches(".dll");
    let alt = dir.join(format!("{stem}-{stamp}.dll"));
    let _ = std::fs::write(&alt, data);
    alt.to_string_lossy().to_string()
}

/// 删不掉的文件（被运行中的程序占用）排到"重启后删除"。
/// 不这样做的话，旧 DLL 会永远留在安装目录里，"删干净"就成了空话。
fn remove_or_schedule(p: &Path) {
    if std::fs::remove_file(p).is_ok() {
        return;
    }
    use std::os::windows::ffi::OsStrExt;
    let mut w: Vec<u16> = p.as_os_str().encode_wide().collect();
    w.push(0);
    unsafe {
        // MOVEFILE_DELAY_UNTIL_REBOOT = 0x4；第二个参数传 NULL 表示删除
        let _ = windows::Win32::Storage::FileSystem::MoveFileExW(
            windows::core::PCWSTR(w.as_ptr()),
            windows::core::PCWSTR::null(),
            windows::Win32::Storage::FileSystem::MOVE_FILE_FLAGS(0x4),
        );
    }
}

/// 把旧版本的所有痕迹清掉。装新版时调用一次。
/// 1) 安装目录里所有不是本次注册的 pangyi_ime*.dll
/// 2) 用户目录里早期"用户级注册"留下的副本、旧原型 exe、旧日志
/// 3) 指向旧路径的桌面快捷方式
/// 仍被运行中程序占用的文件删不掉，跳过即可（下次安装或重启后会自动清掉）。
fn purge_old_versions(dir: &Path, keep: &[String]) {
    let is_keep = |p: &Path| {
        let s = p.to_string_lossy().to_string();
        keep.iter().any(|k| k.eq_ignore_ascii_case(&s))
    };

    // 1) 安装目录：只留这一轮注册的 DLL、pangyi.exe、更新程序，以及新的设置程序。
    //    pangyi-setup.exe 这类旧 exe 仍然删掉。pangyi-settings.exe 在保留名单里。
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let low = name.to_ascii_lowercase();
            let is_old_dll = low.starts_with("pangyi_ime") && low.ends_with(".dll") && !is_keep(&e.path());
            if is_old_dll || plan::is_obsolete_install_file(&name) {
                remove_or_schedule(&e.path());
            }
        }
    }

    // 2) 用户目录：系统级安装之后，早期用户级副本不留。
    //    这里的 pangyi-settings.exe 是旧的用户目录副本，不是 Program Files 里的新程序。
    if let Ok(base) = std::env::var("LOCALAPPDATA") {
        let user_dir = Path::new(&base).join("Pangyi");
        if let Ok(rd) = std::fs::read_dir(&user_dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if plan::is_stale_user_file(&name) {
                    remove_or_schedule(&e.path());
                }
            }
        }
    }

    // 3) 指向旧路径的桌面快捷方式
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let _ = std::fs::remove_file(
            Path::new(&profile).join("Desktop").join("旁译 · 中俄输入.lnk"),
        );
    }
}

/// 桌面上已知的旧版旁译文件名（精确匹配，只删这些）。
///
/// ⚠️ 绝不能用「旁译」前缀匹配清桌面：用户桌面上可能有「旁译-对话存档」这种
/// 以旁译开头的目录或文件，前缀匹配会误删（对目录还会排队"重启后删除"）。
fn legacy_desktop_names() -> [&'static str; 5] {
    plan::legacy_desktop_names()
}

/// 把"旁译"做成【桌面上的一个软件】。
///
/// 做法：先清掉桌面/公共桌面上清单里的旧旁译文件（安装包、旧快捷方式），
/// 再只放一个「旁译.exe」。这份文件是 pangyi.exe 的副本：版本一致时启动设置程序，
/// 版本不同时走安装。桌面上不另放 pangyi-settings.exe。
fn place_single_app() {
    let me = std::env::current_exe().ok();

    let mut desks: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(profile) = std::env::var("USERPROFILE") {
        desks.push(Path::new(&profile).join("Desktop"));
    }
    desks.push(Path::new(r"C:\Users\Public\Desktop").to_path_buf());

    for (i, d) in desks.iter().enumerate() {
        if !d.is_dir() {
            continue;
        }
        // 只删清单里的旧文件（精确匹配 + 只动文件，目录一律不碰）
        if let Ok(rd) = std::fs::read_dir(d) {
            for e in rd.flatten() {
                let Ok(ft) = e.file_type() else { continue };
                if !ft.is_file() {
                    continue;
                }
                let name = e.file_name().to_string_lossy().to_string();
                if legacy_desktop_names().iter().any(|k| *k == name) {
                    remove_or_schedule(&e.path());
                }
            }
        }
        // 只在【用户桌面】放那个唯一的软件；公共桌面不放（避免出现两份）
        if i == 0 {
            if let Some(m) = me.as_ref() {
                let _ = std::fs::copy(m, d.join(DESKTOP_APP));
            }
        }
    }
}

fn install() {
    let dir = Path::new(DIR);
    if std::fs::create_dir_all(dir).is_err() {
        msg("旁译 安装失败", "无法创建安装目录。");
        return;
    }
    let ico = dir.join(ICO_NAME);
    let _ = std::fs::write(&ico, ICO);
    // 示例里没有地址和密钥。真正的 account.toml 若已存在，不要覆盖。
    let _ = std::fs::write(dir.join(ACCOUNT_EXAMPLE_NAME), ACCOUNT_EXAMPLE);
    // 更新提醒程序：登录时可能正跑着（检查只有几十秒，锁窗口很小），
    // 写失败就稍等重试一次；再失败就保持旧版，下次升级会再试。
    let _ = std::fs::write(dir.join(UPDATER_NAME), UPDATER).or_else(|_| {
        std::thread::sleep(std::time::Duration::from_millis(500));
        std::fs::write(dir.join(UPDATER_NAME), UPDATER)
    });
    // 设置程序写在清旧文件之前。清旧文件会保留这个名字，不再把它当残余删掉。
    let _ = std::fs::write(dir.join(SETTINGS_NAME), SETTINGS_EXE).or_else(|_| {
        std::thread::sleep(std::time::Duration::from_millis(500));
        std::fs::write(dir.join(SETTINGS_NAME), SETTINGS_EXE)
    });
    // 开机自启（当前用户，不需要管理员权限），卸载时删除
    reg_set(
        HKEY_CURRENT_USER,
        RUN_KEY,
        "PangyiUpdater",
        &format!("\"{}\\{}\" --silent", DIR, UPDATER_NAME),
    );
    unsafe { shortcuts(); }

    let d64s = write_or_versioned(dir, DLL64_NAME, DLL64);
    let d32s = write_or_versioned(dir, DLL32_NAME, DLL32);
    let d64 = std::path::PathBuf::from(&d64s);
    let d32 = std::path::PathBuf::from(&d32s);
    // 把自己放进安装目录（这就是"那个软件"），卸载时也用它
    if let Ok(me) = std::env::current_exe() {
        let _ = std::fs::copy(&me, dir.join(APP_NAME));
        let _ = std::fs::remove_file(dir.join("pangyi-setup.exe"));
    }
    // 版本标记：下次再双击时，是"开设置"还是"装新版"就看它
    let _ = std::fs::write(dir.join(VERSION_FILE), env!("CARGO_PKG_VERSION"));
    icacls_grant(DIR);

    let d64s = d64.to_string_lossy().to_string();
    let d32s = d32.to_string_lossy().to_string();
    let icos = ico.to_string_lossy().to_string();

    // 每次装新版都把旧版清干净：只留注册表这一轮指向的两个 DLL。
    // 放在「写完新文件之后」执行，任何时刻注册表指向的文件都在，
    // 不会出现先删旧、后写新中间那个空窗。
    purge_old_versions(dir, &[d64s.clone(), d32s.clone()]);

    // COM 注册（64 位 + 32 位）
    let clsid_key = format!("SOFTWARE\\Classes\\CLSID\\{CLSID_STR}");
    reg_set(HKEY_LOCAL_MACHINE, &clsid_key, "", "旁译");
    reg_set(HKEY_LOCAL_MACHINE, &format!("{clsid_key}\\InprocServer32"), "", &d64s);
    reg_set(HKEY_LOCAL_MACHINE, &format!("{clsid_key}\\InprocServer32"), "ThreadingModel", "Apartment");
    let clsid32 = format!("SOFTWARE\\Classes\\WOW6432Node\\CLSID\\{CLSID_STR}");
    reg_set(HKEY_LOCAL_MACHINE, &clsid32, "", "旁译");
    reg_set_32(HKEY_LOCAL_MACHINE, &format!("SOFTWARE\\Classes\\CLSID\\{CLSID_STR}\\InprocServer32"), "", &d32s);
    reg_set_32(HKEY_LOCAL_MACHINE, &format!("SOFTWARE\\Classes\\CLSID\\{CLSID_STR}\\InprocServer32"), "ThreadingModel", "Apartment");

    // TSF 文本服务
    let tip = format!("SOFTWARE\\Microsoft\\CTF\\TIP\\{CLSID_STR}");
    reg_set_dword(HKEY_LOCAL_MACHINE, &tip, "Enable", 1);
    for c in CATS {
        let sub = format!("{tip}\\Category\\Category\\{c}\\{CLSID_STR}");
        reg_set(HKEY_LOCAL_MACHINE, &sub, "", "");
        let sub2 = format!("{tip}\\Category\\Item\\{CLSID_STR}\\{c}");
        reg_set(HKEY_LOCAL_MACHINE, &sub2, "", "");
    }
    let lp = format!("{tip}\\LanguageProfile\\0x00000804\\{PROFILE_STR}");
    reg_set(HKEY_LOCAL_MACHINE, &lp, "Description", "旁译");
    reg_set(HKEY_LOCAL_MACHINE, &lp, "IconFile", &icos);
    reg_set_dword(HKEY_LOCAL_MACHINE, &lp, "IconIndex", 0);

    // 卸载入口（设置 → 应用）
    let un = format!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Pangyi");
    reg_set(HKEY_LOCAL_MACHINE, &un, "DisplayName", "旁译 中文→俄语输入法");
    reg_set(HKEY_LOCAL_MACHINE, &un, "DisplayVersion", env!("CARGO_PKG_VERSION"));
    reg_set(HKEY_LOCAL_MACHINE, &un, "Publisher", "Pangyi");
    reg_set(HKEY_LOCAL_MACHINE, &un, "InstallLocation", DIR);
    reg_set(HKEY_LOCAL_MACHINE, &un, "DisplayIcon", &icos);
    let inst_dir = Path::new(DIR);
    let app_path = inst_dir.join(APP_NAME);
    let uninstall_cmd = format!("\"{}\" --uninstall", app_path.to_string_lossy());
    reg_set(HKEY_LOCAL_MACHINE, &un, "UninstallString", &uninstall_cmd);
    reg_set_dword(HKEY_LOCAL_MACHINE, &un, "NoModify", 1);
    reg_set_dword(HKEY_LOCAL_MACHINE, &un, "NoRepair", 1);

    // 清掉用户级覆盖（会盖住 HKLM）——64 位和 32 位两处都要清
    reg_del_tree(HKEY_CURRENT_USER, &format!("Software\\Classes\\CLSID\\{CLSID_STR}"));
    reg_del_tree(HKEY_CURRENT_USER, &format!("Software\\Classes\\WOW6432Node\\CLSID\\{CLSID_STR}"));

    place_single_app();

    msg(
        "旁译 安装完成",
        "旁译已安装。\n\n下一步：注销后重新登录（或重启一次），然后在任意程序里按 Win+空格 切到「旁译」即可。\n\n设置：双击桌面上的「旁译」，开始菜单里的「旁译」，或运行 C:\\Program Files\\Pangyi\\pangyi-settings.exe",
    );
}

fn uninstall() {
    let dir = Path::new(DIR);
    let clsid_key = format!("SOFTWARE\\Classes\\CLSID\\{CLSID_STR}");
    reg_del_tree(HKEY_LOCAL_MACHINE, &clsid_key);
    reg_del_tree(HKEY_LOCAL_MACHINE, &format!("SOFTWARE\\Classes\\WOW6432Node\\CLSID\\{CLSID_STR}"));
    reg_del_tree(HKEY_LOCAL_MACHINE, &format!("SOFTWARE\\Microsoft\\CTF\\TIP\\{CLSID_STR}"));
    reg_del_tree(HKEY_LOCAL_MACHINE, "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Pangyi");
    reg_del_tree(HKEY_CURRENT_USER, &format!("Software\\Classes\\CLSID\\{CLSID_STR}"));
    reg_del_value(HKEY_CURRENT_USER, RUN_KEY, "PangyiUpdater");
    for link in plan::uninstall_links() {
        let _ = std::fs::remove_file(link);
    }
    // 桌面上的那个软件也删掉
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let _ = std::fs::remove_file(Path::new(&profile).join("Desktop").join(DESKTOP_APP));
    }
    for f in plan::uninstall_install_dir_files() {
        let _ = std::fs::remove_file(dir.join(f));
    }
    let _ = std::fs::remove_dir(dir);
    msg("旁译 已卸载", "旁译已卸载。注销或重启后，输入法列表里就不会再有它了。");
}

/// 已经装好了吗？（安装目录里有 version.txt 且版本号与当前 exe 一致）
fn installed_current() -> bool {
    let dir = Path::new(DIR);
    let Ok(text) = std::fs::read_to_string(dir.join(VERSION_FILE)) else {
        return false;
    };
    plan::version_matches(
        &text,
        env!("CARGO_PKG_VERSION"),
        dir.join(DLL64_NAME).is_file(),
    )
}

/// 旧 GDI 设置面板。桌面、开始菜单和安装结束都不再把它当作用户入口。
unsafe fn open_legacy_settings() -> ! {
    pangyi_ime::settings::load();
    pangyi_ime::settings::open_app();
    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    std::process::exit(0);
}

fn try_spawn_settings() -> bool {
    let path = plan::settings_exe_path();
    let present = Path::new(&path).is_file();
    let spawned = present && std::process::Command::new(&path).spawn().is_ok();
    !plan::use_legacy_settings(present, spawned)
}

unsafe fn open_settings_or_legacy() -> ! {
    if try_spawn_settings() {
        std::process::exit(0);
    }
    msg(
        "旁译设置",
        "没能打开 pangyi-settings.exe。先打开原来的设置窗口。\n\n可以重新运行一次安装包，把设置程序装回去。",
    );
    open_legacy_settings()
}

fn main() {
    unsafe {
        let args: Vec<String> = std::env::args().collect();
        let entry = plan::route(&args, installed_current());
        // 提权重新启动时带上 --install / --uninstall，避免 UAC 之后把强制重装丢掉。
        if let Some(arg) = plan::elevation_arg(entry) {
            if !elevated() {
                relaunch_elevated(arg);
                return;
            }
        }
        match entry {
            plan::Entry::Account => auth::open_account_window(),
            plan::Entry::LegacySettings => open_legacy_settings(),
            plan::Entry::OpenSettings => open_settings_or_legacy(),
            plan::Entry::Uninstall => uninstall(),
            plan::Entry::Install => {
                install();
                // 装完打开新的设置窗口。文件没写上时退回旧面板。
                open_settings_or_legacy();
            }
        }
    }
}
