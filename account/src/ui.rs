use std::time::{Duration, SystemTime};

use crate::error::AccountError;

pub const PENDING_TTL: Duration = Duration::from_secs(10 * 60);
/// 和 Supabase 默认的发信间隔一致。界面上的等待时间必须真的挡住按钮。
pub const RESEND_COOLDOWN: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PhaseKind {
    Unconfigured,
    LoggedOut,
    AwaitingCode,
    LoggedIn,
    Stale,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Send,
    Verify,
    Resend,
    Cancel,
    Logout,
    Retry,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ActionSpec {
    pub action: Action,
    pub label: &'static str,
    pub enabled: bool,
}

pub fn actions(phase: PhaseKind, busy: bool, resend_ready: bool) -> Vec<ActionSpec> {
    let mut specs = match phase {
        PhaseKind::Unconfigured => vec![ActionSpec {
            action: Action::Send,
            label: "登录不可用",
            enabled: false,
        }],
        PhaseKind::LoggedOut => {
            let mut v = vec![ActionSpec {
                action: Action::Send,
                label: "发送验证码",
                enabled: !busy,
            }];
            if busy {
                v.push(ActionSpec {
                    action: Action::Cancel,
                    label: "取消",
                    enabled: true,
                });
            }
            v
        }
        PhaseKind::AwaitingCode => vec![
            ActionSpec {
                action: Action::Verify,
                label: "登录",
                enabled: !busy,
            },
            ActionSpec {
                action: Action::Resend,
                label: "重新发送",
                enabled: resend_ready && !busy,
            },
            ActionSpec {
                action: Action::Cancel,
                label: "取消",
                enabled: true,
            },
        ],
        PhaseKind::LoggedIn => vec![ActionSpec {
            action: Action::Logout,
            label: "退出登录",
            enabled: !busy,
        }],
        PhaseKind::Stale => vec![
            ActionSpec {
                action: Action::Retry,
                label: "重试",
                enabled: !busy,
            },
            ActionSpec {
                action: Action::Logout,
                label: "退出登录",
                enabled: !busy,
            },
        ],
    };
    if busy {
        for spec in &mut specs {
            if spec.action != Action::Cancel {
                spec.enabled = false;
            }
        }
    }
    specs
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LateDecision {
    Apply,
    Drop,
    RevokeSession,
}

pub fn late_result(current_gen: u64, event_gen: u64, login_succeeded: bool) -> LateDecision {
    if current_gen == event_gen {
        LateDecision::Apply
    } else if login_succeeded {
        LateDecision::RevokeSession
    } else {
        LateDecision::Drop
    }
}

pub fn resend_ready(sent_at: SystemTime, now: SystemTime) -> bool {
    match now.duration_since(sent_at) {
        Ok(elapsed) => elapsed >= RESEND_COOLDOWN,
        Err(_) => true,
    }
}

pub fn cooldown_left(sent_at: SystemTime, now: SystemTime) -> u64 {
    let Some(until) = sent_at.checked_add(RESEND_COOLDOWN) else {
        return 0;
    };
    match until.duration_since(now) {
        Ok(left) => left.as_secs() + u64::from(left.subsec_nanos() > 0),
        Err(_) => 0,
    }
}

pub fn logout_message(remote: &Result<(), AccountError>) -> &'static str {
    match remote {
        Ok(()) => "已退出登录。",
        Err(AccountError::Offline | AccountError::Tls) => {
            "已退出本机登录。网络不可用，服务器上的会话会在过期后失效。"
        }
        Err(_) => "已退出本机登录。服务器会话可能要等过期后才失效。",
    }
}

pub fn normalize_email(input: &str) -> Result<String, AccountError> {
    let text = input.trim();
    if text.len() > 320 || text.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(AccountError::InvalidEmail);
    }
    let Some((local, domain)) = text.split_once('@') else {
        return Err(AccountError::InvalidEmail);
    };
    if local.is_empty()
        || local.len() > 64
        || domain.len() < 3
        || domain.len() > 255
        || !domain.contains('.')
        || domain.starts_with('.')
        || domain.ends_with('.')
        || domain.contains("..")
        || !domain.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        || !local.chars().all(|c| c.is_ascii() && !matches!(c, '<' | '>' | '"' | '\\'))
    {
        return Err(AccountError::InvalidEmail);
    }
    Ok(format!("{local}@{}", domain.to_ascii_lowercase()))
}

pub fn normalize_code(input: &str) -> Result<String, AccountError> {
    let text: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    if (6..=10).contains(&text.len()) && text.chars().all(|c| c.is_ascii_digit()) {
        Ok(text)
    } else {
        Err(AccountError::InvalidCode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unconfigured_and_busy_and_cooldown() {
        assert!(actions(PhaseKind::Unconfigured, false, true).iter().all(|s| !s.enabled));
        let busy = actions(PhaseKind::AwaitingCode, true, true);
        assert!(busy.iter().any(|s| s.action == Action::Cancel && s.enabled));
        assert!(busy.iter().filter(|s| s.action != Action::Cancel).all(|s| !s.enabled));
        let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        assert!(!resend_ready(start, start + Duration::from_secs(30)));
        assert_eq!(cooldown_left(start, start + Duration::from_secs(10)), 50);
        assert!(resend_ready(start, start + Duration::from_secs(60)));
        assert_eq!(late_result(2, 1, true), LateDecision::RevokeSession);
        assert_eq!(normalize_email("A@Example.COM").unwrap(), "A@example.com");
        assert_eq!(normalize_code(" 123 456 ").unwrap(), "123456");
        assert!(normalize_code("abc").is_err());
        let offline = logout_message(&Err(AccountError::Offline));
        assert!(offline.contains("已退出本机登录"));
        assert!(offline.contains("网络不可用"));
    }
}
