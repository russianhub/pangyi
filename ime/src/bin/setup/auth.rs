//! 旁译账号窗口：Supabase 邮箱 OTP 登录。
//!
//! 这个模块只由 setup.exe 编译，TSF DLL 不引用它，因此打字过程仍然完全离线。
//! Supabase URL 和 publishable/anon key 通过构建环境变量注入；它们本来就是公开
//! 客户端配置，绝不能在这里使用 service_role key。

use serde::Deserialize;
use std::path::PathBuf;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Networking::WinHttp::*;
use windows::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

const SUPABASE_URL: &str = match option_env!("PANGYI_SUPABASE_URL") { Some(v) => v, None => "" };
const SUPABASE_KEY: &str = match option_env!("PANGYI_SUPABASE_PUBLISHABLE_KEY") { Some(v) => v, None => "" };

const ID_EMAIL: i32 = 101;
const ID_CODE: i32 = 102;
const ID_SEND: i32 = 103;
const ID_VERIFY: i32 = 104;
const ID_LOGOUT: i32 = 105;

static mut EMAIL_EDIT: HWND = HWND(0);
static mut CODE_EDIT: HWND = HWND(0);
static mut SEND_BUTTON: HWND = HWND(0);
static mut VERIFY_BUTTON: HWND = HWND(0);
static mut LOGOUT_BUTTON: HWND = HWND(0);
static mut STATUS: HWND = HWND(0);

#[derive(Deserialize)]
struct User { email: Option<String> }

#[derive(Deserialize)]
struct AuthResponse {
    refresh_token: String,
    user: User,
}

#[derive(Deserialize)]
struct AuthError {
    message: Option<String>,
    msg: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

impl AuthError {
    fn text(self) -> Option<String> {
        self.message.or(self.msg).or(self.error_description).or(self.error)
    }
}

fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }

fn account_file() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").unwrap_or_else(|| ".".into());
    PathBuf::from(base).join("Pangyi").join("account.dat")
}

fn hex(bytes: &[u8]) -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() }

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 { return None; }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

fn protect(data: &[u8]) -> Result<Vec<u8>, String> {
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut output = CRYPT_INTEGER_BLOB::default();
        CryptProtectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output)
            .map_err(|e| format!("Windows 无法加密登录状态：{e}"))?;
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(output.pbData.cast()));
        Ok(bytes)
    }
}

fn unprotect(data: &mut [u8]) -> Option<Vec<u8>> {
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_mut_ptr() };
        let mut output = CRYPT_INTEGER_BLOB::default();
        CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output).ok()?;
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(output.pbData.cast()));
        Some(bytes)
    }
}

fn save_session(email: &str, refresh_token: &str) -> Result<(), String> {
    let path = account_file();
    if let Some(dir) = path.parent() { std::fs::create_dir_all(dir).map_err(|e| e.to_string())?; }
    let encrypted = protect(refresh_token.as_bytes())?;
    let safe_email = email.replace(['\r', '\n'], "");
    std::fs::write(path, format!("email={safe_email}\nrefresh={}\n", hex(&encrypted))).map_err(|e| e.to_string())
}

fn saved_email() -> Option<String> {
    let text = std::fs::read_to_string(account_file()).ok()?;
    // 同时验证 DPAPI 密文仍可由当前 Windows 用户解开；坏文件不显示为已登录。
    let email = text.lines().find_map(|l| l.strip_prefix("email="))?.to_string();
    let mut blob = unhex(text.lines().find_map(|l| l.strip_prefix("refresh="))?)?;
    let _ = unprotect(&mut blob)?;
    Some(email)
}

fn configured() -> bool { SUPABASE_URL.starts_with("https://") && !SUPABASE_KEY.is_empty() }

fn error_text(body: &str) -> String {
    serde_json::from_str::<AuthError>(body).ok()
        .and_then(AuthError::text)
        .unwrap_or_else(|| "登录服务暂时不可用，请稍后再试。".into())
}

