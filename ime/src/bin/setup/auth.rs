//! 旁译账号窗口。只编进 setup.exe / pangyi.exe，不编进输入法 DLL。
//!
//! 联网只发生在用户打开这个窗口并登录、刷新或退出时。打字、词库、表情包不经过这里。

use pangyi_account::{
    actions, cooldown_left, decode_session, encode_session, late_result, logout_message, parse_config,
    resend_ready, AccountError, Action, AuthConfig, Client, HttpTransport, Incoming, LateDecision,
    Outgoing, PendingOtp, PhaseKind, Session, TransportFail, PENDING_TTL,
};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Networking::WinHttp::*;
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::WindowsAndMessaging::*;

const MAGIC: &[u8] = b"PYAC1\0";
const WM_ACCOUNT: u32 = WM_APP + 20;

const ID_EMAIL: i32 = 101;
const ID_CODE: i32 = 102;
const ID_SEND: i32 = 103;
const ID_VERIFY: i32 = 104;
const ID_LOGOUT: i32 = 105;
const ID_CANCEL: i32 = 106;
const ID_RESEND: i32 = 107;
const ID_RETRY: i32 = 108;

struct WinHttp;

impl HttpTransport for WinHttp {
    fn post(&self, req: &Outgoing) -> Result<Incoming, TransportFail> {
        winhttp_post(req)
    }
}

fn winhttp_post(req: &Outgoing) -> Result<Incoming, TransportFail> {
    if !req.url.starts_with("https://") || req.url.contains('@') {
        return Err(TransportFail::Tls);
    }
    let (host, port, path) = split_https(&req.url)?;
    let host_w = wide(&host);
    let path_w = wide(&path);
    let agent = wide("PangyiAccount");
    let mut header = String::new();
    for (name, value) in &req.headers {
        header.push_str(name);
        header.push_str(": ");
        header.push_str(value);
        header.push_str("\r\n");
    }
    let headers = wide(&header);
    unsafe {
        let session = WinHttpOpen(
            PCWSTR(agent.as_ptr()),
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        );
        if session.is_null() {
            return Err(TransportFail::Offline);
        }
        let _ = WinHttpSetTimeouts(session, 8_000, 8_000, 10_000, 15_000);
        let connect = WinHttpConnect(session, PCWSTR(host_w.as_ptr()), port, 0);
        if connect.is_null() {
            let _ = WinHttpCloseHandle(session);
            return Err(TransportFail::Offline);
        }
        let request = WinHttpOpenRequest(
            connect,
            w!("POST"),
            PCWSTR(path_w.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        );
        if request.is_null() {
            let _ = WinHttpCloseHandle(connect);
            let _ = WinHttpCloseHandle(session);
            return Err(TransportFail::Offline);
        }
        let disable = WINHTTP_DISABLE_REDIRECTS.to_le_bytes();
        let _ = WinHttpSetOption(
            Some(request as *const std::ffi::c_void),
            WINHTTP_OPTION_DISABLE_FEATURE,
            Some(&disable),
        );
        let header_slice = if headers.len() > 1 { &headers[..headers.len() - 1] } else { &[] };
        let send = if req.body.is_empty() {
            WinHttpSendRequest(request, Some(header_slice), None, 0, 0, 0)
        } else {
            WinHttpSendRequest(
                request,
                Some(header_slice),
                Some(req.body.as_ptr().cast()),
                req.body.len() as u32,
                req.body.len() as u32,
                0,
            )
        };
        let result = (|| -> Result<Incoming, TransportFail> {
            send.map_err(map_win)?;
            WinHttpReceiveResponse(request, std::ptr::null_mut()).map_err(map_win)?;
            let mut status = 0u32;
            let mut len = 4u32;
            WinHttpQueryHeaders(
                request,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                PCWSTR::null(),
                Some((&mut status as *mut u32).cast()),
                &mut len,
                std::ptr::null_mut(),
            )
            .map_err(map_win)?;
            let mut all = Vec::new();
            loop {
                if all.len() >= 65_536 {
                    break;
                }
                let mut available = 0u32;
                WinHttpQueryDataAvailable(request, &mut available).map_err(map_win)?;
                if available == 0 {
                    break;
                }
                let room = 65_536 - all.len();
                let take = (available as usize).min(room);
                let start = all.len();
                all.resize(start + take, 0);
                let mut read = 0u32;
                WinHttpReadData(request, all[start..].as_mut_ptr().cast(), take as u32, &mut read)
                    .map_err(map_win)?;
                all.truncate(start + read as usize);
                if read == 0 {
                    break;
                }
            }
            Ok(Incoming {
                status: status as u16,
                body: String::from_utf8_lossy(&all).into_owned(),
            })
        })();
        let _ = WinHttpCloseHandle(request);
        let _ = WinHttpCloseHandle(connect);
        let _ = WinHttpCloseHandle(session);
        result
    }
}

fn map_win(_: windows::core::Error) -> TransportFail {
    // 证书失败和断网都提示网络不可用，不把 WinHTTP 的原始错误显示出来。
    // http 地址在进 WinHTTP 之前就会被拒绝。
    TransportFail::Offline
}

fn split_https(url: &str) -> Result<(String, u16, String), TransportFail> {
    let rest = url.strip_prefix("https://").ok_or(TransportFail::Tls)?;
    let (hostport, path) = match rest.split_once('/') {
        Some((host, path)) => (host, format!("/{path}")),
        None => (rest, "/".to_string()),
    };
    if hostport.is_empty() || hostport.contains('@') {
        return Err(TransportFail::Tls);
    }
    let (host, port) = match hostport.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
            (host.to_string(), port.parse().unwrap_or(443))
        }
        _ => (hostport.to_string(), 443),
    };
    Ok((host, port, path))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn account_file() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").unwrap_or_else(|| ".".into());
    PathBuf::from(base).join("Pangyi").join("account.dat")
}

