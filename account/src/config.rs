use std::path::{Path, PathBuf};

use crate::error::AccountError;

#[derive(Clone)]
pub struct AuthConfig {
    auth_base: String,
    anon_key: String,
    create_user: bool,
}

impl std::fmt::Debug for AuthConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthConfig")
            .field("auth_base", &self.auth_base)
            .field("anon_key", &"<redacted>")
            .field("create_user", &self.create_user)
            .finish()
    }
}

impl AuthConfig {
    pub fn auth_base(&self) -> &str {
        &self.auth_base
    }
    pub fn anon_key(&self) -> &str {
        &self.anon_key
    }
    pub fn create_user(&self) -> bool {
        self.create_user
    }

    pub fn from_parts(url: &str, anon_key: &str, create_user: bool) -> Result<Self, AccountError> {
        Ok(Self {
            auth_base: normalize_auth_base(url)?,
            anon_key: {
                validate_client_key(anon_key)?;
                anon_key.trim().to_string()
            },
            create_user,
        })
    }
}

/// 管理员配置优先于 exe 旁边的文件。不读 LocalAppData，避免和词库混在一起，
/// 也避免用户目录里的文件换掉机器配置。
pub fn candidate_paths(program_data: &Path, exe_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = vec![program_data.join("Pangyi").join("account.toml")];
    if let Some(dir) = exe_dir {
        paths.push(dir.join("account.toml"));
    }
    paths
}

pub fn parse_config(text: &str) -> Result<AuthConfig, AccountError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut provider = None;
    let mut url = None;
    let mut anon_key = None;
    let mut create_user = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(AccountError::BadConfig("配置行里没有等号"));
        };
        let key = key.trim();
        let value = unquote(value.trim());
        match key {
            "provider" => assign(&mut provider, value)?,
            "url" => assign(&mut url, value)?,
            "anon_key" => assign(&mut anon_key, value)?,
            "create_user" => {
                if create_user.is_some() {
                    return Err(AccountError::BadConfig("配置项重复"));
                }
                create_user = Some(match value.as_str() {
                    "true" | "TRUE" | "1" => true,
                    "false" | "FALSE" | "0" => false,
                    _ => return Err(AccountError::BadConfig("create_user 只能是 true 或 false")),
                });
            }
            _ => return Err(AccountError::BadConfig("有不认识的配置项")),
        }
    }
    let provider = provider.ok_or(AccountError::BadConfig("缺少 provider"))?;
    if provider != "supabase" {
        return Err(AccountError::BadConfig("不支持的登录服务，目前只支持 supabase"));
    }
    let url = url.ok_or(AccountError::BadConfig("缺少地址"))?;
    if url.is_empty() {
        return Err(AccountError::BadConfig("缺少地址"));
    }
    let anon_key = anon_key.ok_or(AccountError::BadConfig("缺少公开客户端标识"))?;
    if anon_key.is_empty() {
        return Err(AccountError::BadConfig("缺少公开客户端标识"));
    }
    AuthConfig::from_parts(&url, &anon_key, create_user.unwrap_or(true))
}

fn assign(slot: &mut Option<String>, value: String) -> Result<(), AccountError> {
    if slot.is_some() {
        return Err(AccountError::BadConfig("配置项重复"));
    }
    *slot = Some(value);
    Ok(())
}

fn unquote(value: &str) -> String {
    let b = value.as_bytes();
    if b.len() >= 2 {
        let q = b[0];
        if (q == b'"' || q == b'\'') && b[b.len() - 1] == q {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}

pub fn normalize_auth_base(raw: &str) -> Result<String, AccountError> {
    let s = raw.trim().trim_end_matches('/');
    let Some(rest) = s.strip_prefix("https://") else {
        return Err(AccountError::BadConfig("地址必须是 https，不能带账号密码"));
    };
    if rest.is_empty()
        || rest.contains('@')
        || rest.contains('\\')
        || rest.contains('?')
        || rest.contains('#')
        || rest.contains(' ')
        || rest.contains("://")
    {
        return Err(AccountError::BadConfig("地址必须是 https，不能带账号密码"));
    }
    let hostport = if let Some(host) = rest.strip_suffix("/auth/v1") {
        if host.contains('/') {
            return Err(AccountError::BadConfig("地址路径不正确"));
        }
        host
    } else if rest.contains('/') {
        return Err(AccountError::BadConfig("地址路径不正确"));
    } else {
        rest
    };
    if !valid_hostport(hostport) {
        return Err(AccountError::BadConfig("地址路径不正确"));
    }
    Ok(format!("https://{hostport}/auth/v1"))
}

fn valid_hostport(hostport: &str) -> bool {
    if hostport.is_empty() || hostport.starts_with(':') || hostport.ends_with(':') {
        return false;
    }
    let host = match hostport.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
            if port.len() > 5 || port.parse::<u16>().is_err() {
                return false;
            }
            host
        }
        Some(_) => return false,
        None => hostport,
    };
    if host.is_empty() || host.starts_with('.') || host.ends_with('.') || host.contains("..") {
        return false;
    }
    host.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

pub fn validate_client_key(raw: &str) -> Result<(), AccountError> {
    let key = raw.trim();
    if key.len() < 20
        || key.len() > 4096
        || key.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(AccountError::BadConfig("公开客户端标识无效"));
    }
    if key.contains("service_role") || key.starts_with("sb_secret_") {
        return Err(AccountError::ServerSecret);
    }
    if key.starts_with("sb_publishable_") {
        return Ok(());
    }
    let mut parts = key.split('.');
    let (Some(header), Some(payload), Some(sig)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(AccountError::BadConfig("公开客户端标识无效"));
    };
    if parts.next().is_some() || header.is_empty() || payload.is_empty() || sig.is_empty() {
        return Err(AccountError::BadConfig("公开客户端标识无效"));
    }
    let bytes = b64url_decode(payload).ok_or(AccountError::BadConfig("公开客户端标识无效"))?;
    let text = String::from_utf8(bytes).map_err(|_| AccountError::BadConfig("公开客户端标识无效"))?;
    let role = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v.get("role")?.as_str().map(str::to_string))
        .ok_or(AccountError::BadConfig("公开客户端标识无效"))?;
    match role.as_str() {
        "anon" => Ok(()),
        "service_role" => Err(AccountError::ServerSecret),
        _ => Err(AccountError::BadConfig("公开客户端标识无效")),
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn b64url_encode(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8) | data[i + 2] as u32;
        for shift in [18, 12, 6, 0] {
            out.push(T[((n >> shift) & 63) as usize] as char);
        }
        i += 3;
    }
    if i < data.len() {
        let rem = data.len() - i;
        let n = if rem == 1 {
            (data[i] as u32) << 16
        } else {
            ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8)
        };
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        if rem == 2 {
            out.push(T[((n >> 6) & 63) as usize] as char);
        }
    }
    out
}

