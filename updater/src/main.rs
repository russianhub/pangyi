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
#[cfg(windows)]
use windows_sys::Win32::UI::{
    Shell::ShellExecuteW,
    WindowsAndMessaging::{
        MessageBoxW, IDNO, IDYES, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MB_YESNOCANCEL,
        SW_SHOWNORMAL,
    },
};

/// 非 Windows 只为 `cargo test` 提供同名符号。这段不参与 Windows 编译，
/// 正式构建里的弹窗和启动安装包仍直接调用 `windows-sys`。
#[cfg(not(windows))]
use win_stub::{
    MessageBoxW, ShellExecuteW, IDNO, IDYES, MB_ICONERROR, MB_ICONINFORMATION, MB_OK,
    MB_YESNOCANCEL, SW_SHOWNORMAL,
};

#[cfg(not(windows))]
#[allow(non_snake_case)]
mod win_stub {
    // `windows-sys` 的 Win32 绑定只在 Windows 上存在。这里碰一下各平台都有的类型，
    // 非 Windows 的 `cargo test` 才不会把这个依赖报成未使用。Windows 构建不编译本模块。
    #[allow(dead_code)]
    const WINDOWS_SYS_LINK: usize = core::mem::size_of::<windows_sys::core::PCWSTR>();

    pub const IDYES: i32 = 6;
    pub const IDNO: i32 = 7;
    pub const MB_OK: u32 = 0x0000_0000;
    pub const MB_YESNOCANCEL: u32 = 0x0000_0003;
    pub const MB_ICONINFORMATION: u32 = 0x0000_0040;
    pub const MB_ICONERROR: u32 = 0x0000_0010;
    pub const SW_SHOWNORMAL: i32 = 1;

    pub unsafe fn MessageBoxW(
        _hwnd: *mut core::ffi::c_void,
        _text: *const u16,
        _caption: *const u16,
        _flags: u32,
    ) -> i32 {
        0
    }

    pub unsafe fn ShellExecuteW(
        _hwnd: *mut core::ffi::c_void,
        _operation: *const u16,
        _file: *const u16,
        _parameters: *const u16,
        _directory: *const u16,
        _show: i32,
    ) -> isize {
        0
    }
}

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
    verify_manifest_with_key(m, PUBLIC_KEY_B64)
}

