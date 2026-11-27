//! 编辑输入法已有的本机词库文件。

use std::collections::{HashMap, HashSet};
#[cfg(test)]
use std::fs;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::settings_file::write_atomic;

#[path = "../../shared/lexicon_backup.rs"]
mod backup;
use backup::{lock, snapshot_locked, GLOSSES_FILE, WORDS_FILE};

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
            Self::Words => WORDS_FILE,
            Self::Glosses => GLOSSES_FILE,
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
    let mut fields = line.trim_end_matches(['\r', '\n']).split('\t');
    if fields
        .clone()
        .any(|field| field.chars().any(char::is_control))
    {
        return Err("包含非法控制字符");
    }
    let columns = fields.clone().count();
    if columns < if kind == LexiconKind::Words { 2 } else { 3 } || columns > 4 {
        return Err("列数不符：自定义词需 2–4 列，释义覆盖需 3–4 列");
    }
    let mut fields = fields.by_ref().map(str::trim);
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

const MAX_IMPORT_BYTES: usize = 4 * 1024 * 1024;

fn read_capped(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(invalid(format!("文件超过 {} KiB", limit / 1024)));
    }
    Ok(bytes)
}

pub fn read_import(path: &Path) -> io::Result<Vec<u8>> {
    read_capped(path, MAX_IMPORT_BYTES)
}

fn read_local(path: &Path) -> io::Result<String> {
    let bytes = match read_capped(path, MAX_BYTES) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(String::new()),
        result => result?,
    };
    let text = String::from_utf8(bytes).map_err(|_| invalid("本机词库不是 UTF-8"))?;
    check_size(&text)?;
    Ok(text)
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