fn b64url_decode(input: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'-' => Some(62),
            b'_' => Some(63),
            _ => None,
        }
    }
    let bytes = input.as_bytes();
    if bytes.is_empty() || bytes.contains(&b'=') || bytes.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i + 4 <= bytes.len() {
        let n = ((val(bytes[i])? as u32) << 18)
            | ((val(bytes[i + 1])? as u32) << 12)
            | ((val(bytes[i + 2])? as u32) << 6)
            | val(bytes[i + 3])? as u32;
        out.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8]);
        i += 4;
    }
    let rem = bytes.len() - i;
    if rem == 2 {
        let n = ((val(bytes[i])? as u32) << 18) | ((val(bytes[i + 1])? as u32) << 12);
        out.push((n >> 16) as u8);
    } else if rem == 3 {
        let n = ((val(bytes[i])? as u32) << 18)
            | ((val(bytes[i + 1])? as u32) << 12)
            | ((val(bytes[i + 2])? as u32) << 6);
        out.push((n >> 16) as u8);
        out.push((n >> 8) as u8);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn anon_jwt() -> String {
        let h = b64url_encode(br#"{"alg":"none","typ":"JWT"}"#);
        let p = b64url_encode(br#"{"role":"anon","iss":"supabase"}"#);
        format!("{h}.{p}.sig")
    }

    pub fn service_jwt() -> String {
        let h = b64url_encode(br#"{"alg":"none","typ":"JWT"}"#);
        let p = b64url_encode(br#"{"role":"service_role"}"#);
        format!("{h}.{p}.sig")
    }

    #[test]
    fn https_and_secrets() {
        assert_eq!(
            normalize_auth_base("https://abc.supabase.co/auth/v1/").unwrap(),
            "https://abc.supabase.co/auth/v1"
        );
        assert!(normalize_auth_base("http://abc.supabase.co").is_err());
        assert!(normalize_auth_base("https://user:pass@abc.supabase.co").is_err());
        assert!(normalize_auth_base("https://abc.supabase.co/steal").is_err());
        assert!(validate_client_key(&anon_jwt()).is_ok());
        assert_eq!(validate_client_key(&service_jwt()).unwrap_err(), AccountError::ServerSecret);
        assert!(validate_client_key("sb_publishable_example_public_key").is_ok());
        assert_eq!(
            validate_client_key("sb_secret_example_server_key_value").unwrap_err(),
            AccountError::ServerSecret
        );
    }

    #[test]
    fn empty_config_is_not_success_and_paths_skip_localappdata() {
        assert_eq!(
            parse_config("provider = supabase\nurl =\nanon_key =\n").unwrap_err(),
            AccountError::BadConfig("缺少地址")
        );
        let shown = parse_config(&format!(
            "provider = supabase\nurl = https://abc.supabase.co\nanon_key = {}\n",
            service_jwt()
        ))
        .unwrap_err()
        .to_string();
        assert!(!shown.contains("service_role"));
        assert!(!shown.contains("eyJ"));
        let root = PathBuf::from(format!("/tmp/pangyi-cfg-{}", std::process::id()));
        let paths = candidate_paths(&root, Some(Path::new("/opt/pangyi")));
        assert!(paths.iter().all(|p| !p.to_string_lossy().to_ascii_lowercase().contains("local")));
        assert!(paths[0].ends_with("account.toml"));
    }
}
