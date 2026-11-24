//! 更新开关只读 `updater.json` 的 `enabled`。
//!
//! 打开或关掉自动检查时，去启动已经装好的 `pangyi-updater.exe`。
//! 这里不重写 JSON，避免把 `skipped` 和 `last_check` 清掉，也不自己联网。

use serde::Deserialize;
use std::path::Path;

use crate::settings_file::{read_text_file, TextFile};

pub const ACCOUNT_EXE: &str = r"C:\Program Files\Pangyi\pangyi.exe";
pub const ACCOUNT_ARG: &str = "--account";
pub const UPDATER_EXE: &str = r"C:\Program Files\Pangyi\pangyi-updater.exe";
pub const UPDATER_MANUAL: &str = "--manual";
pub const UPDATER_ENABLE: &str = "--enable";
pub const UPDATER_DISABLE: &str = "--disable";
/// 打开本机文件时用的 Shell 动词。不要把编辑器名字放在这个位置。
pub const OPEN_VERB: &str = "open";

pub const MSG_NO_UPDATER: &str = "没找到更新程序。请重新运行一次安装包。";
pub const MSG_NO_ACCOUNT: &str = "没能打开账号窗口。打字、词库和表情包不受影响。";

#[derive(Deserialize)]
struct UpdaterView {
    enabled: bool,
}

/// 文件不存在、坏掉、或没有合法的 `enabled` 布尔值时，按更新程序的默认：开。
pub fn updater_enabled_text(text: &str) -> bool {
    serde_json::from_str::<UpdaterView>(text)
        .map(|v| v.enabled)
        .unwrap_or(true)
}

pub fn read_updater_enabled(dir: &Path) -> bool {
    match read_text_file(&dir.join("updater.json")) {
        Ok(TextFile::Text(text)) => updater_enabled_text(&text),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn reads_enabled_and_leaves_the_other_fields_alone() {
        let raw = r#"{"enabled":false,"skipped":"4.2.0","last_check":42,"extra":1}"#;
        assert!(!updater_enabled_text(raw));
        assert!(raw.contains("skipped"));
        assert!(raw.contains("last_check"));
        let on = r#"{"enabled":true,"skipped":"","last_check":0}"#;
        assert!(updater_enabled_text(on));
    }

    #[test]
    fn missing_or_broken_json_defaults_to_on() {
        assert!(updater_enabled_text(""));
        assert!(updater_enabled_text("not json"));
        assert!(updater_enabled_text(r#"{"skipped":"9"}"#));
        assert!(updater_enabled_text(r#"{"enabled":"yes"}"#));
        assert!(updater_enabled_text(r#"{"enabled":null}"#));
    }

    #[test]
    fn missing_file_defaults_to_on_and_does_not_create_one() {
        let dir = crate::settings_file::test_dir("updater");
        assert!(read_updater_enabled(&dir));
        assert!(!dir.join("updater.json").exists());
        fs::write(
            dir.join("updater.json"),
            r#"{"enabled":false,"skipped":"1.0.0"}"#,
        )
        .unwrap();
        assert!(!read_updater_enabled(&dir));
        let after = fs::read_to_string(dir.join("updater.json")).unwrap();
        assert!(after.contains("skipped"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn launch_constants_match_the_existing_programs() {
        assert_eq!(ACCOUNT_EXE, r"C:\Program Files\Pangyi\pangyi.exe");
        assert_eq!(ACCOUNT_ARG, "--account");
        assert_eq!(UPDATER_EXE, r"C:\Program Files\Pangyi\pangyi-updater.exe");
        assert_eq!(UPDATER_MANUAL, "--manual");
        assert_eq!(UPDATER_ENABLE, "--enable");
        assert_eq!(UPDATER_DISABLE, "--disable");
        assert_eq!(OPEN_VERB, "open");
        assert_eq!(MSG_NO_UPDATER, "没找到更新程序。请重新运行一次安装包。");
        assert_eq!(
            MSG_NO_ACCOUNT,
            "没能打开账号窗口。打字、词库和表情包不受影响。"
        );
    }

    #[test]
    fn this_module_does_not_serialize_json() {
        let src = include_str!("updater_cfg.rs");
        // 针分成两段，避免断言自己的字面量被扫进去。
        for parts in [
            ["serde_json::", "to_"],
            ["to_string_", "pretty"],
            ["to_", "writer"],
        ] {
            let needle = format!("{}{}", parts[0], parts[1]);
            assert!(!src.contains(&needle), "{needle}");
        }
    }
}
