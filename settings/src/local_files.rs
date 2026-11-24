//! 词库页只数用户电脑上的文件。
//!
//! 不读内置词库表，也不把词条总数写死。短语模板和输入法
//! `dict::ensure_phrases_file` 写的是同一份，这样打开按钮不必去链接词库。

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::settings_file::{read_text_file, TextFile};

/// 与 `ime/src/dict.rs` 里第一次生成 `phrases.txt` 的模板相同。
pub const PHRASE_TEMPLATE: &str = "# 自定义短语：每行「触发字母 TAB 中文 TAB 俄语(可省)」\n# 打 dz 第一个就是「电子邮箱」。改完保存立刻生效。\ndz\t电子邮箱\tэлектронная почта\nwxh\t微信号\tWeChat ID\n";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalCounts {
    pub phrases: Option<usize>,
    pub user_dict: Option<usize>,
    pub user_gloss: Option<usize>,
    pub packs: Option<usize>,
    pub learned: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenTarget {
    File(PathBuf),
    Directory(PathBuf),
}

pub fn phrase_path(dir: &Path) -> PathBuf {
    dir.join("phrases.txt")
}

pub fn packs_dir(dir: &Path) -> PathBuf {
    dir.join("packs")
}

pub fn load_counts(dir: &Path) -> LocalCounts {
    LocalCounts {
        phrases: count_file(&phrase_path(dir)),
        user_dict: count_file(&dir.join("user-dict.tsv")),
        user_gloss: count_file(&dir.join("user-gloss.tsv")),
        learned: count_file(&dir.join("learned.txt")),
        packs: count_pack_files(&packs_dir(dir)),
    }
}

pub fn format_count(count: Option<usize>, unit: &str) -> String {
    match count {
        Some(n) => format!("{n} {unit}"),
        None => "还没有".to_string(),
    }
}

/// 去掉 BOM 后，空行和以 `#` 开头的行不计数。不套用输入法内部的行数上限。
pub fn count_data_lines(text: &str) -> usize {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    text.lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !trimmed.starts_with('#')
        })
        .count()
}

pub fn count_pack_files(dir: &Path) -> Option<usize> {
    if !dir.exists() {
        return None;
    }
    let entries = fs::read_dir(dir).ok()?;
    let mut count = 0usize;
    for entry in entries {
        let entry = entry.ok()?;
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if name.ends_with(".tsv") && entry.path().is_file() {
            count += 1;
        }
    }
    Some(count)
}

/// 文件不存在时写下模板。已经存在则原样留下，包括用户改过的内容。
pub fn ensure_phrases_file(path: &Path) -> io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            fs::create_dir_all(dir)?;
        }
    }
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => file.write_all(PHRASE_TEMPLATE.as_bytes()),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(err) => Err(err),
    }
}

pub fn existing_file_or_dir(file: &Path, dir: &Path) -> OpenTarget {
    if file.is_file() {
        OpenTarget::File(file.to_path_buf())
    } else {
        OpenTarget::Directory(dir.to_path_buf())
    }
}

fn count_file(path: &Path) -> Option<usize> {
    match read_text_file(path).ok()? {
        TextFile::Text(text) => Some(count_data_lines(&text)),
        TextFile::Missing | TextFile::InvalidUtf8 => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn phrase_template_matches_the_ime_demo() {
        assert!(PHRASE_TEMPLATE.contains("dz\t电子邮箱\tэлектронная почта\n"));
        assert!(PHRASE_TEMPLATE.contains("wxh\t微信号\tWeChat ID\n"));
        assert!(PHRASE_TEMPLATE.starts_with("# 自定义短语："));
        assert_eq!(count_data_lines(PHRASE_TEMPLATE), 2);
    }

    #[test]
    fn data_lines_skip_comments_blank_lines_and_a_bom() {
        let text = "\u{feff}# 注释\r\n\r\n  # 也是注释\r\n甲\tjia\r\n乙\tyi\r\n";
        assert_eq!(count_data_lines(text), 2);
        assert_eq!(count_data_lines(""), 0);
    }

    #[test]
    fn counts_local_files_and_says_missing_when_absent() {
        let dir = crate::settings_file::test_dir("counts");
        let counts = load_counts(&dir);
        assert_eq!(counts.phrases, None);
        assert_eq!(counts.user_dict, None);
        assert_eq!(counts.packs, None);
        assert_eq!(format_count(None, "行"), "还没有");

        fs::write(phrase_path(&dir), PHRASE_TEMPLATE).unwrap();
        fs::write(dir.join("user-dict.tsv"), "# 头\n张三\tzhangsan\t\n\n").unwrap();
        fs::write(dir.join("user-gloss.tsv"), "的\tde\tчастица\n").unwrap();
        fs::write(dir.join("learned.txt"), "邮箱\t3\n").unwrap();
        fs::create_dir(packs_dir(&dir)).unwrap();
        fs::write(packs_dir(&dir).join("Smile.TSV"), "x\ty\n").unwrap();
        fs::write(packs_dir(&dir).join("notes.txt"), "nope").unwrap();

        let counts = load_counts(&dir);
        assert_eq!(counts.phrases, Some(2));
        assert_eq!(counts.user_dict, Some(1));
        assert_eq!(counts.user_gloss, Some(1));
        assert_eq!(counts.learned, Some(1));
        assert_eq!(counts.packs, Some(1));
        assert_eq!(format_count(counts.packs, "个"), "1 个");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unreadable_text_counts_as_absent() {
        let dir = crate::settings_file::test_dir("badcount");
        fs::write(dir.join("user-dict.tsv"), [0xff, 0xfe]).unwrap();
        assert_eq!(load_counts(&dir).user_dict, None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn phrase_file_is_created_once_and_user_dict_is_not() {
        let dir = crate::settings_file::test_dir("phrase");
        let path = phrase_path(&dir);
        ensure_phrases_file(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), PHRASE_TEMPLATE);
        fs::write(&path, "mine\t我的\tмоё\n").unwrap();
        ensure_phrases_file(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "mine\t我的\tмоё\n");

        let dict = dir.join("user-dict.tsv");
        assert_eq!(
            existing_file_or_dir(&dict, &dir),
            OpenTarget::Directory(dir.clone())
        );
        fs::write(&dict, "# only\n").unwrap();
        assert_eq!(existing_file_or_dir(&dict, &dir), OpenTarget::File(dict));
        assert!(!dir.join("user-gloss.tsv").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_pack_directory_counts_as_zero() {
        let dir = crate::settings_file::test_dir("packs");
        fs::create_dir(packs_dir(&dir)).unwrap();
        assert_eq!(count_pack_files(&packs_dir(&dir)), Some(0));
        assert_eq!(format_count(Some(0), "个"), "0 个");
        let _ = fs::remove_dir_all(&dir);
    }
}