fn protect(data: &[u8]) -> Result<Vec<u8>, AccountError> {
    unsafe {
        let input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let entropy_bytes = b"pangyi-account-v1";
        let entropy = CRYPT_INTEGER_BLOB {
            cbData: entropy_bytes.len() as u32,
            pbData: entropy_bytes.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        CryptProtectData(
            &input,
            None,
            Some(&entropy as *const CRYPT_INTEGER_BLOB),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|_| AccountError::Internal("Windows 无法加密登录状态"))?;
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(output.pbData.cast()));
        Ok(bytes)
    }
}

fn unprotect(data: &[u8], with_entropy: bool) -> Result<Vec<u8>, AccountError> {
    unsafe {
        let mut owned = data.to_vec();
        let input = CRYPT_INTEGER_BLOB {
            cbData: owned.len() as u32,
            pbData: owned.as_mut_ptr(),
        };
        let entropy_bytes = b"pangyi-account-v1";
        let entropy = CRYPT_INTEGER_BLOB {
            cbData: entropy_bytes.len() as u32,
            pbData: entropy_bytes.as_ptr() as *mut u8,
        };
        let entropy_ptr = if with_entropy {
            Some(&entropy as *const CRYPT_INTEGER_BLOB)
        } else {
            None
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        CryptUnprotectData(
            &input,
            None,
            entropy_ptr,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|_| AccountError::Internal("无法读取本机登录状态"))?;
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(output.pbData.cast()));
        Ok(bytes)
    }
}

fn save_session_at(path: &Path, session: &Session) -> Result<(), AccountError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|_| AccountError::Internal("无法保存登录状态"))?;
    }
    let mut plain = encode_session(session)?;
    let sealed = match protect(&plain) {
        Ok(s) => s,
        Err(e) => {
            wipe(&mut plain);
            return Err(e);
        }
    };
    wipe(&mut plain);
    if sealed.windows(12).any(|w| w == b"pangyi-sessi") {
        return Err(AccountError::Internal("登录状态没有加密"));
    }
    let mut out = Vec::with_capacity(MAGIC.len() + sealed.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&sealed);
    let tmp = path.with_extension("dat.tmp");
    std::fs::write(&tmp, &out).map_err(|_| AccountError::Internal("无法保存登录状态"))?;
    let _ = std::fs::remove_file(path);
    std::fs::rename(&tmp, path).map_err(|_| AccountError::Internal("无法保存登录状态"))
}

