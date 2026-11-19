//! 旁译更新提醒程序（独立于输入法 DLL，输入法本体始终不联网）。
//!
//! 用法：
//!   pangyi-updater.exe --silent   开机/登录时运行：每 24 小时最多检查一次，有新版才弹窗
//!   pangyi-updater.exe --manual   设置面板「检查更新」按钮：无视间隔和“跳过”，结果都会提示
//!   pangyi-updater.exe --disable  关闭自动检查
//!   pangyi-updater.exe --enable   重新开启自动检查
#![windows_subsystem = "windows"]

use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::UI::{
    Shell::ShellExecuteW,
    WindowsAndMessaging::{
        MessageBoxW, IDNO, IDYES, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MB_YESNOCANCEL,
        SW_SHOWNORMAL,
    },
};

// ===== 部署相关常量 =====
const MANIFEST_URL: &str = "https://eyuxq.com/latest.json";
/// ed25519 公钥（base64，32 字节）。与发布工作流里的私钥 PANGYI_SIGN_KEY 配对。
const PUBLIC_KEY_B64: &str = "TSxvtZa8K75tkTxpt+EGOTqr+hpcDDBFPeQbaWqO5as=";
// =====================

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const CHECK_INTERVAL_SECS: u64 = 24 * 3600;
const MAX_INSTALLER_BYTES: u64 = 500 * 1024 * 1024;

#[derive(Deserialize)]
struct Manifest {
    version: String,
    #[serde(default)]
    notes: Vec<String>,
    url: String,
    #[serde(default)]
    mirrors: Vec<String>,
    sha256: String,
    size: u64,
    sig: String,
}

#[derive(Serialize, Deserialize)]
struct Config {
    enabled: bool,
    skipped: String,
    last_check: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config { enabled: true, skipped: String::new(), last_check: 0 }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    let manual = has("--manual");

    if has("--disable") {
        set_enabled(false);
        return;
    }
    if has("--enable") {
        set_enabled(true);
        return;
    }

    let mut cfg = load_config();
    if !manual {
        if !cfg.enabled {
            return;
        }
        // 开机后稍等，避免拖慢登录
        std::thread::sleep(Duration::from_secs(60));
        if now() < cfg.last_check + CHECK_INTERVAL_SECS {
            return;
        }
    }

    if let Err(e) = run(manual, &mut cfg) {
        if manual {
            error(&format!("检查更新失败：{e}"));
        }
        // 静默模式下（断网等）直接退出，下次登录再试
    }
}

fn run(manual: bool, cfg: &mut Config) -> Result<(), String> {
    let m = fetch_manifest()?;
    cfg.last_check = now();
    save_config(cfg);

    verify_manifest(&m)?; // 签名不对就当没有更新

    if !is_newer(&m.version, CURRENT_VERSION) {
        if manual {
            info(&format!("已是最新版本（v{CURRENT_VERSION}）。"));
        }
        return Ok(());
    }
    if !manual && cfg.skipped == m.version {
        return Ok(());
    }

    let notes: Vec<String> = m.notes.iter().take(8).map(|n| format!("· {n}")).collect();
    let text = format!(
        "旁译有新版本 v{}（当前 v{}）。\n\n{}\n\n是：现在下载并安装\n否：跳过此版本\n取消：以后再说",
        m.version,
        CURRENT_VERSION,
        notes.join("\n")
    );
    let r = msg(&text, "旁译更新", MB_YESNOCANCEL | MB_ICONINFORMATION);
    if r == IDYES {
        let path = download(&m)?;
        launch(&path)?;
    } else if r == IDNO {
        cfg.skipped = m.version.clone();
        save_config(cfg);
    }
    Ok(())
}

// ---------- 网络与校验 ----------

fn agent(read_secs: u64) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(read_secs))
        .user_agent(&format!("pangyi-updater/{CURRENT_VERSION}"))
        .build()
}

fn fetch_manifest() -> Result<Manifest, String> {
    agent(30)
        .get(MANIFEST_URL)
        .call()
        .map_err(e2s)?
        .into_json::<Manifest>()
        .map_err(e2s)
}

/// 签名内容必须与 tools/make_manifest.py 完全一致。
fn signed_message(m: &Manifest) -> String {
    format!("pangyi\n{}\n{}\n{}", m.version, m.sha256.trim().to_lowercase(), m.size)
}

