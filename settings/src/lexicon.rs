//! 编辑输入法已有的本机词库文件。

use std::collections::{HashMap, HashSet};
#[cfg(test)]
use std::fs;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::settings_file::write_atomic;

mod history;
pub use history::{undo_action, undo_last};

#[path = "../../shared/lexicon_backup.rs"]
pub(crate) mod backup;
use backup::{lock, snapshot_locked, GLOSSES_FILE, WORDS_FILE};

#[path = "../../shared/lexicon_format.rs"]
mod format;
use format::{
    analyze_reading, parse_weight, validate_head, MAX_BYTES, MAX_DATA_LINES, MAX_GLOSS_CHARS,
};

#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

#[derive(Clone, Default, Hash, PartialEq, Eq)]
pub struct LexiconEntry {
    pub head: String,
    pub reading: String,
    pub russian: String,
    pub extra: String,
    pub theme: String,
}

type Key = (String, String, String);

impl LexiconEntry {
    pub fn trigger(&self) -> String {
        analyze_reading(&self.reading)
            .map_or_else(|_| self.reading.clone(), |reading| reading.ascii)
    }

    fn key(&self, kind: LexiconKind) -> Result<Key, &'static str> {
        for field in [
            &self.head,
            &self.reading,
            &self.russian,
            &self.extra,
            &self.theme,
        ] {
            if field.chars().any(char::is_control) {
                return Err("词条中不能有制表符、换行或其他控制字符");
            }
        }
        if self.head.starts_with(['#', '\u{feff}']) {
            return Err("词头不能以 # 或 BOM 开头");
        }
        validate_head(&self.head)?;
        if self.theme.chars().count() > 80 {
            return Err("主题超过 80 个字符");
        }
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
            &mut self.theme,
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
    parse_record(line, kind, false).map(|(key, entry, _)| (key, entry))
}

fn parse_record(
    line: &str,
    kind: LexiconKind,
    default_weight: bool,
) -> Result<(Key, LexiconEntry, bool), &'static str> {
    let fields: Vec<_> = line.split('\t').collect();
    if fields.len() < if kind == LexiconKind::Words { 2 } else { 3 } || fields.len() > 4 {
        return Err("列数不符：自定义词需 2–4 列，释义覆盖需 3–4 列");
    }
    let import_weight = default_weight && kind == LexiconKind::Words;
    let extra = fields.get(3).copied().unwrap_or_default();
    let used_default = import_weight
        && (extra.trim().is_empty()
            || extra.chars().any(char::is_control)
            || parse_weight(extra.trim()).is_err());
    if fields
        .iter()
        .enumerate()
        .any(|(index, field)| !(index == 3 && used_default) && field.chars().any(char::is_control))
    {
        return Err("包含非法控制字符");
    }
    let entry = LexiconEntry {
        head: fields[0].trim().into(),
        reading: fields[1].trim().into(),
        russian: fields.get(2).copied().unwrap_or_default().trim().into(),
        extra: if used_default {
            String::new()
        } else {
            extra.trim().into()
        },
        theme: String::new(),
    };
    Ok((entry.key(kind)?, entry, used_default))
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

