//! 编辑输入法已有的本机词库文件。

use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::Path;

use crate::settings_file::write_atomic;

#[path = "../../shared/lexicon_format.rs"]
mod format;
use format::{
    analyze_reading, parse_weight, validate_head, MAX_BYTES, MAX_DATA_LINES, MAX_GLOSS_CHARS,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LexiconKind {
    Words,
    Glosses,
}

impl LexiconKind {
    fn filename(self) -> &'static str {
        match self {
            Self::Words => "user-dict.tsv",
            Self::Glosses => "user-gloss.tsv",
        }
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct LexiconEntry {
    pub head: String,
    pub reading: String,
    pub russian: String,
    pub extra: String,
}

type Key = (String, String, String);

impl LexiconEntry {
    pub fn trigger(&self) -> String {
        analyze_reading(&self.reading)
            .map_or_else(|_| self.reading.clone(), |reading| reading.ascii)
    }

    fn key(&self, kind: LexiconKind) -> Result<Key, &'static str> {
        for field in [&self.head, &self.reading, &self.russian, &self.extra] {
            if field.chars().any(char::is_control) {
                return Err("词条中不能有制表符、换行或其他控制字符");
            }
        }
        if self.head.starts_with(['#', '\u{feff}']) {
            return Err("词头不能以 # 或 BOM 开头");
        }
        validate_head(&self.head)?;
        let reading = analyze_reading(&self.reading)?;
        if self.russian.chars().count() > MAX_GLOSS_CHARS {
            return Err("俄语超过 400 个字符");
        }
        if kind == LexiconKind::Words {
            parse_weight(&self.extra)?;
            Ok((self.head.clone(), reading.ascii, String::new()))
        } else if self.russian.is_empty() {
            Err("俄语不能为空")
        } else {
            Ok((self.head.clone(), reading.ascii, reading.tone))
        }
    }

    fn normalize(&mut self) {
        for field in [
            &mut self.head,
            &mut self.reading,
            &mut self.russian,
            &mut self.extra,
        ] {
            *field = field.trim().to_owned();
        }
    }

    fn line(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}",
            self.head, self.reading, self.russian, self.extra
        )
    }
}

fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

fn is_data(line: &str) -> bool {
    let line = line.trim();
    !line.is_empty() && !line.starts_with('#')
}

fn parse_line(line: &str, kind: LexiconKind) -> Result<(Key, LexiconEntry), &'static str> {
    let mut fields = line
        .trim_end_matches(['\r', '\n'])
        .split('\t')
        .map(str::trim);
    let head = fields.next().unwrap_or_default();
    let reading = fields.next().ok_or("至少需要词头和拼音两列")?;
    let russian = fields.next().unwrap_or_default();
    let extra = fields.next().unwrap_or_default();
    if fields.next().is_some() {
        return Err("每行最多四列");
    }
    let entry = LexiconEntry {
        head: head.into(),
        reading: reading.into(),
        russian: russian.into(),
        extra: extra.into(),
    };
    Ok((entry.key(kind)?, entry))
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn check_size(text: &str) -> io::Result<()> {
    if text.len() > MAX_BYTES
        || strip_bom(text).lines().filter(|line| is_data(line)).count() > MAX_DATA_LINES
    {
        return Err(invalid("词库最多 256 KiB、5000 行。请缩小词库后再保存"));
    }
    Ok(())
}

pub fn read_import(path: &Path) -> io::Result<String> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    let text = String::from_utf8(bytes).map_err(|_| invalid("请选择 UTF-8 编码的 TSV 文件"))?;
    check_size(&text)?;
    Ok(text)
}

fn read_local(path: &Path) -> io::Result<String> {
    match read_import(path) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        result => result,
    }
}

fn rows(text: &str, kind: LexiconKind) -> HashMap<Key, LexiconEntry> {
    strip_bom(text)
        .lines()
        .filter(|line| is_data(line))
        .filter_map(|line| parse_line(line, kind).ok())
        .collect()
}

