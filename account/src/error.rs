use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountError {
    NotConfigured,
    BadConfig(&'static str),
    ServerSecret,
    InvalidEmail,
    InvalidCode,
    Busy,
    NoPending,
    FlowMismatch,
    Expired,
    Offline,
    Tls,
    Redirect,
    RateLimited,
    CodeRejected,
    SessionExpired,
    SignupDisabled,
    ProviderOff,
    Unavailable,
    Incomplete,
    Internal(&'static str),
}

impl AccountError {
    pub fn user_message(&self) -> &'static str {
        match self {
            AccountError::NotConfigured => {
                "登录不可用。还没有配置邮箱登录服务。这不是登录成功。"
            }
            AccountError::BadConfig(reason) => reason,
            AccountError::ServerSecret => {
                "登录不可用。配置里的是服务端密钥，不能放进旁译。请改用 anon 或 publishable key。"
            }
            AccountError::InvalidEmail => "请输入有效的邮箱地址。",
            AccountError::InvalidCode => "请输入邮件里的 6 到 10 位数字验证码。",
            AccountError::Busy => "正在处理，请稍候。",
            AccountError::NoPending => "请先发送验证码。",
            AccountError::FlowMismatch => "这次验证已取消或已更换，请重新发送验证码。",
            AccountError::Expired => "验证码已过期，请重新发送。",
            AccountError::Offline => "网络不可用，请检查连接后再试。",
            AccountError::Tls => "无法建立安全连接。请检查系统时间后再试。",
            AccountError::Redirect => "登录服务要求跳转，已拒绝。",
            AccountError::RateLimited => "发送太频繁，请稍后再试。",
            AccountError::CodeRejected => "验证码不正确或已过期。",
            AccountError::SessionExpired => "登录已过期，请重新登录。",
            AccountError::SignupDisabled => "这个邮箱目前不能登录。",
            AccountError::ProviderOff => "邮箱登录没有在服务端打开。",
            AccountError::Unavailable => "登录服务暂时不可用。",
            AccountError::Incomplete => "登录服务没有返回完整会话，已放弃，没有登录。",
            AccountError::Internal(msg) => msg,
        }
    }
}

impl fmt::Display for AccountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.user_message())
    }
}
