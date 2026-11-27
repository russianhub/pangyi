//! `settings.txt` 的读和「只改一个键」的写。
//!
//! 读入规则对齐 `ime/src/settings.rs` 的 `load()`：夹取值、认 `1`/`true`、
//! 不认识的键忽略、同一键以后出现的为准。非法 UTF-8 整份当没读到。
//!
//! 写回不调用旧的 `save()`。改之前重读，只替换用户刚动过的那一个键，
//! 注释、空行、未知键、隐藏的 `debug` 和它们的顺序都留着。
//! 文件还不存在时才写出全部已知键。先写同目录临时文件，再改名替换。

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// 和旧 `save()` 的默认正文一致：UTF-8、无 BOM、`\n`。
pub const DEFAULT_BODY: &str = "\
max=9
scale=100
layout=v
theme=0
english=0
debug=0
cnpunct=1
trmode=0
accent=0
emoji=0
";

const KNOWN_KEYS: &[&str] = &[
    "max", "scale", "layout", "theme", "english", "debug", "cnpunct", "trmode", "accent", "emoji",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub max: usize,
    pub scale: i32,
    pub horizontal: bool,
    pub theme: i32,
    pub english: bool,
    pub debug: bool,
    pub cnpunct: bool,
    pub trmode: i32,
    pub accent: i32,
    pub emoji: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            max: 9,
            scale: 100,
            horizontal: false,
            theme: 0,
            english: false,
            debug: false,
            cnpunct: true,
            trmode: 0,
            accent: 0,
            emoji: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextFile {
    Missing,
    /// 文件在，但不是合法 UTF-8。调用方必须把它当成没读到，并且不要覆盖。
    InvalidUtf8,
    Text(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loaded {
    pub settings: Settings,
    /// 非法 UTF-8 时为 false，避免用默认值把原文件盖掉。
    pub can_write: bool,
}

pub fn pangyi_dir(local_app_data: &str) -> PathBuf {
    PathBuf::from(local_app_data).join("Pangyi")
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    pangyi_dir(&base)
}

pub fn settings_path(dir: &Path) -> PathBuf {
    dir.join("settings.txt")
}

pub fn default_body() -> String {
    DEFAULT_BODY.to_string()
}

pub fn read_text_file(path: &Path) -> io::Result<TextFile> {
    match fs::read(path) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => Ok(TextFile::Text(text)),
            Err(_) => Ok(TextFile::InvalidUtf8),
        },
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(TextFile::Missing),
        Err(err) => Err(err),
    }
}

/// `settings_text` 为 `None` 表示文件不存在或不是合法 UTF-8。
/// `layout_txt` 只在设置里没有 `layout` 键时还能决定横竖排，和旧的 `load()` 一样先读它。
pub fn load_from_parts(settings_text: Option<&str>, layout_txt: Option<&str>) -> Settings {
    let mut settings = Settings::default();
    if let Some(layout) = layout_txt {
        if layout.trim().eq_ignore_ascii_case("h") {
            settings.horizontal = true;
        }
    }
    if let Some(text) = settings_text {
        apply_text(&mut settings, text);
    }
    settings
}

pub fn parse_settings(text: &str) -> Settings {
    load_from_parts(Some(text), None)
}

pub fn load_dir(dir: &Path) -> io::Result<Loaded> {
    let file = read_text_file(&settings_path(dir))?;
    let can_write = !matches!(file, TextFile::InvalidUtf8);
    let settings_text = match &file {
        TextFile::Text(text) => Some(text.as_str()),
        _ => None,
    };
    let layout_txt = match read_text_file(&dir.join("layout.txt"))? {
        TextFile::Text(text) => Some(text),
        _ => None,
    };
    Ok(Loaded {
        settings: load_from_parts(settings_text, layout_txt.as_deref()),
        can_write,
    })
}

pub fn max_segment(max: usize) -> i32 {
    if max <= 5 {
        0
    } else if max <= 7 {
        1
    } else {
        2
    }
}

pub fn scale_segment(scale: i32) -> i32 {
    if scale <= 90 {
        0
    } else if scale >= 115 {
        2
    } else {
        1
    }
}

pub fn layout_segment(horizontal: bool) -> i32 {
    if horizontal {
        1
    } else {
        0
    }
}

pub fn theme_segment(theme: i32) -> i32 {
    theme.clamp(0, 2)
}

pub fn trmode_segment(trmode: i32) -> i32 {
    trmode.clamp(0, 2)
}

pub fn max_value_for_segment(index: i32) -> Option<usize> {
    match index {
        0 => Some(5),
        1 => Some(7),
        2 => Some(9),
        _ => None,
    }
}

pub fn scale_value_for_segment(index: i32) -> Option<i32> {
    match index {
        0 => Some(90),
        1 => Some(100),
        2 => Some(115),
        _ => None,
    }
}

/// 在已有正文上只改一个键。空白文件按默认全文写。`debug` 不能从这里改。
pub fn apply_one_key(original: &str, key: &str, value: &str) -> Result<String, io::Error> {
    if key == "debug" || !KNOWN_KEYS.contains(&key) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "这个键不能从设置窗口写入",
        ));
    }
    if value.is_empty() || value.contains(['\n', '\r']) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "设置值不合法"));
    }
    let base = if is_blank(original) {
        DEFAULT_BODY.to_string()
    } else {
        original.to_string()
    };
    let mut found = false;
    let mut out = String::new();
    for line in base.lines() {
        if let Some((existing, _)) = split_kv(line) {
            if existing == key {
                out.push_str(key);
                out.push('=');
                out.push_str(value);
                out.push('\n');
                found = true;
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    if !found {
        out.push_str(key);
        out.push('=');
        out.push_str(value);
        out.push('\n');
    }
    Ok(out)
}

pub fn commit_key(dir: &Path, key: &str, value: &str) -> io::Result<()> {
    let path = settings_path(dir);
    let original = match read_text_file(&path)? {
        TextFile::InvalidUtf8 => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "settings.txt 不是合法的 UTF-8，没有写回",
            ));
        }
        // 文件还不存在时写出全部已知键。旧的 layout.txt 若是横排，要写进 layout，
        // 不能用默认的竖排把已经生效的横排盖掉。
        TextFile::Missing => seed_missing(dir)?,
        TextFile::Text(text) => text,
    };
    let body = apply_one_key(&original, key, value)?;
    write_atomic(&path, &body)
}