/// HTTPS POST via Windows WinHTTP/Schannel. No third-party TLS runtime is shipped.
fn post_json(path: &str, json: &str) -> Result<String, String> {
    let rest = SUPABASE_URL.strip_prefix("https://").ok_or("登录服务地址不是 HTTPS。")?;
    let (host, base_path) = match rest.split_once('/') {
        Some((h, p)) => (h, format!("/{p}")),
        None => (rest, String::new()),
    };
    let object = format!("{}{path}", base_path.trim_end_matches('/'));
    let agent = wide("Pangyi/4");
    let host_w = wide(host);
    let object_w = wide(&object);
    let headers = wide(&format!(
        "Content-Type: application/json\r\napikey: {SUPABASE_KEY}\r\nAuthorization: Bearer {SUPABASE_KEY}\r\n"
    ));
    unsafe {
        let session = WinHttpOpen(PCWSTR(agent.as_ptr()), WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            PCWSTR::null(), PCWSTR::null(), 0);
        if session.is_null() { return Err("无法启动 Windows 网络服务。".into()); }
        let connect = WinHttpConnect(session, PCWSTR(host_w.as_ptr()), 443, 0);
        if connect.is_null() { let _ = WinHttpCloseHandle(session); return Err("无法连接登录服务器。".into()); }
        let request = WinHttpOpenRequest(connect, w!("POST"), PCWSTR(object_w.as_ptr()),
            PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE);
        if request.is_null() {
            let _ = WinHttpCloseHandle(connect); let _ = WinHttpCloseHandle(session);
            return Err("无法创建登录请求。".into());
        }
        let result = (|| -> windows::core::Result<String> {
            WinHttpSendRequest(request, Some(&headers[..headers.len() - 1]),
                Some(json.as_bytes().as_ptr().cast()), json.len() as u32, json.len() as u32, 0)?;
            WinHttpReceiveResponse(request, std::ptr::null_mut())?;
            let mut all = Vec::new();
            loop {
                let mut available = 0u32;
                WinHttpQueryDataAvailable(request, &mut available)?;
                if available == 0 { break; }
                let start = all.len();
                all.resize(start + available as usize, 0);
                let mut read = 0u32;
                WinHttpReadData(request, all[start..].as_mut_ptr().cast(), available, &mut read)?;
                all.truncate(start + read as usize);
                if read == 0 { break; }
            }
            Ok(String::from_utf8_lossy(&all).into_owned())
        })();
        let _ = WinHttpCloseHandle(request); let _ = WinHttpCloseHandle(connect); let _ = WinHttpCloseHandle(session);
        result.map_err(|e| format!("无法连接登录服务：{e}"))
    }
}

fn send_otp(email: &str) -> Result<(), String> {
    let body = serde_json::json!({"email": email, "create_user": true}).to_string();
    let response = post_json("/auth/v1/otp", &body)?;
    if serde_json::from_str::<AuthError>(&response).ok().and_then(AuthError::text).is_some() {
        Err(error_text(&response))
    } else { Ok(()) }
}

fn verify_otp(email: &str, token: &str) -> Result<String, String> {
    let body = serde_json::json!({"email": email, "token": token, "type": "email"}).to_string();
    let response = post_json("/auth/v1/verify", &body)?;
    let auth: AuthResponse = serde_json::from_str(&response).map_err(|_| error_text(&response))?;
    let actual_email = auth.user.email.unwrap_or_else(|| email.to_string());
    save_session(&actual_email, &auth.refresh_token)?;
    Ok(actual_email)
}

unsafe fn text_of(h: HWND) -> String {
    let n = GetWindowTextLengthW(h);
    let mut buf = vec![0u16; n as usize + 1];
    GetWindowTextW(h, &mut buf);
    String::from_utf16_lossy(&buf[..n as usize]).trim().to_string()
}

unsafe fn set_text(h: HWND, s: &str) { let v = wide(s); let _ = SetWindowTextW(h, PCWSTR(v.as_ptr())); }

unsafe fn show_logged_in(email: &str) {
    let _ = ShowWindow(EMAIL_EDIT, SW_HIDE); let _ = ShowWindow(CODE_EDIT, SW_HIDE);
    let _ = ShowWindow(SEND_BUTTON, SW_HIDE); let _ = ShowWindow(VERIFY_BUTTON, SW_HIDE);
    let _ = ShowWindow(LOGOUT_BUTTON, SW_SHOW);
    set_text(STATUS, &format!("已登录\n{email}\n\n你的词库仍只保存在本机。"));
}

unsafe fn show_logged_out() {
    let _ = ShowWindow(EMAIL_EDIT, SW_SHOW); let _ = ShowWindow(CODE_EDIT, SW_SHOW);
    let _ = ShowWindow(SEND_BUTTON, SW_SHOW); let _ = ShowWindow(VERIFY_BUTTON, SW_SHOW);
    let _ = ShowWindow(LOGOUT_BUTTON, SW_HIDE);
    set_text(STATUS, if configured() { "输入邮箱，获取 6 位验证码。首次登录会自动创建账号。" } else { "登录服务尚未配置。请使用带 Supabase 配置构建的正式版。" });
}

