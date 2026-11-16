use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::AuthConfig;
use crate::error::AccountError;
use crate::session::Session;
use crate::ui::{normalize_code, normalize_email, PENDING_TTL};

#[derive(Clone, Debug)]
pub struct PendingOtp {
    pub email: String,
    pub flow_id: String,
    pub sent_at: SystemTime,
    pub expires_at: SystemTime,
}

pub struct Outgoing {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

pub struct Incoming {
    pub status: u16,
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportFail {
    Offline,
    Tls,
}

pub trait HttpTransport: Send + Sync {
    fn post(&self, req: &Outgoing) -> Result<Incoming, TransportFail>;
}

struct Flow {
    in_flight: bool,
    pending: Option<PendingOtp>,
}

pub struct Client<H> {
    config: AuthConfig,
    http: H,
    state: Mutex<Flow>,
}

struct Flight<'a> {
    state: &'a Mutex<Flow>,
}

impl Drop for Flight<'_> {
    fn drop(&mut self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).in_flight = false;
    }
}

impl<H: HttpTransport> Client<H> {
    pub fn new(config: AuthConfig, http: H) -> Self {
        Self {
            config,
            http,
            state: Mutex::new(Flow { in_flight: false, pending: None }),
        }
    }

    pub fn clear_pending(&self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).pending = None;
    }

    pub fn clear_pending_if(&self, flow_id: &str) {
        let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if guard.pending.as_ref().is_some_and(|p| p.flow_id == flow_id) {
            guard.pending = None;
        }
    }

    pub fn send_otp(&self, email: &str, now: SystemTime) -> Result<PendingOtp, AccountError> {
        let email = normalize_email(email)?;
        let _flight = self.begin()?;
        let body = serde_json::json!({
            "email": email,
            "create_user": self.config.create_user(),
            "data": {}
        });
        let (status, text) = self.post("/otp", self.config.anon_key(), Some(&body))?;
        if is_redirect(status) {
            return Err(AccountError::Redirect);
        }
        if !(200..300).contains(&status) || error_code(&text).is_some() {
            return Err(classify(Call::Send, status, &text));
        }
        let pending = PendingOtp {
            email,
            flow_id: random_flow_id()?,
            sent_at: now,
            expires_at: now + PENDING_TTL,
        };
        self.state.lock().unwrap_or_else(|e| e.into_inner()).pending = Some(pending.clone());
        Ok(pending)
    }

    pub fn verify(&self, email: &str, code: &str, flow_id: &str, now: SystemTime) -> Result<Session, AccountError> {
        let email = normalize_email(email)?;
        let code = normalize_code(code)?;
        let _flight = self.begin()?;
        {
            let guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let Some(pending) = guard.pending.as_ref() else {
                return Err(AccountError::NoPending);
            };
            if pending.flow_id != flow_id || pending.email != email {
                return Err(AccountError::FlowMismatch);
            }
            if now >= pending.expires_at {
                drop(guard);
                self.clear_pending_if(flow_id);
                return Err(AccountError::Expired);
            }
        }
        let body = serde_json::json!({"type": "email", "email": email, "token": code});
        let (status, text) = self.post("/verify", self.config.anon_key(), Some(&body))?;
        if is_redirect(status) {
            return Err(AccountError::Redirect);
        }
        if !(200..300).contains(&status) {
            let err = classify(Call::Verify, status, &text);
            if matches!(err, AccountError::SessionExpired) {
                self.clear_pending_if(flow_id);
            }
            return Err(err);
        }
        let session = session_from_body(&text, &email, now)?;
        self.clear_pending_if(flow_id);
        Ok(session)
    }

    pub fn refresh(&self, session: &Session, now: SystemTime) -> Result<Session, AccountError> {
        let _flight = self.begin()?;
        let body = serde_json::json!({"refresh_token": session.refresh_token});
        let (status, text) = self.post("/token?grant_type=refresh_token", self.config.anon_key(), Some(&body))?;
        if is_redirect(status) {
            return Err(AccountError::Redirect);
        }
        if !(200..300).contains(&status) {
            return Err(classify(Call::Refresh, status, &text));
        }
        session_from_body(&text, &session.email, now)
    }

    pub fn revoke(&self, session: &Session) -> Result<(), AccountError> {
        let _flight = self.begin()?;
        let (status, text) = self.post("/logout", &session.access_token, None)?;
        if is_redirect(status) {
            return Err(AccountError::Redirect);
        }
        if (200..300).contains(&status) || matches!(status, 401 | 403 | 404) {
            return Ok(());
        }
        Err(classify(Call::Logout, status, &text))
    }

    fn begin(&self) -> Result<Flight<'_>, AccountError> {
        let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if guard.in_flight {
            return Err(AccountError::Busy);
        }
        guard.in_flight = true;
        drop(guard);
        Ok(Flight { state: &self.state })
    }

    fn post(&self, path: &str, bearer: &str, body: Option<&serde_json::Value>) -> Result<(u16, String), AccountError> {
        let url = format!("{}{path}", self.config.auth_base());
        if !url.starts_with("https://") || url.contains('@') || bearer.is_empty() {
            return Err(AccountError::Tls);
        }
        let raw = match body {
            Some(v) => serde_json::to_vec(v).map_err(|_| AccountError::Internal("无法编码请求"))?,
            None => Vec::new(),
        };
        let mut headers = vec![
            ("apikey".into(), self.config.anon_key().to_string()),
            ("Authorization".into(), format!("Bearer {bearer}")),
            ("Accept".into(), "application/json".into()),
            ("User-Agent".into(), "PangyiAccount".into()),
        ];
        if body.is_some() {
            headers.push(("Content-Type".into(), "application/json".into()));
        }
        match self.http.post(&Outgoing { url, headers, body: raw }) {
            Ok(Incoming { status, body }) => Ok((status, body)),
            Err(TransportFail::Offline) => Err(AccountError::Offline),
            Err(TransportFail::Tls) => Err(AccountError::Tls),
        }
    }
}