pub(crate) fn read_capped(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
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

const THEME_TAG: &str = "# pangyi-theme\t";

fn theme_tag(row: &LexiconEntry) -> String {
    format!("{THEME_TAG}{}\t{}\t{}", row.head, row.reading, row.theme)
}

fn theme_line(line: &str, kind: LexiconKind) -> Option<(Key, &str)> {
    let (head, rest) = line
        .strip_prefix(THEME_TAG)?
        .trim_end_matches(['\r', '\n'])
        .split_once('\t')?;
    let (reading, theme) = rest.split_once('\t')?;
    let reading = analyze_reading(reading).ok()?;
    if theme.is_empty() || theme.chars().count() > 80 || theme.chars().any(char::is_control) {
        return None;
    }
    let tone = if kind == LexiconKind::Words {
        String::new()
    } else {
        reading.tone
    };
    Some(((head.to_owned(), reading.ascii, tone), theme))
}

fn rows(text: &str, kind: LexiconKind) -> HashMap<Key, LexiconEntry> {
    let body = strip_bom(text);
    let mut entries: HashMap<_, _> = body
        .lines()
        .filter(|line| is_data(line))
        .filter_map(|line| parse_line(line, kind).ok())
        .collect();
    for (key, theme) in body.lines().filter_map(|line| theme_line(line, kind)) {
        if let Some(row) = entries.get_mut(&key) {
            row.theme = theme.into();
        }
    }
    entries
}

pub fn crowded_reading(reading: &str, rows: &[LexiconEntry]) -> bool {
    const BUSY_KEYS: &str = include_str!(concat!(env!("OUT_DIR"), "/busy-readings.txt"));
    let Ok(reading) = analyze_reading(reading) else {
        return false;
    };
    BUSY_KEYS.lines().any(|key| key == reading.ascii)
        || rows
            .iter()
            .filter(|row| row.trigger() == reading.ascii)
            .take(8)
            .count()
            == 8
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

    let changed_weight = kind == LexiconKind::Words
        && original
            .zip(replacement.as_ref())
            .is_some_and(|(old, new)| effective_weight(old) != effective_weight(new));
    let (updates, append) = match (original_key, replacement.as_ref()) {
        (Some(old), row) => (HashMap::from([(old, row)]), Vec::new()),
        (None, row) => (HashMap::new(), row.into_iter().collect()),
    };
    let output = rewrite_rows(&text, kind, &updates, append);
    check_size(&output)?;
    if changed_weight {
        history::save_change(dir, kind, "调整权重".into(), text, output)
    } else {
        write_atomic(&path, &output)
    }
}

/// Add a candidate without replacing any existing custom entry.
pub fn quick_add_word(dir: &Path, candidate: crate::quick_add::Candidate) -> io::Result<bool> {
    add_word_with_undo(dir, LexiconEntry {
        head: candidate.head,
        reading: candidate.reading,
        russian: candidate.russian,
        ..Default::default()
    })
}

pub(crate) fn add_word_with_undo(dir: &Path, mut entry: LexiconEntry) -> io::Result<bool> {
    entry.normalize();
    let kind = LexiconKind::Words;
    let key = entry.key(kind).map_err(invalid)?;
    let _lock = lock(dir)?;
    let before = read_local(&dir.join(kind.filename()))?;
    if rows(&before, kind).contains_key(&key) {
        return Ok(false);
    }
    let after = rewrite_rows(&before, kind, &HashMap::new(), vec![&entry]);
    check_size(&after)?;
    snapshot_locked(dir, false)?;
    history::save_required_change(dir, kind, "添加 1 条".into(), before, after)?;
    Ok(true)
}

fn effective_weight(row: &LexiconEntry) -> i64 {
    parse_weight(&row.extra).ok().flatten().unwrap_or(0)
}

/// Move a saved custom word within the ranked list for its normalized reading.
pub fn move_weight(
    dir: &Path,
    selected: &LexiconEntry,
    up: bool,
) -> io::Result<Option<LexiconEntry>> {
    let kind = LexiconKind::Words;
    let key = selected.key(kind).map_err(invalid)?;
    let _lock = lock(dir)?;
    let path = dir.join(kind.filename());
    let before = read_local(&path)?;
    let current = rows(&before, kind);
    if current.get(&key) != Some(selected) {
        return Err(invalid("这条词已被修改，请重新选择后再调整"));
    }

    // The IME resolves duplicates with the final physical row and breaks ties by that row.
    let selected_reading = key.1;
    let mut seen = HashSet::new();
    let mut slots: Vec<_> = strip_bom(&before)
        .lines()
        .rev()
        .filter(|line| is_data(line))
        .filter_map(|line| parse_line(line, kind).ok().map(|(key, _)| key))
        .filter(|key| key.1 == selected_reading && seen.insert(key.clone()))
        .collect();
    slots.reverse();
    let mut ranked: Vec<_> = slots.iter().map(|key| &current[key]).collect();
    ranked.sort_by_key(|row| std::cmp::Reverse(effective_weight(row)));
    let index = ranked.iter().position(|row| *row == selected).unwrap();
    let neighbor = if up {
        index.checked_sub(1)
    } else {
        index.checked_add(1).filter(|&i| i < ranked.len())
    };
    let Some(neighbor) = neighbor else {
        return Ok(None);
    };
    let mut moved: Vec<_> = ranked.iter().map(|row| (*row).clone()).collect();
    moved.swap(index, neighbor);
    // Keep the weight of each rank position. Reorder the file slots as well so equal
    // weights have a deterministic order and no candidate is moved into another band.
    for (row, original) in moved.iter_mut().zip(ranked) {
        row.extra.clone_from(&original.extra);
    }
    let updates = slots.into_iter().zip(moved.iter().map(Some)).collect();
    let after = rewrite_rows(&before, kind, &updates, Vec::new());
    check_size(&after)?;
    history::save_change(dir, kind, "调整权重顺序".into(), before, after)?;
    Ok(Some(moved.swap_remove(neighbor)))
}

fn rewrite_rows(
    text: &str,
    kind: LexiconKind,
    updates: &HashMap<Key, Option<&LexiconEntry>>,
    appended: Vec<&LexiconEntry>,
) -> String {
    let body = strip_bom(text);
    let tagged_keys: HashSet<_> = updates
        .keys()
        .cloned()
        .chain(
            updates
                .values()
                .flatten()
                .chain(appended.iter())
                .filter_map(|row| row.key(kind).ok()),
        )
        .collect();
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for line in body.split_inclusive('\n').rev() {
        if let Some((key, _)) = theme_line(line, kind) {
            if tagged_keys.contains(&key) {
                continue;
            }
        }
        if is_data(line) {
            if let Ok((key, _)) = parse_line(without_ending(line), kind) {
                if let Some(row) = updates.get(&key) {
                    if let Some(row) = row.filter(|_| seen.insert(key)) {
                        output.push(format!("{}{}", row.line(), line_ending(line)));
                    }
                    continue;
                }
            }
        }
        output.push(line.to_owned());
    }
    let mut after = String::new();
    if body.len() != text.len() {
        after.push('\u{feff}');
    }
    after.extend(output.into_iter().rev());
    let newline = newline_style(text);
    for row in &appended {
        append_line(&mut after, &row.line(), newline);
    }
    let mut tags: Vec<_> = updates
        .values()
        .flatten()
        .copied()
        .chain(appended)
        .filter(|row| !row.theme.is_empty())
        .collect();
    tags.sort_by(|a, b| (&a.head, &a.reading).cmp(&(&b.head, &b.reading)));
    for row in tags {
        append_line(&mut after, &theme_tag(row), newline);
    }
    after
}

pub fn delete_entries(dir: &Path, kind: LexiconKind, selected: &[LexiconEntry]) -> io::Result<()> {
    let _lock = lock(dir)?;
    let path = dir.join(kind.filename());
    let before = read_local(&path)?;
    let current = rows(&before, kind);
    let mut updates = HashMap::new();
    for entry in selected {
        let key = entry.key(kind).map_err(invalid)?;
        if current.get(&key) != Some(entry) {
            return Err(invalid("有词条已被修改，请刷新后重新选择"));
        }
        updates.insert(key, None);
    }
    if updates.is_empty() {
        return Err(invalid("请选择要删除的词条"));
    }
    let after = rewrite_rows(&before, kind, &updates, Vec::new());
    let count = updates.len();
    history::save_change(dir, kind, format!("删除 {count} 条"), before, after)
}

fn newline_style(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

fn append_line(text: &mut String, line: &str, newline: &str) {
    if !strip_bom(text).is_empty() && !text.ends_with('\n') {
        text.push_str(newline);
    }
    text.push_str(line);
    text.push_str(newline);
}

#[derive(Clone, Copy)]
pub enum DuplicatePolicy {
    Skip,
    Replace,
    Merge,
}

#[derive(Default)]
pub struct DictionaryMetadata {
    pub name: String,
    pub author: String,
    pub version: String,
    pub language: String,
    pub description: String,
}

fn dictionary_header(bytes: &[u8]) -> (DictionaryMetadata, Vec<String>) {
    let mut header = DictionaryMetadata::default();
    let mut issues = Vec::new();
    for (index, raw) in bytes.split(|&byte| byte == b'\n').enumerate() {
        let Ok(line) = std::str::from_utf8(raw) else {
            break;
        };
        let line = if index == 0 { strip_bom(line) } else { line }.trim();
        if line.is_empty() {
            continue;
        }
        let Some(comment) = line.strip_prefix('#') else {
            break;
        };
        let Some((key, value)) = comment.trim_start().split_once(':') else {
            continue;
        };
        let (slot, max) = match key.trim() {
            "name" => (&mut header.name, 80),
            "author" => (&mut header.author, 100),
            "version" => (&mut header.version, 40),
            "language" => (&mut header.language, 40),
            "description" => (&mut header.description, 400),
            _ => continue,
        };
        let value = value.trim();
        if value.is_empty() || value.chars().count() > max || value.chars().any(char::is_control) {
            if issues.len() < 100 {
                issues.push(format!(
                    "第 {} 行：{key} 元信息为空、超过 {max} 字符或包含控制字符",
                    index + 1
                ));
            }
        } else if slot.is_empty() {
            slot.push_str(value);
        } else if issues.len() < 100 {
            issues.push(format!("第 {} 行：{key} 元信息重复，保留第一项", index + 1));
        }
    }
    (header, issues)
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
    pub defaulted_weights: usize,
    pub metadata: DictionaryMetadata,
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
    if merged.theme.is_empty() {
        merged.theme.clone_from(&incoming.theme);
    }
    if merged.extra.is_empty() {
        merged.extra.clone_from(&incoming.extra);
    }
    merged
}

#[derive(Default)]
struct RowCost {
    bytes: usize,
    lines: usize,
    ending: usize,
}

struct ImportBudget {
    bytes: usize,
    lines: usize,
    newline: usize,
    existing: HashMap<Key, RowCost>,
}

impl ImportBudget {
    fn new(text: &str, kind: LexiconKind) -> Self {
        let newline = newline_style(text).len();
        let body = strip_bom(text);
        // Appending can need one separator if the old file lacked a final newline.
        let separator = if !body.is_empty() && !body.ends_with('\n') {
            newline
        } else {
            0
        };
        let mut budget = Self {
            bytes: text.len() + separator,
            lines: 0,
            newline,
            existing: HashMap::new(),
        };
        for line in body.split_inclusive('\n') {
            if let Some((key, _)) = theme_line(line, kind) {
                budget.existing.entry(key).or_default().bytes += line.len();
            } else if is_data(line) {
                budget.lines += 1;
                if let Ok((key, _)) = parse_line(without_ending(line), kind) {
                    let cost = budget.existing.entry(key).or_default();
                    cost.bytes += line.len();
                    cost.lines += 1;
                    cost.ending = line_ending(line).len();
                }
            }
        }
        budget
    }

    fn accept(&mut self, key: &Key, entry: &LexiconEntry) -> bool {
        let old = self.existing.entry(key.clone()).or_insert_with(|| RowCost {
            ending: self.newline,
            ..RowCost::default()
        });
        let ending = if old.lines == 0 {
            self.newline
        } else {
            old.ending
        };
        let tag = if entry.theme.is_empty() {
            0
        } else {
            theme_tag(entry).len() + self.newline
        };
        let row = entry.line().len() + ending + tag;
        let bytes = self.bytes - old.bytes + row;
        let lines = self.lines - old.lines + 1;
        if bytes > MAX_BYTES || lines > MAX_DATA_LINES {
            return false;
        }
        self.bytes = bytes;
        self.lines = lines;
        *old = RowCost {
            bytes: row,
            lines: 1,
            ending,
        };
        true
    }
}

fn report_import_issue(issues: &mut Vec<String>, omitted: &mut usize, line: usize, message: &str) {
    if issues.len() < 100 {
        issues.push(format!("第 {line} 行：{message}"));
    } else {
        *omitted += 1;
    }
}

pub fn preview_import(
    dir: &Path,
    kind: LexiconKind,
    bytes: &[u8],
    policy: DuplicatePolicy,
    theme: &str,
) -> io::Result<ImportPreview> {
    if theme.chars().count() > 80 || theme.chars().any(char::is_control) {
        return Err(invalid("主题超过 80 个字符或包含控制字符"));
    }
    if bytes.len() > MAX_IMPORT_BYTES {
        return Err(invalid("导入文件超过 4 MiB"));
    }
    let (metadata, mut issues) = dictionary_header(bytes);
    let theme = if metadata.name.is_empty() {
        theme
    } else {
        metadata.name.as_str()
    };
    let before = read_local(&dir.join(kind.filename()))?;
    let baseline = rows(&before, kind);
    let mut budget = ImportBudget::new(&before, kind);
    let mut current = baseline.clone();
    let mut updates = HashMap::new();
    let mut new_order = Vec::new();
    let mut added = 0;
    let mut skipped = 0;
    let mut replaced = HashSet::new();
    let mut merged = HashSet::new();
    let mut omitted = 0;
    let mut defaulted_weights = 0;

    for (index, raw) in bytes.split(|&byte| byte == b'\n').enumerate() {
        let result = (|| {
            let line = std::str::from_utf8(raw).map_err(|_| "不是有效的 UTF-8")?;
            let line = if index == 0 { strip_bom(line) } else { line };
            let line = line.strip_suffix('\r').unwrap_or(line);
            if line.trim().is_empty() || line.trim_start().starts_with('#') {
                return Ok(None);
            }
            parse_record(line, kind, true).map(Some)
        })();
        let (key, mut incoming, used_default) = match result {
            Ok(Some(row)) => row,
            Ok(None) => continue,
            Err(message) => {
                skipped += 1;
                report_import_issue(&mut issues, &mut omitted, index + 1, message);
                continue;
            }
        };
        if used_default {
            defaulted_weights += 1;
            report_import_issue(
                &mut issues,
                &mut omitted,
                index + 1,
                "权重缺失或无效，使用默认权重 0",
            );
        }
        incoming.theme = theme.to_owned();
        let existing = current.get(&key);
        let entry = match (existing, policy) {
            (None, _) => incoming,
            (Some(_), DuplicatePolicy::Skip) => {
                skipped += 1;
                continue;
            }
            (Some(existing), DuplicatePolicy::Replace) => {
                if incoming.theme.is_empty() {
                    incoming.theme.clone_from(&existing.theme);
                }
                incoming
            }
            (Some(existing), DuplicatePolicy::Merge) => merge_entry(existing, &incoming),
        };
        if existing == Some(&entry) {
            skipped += 1;
            continue;
        }
        if let Err(message) = entry.key(kind) {
            skipped += 1;
            report_import_issue(&mut issues, &mut omitted, index + 1, message);
            continue;
        }
        if !budget.accept(&key, &entry) {
            skipped += 1;
            report_import_issue(
                &mut issues,
                &mut omitted,
                index + 1,
                "超过本机词库容量（256 KiB 或 5000 行）",
            );
            continue;
        }
        if existing.is_none() {
            added += 1;
            new_order.push(key.clone());
        } else if baseline.contains_key(&key) {
            match policy {
                DuplicatePolicy::Replace => {
                    replaced.insert(key.clone());
                }
                DuplicatePolicy::Merge => {
                    merged.insert(key.clone());
                }
                DuplicatePolicy::Skip => {}
            }
        }
        current.insert(key.clone(), entry.clone());
        updates.insert(key, entry);
    }
    if omitted > 0 {
        issues.push(format!("另有 {omitted} 条问题"));
    }

    let appended = new_order.iter().map(|key| &updates[key]).collect();
    let replacements = updates
        .iter()
        .filter(|(key, _)| baseline.contains_key(*key))
        .map(|(key, row)| (key.clone(), Some(row)))
        .collect();
    let after = rewrite_rows(&before, kind, &replacements, appended);
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
        defaulted_weights,
        metadata,
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

fn without_ending(line: &str) -> &str {
    &line[..line.len() - line_ending(line).len()]
}

pub fn commit_import(dir: &Path, preview: &ImportPreview) -> io::Result<()> {
    let _lock = lock(dir)?;
    let path = dir.join(preview.kind.filename());
    if read_local(&path)? != preview.before {
        return Err(invalid("预览后词库已修改，请重新生成预览"));
    }
    if preview.has_changes() {
        snapshot_locked(dir, false)?;
        let count = preview.added + preview.replaced + preview.merged;
        history::save_change(
            dir,
            preview.kind,
            format!("导入 {count} 条"),
            preview.before.clone(),
            preview.after.clone(),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate() -> crate::quick_add::Candidate {
        crate::quick_add::Candidate {
            head: "乙".into(),
            reading: " yi ".into(),
            russian: "второй".into(),
        }
    }

    #[test]
    fn quick_add_backs_up_deduplicates_and_undoes_exact_bytes() {
        let dir = test_dir("快捷 слова");
        let words = dir.join(WORDS_FILE);
        let glosses = dir.join(GLOSSES_FILE);
        let original = "\u{feff}# 旧词库\r\n甲\tjia\tпервый\r\n";
        fs::write(&words, original).unwrap();
        fs::write(&glosses, "甲\tjia\tстарый\r\n").unwrap();

        assert!(quick_add_word(&dir, candidate()).unwrap());
        assert_eq!(
            fs::read_to_string(&words).unwrap(),
            format!("{original}乙\tyi\tвторой\t\r\n")
        );
        let action = undo_action(&dir).unwrap();
        assert_eq!(action.description, "添加 1 条");
        let backup = fs::read_dir(dir.join("lexicon-backups")).unwrap().next().unwrap().unwrap().path();
        assert_eq!(fs::read(backup.join(WORDS_FILE)).unwrap(), original.as_bytes());
        assert_eq!(fs::read(backup.join(GLOSSES_FILE)).unwrap(), fs::read(glosses).unwrap());
        let mut duplicate = candidate();
        duplicate.reading = "yǐ".into();
        duplicate.russian = "другое значение".into();
        assert!(!quick_add_word(&dir, duplicate).unwrap());
        assert_eq!(undo_action(&dir).unwrap().id, action.id);
        assert_eq!(fs::read_dir(dir.join("lexicon-backups")).unwrap().count(), 1);
        assert_eq!(undo_last(&dir, Some(&action.id)).unwrap().as_deref(), Some("添加 1 条"));
        assert_eq!(fs::read(&words).unwrap(), original.as_bytes());
        assert!(undo_action(&dir).is_none());
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn quick_add_handles_exclusive_dictionary_and_journal_handles() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = test_dir("词库 занято");
        let words = dir.join(WORDS_FILE);
        let journal = dir.join("lexicon-undo.json");
        let original = "# 原词库\r\n甲\tjia\tпервый\r\n";
        fs::write(&words, original).unwrap();
        fs::write(&journal, r#"{"version":1,"entries":[],"pending":null}"#).unwrap();
        for path in [&words, &journal] {
            let held = fs::OpenOptions::new().read(true).share_mode(0).open(path).unwrap();
            assert!(quick_add_word(&dir, candidate()).is_err());
            drop(held);
            assert_eq!(fs::read(&words).unwrap(), original.as_bytes());
            assert!(undo_action(&dir).is_none());
        }
        assert!(quick_add_word(&dir, candidate()).unwrap());
        assert_eq!(undo_action(&dir).unwrap().description, "添加 1 条");
        fs::remove_dir_all(dir).unwrap();
    }
    use crate::settings_file::test_dir;

    fn import_lexicon(dir: &Path, kind: LexiconKind, text: &str) -> io::Result<(usize, usize)> {
        let plan = preview_import(dir, kind, text.as_bytes(), DuplicatePolicy::Skip, "")?;
        commit_import(dir, &plan)?;
        Ok((plan.added, plan.skipped))
    }

    fn entry(head: &str, reading: &str, russian: &str, extra: &str) -> LexiconEntry {
        LexiconEntry {
            head: head.into(),
            reading: reading.into(),
            russian: russian.into(),
            extra: extra.into(),
            theme: String::new(),
        }
    }

    #[test]
    fn numeric_weight_boundaries_and_legacy_import_defaults() {
        let dir = test_dir("weight-range");
        let kind = LexiconKind::Words;
        for weight in ["-9999", "0", "1", "9999", ""] {
            assert!(entry("甲", "jia", "", weight).key(kind).is_ok());
        }
        for weight in ["-10000", "10000", "2.5", "invalid"] {
            assert!(entry("甲", "jia", "", weight).key(kind).is_err());
        }
        let input = "甲\tjia\r\n乙\tyi\tдва\tbad\r\n丙\tbing\tтри\t-10";
        let plan = preview_import(&dir, kind, input.as_bytes(), DuplicatePolicy::Skip, "").unwrap();
        assert_eq!(
            (plan.added, plan.skipped, plan.defaulted_weights),
            (3, 0, 2)
        );
        assert!(plan.issues.iter().all(|issue| issue.contains("默认权重 0")));
        commit_import(&dir, &plan).unwrap();
        let words = lexicon_entries(&dir, kind).unwrap();
        assert!(words
            .iter()
            .any(|row| row.head == "丙" && row.extra == "-10"));
        assert!(words.iter().filter(|row| row.extra.is_empty()).count() == 2);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn weight_moves_keep_rank_slots_and_stable_file_ties() {
        let dir = test_dir("weight-move");
        let path = dir.join(WORDS_FILE);
        fs::write(&path, "\u{feff}# keep\r\n甲\tjia\tодин\t200\r\n旁\tpang\r\n乙\tjiā\tдва\t200\r\n丙\tjia\tтри\t100\r\n丁\tjia\r\n").unwrap();
        let kind = LexiconKind::Words;
        let entries = lexicon_entries(&dir, kind).unwrap();
        let row = |head| entries.iter().find(|row| row.head == head).unwrap();
        assert!(move_weight(&dir, row("甲"), true).unwrap().is_none());
        let changed = move_weight(&dir, row("乙"), true).unwrap().unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with(
            "\u{feff}# keep\r\n乙\tjiā\tдва\t200\r\n旁\tpang\r\n甲\tjia\tодин\t200\r\n"
        ));
        let changed = move_weight(&dir, &changed, false).unwrap().unwrap();
        let changed = move_weight(&dir, &changed, false).unwrap().unwrap();
        assert_eq!(changed.extra, "100");
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("丙\tjia\tтри\t200\r\n乙\tjiā\tдва\t100\r\n丁\tjia"));
        fs::remove_dir_all(dir).unwrap();
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
            "",
        )
        .unwrap();
        assert_eq!((plan.added, plan.skipped), (1, 1));
        assert!(plan.issues.iter().any(|issue| issue.contains("第 3 行")));
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
        let skip = preview_import(&dir, kind, input.as_bytes(), DuplicatePolicy::Skip, "").unwrap();
        assert_eq!((skip.added, skip.skipped, skip.replaced), (1, 2, 0));
        let replace =
            preview_import(&dir, kind, input.as_bytes(), DuplicatePolicy::Replace, "").unwrap();
        assert_eq!((replace.added, replace.replaced), (1, 1));
        assert!(replace.after.contains("女\tnǚ\tдевушка | женщина\t50"));
        let merge =
            preview_import(&dir, kind, input.as_bytes(), DuplicatePolicy::Merge, "").unwrap();
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
            "",
        )
        .unwrap();
        assert_eq!(
            (plan.added, plan.skipped, plan.defaulted_weights),
            (1, 4, 1)
        );
        for (index, issue) in plan.issues.iter().take(4).enumerate() {
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
                "",
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
            "",
        )
        .unwrap();
        assert!(commit_import(&dir, &plan).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), old);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn themes_batch_delete_and_undo_restore_exact_bytes() {
        let dir = test_dir("lex-theme-undo");
        let kind = LexiconKind::Words;
        let path = dir.join(WORDS_FILE);
        fs::write(&path, "\u{feff}# 保留\r\n女\tnv\r\n").unwrap();
        let original = fs::read(&path).unwrap();
        let plan = preview_import(
            &dir,
            kind,
            "甲\tjia\n乙\tyi".as_bytes(),
            DuplicatePolicy::Skip,
            "出行",
        )
        .unwrap();
        commit_import(&dir, &plan).unwrap();
        let imported = fs::read(&path).unwrap();
        let entries = lexicon_entries(&dir, kind).unwrap();
        let marked = entries
            .into_iter()
            .filter(|row| row.theme == "出行")
            .collect::<Vec<_>>();
        assert_eq!(marked.len(), 2);
        delete_entries(&dir, kind, &marked).unwrap();
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(undo_last(&dir, None).unwrap().as_deref(), Some("删除 2 条"));
        assert_eq!(fs::read(&path).unwrap(), imported);
        assert_eq!(undo_last(&dir, None).unwrap().as_deref(), Some("导入 2 条"));
        assert_eq!(fs::read(&path).unwrap(), original);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn editing_moves_theme_to_new_reading_and_undo_guards_new_edits() {
        let dir = test_dir("lex-theme-edit");
        let kind = LexiconKind::Words;
        let plan = preview_import(
            &dir,
            kind,
            "甲\tjia".as_bytes(),
            DuplicatePolicy::Skip,
            "办公",
        )
        .unwrap();
        commit_import(&dir, &plan).unwrap();
        let row = lexicon_entries(&dir, kind).unwrap().pop().unwrap();
        let renamed = LexiconEntry {
            reading: "myjia".into(),
            ..row.clone()
        };
        edit_lexicon(&dir, kind, Some(&row), Some(renamed.clone())).unwrap();
        assert!(lexicon_entries(&dir, kind).unwrap() == [renamed]);
        assert!(undo_last(&dir, None).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn crowded_reading_includes_user_words_and_builtin_summary() {
        assert!(crowded_reading("shi4", &[]));
        assert!(crowded_reading("nv", &[]));
        assert!(crowded_reading("lǜ", &[]));
        assert!(!crowded_reading("mygate", &[]));
        let rows = (0..8)
            .map(|i| entry(&format!("旁译{i}"), "mygate", "", ""))
            .collect::<Vec<_>>();
        assert!(crowded_reading("mygate", &rows));
        assert!(!crowded_reading("mygate", &rows[..7]));
    }

    #[test]
    fn optional_header_previews_metadata_and_assigns_theme() {
        let dir = test_dir("lex-header");
        let kind = LexiconKind::Words;
        let source = "\u{feff}# name: 俄语练习\r\n# author: Автор\r\n# version: 1.2\r\n# language: zh-CN -> ru\r\n# description: 练习词\r\n# name: duplicate\r\n\r\n甲\tjia\r\n";
        let plan = preview_import(
            &dir,
            kind,
            source.as_bytes(),
            DuplicatePolicy::Skip,
            "旧文件名",
        )
        .unwrap();
        assert_eq!(plan.metadata.name, "俄语练习");
        assert_eq!(plan.metadata.author, "Автор");
        assert_eq!(plan.metadata.version, "1.2");
        assert_eq!(plan.metadata.language, "zh-CN -> ru");
        assert_eq!(plan.metadata.description, "练习词");
        assert!(plan.issues[0].contains("第 6 行"));
        assert_eq!(plan.added, 1);
        commit_import(&dir, &plan).unwrap();
        let legacy = preview_import(
            &dir,
            kind,
            "乙\tyi\n# name: ordinary comment".as_bytes(),
            DuplicatePolicy::Skip,
            "文件名",
        )
        .unwrap();
        assert!(legacy.metadata.name.is_empty());
        commit_import(&dir, &legacy).unwrap();
        let entries = lexicon_entries(&dir, kind).unwrap();
        assert!(entries
            .iter()
            .any(|row| row.head == "甲" && row.theme == "俄语练习"));
        assert!(entries
            .iter()
            .any(|row| row.head == "乙" && row.theme == "文件名"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn overflow_skips_only_rows_that_do_not_fit() {
        let dir = test_dir("lex-capacity");
        let kind = LexiconKind::Words;
        let path = dir.join(WORDS_FILE);
        let mut current = "#".repeat(MAX_BYTES - 52);
        current.push_str("\r\n");
        fs::write(&path, current).unwrap();
        let input = format!("甲\tjia\t{}\r\n乙\tyi", "я".repeat(50));
        let plan = preview_import(&dir, kind, input.as_bytes(), DuplicatePolicy::Skip, "").unwrap();
        assert_eq!((plan.added, plan.skipped), (1, 1));
        assert!(plan.issues[0].contains("第 1 行"));
        commit_import(&dir, &plan).unwrap();
        assert_eq!(lexicon_entries(&dir, kind).unwrap()[0].head, "乙");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unicode_paths_crlf_and_lock_contention_preserve_data() {
        let parent = test_dir("lex-windows");
        let dir = parent.join("旁 译 · Русский");
        fs::create_dir(&dir).unwrap();
        let path = dir.join(WORDS_FILE);
        let text = "\u{feff}# 旧词库\r\n甲\tjia\tпервый\r\n";
        fs::write(&path, text).unwrap();
        let source = dir.join("导入 俄语.tsv");
        fs::write(&source, "\u{feff}乙\tyi\tвторой\r\n").unwrap();
        let plan = preview_import(
            &dir,
            LexiconKind::Words,
            &read_import(&source).unwrap(),
            DuplicatePolicy::Skip,
            "",
        )
        .unwrap();
        let held = lock(&dir).unwrap();
        match commit_import(&dir, &plan) {
            Err(error) => assert_eq!(error.kind(), io::ErrorKind::WouldBlock),
            Ok(_) => panic!("concurrent write should be blocked"),
        }
        assert_eq!(fs::read(&path).unwrap(), text.as_bytes());
        drop(held);
        commit_import(&dir, &plan).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            format!("{text}乙\tyi\tвторой\t\r\n")
        );
        fs::remove_dir_all(parent).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn exclusive_windows_handle_leaves_complete_data_and_recovers() {
        use std::os::windows::fs::OpenOptionsExt;

        let dir = test_dir("lex-share-delete");
        let path = dir.join(WORDS_FILE);
        let old = "甲\tjia\r\n";
        let next = format!("{old}乙\tyi\r\n");
        fs::write(&path, old).unwrap();
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        let edit = edit_lexicon(
            &dir,
            LexiconKind::Words,
            None,
            Some(entry("乙", "yi", "", "")),
        );
        assert!(
            edit.is_err(),
            "exclusive handle should block reading the current dictionary"
        );
        let replaced = write_atomic(&path, &next).is_ok();
        assert!(fs::read_dir(&dir).unwrap().all(|entry| !entry
            .unwrap()
            .path()
            .extension()
            .is_some_and(|ext| ext == "tmp")));
        drop(held);
        // Windows volumes differ in whether they support replacing an open file.
        // Either way, the destination is always one complete snapshot.
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            if replaced { next.as_str() } else { old }
        );
        write_atomic(&path, &next).unwrap();
        assert_eq!(lexicon_entries(&dir, LexiconKind::Words).unwrap().len(), 2);
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