fn verify_manifest_with_key(m: &Manifest, pubkey_b64: &str) -> Result<(), String> {
    let eng = base64::engine::general_purpose::STANDARD;
    let pk: [u8; 32] = eng
        .decode(pubkey_b64.trim())
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
    if !installer_size_ok(m.size) {
        return Err("安装包大小异常".into());
    }
    let dir = std::env::temp_dir().join("pangyi-update");
    fs::create_dir_all(&dir).map_err(e2s)?;
    let path = dir.join(format!("pangyi-v{}.exe", m.version));

    let mut last_err = String::from("没有可用的下载地址");
    for url in https_download_urls(&m.url, &m.mirrors) {
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

/// 先主地址 `url`，再按 `mirrors` 的原顺序。不是 `https://` 的地址丢掉。
fn https_download_urls<'a>(url: &'a str, mirrors: &'a [String]) -> Vec<&'a str> {
    std::iter::once(url)
        .chain(mirrors.iter().map(String::as_str))
        .filter(|u| u.starts_with("https://"))
        .collect()
}

fn installer_size_ok(size: u64) -> bool {
    size > 0 && size <= MAX_INSTALLER_BYTES
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

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use ed25519_dalek::Signer;

    /// 确定性测试种子，不是生产私钥，也不能用来签线上清单。
    const TEST_SECRET: [u8; 32] = [7u8; 32];
    const SHA_HEX: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn b64() -> base64::engine::GeneralPurpose {
        base64::engine::general_purpose::STANDARD
    }

    fn test_key() -> ed25519_dalek::SigningKey {
        ed25519_dalek::SigningKey::from_bytes(&TEST_SECRET)
    }

    fn pubkey_b64(sk: &ed25519_dalek::SigningKey) -> String {
        b64().encode(sk.verifying_key().to_bytes())
    }

    fn sample(version: &str, sha256: &str, size: u64, sig: &str) -> Manifest {
        Manifest {
            version: version.to_string(),
            notes: vec!["说明".to_string()],
            url: format!("https://download.eyuxq.com/pangyi-v{version}.exe"),
            mirrors: vec![format!(
                "https://github.com/russianhub/pangyi/releases/download/v{version}/pangyi-v{version}.exe"
            )],
            sha256: sha256.to_string(),
            size,
            sig: sig.to_string(),
        }
    }

    /// 按 pangyi-make-manifest.py 的明文签名，不经过 signed_message，用来核对格式。
    fn sign_canonical(version: &str, sha256: &str, size: u64, sk: &ed25519_dalek::SigningKey) -> String {
        let message = format!("pangyi\n{version}\n{sha256}\n{size}");
        b64().encode(sk.sign(message.as_bytes()).to_bytes())
    }

    fn signed_sample(sha_field: &str) -> (Manifest, String) {
        let sk = test_key();
        let sig = sign_canonical("4.20.0", SHA_HEX, 4096, &sk);
        (sample("4.20.0", sha_field, 4096, &sig), pubkey_b64(&sk))
    }

    #[test]
    fn parse_ver_strips_v_prefix_and_surrounding_space() {
        assert_eq!(parse_ver("4.20.0"), vec![4, 20, 0]);
        assert_eq!(parse_ver("v4.20.0"), vec![4, 20, 0]);
        assert_eq!(parse_ver("  v4.20.0  "), vec![4, 20, 0]);
        assert_eq!(parse_ver("\t4.19.0\n"), vec![4, 19, 0]);
    }

    #[test]
    fn is_newer_upgrade_equal_and_downgrade() {
        assert!(is_newer("4.20.0", "4.19.0"));
        assert!(is_newer("  v4.20.0  ", "4.19.0"));
        assert!(is_newer("4.20.0", "  v4.19.0 "));
        assert!(!is_newer("4.19.0", "4.19.0"));
        assert!(!is_newer("v4.19.0", "4.19.0"));
        assert!(!is_newer("  4.19.0  ", "v4.19.0"));
        assert!(!is_newer("4.19.0", "4.20.0"));
        assert!(!is_newer("v4.18.0", "4.19.0"));
    }

    #[test]
    fn is_newer_numeric_segments_and_uneven_lengths() {
        assert!(is_newer("4.19.0", "4.9.0"), "19 要比 9 新，不能按字符串比");
        assert!(!is_newer("4.9.0", "4.19.0"));
        assert!(is_newer("4.10.0", "4.9.0"));
        assert_eq!(parse_ver("4.19.0"), vec![4, 19, 0]);
        assert_eq!(parse_ver("4.9.0"), vec![4, 9, 0]);

        assert!(!is_newer("4.19", "4.19.0"), "少一段按 0 补齐，与 4.19.0 相等");
        assert!(!is_newer("4.19.0", "4.19"));
        assert!(is_newer("4.19.1", "4.19"));
        assert!(!is_newer("4.19", "4.19.1"));
        assert!(is_newer("4.19.0.1", "4.19.0"));
        assert!(!is_newer("4.19.0", "4.19.0.0"));
    }

    #[test]
    fn parse_ver_stops_at_non_digit_suffix() {
        assert_eq!(parse_ver("4.19.0-rc1"), vec![4, 19, 0]);
        assert_eq!(parse_ver("v4.19.1beta"), vec![4, 19, 1]);
        assert_eq!(parse_ver("4.19.0rc2"), vec![4, 19, 0]);
        assert!(!is_newer("4.19.0-rc1", "4.19.0"));
        assert!(!is_newer("4.19.0", "4.19.0-rc1"));
        assert!(is_newer("4.19.1beta", "4.19"));
        assert!(is_newer("v4.20.0-beta", "4.19.9"));
        assert!(!is_newer("4.19.0-rc1", "4.20.0"));
    }

    #[test]
    fn production_public_key_is_32_byte_verifying_key() {
        assert_eq!(PUBLIC_KEY_B64, "TSxvtZa8K75tkTxpt+EGOTqr+hpcDDBFPeQbaWqO5as=");
        let raw = b64().decode(PUBLIC_KEY_B64.trim()).expect("生产公钥应能 base64 解码");
        assert_eq!(raw.len(), 32);
        let pk: [u8; 32] = raw.try_into().unwrap();
        VerifyingKey::from_bytes(&pk).expect("生产公钥应是有效的 ed25519 VerifyingKey");
        assert_ne!(pk, test_key().verifying_key().to_bytes());
    }

    #[test]
    fn verify_accepts_make_manifest_signature_with_sha_normalization() {
        let messy = format!("  {upper}  ", upper = SHA_HEX.to_ascii_uppercase());
        let (m, pk) = signed_sample(&messy);
        assert_eq!(signed_message(&m), format!("pangyi\n4.20.0\n{SHA_HEX}\n4096"));
        assert!(verify_manifest_with_key(&m, &pk).is_ok());

        let mut spaced = m;
        spaced.sig = format!("  {}\n", spaced.sig);
        assert!(verify_manifest_with_key(&spaced, &pk).is_ok(), "签名首尾空白应被去掉");

        let mut other_url = sample("4.20.0", &messy, 4096, &spaced.sig.trim());
        other_url.url = "https://download.eyuxq.com/moved.exe".into();
        other_url.mirrors.push("http://insecure.example/pangyi.exe".into());
        assert!(
            verify_manifest_with_key(&other_url, &pk).is_ok(),
            "签名不覆盖 url；文件内容仍由 SHA256 约束"
        );
    }

    #[test]
    fn verify_rejects_tampered_version_sha_and_size() {
        let (m, pk) = signed_sample(&format!(" {SHA_HEX} "));
        assert!(verify_manifest_with_key(&m, &pk).is_ok());

        let mut version = sample("4.20.1", &format!(" {SHA_HEX} "), 4096, &m.sig);
        assert_eq!(
            verify_manifest_with_key(&version, &pk).unwrap_err(),
            "更新信息签名无效"
        );
        version.version = "4.19.0".into();
        assert!(verify_manifest_with_key(&version, &pk).is_err());

        let mut sha = m;
        sha.sha256 = SHA_HEX.replacen('0', "1", 1);
        assert_ne!(signed_message(&sample("4.20.0", SHA_HEX, 4096, "")), signed_message(&sha));
        assert_eq!(verify_manifest_with_key(&sha, &pk).unwrap_err(), "更新信息签名无效");

        let mut size = sample("4.20.0", SHA_HEX, 4097, &signed_sample(SHA_HEX).0.sig);
        assert_eq!(verify_manifest_with_key(&size, &pk).unwrap_err(), "更新信息签名无效");
        size.size = 0;
        assert!(verify_manifest_with_key(&size, &pk).is_err());
    }

    #[test]
    fn verify_rejects_flipped_sig_bit() {
        let (m, pk) = signed_sample(SHA_HEX);
        let mut raw = b64().decode(m.sig.trim()).unwrap();
        assert_eq!(raw.len(), 64);
        raw[0] ^= 0b0000_0001;
        let mut bad = m;
        bad.sig = b64().encode(&raw);
        assert_eq!(verify_manifest_with_key(&bad, &pk).unwrap_err(), "更新信息签名无效");
    }

    #[test]
    fn verify_rejects_wrong_public_key() {
        let (m, pk) = signed_sample(SHA_HEX);
        assert!(verify_manifest_with_key(&m, &pk).is_ok());

        let other = ed25519_dalek::SigningKey::from_bytes(&[8u8; 32]);
        assert_eq!(
            verify_manifest_with_key(&m, &pubkey_b64(&other)).unwrap_err(),
            "更新信息签名无效"
        );
        assert!(verify_manifest(&m).is_err(), "测试密钥签的清单不能通过生产公钥");

        let short_key = b64().encode([9u8; 31]);
        assert_eq!(verify_manifest_with_key(&m, &short_key).unwrap_err(), "公钥长度不对");
        let long_key = b64().encode([9u8; 33]);
        assert_eq!(verify_manifest_with_key(&m, &long_key).unwrap_err(), "公钥长度不对");
    }

    #[test]
    fn verify_rejects_invalid_base64_and_wrong_sig_length() {
        let (m, pk) = signed_sample(SHA_HEX);

        let bad_sig = sample(&m.version, &m.sha256, m.size, "@@@@");
        assert!(verify_manifest_with_key(&bad_sig, &pk).is_err());

        assert!(verify_manifest_with_key(&signed_sample(SHA_HEX).0, "@@@@").is_err());
        assert!(verify_manifest_with_key(&signed_sample(SHA_HEX).0, "not base64!!!").is_err());
        assert!(verify_manifest_with_key(&signed_sample(SHA_HEX).0, "").is_err());

        for len in [0usize, 32, 63, 65, 128] {
            let wrong_len = sample("4.20.0", SHA_HEX, 4096, &b64().encode(vec![1u8; len]));
            assert!(
                verify_manifest_with_key(&wrong_len, &pk).is_err(),
                "签名长度 {len} 必须失败"
            );
        }
    }

    #[test]
    fn https_download_urls_try_r2_then_github_mirrors() {
        let primary = "https://download.eyuxq.com/pangyi-v4.20.0.exe";
        let github = "https://github.com/russianhub/pangyi/releases/download/v4.20.0/pangyi-v4.20.0.exe";
        let second = "https://github.com/russianhub/pangyi/releases/download/v4.20.0/pangyi-v4.20.0.exe?fallback=1";
        let mirrors = vec![
            "http://download.eyuxq.com/pangyi-v4.20.0.exe".to_string(),
            github.to_string(),
            String::new(),
            "not-a-url".to_string(),
            "ftp://example.invalid/pangyi.exe".to_string(),
            second.to_string(),
        ];
        assert_eq!(https_download_urls(primary, &mirrors), vec![primary, github, second]);

        let only_mirror = vec![github.to_string()];
        assert_eq!(
            https_download_urls("http://download.eyuxq.com/pangyi-v4.20.0.exe", &only_mirror),
            vec![github]
        );
        assert!(https_download_urls("http://example.invalid/a", &["file:///tmp/a".into()]).is_empty());
    }

    #[test]
    fn installer_size_rejects_zero_and_over_max() {
        assert!(!installer_size_ok(0));
        assert!(installer_size_ok(1));
        assert!(installer_size_ok(MAX_INSTALLER_BYTES));
        assert!(!installer_size_ok(MAX_INSTALLER_BYTES + 1));
    }
}