pub fn export_backup(dir: &Path) -> io::Result<PathBuf> {
    let _lock = lock(dir)?;
    snapshot_locked(dir, true)?.ok_or_else(|| invalid("没有生成备份"))
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

#[derive(Clone, Copy)]
pub enum DuplicatePolicy {
    Skip,
    Replace,
    Merge,
}

pub struct ImportPreview {
    kind: LexiconKind,
    before: String,
    after: String,
    pub added: usize,
    pub skipped: usize,
    pub replaced: usize,
    pub merged: usize,
    pub issues: Vec<String>,
}

impl ImportPreview {
    pub fn has_changes(&self) -> bool {
        self.before != self.after
    }
}

fn merge_entry(existing: &LexiconEntry, incoming: &LexiconEntry) -> LexiconEntry {
    let mut merged = existing.clone();
    let mut seen = HashSet::new();
    merged.russian = [&existing.russian, &incoming.russian]
        .into_iter()
        .flat_map(|text| text.split('|').map(str::trim))
        .filter(|part| !part.is_empty() && seen.insert(*part))
        .collect::<Vec<_>>()
        .join(" | ");
    if merged.extra.is_empty() {
        merged.extra.clone_from(&incoming.extra);
    }
    merged
}

pub fn preview_import(
    dir: &Path,
    kind: LexiconKind,
    bytes: &[u8],
    policy: DuplicatePolicy,
) -> io::Result<ImportPreview> {
    if bytes.len() > MAX_IMPORT_BYTES {
        return Err(invalid("导入文件超过 4 MiB"));
    }
    let before = read_local(&dir.join(kind.filename()))?;
    let baseline = rows(&before, kind);
    let mut current = baseline.clone();
    let mut updates = HashMap::new();
    let mut new_order = Vec::new();
    let mut added = 0;
    let mut skipped = 0;
    let mut replaced = HashSet::new();
    let mut merged = HashSet::new();
    let mut issues = Vec::new();
    let mut omitted = 0;

    for (index, raw) in bytes.split(|&byte| byte == b'\n').enumerate() {
        let result = (|| {
            let line = std::str::from_utf8(raw).map_err(|_| "不是有效的 UTF-8")?;
            let line = if index == 0 { strip_bom(line) } else { line };
            let line = line.strip_suffix('\r').unwrap_or(line);
            if line.trim().is_empty() || line.trim_start().starts_with('#') {
                return Ok(None);
            }
            parse_line(line, kind).map(Some)
        })();
        let (key, incoming) = match result {
            Ok(Some(row)) => row,
            Ok(None) => continue,
            Err(message) => {
                skipped += 1;
                if issues.len() < 100 {
                    issues.push(format!("第 {} 行：{message}", index + 1));
                } else {
                    omitted += 1;
                }
                continue;
            }
        };
        let entry = match current.get(&key) {
            None => {
                added += 1;
                new_order.push(key.clone());
                incoming
            }
            Some(existing) => {
                let next = match policy {
                    DuplicatePolicy::Skip => existing.clone(),
                    DuplicatePolicy::Replace => incoming,
                    DuplicatePolicy::Merge => merge_entry(existing, &incoming),
                };
                if let Err(message) = next.key(kind) {
                    skipped += 1;
                    if issues.len() < 100 {
                        issues.push(format!("第 {} 行：合并后{message}", index + 1));
                    } else {
                        omitted += 1;
                    }
                    continue;
                }
                if &next == existing {
                    skipped += 1;
                    continue;
                }
                match policy {
                    DuplicatePolicy::Replace => {
                        replaced.insert(key.clone());
                    }
                    DuplicatePolicy::Merge => {
                        merged.insert(key.clone());
                    }
                    DuplicatePolicy::Skip => {}
                }
                next
            }
        };
        current.insert(key.clone(), entry.clone());
        updates.insert(key, entry);
    }
    if omitted > 0 {
        issues.push(format!("另有 {omitted} 条问题"));
    }

    updates.retain(|key, row| baseline.get(key) != Some(row));
    replaced.retain(|key| baseline.contains_key(key) && updates.contains_key(key));
    merged.retain(|key| baseline.contains_key(key) && updates.contains_key(key));
    let mut after = String::new();
    let body = strip_bom(&before);
    if body.len() != before.len() {
        after.push('\u{feff}');
    }
    let mut seen = HashSet::new();
    // 倒序识别最后一条生效记录，输出保持原文件顺序。
    let mut output = Vec::new();
    for line in body.split_inclusive('\n').rev() {
        if let Some((key, row)) = is_data(line)
            .then(|| parse_line(line, kind).ok())
            .flatten()
            .and_then(|(key, _)| updates.get(&key).map(|row| (key, row)))
        {
            if seen.insert(key) {
                output.push(format!("{}{}", row.line(), line_ending(line)));
            }
        } else {
            output.push(line.to_owned());
        }
    }
    after.extend(output.into_iter().rev());
    let newline = newline_style(&before);
    for key in new_order {
        append_entry(&mut after, &updates[&key], newline);
    }
    check_size(&after)?;
    Ok(ImportPreview {
        kind,
        before,
        after,
        added,
        skipped,
        replaced: replaced.len(),
        merged: merged.len(),
        issues,
    })
}

fn line_ending(line: &str) -> &'static str {
    if line.ends_with("\r\n") {
        "\r\n"
    } else if line.ends_with('\n') {
        "\n"
    } else {
        ""
    }
}