#[derive(Clone, Copy)]
enum Call { Send, Verify, Refresh, Logout }

fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

fn error_code(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    for key in ["error_code", "error"] {
        if let Some(text) = value.get(key).and_then(|v| v.as_str()) {
            if !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Some(text.to_ascii_lowercase());
            }
        }
    }
    None
}

fn classify(kind: Call, status: u16, body: &str) -> AccountError {
    if is_redirect(status) {
        return AccountError::Redirect;
    }
    if status == 429 {
        return AccountError::RateLimited;
    }
    if let Some(code) = error_code(body) {
        let mapped = match code.as_str() {
            "otp_expired" | "invalid_otp" => AccountError::CodeRejected,
            "over_request_rate_limit" | "over_email_send_rate_limit" | "too_many_requests" => {
                AccountError::RateLimited
            }
            "signup_disabled" | "user_not_found" | "user_banned" => AccountError::SignupDisabled,
            "otp_disabled" | "email_provider_disabled" => AccountError::ProviderOff,
            "email_address_invalid" | "validation_failed" => AccountError::InvalidEmail,
            _ => AccountError::Unavailable,
        };
        return match kind {
            Call::Refresh if matches!(mapped, AccountError::CodeRejected) => AccountError::SessionExpired,
            Call::Send if matches!(mapped, AccountError::CodeRejected) => AccountError::Unavailable,
            _ => mapped,
        };
    }
    let hint = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            ["msg", "message", "error_description"]
                .iter()
                .find_map(|k| v.get(*k).and_then(|x| x.as_str()).map(|s| s.to_ascii_lowercase()))
        })
        .unwrap_or_default();
    if matches!(kind, Call::Verify) && (hint.contains("expired") || hint.contains("invalid") || hint.contains("otp")) {
        return AccountError::CodeRejected;
    }
    if matches!(kind, Call::Refresh) && matches!(status, 400 | 401 | 403) {
        return AccountError::SessionExpired;
    }
    AccountError::Unavailable
}

fn session_from_body(body: &str, fallback_email: &str, now: SystemTime) -> Result<Session, AccountError> {
    let value: serde_json::Value = serde_json::from_str(body).map_err(|_| AccountError::Incomplete)?;
    let access = text_field(&value, "access_token");
    let refresh = text_field(&value, "refresh_token");
    if access.is_empty() || refresh.is_empty() {
        return Err(AccountError::Incomplete);
    }
    let user_id = value.get("user").and_then(|u| u.get("id")).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if user_id.is_empty() || user_id.len() > 128 || user_id.chars().any(|c| !c.is_ascii() || c.is_whitespace()) {
        return Err(AccountError::Incomplete);
    }
    let email = value
        .get("user")
        .and_then(|u| u.get("email"))
        .and_then(|v| v.as_str())
        .and_then(|s| normalize_email(s).ok())
        .unwrap_or_else(|| fallback_email.to_string());
    let expires_at = value.get("expires_at").and_then(|v| v.as_i64());
    let expires_in = value.get("expires_in").and_then(|v| v.as_i64());
    let expires_at_unix = match (expires_at, expires_in) {
        (Some(at), _) if (1_600_000_000..4_000_000_000).contains(&at) => at as u64,
        (_, Some(secs)) if (60..86_400 * 30).contains(&secs) => now
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
            .saturating_add(secs as u64),
        _ => return Err(AccountError::Incomplete),
    };
    Ok(Session { email, user_id, access_token: access, refresh_token: refresh, expires_at_unix })
}