fn load_session_at(path: &Path) -> Result<Option<Session>, AccountError> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(AccountError::Internal("无法读取本机登录状态")),
    };
    if let Some(sealed) = bytes.strip_prefix(MAGIC) {
        let mut plain = unprotect(sealed, true)?;
        let session = decode_session(&plain);
        wipe(&mut plain);
        return session.map(Some);
    }
    // 上一版把邮箱明文写在文件头，刷新令牌仍是 DPAPI 密文。解不开就不显示已登录。
    let text = String::from_utf8_lossy(&bytes);
    let Some((email, blob)) = parse_legacy(&text) else {
        return Err(AccountError::Internal("无法读取本机登录状态"));
    };
    let refresh = String::from_utf8(unprotect(&blob, false)?).map_err(|_| AccountError::Internal("无法读取本机登录状态"))?;
    if refresh.is_empty() || refresh.bytes().any(|b| b == b'\n') {
        return Err(AccountError::Internal("无法读取本机登录状态"));
    }
    Ok(Some(Session {
        email,
        user_id: "legacy".into(),
        access_token: String::new(),
        refresh_token: refresh,
        expires_at_unix: 0,
    }))
}

fn parse_legacy(text: &str) -> Option<(String, Vec<u8>)> {
    let email = text.lines().find_map(|l| l.strip_prefix("email="))?.trim().to_string();
    let hex = text.lines().find_map(|l| l.strip_prefix("refresh="))?.trim();
    if email.is_empty() || email.contains(['\n', '\r']) {
        return None;
    }
    Some((email, unhex(hex)?))
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 || s.is_empty() {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

fn wipe(buf: &mut [u8]) {
    for b in buf {
        *b = 0;
    }
}

fn clear_session_file() {
    let path = account_file();
    if let Ok(len) = std::fs::metadata(&path).map(|m| m.len() as usize) {
        let _ = std::fs::write(&path, vec![0u8; len]);
    }
    let _ = std::fs::remove_file(path);
}

fn load_auth_config() -> Result<Option<AuthConfig>, AccountError> {
    let program_data = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()));
    for path in pangyi_account::candidate_paths(Path::new(&program_data), exe_dir.as_deref()) {
        if path.is_file() {
            let text = std::fs::read_to_string(&path).map_err(|_| AccountError::BadConfig("配置文件无法读取"))?;
            return parse_config(&text).map(Some);
        }
    }
    let url = option_env!("PANGYI_SUPABASE_URL").unwrap_or("");
    let key = option_env!("PANGYI_SUPABASE_PUBLISHABLE_KEY").unwrap_or("");
    if url.is_empty() && key.is_empty() {
        return Ok(None);
    }
    AuthConfig::from_parts(url, key, true).map(Some)
}

struct Model {
    client: Option<Arc<Client<WinHttp>>>,
    phase: PhaseKind,
    session: Option<Session>,
    flow_id: Option<String>,
    sent_at: Option<SystemTime>,
    gen: u64,
    busy: bool,
    status: String,
}

enum DoneKind {
    Send(Result<PendingOtp, AccountError>),
    Verify(Result<Session, AccountError>),
    Refresh(Result<Session, AccountError>),
    Revoke(Result<(), AccountError>),
}

struct Done {
    gen: u64,
    kind: DoneKind,
}

static MODEL: Mutex<Option<Model>> = Mutex::new(None);
static mut HWND_MAIN: HWND = HWND(0);
static mut EMAIL_EDIT: HWND = HWND(0);
static mut CODE_EDIT: HWND = HWND(0);
static mut STATUS: HWND = HWND(0);
static mut BUTTONS: [(i32, HWND); 6] = [
    (ID_SEND, HWND(0)),
    (ID_VERIFY, HWND(0)),
    (ID_RESEND, HWND(0)),
    (ID_CANCEL, HWND(0)),
    (ID_LOGOUT, HWND(0)),
    (ID_RETRY, HWND(0)),
];
static mut CANVAS: HBRUSH = HBRUSH(0);
static mut EDIT_BG: HBRUSH = HBRUSH(0);
static mut FONT: HFONT = HFONT(0);

fn model() -> std::sync::MutexGuard<'static, Option<Model>> {
    MODEL.lock().unwrap_or_else(|e| e.into_inner())
}

fn post_done(done: Done) {
    let raw = Box::into_raw(Box::new(done));
    let ok = unsafe { PostMessageW(HWND_MAIN, WM_ACCOUNT, WPARAM(0), LPARAM(raw as isize)).is_ok() };
    if !ok {
        drop(unsafe { Box::from_raw(raw) });
    }
}

