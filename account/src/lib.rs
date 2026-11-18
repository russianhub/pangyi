//! Supabase / GoTrue 邮箱验证码。没有浏览器授权码，所以不用 PKCE。
//! 防重放靠这一次的 flow id、10 分钟有效期，以及只提交当前这次的邮箱和验证码。
//! 服务端仍会作废用过的验证码。未配置或密钥不对时不会报登录成功。

mod config;
mod error;
mod protocol;
mod session;
mod ui;

pub use config::{candidate_paths, normalize_auth_base, parse_config, validate_client_key, AuthConfig};
pub use error::AccountError;
pub use protocol::{Client, HttpTransport, Incoming, Outgoing, PendingOtp, TransportFail};
pub use session::{decode_session, encode_session, Session, EXPIRY_SKEW};
pub use ui::{
    actions, cooldown_left, late_result, logout_message, normalize_code, normalize_email,
    resend_ready, Action, ActionSpec, LateDecision, PhaseKind, PENDING_TTL, RESEND_COOLDOWN,
};