fn text_field(value: &serde_json::Value, key: &str) -> String {
    value.get(key).and_then(|v| v.as_str()).unwrap_or("").trim().to_string()
}

fn random_flow_id() -> Result<String, AccountError> {
    let mut buf = [0u8; 16];
    getrandom::getrandom(&mut buf).map_err(|_| AccountError::Internal("无法生成随机数"))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_config;
    use std::sync::{Arc, Condvar};
    use std::time::Duration;

    fn config() -> AuthConfig {
        let h = crate::config::b64url_encode(br#"{"alg":"none","typ":"JWT"}"#);
        let p = crate::config::b64url_encode(br#"{"role":"anon"}"#);
        parse_config(&format!(
            "provider = supabase\nurl = https://abc.supabase.co\nanon_key = {h}.{p}.sig\n"
        ))
        .unwrap()
    }

    struct Script {
        calls: Mutex<Vec<Outgoing>>,
        responses: Mutex<std::collections::VecDeque<Result<Incoming, TransportFail>>>,
    }

    impl Script {
        fn new(responses: Vec<Result<Incoming, TransportFail>>) -> Arc<Self> {
            Arc::new(Self {
                calls: Mutex::new(Vec::new()),
                responses: Mutex::new(responses.into()),
            })
        }
        fn calls(&self) -> Vec<Outgoing> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl Clone for Outgoing {
        fn clone(&self) -> Self {
            Self { url: self.url.clone(), headers: self.headers.clone(), body: self.body.clone() }
        }
    }

    impl HttpTransport for Arc<Script> {
        fn post(&self, req: &Outgoing) -> Result<Incoming, TransportFail> {
            self.calls.lock().unwrap().push(req.clone());
            self.responses.lock().unwrap().pop_front().unwrap_or(Err(TransportFail::Offline))
        }
    }

    fn ok(body: &str) -> Result<Incoming, TransportFail> {
        Ok(Incoming { status: 200, body: body.into() })
    }

    fn session_json() -> String {
        r#"{"access_token":"access-token-value","refresh_token":"refresh-token-value","expires_in":3600,"user":{"id":"user-id-1","email":"user@example.com"}}"#.into()
    }

    fn now() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000)
    }

    #[test]
    fn send_does_not_put_the_key_in_the_url_or_upload_typing_data() {
        let http = Script::new(vec![ok("{}")]);
        let client = Client::new(config(), Arc::clone(&http));
        client.send_otp("User@Example.com", now()).unwrap();
        let call = &http.calls()[0];
        assert_eq!(call.url, "https://abc.supabase.co/auth/v1/otp");
        assert!(!call.url.contains("eyJ"));
        let body: serde_json::Value = serde_json::from_str(std::str::from_utf8(&call.body).unwrap()).unwrap();
        let obj = body.as_object().unwrap();
        assert_eq!(obj.len(), 3);
        assert_eq!(body["email"], "User@example.com");
        assert_eq!(body["data"], serde_json::json!({}));
        assert!(!call.body.windows(b"pinyin".len()).any(|w| w == b"pinyin"));
        assert!(call.headers.iter().any(|(k, _)| k == "apikey"));
    }

    #[test]
    fn empty_or_redirect_is_not_success_and_replay_stops() {
        let http = Script::new(vec![
            ok(""),
            Ok(Incoming { status: 302, body: "http://evil".into() }),
            ok("{}"),
            ok(&session_json()),
            ok(&session_json()),
        ]);
        let client = Client::new(config(), Arc::clone(&http));
        // 空 200 没有 error 字段，发送可以成功（GoTrue 的成功体就是空对象或空）。
        assert!(client.send_otp("user@example.com", now()).is_ok());
        assert_eq!(client.send_otp("user@example.com", now()).unwrap_err(), AccountError::Redirect);
        let pending = client.send_otp("user@example.com", now()).unwrap();
        let session = client.verify("user@example.com", "123456", &pending.flow_id, now()).unwrap();
        assert!(!format!("{session:?}").contains("access-token-value"));
        assert_eq!(
            client.verify("user@example.com", "123456", &pending.flow_id, now()).unwrap_err(),
            AccountError::NoPending
        );
        assert_eq!(http.calls().len(), 4);
        assert!(!http.calls()[3].url.contains("123456"));
    }

    #[test]
    fn wrong_flow_expiry_and_bad_code_do_not_invent_login() {
        let http = Script::new(vec![
            ok("{}"),
            ok("{}"),
            Ok(Incoming {
                status: 403,
                body: r#"{"error_code":"otp_expired","msg":"Token has expired or is invalid","access_token":"eyJhbGciOiJub25lIn0.eyJyb2xlIjoiYW5vbiJ9.sig"}"#.into(),
            }),
        ]);
        let client = Client::new(config(), Arc::clone(&http));
        let pending = client.send_otp("user@example.com", now()).unwrap();
        assert_eq!(
            client.verify("user@example.com", "123456", "nope", now()).unwrap_err(),
            AccountError::FlowMismatch
        );
        assert_eq!(http.calls().len(), 1);
        assert_eq!(
            client.verify("user@example.com", "123456", &pending.flow_id, now() + PENDING_TTL).unwrap_err(),
            AccountError::Expired
        );
        assert_eq!(http.calls().len(), 1);
        let pending = client.send_otp("user@example.com", now()).unwrap();
        let err = client.verify("user@example.com", "000000", &pending.flow_id, now()).unwrap_err();
        assert_eq!(err, AccountError::CodeRejected);
        assert!(!err.to_string().contains("eyJ"));
        assert!(!err.to_string().contains("000000"));
        assert!(client.verify("nope", "123456", &pending.flow_id, now()).is_err());
        assert_eq!(http.calls().len(), 3);
    }

    #[test]
    fn refresh_logout_and_rate_limit() {
        let http = Script::new(vec![
            ok("{}"),
            ok(&session_json()),
            ok(r#"{"access_token":"access-2","refresh_token":"refresh-2","expires_at":1900000000,"user":{"id":"user-id-1","email":"user@example.com"}}"#),
            ok(""),
            Ok(Incoming { status: 429, body: r#"{"error_code":"over_email_send_rate_limit"}"#.into() }),
        ]);
        let client = Client::new(config(), Arc::clone(&http));
        let pending = client.send_otp("user@example.com", now()).unwrap();
        let session = client.verify("user@example.com", "123456", &pending.flow_id, now()).unwrap();
        let next = client.refresh(&session, now()).unwrap();
        assert_eq!(next.refresh_token, "refresh-2");
        client.revoke(&next).unwrap();
        let calls = http.calls();
        assert!(calls[2].url.ends_with("/token?grant_type=refresh_token"));
        assert!(!calls[2].url.contains("refresh-token-value"));
        assert!(std::str::from_utf8(&calls[2].body).unwrap().contains("refresh-token-value"));
        assert!(calls[3].headers.iter().any(|(k, v)| k == "Authorization" && v == "Bearer access-2"));
        assert!(calls[3].body.is_empty());
        assert_eq!(client.send_otp("user@example.com", now()).unwrap_err(), AccountError::RateLimited);
    }

    #[test]
    fn offline_incomplete_and_double_click() {
        let http = Script::new(vec![
            Err(TransportFail::Offline),
            ok("{}"),
            ok("{}"),
        ]);
        let client = Client::new(config(), Arc::clone(&http));
        assert_eq!(client.send_otp("user@example.com", now()).unwrap_err(), AccountError::Offline);
        let pending = client.send_otp("user@example.com", now()).unwrap();
        assert_eq!(
            client.verify("user@example.com", "123456", &pending.flow_id, now()).unwrap_err(),
            AccountError::Incomplete
        );

        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let started = Arc::new((Mutex::new(false), Condvar::new()));
        let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        struct Block {
            gate: Arc<(Mutex<bool>, Condvar)>,
            started: Arc<(Mutex<bool>, Condvar)>,
            hits: Arc<std::sync::atomic::AtomicUsize>,
        }
        impl HttpTransport for Block {
            fn post(&self, _: &Outgoing) -> Result<Incoming, TransportFail> {
                self.hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                *self.started.0.lock().unwrap() = true;
                self.started.1.notify_all();
                let mut go = self.gate.0.lock().unwrap();
                while !*go {
                    go = self.gate.1.wait(go).unwrap();
                }
                Ok(Incoming { status: 200, body: "{}".into() })
            }
        }
        let client = Arc::new(Client::new(config(), Block {
            gate: Arc::clone(&gate),
            started: Arc::clone(&started),
            hits: Arc::clone(&hits),
        }));
        let worker = {
            let client = Arc::clone(&client);
            std::thread::spawn(move || client.send_otp("user@example.com", now()))
        };
        let mut flag = started.0.lock().unwrap();
        while !*flag {
            flag = started.1.wait(flag).unwrap();
        }
        drop(flag);
        assert_eq!(client.send_otp("user@example.com", now()).unwrap_err(), AccountError::Busy);
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        assert!(worker.join().unwrap().is_ok());
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