fn spawn_send(email: String, gen: u64) {
    let client = model().as_ref().and_then(|m| m.client.clone());
    let Some(client) = client else { return };
    std::thread::spawn(move || {
        let result = client.send_otp(&email, SystemTime::now());
        post_done(Done { gen, kind: DoneKind::Send(result) });
    });
}

fn spawn_verify(email: String, code: String, flow_id: String, gen: u64) {
    let client = model().as_ref().and_then(|m| m.client.clone());
    let Some(client) = client else { return };
    std::thread::spawn(move || {
        let result = client.verify(&email, &code, &flow_id, SystemTime::now());
        post_done(Done { gen, kind: DoneKind::Verify(result) });
    });
}

fn spawn_refresh(session: Session, gen: u64) {
    let client = model().as_ref().and_then(|m| m.client.clone());
    let Some(client) = client else { return };
    std::thread::spawn(move || {
        let result = client.refresh(&session, SystemTime::now());
        post_done(Done { gen, kind: DoneKind::Refresh(result) });
    });
}

fn spawn_revoke(session: Session, gen: u64) {
    let client = model().as_ref().and_then(|m| m.client.clone());
    let Some(client) = client else {
        post_done(Done { gen, kind: DoneKind::Revoke(Ok(())) });
        return;
    };
    std::thread::spawn(move || {
        let result = if session.access_token.is_empty() {
            Ok(())
        } else {
            client.revoke(&session)
        };
        post_done(Done { gen, kind: DoneKind::Revoke(result) });
    });
}

unsafe fn text_of(h: HWND) -> String {
    let n = GetWindowTextLengthW(h);
    let mut buf = vec![0u16; n as usize + 1];
    let _ = GetWindowTextW(h, &mut buf);
    String::from_utf16_lossy(&buf[..n as usize]).trim().to_string()
}

unsafe fn set_text(h: HWND, s: &str) {
    let v = wide(s);
    let _ = SetWindowTextW(h, PCWSTR(v.as_ptr()));
}

fn begin(model: &mut Model) -> u64 {
    model.gen += 1;
    model.busy = true;
    model.gen
}