fn verify_manifest(m: &Manifest) -> Result<(), String> {
    let eng = base64::engine::general_purpose::STANDARD;
    let pk: [u8; 32] = eng
        .decode(PUBLIC_KEY_B64.trim())
        .map_err(e2s)?
        .try_into()
        .map_err(|_| "公钥长度不对".to_string())?;
    let vk = VerifyingKey::from_bytes(&pk).map_err(e2s)?;
    let sig_bytes = eng.decode(m.sig.trim()).map_err(e2s)?;
    let sig = Signature::from_slice(&sig_bytes).map_err(e2s)?;
    vk.verify_strict(signed_message(m).as_bytes(), &sig)
        .map_err(|_| "更新信息签名无效".to_string())
}

fn download(m: &Manifest) -> Result<PathBuf, String> {
    if m.size == 0 || m.size > MAX_INSTALLER_BYTES {
        return Err("安装包大小异常".into());
    }
    let dir = std::env::temp_dir().join("pangyi-update");
    fs::create_dir_all(&dir).map_err(e2s)?;
    let path = dir.join(format!("pangyi-v{}.exe", m.version));

    let mut last_err = String::from("没有可用的下载地址");
    for url in std::iter::once(&m.url).chain(m.mirrors.iter()) {
        if !url.starts_with("https://") {
            continue;
        }
        match try_download(url, &path, m) {
            Ok(()) => return Ok(path),
            Err(e) => {
                last_err = e;
                let _ = fs::remove_file(&path);
            }
        }
    }
    Err(last_err)
}

fn try_download(url: &str, path: &PathBuf, m: &Manifest) -> Result<(), String> {
    let resp = agent(60).get(url).call().map_err(e2s)?;
    let mut reader = resp.into_reader().take(MAX_INSTALLER_BYTES + 1);
    let mut file = fs::File::create(path).map_err(e2s)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = reader.read(&mut buf).map_err(e2s)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n]).map_err(e2s)?;
        total += n as u64;
    }
    file.flush().map_err(e2s)?;
    drop(file);

    if total != m.size {
        return Err("下载大小与更新信息不符".into());
    }
    let got = format!("{:x}", hasher.finalize());
    if !got.eq_ignore_ascii_case(m.sha256.trim()) {
        return Err("安装包校验失败（SHA256 不符）".into());
    }
    Ok(())
}

fn launch(path: &PathBuf) -> Result<(), String> {
    let file = wide(path.to_str().ok_or("路径无效")?);
    let op = wide("open");
    let r = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            op.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if (r as usize) <= 32 {
        return Err("无法启动安装程序".into());
    }
    Ok(())
}

// ---------- 版本比较 ----------

fn parse_ver(v: &str) -> Vec<u64> {
    v.trim()
        .trim_start_matches('v')
        .split('.')
        .map(|p| {
            p.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .unwrap_or(0)
        })
        .collect()
}

fn is_newer(a: &str, b: &str) -> bool {
    let (mut x, mut y) = (parse_ver(a), parse_ver(b));
    let n = x.len().max(y.len());
    x.resize(n, 0);
    y.resize(n, 0);
    x > y
}

// ---------- 配置文件：%LOCALAPPDATA%\Pangyi\updater.json ----------

fn config_path() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("Pangyi").join("updater.json")
}

fn load_config() -> Config {
    fs::read_to_string(config_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_config(cfg: &Config) {
    let p = config_path();
    if let Some(dir) = p.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(s) = serde_json::to_string_pretty(cfg) {
        let _ = fs::write(p, s);
    }
}

fn set_enabled(v: bool) {
    let mut cfg = load_config();
    cfg.enabled = v;
    save_config(&cfg);
}

// ---------- 小工具 ----------

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn e2s<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn msg(text: &str, title: &str, flags: u32) -> i32 {
    let t = wide(text);
    let c = wide(title);
    unsafe { MessageBoxW(std::ptr::null_mut(), t.as_ptr(), c.as_ptr(), flags) }
}

fn info(text: &str) {
    let _ = msg(text, "旁译更新", MB_OK | MB_ICONINFORMATION);
}

fn error(text: &str) {
    let _ = msg(text, "旁译更新", MB_OK | MB_ICONERROR);
}