fn seed_missing(dir: &Path) -> io::Result<String> {
    let horizontal = match read_text_file(&dir.join("layout.txt"))? {
        TextFile::Text(text) => text.trim().eq_ignore_ascii_case("h"),
        _ => false,
    };
    if horizontal {
        apply_one_key("", "layout", "h")
    } else {
        Ok(String::new())
    }
}

pub fn write_atomic(path: &Path, body: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            fs::create_dir_all(dir)?;
        }
    }
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("settings.txt");
    let tmp = dir.join(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        unique_stamp()
    ));
    let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
    let write_result = (|| {
        file.write_all(body.as_bytes())?;
        file.sync_all()?;
        Ok(())
    })();
    drop(file);
    if let Err(err) = write_result {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = fs::remove_file(&tmp);
            Err(err)
        }
    }
}

fn apply_text(settings: &mut Settings, text: &str) {
    for line in text.lines() {
        let Some((key, value)) = split_kv(line) else {
            continue;
        };
        apply_kv(settings, key, value);
    }
}

fn apply_kv(settings: &mut Settings, key: &str, value: &str) {
    match key {
        "max" => {
            if let Ok(n) = value.parse::<usize>() {
                settings.max = n.clamp(3, 9);
            }
        }
        "scale" => {
            if let Ok(n) = value.parse::<i32>() {
                settings.scale = n.clamp(80, 140);
            }
        }
        "layout" => settings.horizontal = value.eq_ignore_ascii_case("h"),
        "theme" => {
            if let Ok(n) = value.parse::<i32>() {
                settings.theme = n.clamp(0, 2);
            }
        }
        "debug" => settings.debug = parse_bool(value),
        "cnpunct" => settings.cnpunct = parse_bool(value),
        "trmode" => {
            if let Ok(n) = value.parse::<i32>() {
                settings.trmode = n.clamp(0, 2);
            }
        }
        "accent" => {
            if let Ok(n) = value.parse::<i32>() {
                settings.accent = n.clamp(0, 4);
            }
        }
        "english" => settings.english = parse_bool(value),
        "emoji" => settings.emoji = parse_bool(value),
        _ => {}
    }
}

fn parse_bool(value: &str) -> bool {
    value == "1" || value.eq_ignore_ascii_case("true")
}

fn split_kv(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once('=')?;
    Some((key.trim(), value.trim()))
}