pub fn lexicon_entries(dir: &Path, kind: LexiconKind) -> io::Result<Vec<LexiconEntry>> {
    let mut entries: Vec<_> = rows(&read_local(&dir.join(kind.filename()))?, kind)
        .into_values()
        .collect();
    entries.sort_by(|a, b| (&a.head, &a.reading).cmp(&(&b.head, &b.reading)));
    Ok(entries)
}

fn lock(dir: &Path) -> io::Result<File> {
    fs::create_dir_all(dir)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join("lexicon-edit.lock"))?;
    lock.lock()?;
    Ok(lock)
}

pub fn edit_lexicon(
    dir: &Path,
    kind: LexiconKind,
    original: Option<&LexiconEntry>,
    replacement: Option<LexiconEntry>,
) -> io::Result<()> {
    let mut replacement = replacement;
    let key = replacement
        .as_mut()
        .map(|entry| {
            entry.normalize();
            entry.key(kind).map_err(invalid)
        })
        .transpose()?;
    let original_key = original
        .map(|entry| entry.key(kind).map_err(invalid))
        .transpose()?;

    let _lock = lock(dir)?;
    let path = dir.join(kind.filename());
    let text = read_local(&path)?;
    let current = rows(&text, kind);
    if let (Some(original), Some(old_key)) = (original, &original_key) {
        if current.get(old_key) != Some(original) {
            return Err(invalid("这条词已被修改，请重新选择词条后再操作"));
        }
    }
    if let Some(key) = &key {
        if Some(key) != original_key.as_ref() && current.contains_key(key) {
            return Err(invalid("这个词头和拼音已经存在，请选择原词条进行编辑"));
        }
    }

    // 原位替换生效的最后一行，删除同键的旧行，保留其余行和排序。
    let mut output = String::new();
    let body = strip_bom(&text);
    if body.len() != text.len() {
        output.push('\u{feff}');
    }
    let matching = |line: &str| {
        original_key.as_ref().is_some_and(|key| {
            is_data(line) && parse_line(line, kind).is_ok_and(|(found, _)| &found == key)
        })
    };
    let last = body
        .split_inclusive('\n')
        .enumerate()
        .filter_map(|(index, line)| matching(line).then_some(index))
        .last();
    for (index, line) in body.split_inclusive('\n').enumerate() {
        if matching(line) {
            if Some(index) == last {
                if let Some(entry) = &replacement {
                    output.push_str(&entry.line());
                    output.push_str(if line.ends_with("\r\n") {
                        "\r\n"
                    } else if line.ends_with('\n') {
                        "\n"
                    } else {
                        ""
                    });
                }
            }
        } else {
            output.push_str(line);
        }
    }
    if last.is_none() {
        if let Some(entry) = &replacement {
            let newline = newline_style(&output);
            append_entry(&mut output, entry, newline);
        }
    }
    check_size(&output)?;
    write_atomic(&path, &output)
}