unsafe fn sync_window() {
    let guard = model();
    let Some(m) = guard.as_ref() else { return };
    set_text(STATUS, &m.status);
    let ready = m.sent_at.map(|t| resend_ready(t, SystemTime::now())).unwrap_or(true);
    let specs = actions(m.phase, m.busy, ready);
    for (id, hwnd) in BUTTONS {
        if hwnd.0 == 0 {
            continue;
        }
        if let Some(spec) = specs.iter().find(|s| action_id(s.action) == id) {
            let mut label = spec.label.to_string();
            if spec.action == Action::Resend && !ready {
                if let Some(sent) = m.sent_at {
                    let left = cooldown_left(sent, SystemTime::now());
                    if left > 0 {
                        label = format!("请等待 {left} 秒");
                    }
                }
            }
            let _ = ShowWindow(hwnd, SW_SHOW);
            set_text(hwnd, &label);
            let _ = EnableWindow(hwnd, spec.enabled);
        } else {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
    let show_edits = matches!(m.phase, PhaseKind::LoggedOut | PhaseKind::AwaitingCode | PhaseKind::Unconfigured);
    let _ = ShowWindow(EMAIL_EDIT, if show_edits { SW_SHOW } else { SW_HIDE });
    let _ = ShowWindow(CODE_EDIT, if m.phase == PhaseKind::AwaitingCode { SW_SHOW } else { SW_HIDE });
    let _ = EnableWindow(EMAIL_EDIT, !m.busy && m.phase != PhaseKind::Unconfigured);
    let _ = EnableWindow(CODE_EDIT, !m.busy);
}

fn action_id(action: Action) -> i32 {
    match action {
        Action::Send => ID_SEND,
        Action::Verify => ID_VERIFY,
        Action::Resend => ID_RESEND,
        Action::Cancel => ID_CANCEL,
        Action::Logout => ID_LOGOUT,
        Action::Retry => ID_RETRY,
    }
}

fn show_logged_in(m: &mut Model, email: &str) {
    m.phase = PhaseKind::LoggedIn;
    m.busy = false;
    m.flow_id = None;
    m.status = format!("已登录\n{email}\n\n词库、表情和按键仍只在这台电脑上。不上传。");
}

fn apply_done(done: Done) {
    let mut guard = model();
    let Some(m) = guard.as_mut() else { return };
    let decision = match &done.kind {
        DoneKind::Verify(Ok(_)) => late_result(m.gen, done.gen, true),
        _ => late_result(m.gen, done.gen, false),
    };
    if decision != LateDecision::Apply {
        if let DoneKind::Send(Ok(pending)) = &done.kind {
            if let Some(client) = &m.client {
                client.clear_pending_if(&pending.flow_id);
            }
        }
        if let (LateDecision::RevokeSession, DoneKind::Verify(Ok(session))) = (decision, &done.kind) {
            let session = session.clone();
            drop(guard);
            spawn_revoke(session, 0);
        }
        return;
    }
    match done.kind {
        DoneKind::Send(Ok(pending)) => {
            m.flow_id = Some(pending.flow_id);
            m.sent_at = Some(pending.sent_at);
            m.phase = PhaseKind::AwaitingCode;
            m.busy = false;
            m.status = format!(
                "验证码已发到 {}。请查看收件箱和垃圾邮件。{} 分钟内有效。",
                pending.email,
                PENDING_TTL.as_secs() / 60
            );
        }
        DoneKind::Send(Err(e)) => {
            m.busy = false;
            m.status = e.user_message().to_string();
        }
        DoneKind::Verify(Ok(session)) => {
            let email = session.email.clone();
            if save_session_at(&account_file(), &session).is_err() {
                m.busy = false;
                m.status = "验证通过了，但 Windows 没能保存登录状态。请再试一次。没有把它写进普通配置。".into();
                return;
            }
            m.session = Some(session);
            show_logged_in(m, &email);
        }
        DoneKind::Verify(Err(e)) => {
            m.busy = false;
            m.status = e.user_message().to_string();
        }
        DoneKind::Refresh(Ok(session)) => {
            let email = session.email.clone();
            if save_session_at(&account_file(), &session).is_ok() {
                m.session = Some(session);
                show_logged_in(m, &email);
            } else {
                m.busy = false;
                m.phase = PhaseKind::Stale;
                m.status = "登录还在，但没能更新本机保存的会话。".into();
            }
        }
        DoneKind::Refresh(Err(AccountError::SessionExpired)) => {
            clear_session_file();
            m.session = None;
            m.busy = false;
            m.phase = PhaseKind::LoggedOut;
            m.status = AccountError::SessionExpired.user_message().into();
        }
        DoneKind::Refresh(Err(e)) => {
            m.busy = false;
            m.phase = PhaseKind::Stale;
            m.status = format!("{}\n本机仍保留着登录状态，可以重试或退出。", e.user_message());
        }
        DoneKind::Revoke(result) => {
            m.busy = false;
            m.phase = PhaseKind::LoggedOut;
            m.session = None;
            m.status = logout_message(&result).into();
        }
    }
}

unsafe fn on_command(id: i32) {
    if id == ID_CANCEL {
        let mut guard = model();
        let Some(m) = guard.as_mut() else { return };
        m.gen += 1;
        m.busy = false;
        m.flow_id = None;
        m.phase = PhaseKind::LoggedOut;
        m.status = "已取消。".into();
        if let Some(client) = &m.client {
            client.clear_pending();
        }
        drop(guard);
        sync_window();
        return;
    }
    if id == ID_LOGOUT {
        let mut guard = model();
        let Some(m) = guard.as_mut() else { return };
        let session = m.session.take();
        clear_session_file();
        m.flow_id = None;
        let gen = begin(m);
        m.status = "正在退出…".into();
        drop(guard);
        sync_window();
        if let Some(session) = session {
            spawn_revoke(session, gen);
        } else {
            let mut guard = model();
            if let Some(m) = guard.as_mut() {
                if m.gen == gen {
                    m.busy = false;
                    m.phase = PhaseKind::LoggedOut;
                    m.status = "已退出登录。".into();
                }
            }
            drop(guard);
            sync_window();
        }
        return;
    }
    let mut guard = model();
    let Some(m) = guard.as_mut() else { return };
    if m.client.is_none() {
        m.status = m.status.clone();
        if m.phase == PhaseKind::Unconfigured {
            drop(guard);
            sync_window();
            return;
        }
    }
    if m.busy && id != ID_CANCEL {
        m.status = AccountError::Busy.user_message().into();
        drop(guard);
        sync_window();
        return;
    }
    if id == ID_RETRY {
        let session = m.session.clone();
        let gen = begin(m);
        m.status = "正在重新连接…".into();
        drop(guard);
        sync_window();
        if let Some(session) = session {
            spawn_refresh(session, gen);
        }
        return;
    }
    let email = text_of(EMAIL_EDIT);
    if id == ID_SEND || id == ID_RESEND {
        if let Some(sent) = m.sent_at {
            if !resend_ready(sent, SystemTime::now()) {
                let left = cooldown_left(sent, SystemTime::now());
                m.status = format!("请等待 {left} 秒后再发送。");
                drop(guard);
                sync_window();
                return;
            }
        }
        if pangyi_account::normalize_email(&email).is_err() {
            m.status = AccountError::InvalidEmail.user_message().into();
            drop(guard);
            sync_window();
            return;
        }
        let gen = begin(m);
        m.status = "正在发送验证码…".into();
        drop(guard);
        sync_window();
        spawn_send(email, gen);
        return;
    }
    if id == ID_VERIFY {
        let code = text_of(CODE_EDIT);
        let Some(flow_id) = m.flow_id.clone() else {
            m.status = AccountError::NoPending.user_message().into();
            drop(guard);
            sync_window();
            return;
        };
        if pangyi_account::normalize_code(&code).is_err() {
            m.status = AccountError::InvalidCode.user_message().into();
            drop(guard);
            sync_window();
            return;
        }
        let gen = begin(m);
        m.status = "正在验证…".into();
        drop(guard);
        sync_window();
        spawn_verify(email, code, flow_id, gen);
    }
}

fn startup(client: Option<Arc<Client<WinHttp>>>, config_message: Option<String>) -> Model {
    if client.is_none() {
        return Model {
            client: None,
            phase: PhaseKind::Unconfigured,
            session: None,
            flow_id: None,
            sent_at: None,
            gen: 1,
            busy: false,
            status: config_message.unwrap_or_else(|| AccountError::NotConfigured.user_message().into()),
        };
    }
    match load_session_at(&account_file()) {
        Ok(Some(session)) if session.access_usable(SystemTime::now()) || session.expires_at_unix == 0 && !session.refresh_token.is_empty() && session.access_token.is_empty() => {
            let email = session.email.clone();
            let mut m = Model {
                client,
                phase: PhaseKind::LoggedIn,
                session: Some(session),
                flow_id: None,
                sent_at: None,
                gen: 1,
                busy: false,
                status: String::new(),
            };
            // 旧文件只有刷新令牌。有配置时启动后刷新一次，没有可用访问令牌就不把它当成新会话。
            if m.session.as_ref().is_some_and(|s| s.access_token.is_empty()) {
                m.phase = PhaseKind::Stale;
                m.busy = true;
                m.status = "正在确认登录状态…".into();
            } else {
                show_logged_in(&mut m, &email);
            }
            m
        }
        Ok(Some(session)) => Model {
            client,
            phase: PhaseKind::Stale,
            session: Some(session),
            flow_id: None,
            sent_at: None,
            gen: 1,
            busy: true,
            status: "登录已过期，正在重新连接…".into(),
        },
        Ok(None) => Model {
            client,
            phase: PhaseKind::LoggedOut,
            session: None,
            flow_id: None,
            sent_at: None,
            gen: 1,
            busy: false,
            status: "输入邮箱，向它发送一次性验证码。不设置密码。不登录也可以打字。".into(),
        },
        Err(e) => Model {
            client,
            phase: PhaseKind::Stale,
            session: None,
            flow_id: None,
            sent_at: None,
            gen: 1,
            busy: false,
            status: format!("{}\n可以退出后重新登录。", e.user_message()),
        },
    }
}

unsafe fn child(class: PCWSTR, text: &str, style: WINDOW_STYLE, x: i32, y: i32, w: i32, h: i32, parent: HWND, id: i32) -> HWND {
    let t = wide(text);
    CreateWindowExW(
        WINDOW_EX_STYLE(0),
        class,
        PCWSTR(t.as_ptr()),
        style | WS_CHILD,
        x,
        y,
        w,
        h,
        parent,
        HMENU(id as isize),
        GetModuleHandleW(None).unwrap(),
        None,
    )
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16))
}