pub fn commit_import(dir: &Path, preview: &ImportPreview) -> io::Result<()> {
    let _lock = lock(dir)?;
    let path = dir.join(preview.kind.filename());
    if read_local(&path)? != preview.before {
        return Err(invalid("预览后词库已修改，请重新生成预览"));
    }
    if preview.has_changes() {
        snapshot_locked(dir, false)?;
        write_atomic(&path, &preview.after)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_file::test_dir;

    fn import_lexicon(dir: &Path, kind: LexiconKind, text: &str) -> io::Result<(usize, usize)> {
        let plan = preview_import(dir, kind, text.as_bytes(), DuplicatePolicy::Skip)?;
        commit_import(dir, &plan)?;
        Ok((plan.added, plan.skipped))
    }

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
        let plan = preview_import(
            &dir,
            kind,
            b"\xef\xbb\xbf\xe7\x94\xb2\tjia\r\n\r\n\xff\n",
            DuplicatePolicy::Skip,
        )
        .unwrap();
        assert_eq!((plan.added, plan.skipped), (1, 1));
        assert!(plan.issues[0].contains("第 3 行"));
        assert_eq!(fs::read(&path).unwrap(), before);
        commit_import(&dir, &plan).unwrap();
        assert!(lexicon_entries(&dir, kind)
            .unwrap()
            .iter()
            .any(|row| row.head == "甲"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn duplicate_policies_and_stale_preview() {
        let dir = test_dir("lex-policies");
        let kind = LexiconKind::Words;
        import_lexicon(&dir, kind, "女\tnv\tженщина\t100\n").unwrap();
        let input = "女\tnǚ\tдевушка | женщина\t50\n甲\tjia\tпервый\n甲\tjia\tновый";
        let skip = preview_import(&dir, kind, input.as_bytes(), DuplicatePolicy::Skip).unwrap();
        assert_eq!((skip.added, skip.skipped, skip.replaced), (1, 2, 0));
        let replace =
            preview_import(&dir, kind, input.as_bytes(), DuplicatePolicy::Replace).unwrap();
        assert_eq!((replace.added, replace.replaced), (1, 1));
        assert!(replace.after.contains("女\tnǚ\tдевушка | женщина\t50"));
        let merge = preview_import(&dir, kind, input.as_bytes(), DuplicatePolicy::Merge).unwrap();
        assert_eq!((merge.added, merge.merged), (1, 1));
        commit_import(&dir, &merge).unwrap();
        assert!(merge.after.contains("女\tnv\tженщина | девушка\t100"));
        assert!(merge.after.contains("甲\tjia\tпервый | новый"));
        assert!(commit_import(&dir, &replace).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reports_column_size_and_invalid_character_per_line() {
        let dir = test_dir("lex-alformed");
        let bytes = format!(
            "甲\n乙\tyi\t{}\n丙\tbi ng\n丁\tding\tx\u{0}\n\n戊\twu",
            "я".repeat(401)
        );
        let plan = preview_import(
            &dir,
            LexiconKind::Words,
            bytes.as_bytes(),
            DuplicatePolicy::Skip,
        )
        .unwrap();
        assert_eq!((plan.added, plan.skipped), (1, 4));
        for (index, issue) in plan.issues.iter().enumerate() {
            assert!(issue.starts_with(&format!("第 {} 行", index + 1)));
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn backups_preserve_both_files_and_only_prune_automatic_history() {
        let dir = test_dir("lex-backups");
        let words = dir.join(WORDS_FILE);
        let gloss = dir.join(GLOSSES_FILE);
        fs::write(&words, "\u{feff}甲\tjia\r\n").unwrap();
        fs::write(&gloss, "甲\tjia\tпервый\r\n").unwrap();
        let exported = export_backup(&dir).unwrap();
        assert_eq!(
            fs::read(exported.join(WORDS_FILE)).unwrap(),
            fs::read(&words).unwrap()
        );
        assert_eq!(
            fs::read(exported.join(GLOSSES_FILE)).unwrap(),
            fs::read(&gloss).unwrap()
        );
        for index in 0..7 {
            let text = format!("甲\tjia\t{index}");
            let plan = preview_import(
                &dir,
                LexiconKind::Words,
                text.as_bytes(),
                DuplicatePolicy::Replace,
            )
            .unwrap();
            commit_import(&dir, &plan).unwrap();
        }
        backup::automatic_upgrade_backup(&dir).unwrap();
        let root = exported.parent().unwrap();
        let mut automatic = fs::read_dir(root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("auto-"))
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        automatic.sort();
        assert_eq!(automatic.len(), 5);
        assert_eq!(
            fs::read(automatic.last().unwrap().join(WORDS_FILE)).unwrap(),
            fs::read(&words).unwrap()
        );
        assert!(exported.is_dir());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn import_write_waits_for_successful_backup() {
        let dir = test_dir("lex-backup-error");
        let path = dir.join(WORDS_FILE);
        let old = "甲\tjia\n";
        fs::write(&path, old).unwrap();
        fs::write(dir.join("lexicon-backups"), "占用").unwrap();
        let plan = preview_import(
            &dir,
            LexiconKind::Words,
            "乙\tyi".as_bytes(),
            DuplicatePolicy::Skip,
        )
        .unwrap();
        assert!(commit_import(&dir, &plan).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), old);
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