fn newline_style(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

fn append_entry(text: &mut String, entry: &LexiconEntry, newline: &str) {
    if !strip_bom(text).is_empty() && !text.ends_with('\n') {
        text.push_str(newline);
    }
    text.push_str(&entry.line());
    text.push_str(newline);
}

pub fn import_lexicon(dir: &Path, kind: LexiconKind, text: &str) -> io::Result<(usize, usize)> {
    check_size(text)?;
    let mut incoming = Vec::new();
    let mut seen = HashSet::new();
    for (index, line) in strip_bom(text)
        .lines()
        .enumerate()
        .filter(|(_, line)| is_data(line))
    {
        let row = parse_line(line, kind)
            .map_err(|message| invalid(format!("导入文件第 {} 行：{message}", index + 1)))?;
        if !seen.insert(row.0.clone()) {
            return Err(invalid(format!("导入文件第 {} 行有重复词条", index + 1)));
        }
        incoming.push(row);
    }
    if incoming.is_empty() {
        return Err(invalid("文件没有可导入的词条"));
    }

    let _lock = lock(dir)?;
    let path = dir.join(kind.filename());
    let mut output = read_local(&path)?;
    let current = rows(&output, kind);
    let newline = newline_style(&output);
    let mut added = 0;
    let mut skipped = 0;
    for (key, entry) in incoming {
        if current.contains_key(&key) {
            skipped += 1;
        } else {
            append_entry(&mut output, &entry, newline);
            added += 1;
        }
    }
    check_size(&output)?;
    if added > 0 {
        write_atomic(&path, &output)?;
    }
    Ok((added, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_file::test_dir;

    fn entry(head: &str, reading: &str, russian: &str, extra: &str) -> LexiconEntry {
        LexiconEntry {
            head: head.into(),
            reading: reading.into(),
            russian: russian.into(),
            extra: extra.into(),
        }
    }

    #[test]
    fn editor_preserves_legacy_file_and_rejects_a_stale_edit() {
        let dir = test_dir("lex-edit");
        let kind = LexiconKind::Words;
        let path = dir.join(kind.filename());
        fs::write(
            &path,
            "\u{feff}# 我的词\r\n女\tnv\tстарое\r\n坏行\r\n女\tnǚ\tженщина\r\n六月\tliuyue",
        )
        .unwrap();
        let selected = lexicon_entries(&dir, kind)
            .unwrap()
            .into_iter()
            .find(|row| row.head == "女")
            .unwrap();
        assert_eq!(selected.russian, "женщина");

        let changed = entry("女", "nǚ", "девушка", "100");
        edit_lexicon(&dir, kind, Some(&selected), Some(changed.clone())).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "\u{feff}# 我的词\r\n坏行\r\n女\tnǚ\tдевушка\t100\r\n六月\tliuyue"
        );
        assert!(edit_lexicon(&dir, kind, Some(&selected), None).is_err());
        edit_lexicon(&dir, kind, Some(&changed), None).unwrap();
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "\u{feff}# 我的词\r\n坏行\r\n六月\tliuyue"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn imports_keep_personal_words_and_validate_the_whole_file() {
        let dir = test_dir("lex-import");
        let kind = LexiconKind::Words;
        let path = dir.join(kind.filename());
        edit_lexicon(
            &dir,
            kind,
            None,
            Some(entry("登机口", "dengjikou", "мой перевод", "100")),
        )
        .unwrap();
        for (text, expected) in [
            (include_str!("../../dictionaries/travel.tsv"), (11, 1)),
            (include_str!("../../dictionaries/food.tsv"), (12, 0)),
            (include_str!("../../dictionaries/work.tsv"), (12, 0)),
        ] {
            assert_eq!(import_lexicon(&dir, kind, text).unwrap(), expected);
        }
        assert_eq!(
            import_lexicon(&dir, kind, include_str!("../../dictionaries/travel.tsv")).unwrap(),
            (0, 12)
        );
        assert!(lexicon_entries(&dir, kind)
            .unwrap()
            .iter()
            .any(|row| row.head == "登机口" && row.russian == "мой перевод"));
        let before = fs::read(&path).unwrap();
        let error = import_lexicon(&dir, kind, "甲\tjia\tтест\n乙\twrong key\tтест\n").unwrap_err();
        assert!(error.to_string().contains("第 2 行"));
        assert_eq!(fs::read(path).unwrap(), before);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn glosses_can_target_separate_tones_and_alternate_readings() {
        let dir = test_dir("lex-gloss");
        let kind = LexiconKind::Glosses;
        let text = "长\tchang\tдлина\n长\tcháng\tдлинный\n长\tzhǎng\tрасти\n";
        assert_eq!(import_lexicon(&dir, kind, text).unwrap(), (3, 0));
        assert_eq!(
            import_lexicon(&dir, kind, "长\tchang2\tновый\n").unwrap(),
            (0, 1)
        );
        assert!(!dir.join(LexiconKind::Words.filename()).exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