unsafe extern "system" fn proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            HWND_MAIN = h;
            CANVAS = CreateSolidBrush(rgb(0xED, 0xF0, 0xF3));
            EDIT_BG = CreateSolidBrush(rgb(0xFF, 0xFF, 0xFF));
            FONT = CreateFontW(
                -16, 0, 0, 0, 400, 0, 0, 0,
                DEFAULT_CHARSET.0 as u32, OUT_TT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32,
                CLEARTYPE_QUALITY.0 as u32, (VARIABLE_PITCH.0 | FF_DONTCARE.0) as u32,
                w!("Microsoft YaHei UI"),
            );
            let title = child(w!("STATIC"), "旁译账号", WS_VISIBLE, 24, 16, 360, 28, h, 0);
            let hint = child(
                w!("STATIC"),
                "不登录也可以打字、用词库和表情包。只有这个窗口在你登录时才会联网。",
                WS_VISIBLE,
                24,
                48,
                360,
                40,
                h,
                0,
            );
            EMAIL_EDIT = child(w!("EDIT"), "", WS_VISIBLE | WS_BORDER | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32), 24, 100, 360, 28, h, ID_EMAIL);
            CODE_EDIT = child(w!("EDIT"), "", WS_BORDER | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32), 24, 140, 160, 28, h, ID_CODE);
            STATUS = child(w!("STATIC"), "", WS_VISIBLE, 24, 230, 360, 96, h, 0);
            let privacy = child(
                w!("STATIC"),
                "不会上传按键、候选、词库或用户文件。没有云同步。",
                WS_VISIBLE,
                24,
                340,
                360,
                36,
                h,
                0,
            );
            BUTTONS[0].1 = child(w!("BUTTON"), "发送验证码", WS_TABSTOP, 24, 184, 140, 32, h, ID_SEND);
            BUTTONS[1].1 = child(w!("BUTTON"), "登录", WS_TABSTOP, 24, 184, 100, 32, h, ID_VERIFY);
            BUTTONS[2].1 = child(w!("BUTTON"), "重新发送", WS_TABSTOP, 136, 184, 140, 32, h, ID_RESEND);
            BUTTONS[3].1 = child(w!("BUTTON"), "取消", WS_TABSTOP, 288, 184, 96, 32, h, ID_CANCEL);
            BUTTONS[4].1 = child(w!("BUTTON"), "退出登录", WS_TABSTOP, 24, 184, 120, 32, h, ID_LOGOUT);
            BUTTONS[5].1 = child(w!("BUTTON"), "重试", WS_TABSTOP, 160, 184, 88, 32, h, ID_RETRY);
            for hwnd in [title, hint, EMAIL_EDIT, CODE_EDIT, STATUS, privacy] {
                let _ = SendMessageW(hwnd, WM_SETFONT, WPARAM(FONT.0 as usize), LPARAM(1));
            }
            for (_, hwnd) in BUTTONS {
                let _ = SendMessageW(hwnd, WM_SETFONT, WPARAM(FONT.0 as usize), LPARAM(1));
            }
            let _ = SendMessageW(EMAIL_EDIT, 0x00C5, WPARAM(320), LPARAM(0));
            let _ = SendMessageW(CODE_EDIT, 0x00C5, WPARAM(10), LPARAM(0));
            let email_hint = wide("邮箱地址");
            let code_hint = wide("验证码");
            let _ = SendMessageW(EMAIL_EDIT, 0x1501, WPARAM(1), LPARAM(email_hint.as_ptr() as isize));
            let _ = SendMessageW(CODE_EDIT, 0x1501, WPARAM(1), LPARAM(code_hint.as_ptr() as isize));
            let _ = SetTimer(h, 1, 1000, None);
            sync_window();
            // 需要刷新的会话在窗口出来之后再联网，避免卡住创建过程。
            let refresh = {
                let guard = model();
                guard.as_ref().and_then(|m| {
                    if m.busy && m.phase == PhaseKind::Stale {
                        m.session.clone().map(|s| (s, m.gen))
                    } else {
                        None
                    }
                })
            };
            if let Some((session, gen)) = refresh {
                spawn_refresh(session, gen);
            }
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC => {
            let dc = HDC(wp.0 as isize);
            SetBkMode(dc, TRANSPARENT);
            let _ = SetTextColor(dc, rgb(0x1D, 0x1D, 0x1F));
            LRESULT(CANVAS.0)
        }
        WM_CTLCOLOREDIT => {
            let dc = HDC(wp.0 as isize);
            let _ = SetBkColor(dc, rgb(0xFF, 0xFF, 0xFF));
            let _ = SetTextColor(dc, rgb(0x1D, 0x1D, 0x1F));
            LRESULT(EDIT_BG.0)
        }
        WM_ERASEBKGND => {
            let mut rc = RECT::default();
            let _ = GetClientRect(h, &mut rc);
            FillRect(HDC(wp.0 as isize), &rc, CANVAS);
            LRESULT(1)
        }
        WM_COMMAND => {
            on_command((wp.0 & 0xffff) as i32);
            LRESULT(0)
        }
        WM_TIMER => {
            sync_window();
            LRESULT(0)
        }
        WM_ACCOUNT => {
            if lp.0 == 0 {
                return LRESULT(0);
            }
            let done = Box::from_raw(lp.0 as *mut Done);
            apply_done(*done);
            sync_window();
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(h);
            LRESULT(0)
        }
        WM_DESTROY => {
            let mut guard = model();
            if let Some(m) = guard.as_mut() {
                m.gen += 1;
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(h, msg, wp, lp),
    }
}