fn is_blank(text: &str) -> bool {
    text.lines().all(|line| line.trim().is_empty())
}

fn unique_stamp() -> u64 {
    static TICK: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let tick = TICK.fetch_add(1, Ordering::Relaxed);
    nanos ^ tick.rotate_left(17)
}

#[cfg(test)]
pub(crate) fn test_dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "pangyi-settings-{name}-{}-{}",
        std::process::id(),
        unique_stamp()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_ime_atomics() {
        let settings = Settings::default();
        assert_eq!(settings, parse_settings(""));
        assert_eq!(settings, parse_settings(DEFAULT_BODY));
        assert_eq!(default_body(), DEFAULT_BODY);
        assert!(!settings.horizontal);
        assert!(settings.cnpunct);
        assert!(!settings.debug);
        assert!(!settings.english);
        assert!(!settings.emoji);
    }

    #[test]
    fn clamps_and_keeps_the_previous_value_when_parse_fails() {
        let text = "\
max=1
max=99
max=nope
scale=10
scale=999
scale=80
theme=-4
theme=8
trmode=9
accent=-1
accent=4
english=TRUE
emoji=true
cnpunct=0
debug=1
layout=H
";
        let settings = parse_settings(text);
        assert_eq!(settings.max, 9, "99 夹到 9，随后的 nope 保持 9");
        assert_eq!(settings.scale, 80);
        assert_eq!(settings.theme, 2);
        assert_eq!(settings.trmode, 2);
        assert_eq!(settings.accent, 4);
        assert!(settings.english);
        assert!(settings.emoji);
        assert!(!settings.cnpunct);
        assert!(settings.debug);
        assert!(settings.horizontal);
    }

    #[test]
    fn negative_max_does_not_parse_as_usize() {
        // max 按 usize 读，负数读不进去，保持默认 9。
        // scale 按 i32 读，和输入法一样夹到 80–140，所以 -5 变成 80。
        let settings = parse_settings("max=-3\nscale=-5\n");
        assert_eq!(settings.max, 9);
        assert_eq!(settings.scale, 80);
    }

    #[test]
    fn duplicate_keys_last_one_wins_and_extra_equals_stays_in_the_value() {
        let settings = parse_settings("max=5\nmax=6\nnote=a=b\n");
        assert_eq!(settings.max, 6);
        let again = parse_settings("max=6=7\n");
        assert_eq!(again.max, 9);
    }

    #[test]
    fn layout_txt_applies_only_when_settings_have_no_layout_key() {
        let from_legacy = load_from_parts(None, Some(" h \n"));
        assert!(from_legacy.horizontal);
        let ignored = load_from_parts(Some("layout=v\n"), Some("h"));
        assert!(!ignored.horizontal);
        let missing_key = load_from_parts(Some("max=5\n"), Some("H"));
        assert!(missing_key.horizontal);
        assert_eq!(missing_key.max, 5);
        let other = load_from_parts(Some("max=5\n"), Some("vertical"));
        assert!(!other.horizontal);
    }

    #[test]
    fn segments_do_not_rewrite_legal_in_between_values() {
        assert_eq!(max_segment(3), 0);
        assert_eq!(max_segment(5), 0);
        assert_eq!(max_segment(6), 1);
        assert_eq!(max_segment(7), 1);
        assert_eq!(max_segment(8), 2);
        assert_eq!(max_segment(9), 2);
        assert_eq!(scale_segment(80), 0);
        assert_eq!(scale_segment(90), 0);
        assert_eq!(scale_segment(91), 1);
        assert_eq!(scale_segment(114), 1);
        assert_eq!(scale_segment(115), 2);
        assert_eq!(scale_segment(140), 2);
        assert_eq!(max_value_for_segment(1), Some(7));
        assert_eq!(scale_value_for_segment(0), Some(90));
        assert_eq!(max_value_for_segment(3), None);
        let stored = parse_settings("max=6\nscale=80\n");
        assert_eq!(stored.max, 6);
        assert_eq!(max_segment(stored.max), 1);
        assert_eq!(stored.scale, 80);
    }

    #[test]
    fn apply_one_key_preserves_unknown_lines_debug_and_order() {
        let original = "debug=true\nunknown=keep me\nmax=6\n# 注释\n\nscale=80\n";
        let next = apply_one_key(original, "cnpunct", "0").unwrap();
        assert!(next.starts_with("debug=true\nunknown=keep me\nmax=6\n# 注释\n\nscale=80\n"));
        assert!(next.ends_with("cnpunct=0\n"));
        let parsed = parse_settings(&next);
        assert!(parsed.debug);
        assert_eq!(parsed.max, 6);
        assert_eq!(parsed.scale, 80);
        assert!(!parsed.cnpunct);
        assert!(next.contains("unknown=keep me"));
        assert!(next.contains("# 注释"));
    }

    #[test]
    fn apply_replaces_every_copy_of_that_key_and_normalizes_newline() {
        let original = "max = 5\r\nmax=6\r\n";
        let next = apply_one_key(original, "max", "9").unwrap();
        assert_eq!(next, "max=9\nmax=9\n");
        assert!(!next.contains('\r'));
    }

    #[test]
    fn missing_file_writes_every_known_key_with_debug_off() {
        let next = apply_one_key("", "emoji", "1").unwrap();
        assert_eq!(
            next,
            "\
max=9
scale=100
layout=v
theme=0
english=0
debug=0
cnpunct=1
trmode=0
accent=0
emoji=1
"
        );
        assert!(apply_one_key("   \n\n", "theme", "2")
            .unwrap()
            .contains("debug=0\n"));
    }

    #[test]
    fn comment_only_file_is_kept_and_debug_cannot_be_committed() {
        let next = apply_one_key("# keep\n", "english", "1").unwrap();
        assert_eq!(next, "# keep\nenglish=1\n");
        assert!(apply_one_key("debug=1\n", "debug", "0").is_err());
        assert!(apply_one_key("max=9\n", "nope", "1").is_err());
        assert!(apply_one_key("max=9\n", "max", "1\n2").is_err());
    }

    #[test]
    fn round_trip_on_disk_keeps_unrelated_keys() {
        let dir = test_dir("round");
        fs::write(settings_path(&dir), "debug=1\nfoo=bar\nmax=6\n").unwrap();
        commit_key(&dir, "scale", "140").unwrap();
        let text = fs::read_to_string(settings_path(&dir)).unwrap();
        assert_eq!(text, "debug=1\nfoo=bar\nmax=6\nscale=140\n");
        let loaded = load_dir(&dir).unwrap();
        assert!(loaded.can_write);
        assert!(loaded.settings.debug);
        assert_eq!(loaded.settings.max, 6);
        assert_eq!(loaded.settings.scale, 140);
        assert!(dir.read_dir().unwrap().all(|entry| {
            let name = entry.unwrap().file_name().to_string_lossy().to_string();
            !name.ends_with(".tmp")
        }));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_utf8_is_not_read_and_is_not_overwritten() {
        let dir = test_dir("badutf");
        let path = settings_path(&dir);
        fs::write(&path, [0xff, 0xfe, b'm', b'a', b'x', b'=', b'5']).unwrap();
        let loaded = load_dir(&dir).unwrap();
        assert!(!loaded.can_write);
        assert_eq!(loaded.settings, Settings::default());
        let err = commit_key(&dir, "max", "7").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            fs::read(&path).unwrap(),
            vec![0xff, 0xfe, b'm', b'a', b'x', b'=', b'5']
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_does_not_create_a_missing_file() {
        let dir = test_dir("missing");
        let loaded = load_dir(&dir).unwrap();
        assert!(loaded.can_write);
        assert_eq!(loaded.settings, Settings::default());
        assert!(!settings_path(&dir).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_layout_file_is_read_but_not_written() {
        let dir = test_dir("layout");
        fs::write(dir.join("layout.txt"), "h\n").unwrap();
        let loaded = load_dir(&dir).unwrap();
        assert!(loaded.settings.horizontal);
        commit_key(&dir, "max", "5").unwrap();
        assert_eq!(fs::read_to_string(dir.join("layout.txt")).unwrap(), "h\n");
        let text = fs::read_to_string(settings_path(&dir)).unwrap();
        assert!(text.contains("layout=h\n"), "{text}");
        assert!(text.contains("max=5\n"), "{text}");
        assert!(!text.contains("layout=v"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn bools_accept_one_and_true_only() {
        let settings = parse_settings("english=yes\nemoji=false\ncnpunct=1\ndebug=TRUE\n");
        assert!(!settings.english);
        assert!(!settings.emoji);
        assert!(settings.cnpunct);
        assert!(settings.debug);
    }
}