unsafe fn child(class: PCWSTR, text: &str, style: WINDOW_STYLE, x: i32, y: i32, w_: i32, h_: i32, parent: HWND, id: i32) -> HWND {
    let t = wide(text);
    CreateWindowExW(WINDOW_EX_STYLE(0), class, PCWSTR(t.as_ptr()), style | WS_CHILD,
        x, y, w_, h_, parent, HMENU(id as isize), GetModuleHandleW(None).unwrap(), None)
}

unsafe extern "system" fn proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            child(w!("STATIC"), "旁译账号", WS_VISIBLE, 24, 20, 320, 28, h, 0);
            STATUS = child(w!("STATIC"), "", WS_VISIBLE, 24, 56, 320, 68, h, 0);
            EMAIL_EDIT = child(w!("EDIT"), "", WS_VISIBLE | WS_BORDER | WS_TABSTOP, 24, 130, 320, 30, h, ID_EMAIL);
            CODE_EDIT = child(w!("EDIT"), "", WS_VISIBLE | WS_BORDER | WS_TABSTOP, 24, 174, 180, 30, h, ID_CODE);
            let email_hint = wide("邮箱地址");
            let code_hint = wide("6 位验证码");
            let _ = SendMessageW(EMAIL_EDIT, 0x1501, WPARAM(1), LPARAM(email_hint.as_ptr() as isize));
            let _ = SendMessageW(CODE_EDIT, 0x1501, WPARAM(1), LPARAM(code_hint.as_ptr() as isize));
            SEND_BUTTON = child(w!("BUTTON"), "发送验证码", WS_VISIBLE | WS_TABSTOP, 214, 174, 130, 30, h, ID_SEND);
            VERIFY_BUTTON = child(w!("BUTTON"), "登录", WS_VISIBLE | WS_TABSTOP, 24, 218, 320, 34, h, ID_VERIFY);
            LOGOUT_BUTTON = child(w!("BUTTON"), "退出登录", WS_TABSTOP, 24, 218, 320, 34, h, ID_LOGOUT);
            if let Some(email) = saved_email() { show_logged_in(&email); } else { show_logged_out(); }
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wp.0 & 0xffff) as i32;
            if id == ID_LOGOUT {
                let _ = std::fs::remove_file(account_file()); show_logged_out(); return LRESULT(0);
            }
            if !configured() { set_text(STATUS, "登录服务尚未配置，当前构建不能登录。"); return LRESULT(0); }
            let email = text_of(EMAIL_EDIT);
            if !email.contains('@') { set_text(STATUS, "请输入有效的邮箱地址。"); return LRESULT(0); }
            if id == ID_SEND {
                set_text(STATUS, "正在发送验证码…");
                match send_otp(&email) { Ok(()) => set_text(STATUS, "验证码已发送。请检查收件箱和垃圾邮件。\n60 秒后才能再次发送。"), Err(e) => set_text(STATUS, &e) }
            } else if id == ID_VERIFY {
                let code = text_of(CODE_EDIT);
                if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) { set_text(STATUS, "请输入邮件中的 6 位数字验证码。"); return LRESULT(0); }
                set_text(STATUS, "正在验证…");
                match verify_otp(&email, &code) { Ok(actual) => show_logged_in(&actual), Err(e) => set_text(STATUS, &e) }
            }
            LRESULT(0)
        }
        WM_CLOSE => { let _ = DestroyWindow(h); LRESULT(0) }
        WM_DESTROY => { PostQuitMessage(0); LRESULT(0) }
        _ => DefWindowProcW(h, msg, wp, lp),
    }
}

pub unsafe fn open_account_window() {
    let hinst = GetModuleHandleW(None).unwrap();
    let cls = wide("PangyiAccount");
    let wc = WNDCLASSW { lpfnWndProc: Some(proc), lpszClassName: PCWSTR(cls.as_ptr()), hInstance: hinst.into(), hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(), ..Default::default() };
    RegisterClassW(&wc);
    let title = wide("旁译账号");
    let h = CreateWindowExW(WINDOW_EX_STYLE(0), PCWSTR(cls.as_ptr()), PCWSTR(title.as_ptr()),
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU, CW_USEDEFAULT, CW_USEDEFAULT, 390, 320,
        None, None, hinst, None);
    let _ = ShowWindow(h, SW_SHOW);
    let mut m = MSG::default();
    while GetMessageW(&mut m, None, 0, 0).as_bool() { let _ = TranslateMessage(&m); DispatchMessageW(&m); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let original = b"refresh-token-with-nontrivial-bytes";
        assert_eq!(unhex(&hex(original)).unwrap(), original);
        assert!(unhex("abc").is_none());
        assert!(unhex("zz").is_none());
    }

    #[test]
    fn extracts_supabase_error_message() {
        assert_eq!(error_text(r#"{"message":"验证码无效"}"#), "验证码无效");
    }
}