pub unsafe fn open_account_window() {
    let _ = SetProcessDPIAware();
    let hinst = GetModuleHandleW(None).unwrap();
    let cls = wide("PangyiAccount");
    let existing = FindWindowW(PCWSTR(cls.as_ptr()), PCWSTR::null());
    if existing.0 != 0 {
        let _ = ShowWindow(existing, SW_RESTORE);
        let _ = SetForegroundWindow(existing);
        return;
    }
    let (client, message) = match load_auth_config() {
        Ok(Some(cfg)) => (Some(Arc::new(Client::new(cfg, WinHttp))), None),
        Ok(None) => (None, None),
        Err(e) => (None, Some(e.user_message().to_string())),
    };
    *model() = Some(startup(client, message));
    let wc = WNDCLASSW {
        lpfnWndProc: Some(proc),
        lpszClassName: PCWSTR(cls.as_ptr()),
        hInstance: hinst.into(),
        hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
        hbrBackground: CreateSolidBrush(rgb(0xED, 0xF0, 0xF3)),
        ..Default::default()
    };
    RegisterClassW(&wc);
    let title = wide("旁译账号");
    HWND_MAIN = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        PCWSTR(cls.as_ptr()),
        PCWSTR(title.as_ptr()),
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        430,
        460,
        None,
        None,
        hinst,
        None,
    );
    let _ = ShowWindow(HWND_MAIN, SW_SHOW);
    let mut message = MSG::default();
    while GetMessageW(&mut message, None, 0, 0).as_bool() {
        let _ = TranslateMessage(&message);
        DispatchMessageW(&message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_parser_and_hex() {
        assert!(parse_legacy("email=user@example.com\nrefresh=zz\n").is_none());
        let (email, blob) = parse_legacy("email=user@example.com\nrefresh=0a0b\n").unwrap();
        assert_eq!(email, "user@example.com");
        assert_eq!(blob, vec![0x0a, 0x0b]);
        assert!(unhex("abc").is_none());
    }

    #[test]
    fn dpapi_file_does_not_contain_the_token() {
        let dir = std::env::temp_dir().join(format!(
            "pangyi-auth-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("account.dat");
        let session = Session {
            email: "user@example.com".into(),
            user_id: "user-id-1".into(),
            access_token: "access-token-value-dpapi".into(),
            refresh_token: "refresh-token-value-dpapi".into(),
            expires_at_unix: 1_900_000_000,
        };
        save_session_at(&path, &session).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(MAGIC));
        assert!(bytes.windows(session.access_token.len()).all(|w| w != session.access_token.as_bytes()));
        assert!(bytes.windows(session.refresh_token.len()).all(|w| w != session.refresh_token.as_bytes()));
        let loaded = load_session_at(&path).unwrap().unwrap();
        assert_eq!(loaded.access_token, session.access_token);
        assert_eq!(loaded.email, session.email);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
