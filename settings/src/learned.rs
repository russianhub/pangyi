//! Local suggestions from the IME's existing headword/count log.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io;
use std::path::Path;
use std::sync::OnceLock;

use crate::lexicon::{add_word_with_undo, backup::lock, lexicon_entries, read_capped};
use crate::settings_file::write_atomic;
use crate::{LexiconEntry, LexiconKind};

#[path = "../../shared/lexicon_format.rs"]
#[allow(dead_code)]
mod format;

const LEARNED_FILE: &str = "learned.txt";
const IGNORED_FILE: &str = "learned-ignored.txt";
const MAX_LOCAL_BYTES: usize = 2 * 1024 * 1024;
const SHOW_LIMIT: usize = 20;

#[derive(Debug, PartialEq, Eq)]
pub struct LearnedWord {
    pub head: String,
    pub count: u32,
}

pub fn load(dir: &Path) -> Vec<LearnedWord> {
    fn parse(text: &str) -> Option<Vec<LearnedWord>> {
        let mut counts = BTreeMap::new();
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let (head, count) = line.split_once('\t')?;
            if head.is_empty() || head.chars().any(char::is_control) {
                return None;
            }
            let count: u32 = count.trim().parse().ok()?;
            if count > 0 && format::validate_head(head).is_ok() {
                counts.insert(head, count);
            }
        }
        let mut words: Vec<_> = counts.into_iter()
            .map(|(head, count)| LearnedWord { head: head.into(), count })
            .collect();
        words.sort_by(|left, right| {
            right.count.cmp(&left.count).then_with(|| left.head.cmp(&right.head))
        });
        Some(words)
    }

    read_text(dir, LEARNED_FILE).ok().and_then(|text| parse(&text)).unwrap_or_default()
}

fn read_text(dir: &Path, file: &str) -> io::Result<String> {
    let bytes = match read_capped(&dir.join(file), MAX_LOCAL_BYTES) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(String::new()),
        Err(error) => return Err(error),
    };
    String::from_utf8(bytes).map(|text| text.trim_start_matches('\u{feff}').to_owned())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn ignored(dir: &Path) -> io::Result<BTreeSet<String>> {
    Ok(read_text(dir, IGNORED_FILE)?.lines().filter(|line| !line.is_empty())
        .map(str::to_owned).collect())
}

pub fn has_ignored(dir: &Path) -> bool {
    ignored(dir).is_ok_and(|items| !items.is_empty())
}

pub fn suggestions<'a>(dir: &Path, learned: &'a [LearnedWord]) -> Vec<&'a LearnedWord> {
    fn filter<'a>(dir: &Path, learned: &'a [LearnedWord]) -> io::Result<Vec<&'a LearnedWord>> {
        let ignored = ignored(dir)?;
        let known: HashSet<_> = lexicon_entries(dir, LexiconKind::Words)?.into_iter()
            .chain(lexicon_entries(dir, LexiconKind::Glosses)?)
            .map(|entry| entry.head)
            .collect();
        Ok(learned.iter()
            .filter(|word| !ignored.contains(&word.head) && !known.contains(&word.head))
            .take(SHOW_LIMIT)
            .collect())
    }

    if learned.is_empty() { return Vec::new(); }
    filter(dir, learned).unwrap_or_default()
}

pub fn ignore(dir: &Path, head: &str) -> io::Result<()> {
    if format::validate_head(head).is_err() || head.chars().any(char::is_control) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "无效的词头"));
    }
    let _lock = lock(dir)?;
    let mut ignored = ignored(dir)?;
    ignored.insert(head.to_owned());
    write_atomic(&dir.join(IGNORED_FILE), &ignored.into_iter()
        .map(|head| format!("{head}\n")).collect::<String>())
}

pub fn restore_ignored(dir: &Path) -> io::Result<()> {
    let _lock = lock(dir)?;
    if !ignored(dir)?.is_empty() {
        write_atomic(&dir.join(IGNORED_FILE), "")?;
    }
    Ok(())
}

#[derive(PartialEq, Eq)]
pub enum Preparation {
    Ready(LexiconEntry),
    NeedsReading(LexiconEntry),
}

pub fn prepare(head: &str) -> Preparation {
    const INDEX: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/learned-readings.deflate"));
    static WORDS: OnceLock<Option<String>> = OnceLock::new();
    let index = WORDS.get_or_init(|| {
        miniz_oxide::inflate::decompress_to_vec_with_limit(INDEX, 64 * 1024 * 1024)
            .ok().and_then(|bytes| String::from_utf8(bytes).ok())
    });
    prepare_from(head, index.as_deref().unwrap_or(""))
}

fn prepare_from(head: &str, index: &str) -> Preparation {
    let mut entry = LexiconEntry { head: head.into(), ..Default::default() };
    for row in index.lines() {
        let Some((word, details)) = row.split_once('\t') else { continue };
        match word.cmp(head) {
            std::cmp::Ordering::Less => continue,
            std::cmp::Ordering::Greater => break,
            std::cmp::Ordering::Equal => {
                if let Some((reading, russian)) = details.split_once('\t') {
                    let reading = reading.replace('ü', "v");
                    if format::analyze_reading(&reading).is_ok()
                        && russian.chars().count() <= format::MAX_GLOSS_CHARS
                        && !russian.chars().any(char::is_control)
                    {
                        entry.reading = reading;
                        entry.russian = russian.into();
                        return Preparation::Ready(entry);
                    }
                }
                break;
            }
        }
    }
    Preparation::NeedsReading(entry)
}

pub fn add(dir: &Path, entry: LexiconEntry) -> io::Result<bool> {
    add_word_with_undo(dir, entry)
}

#[cfg(test)]
#[path = "../learned_index.rs"]
mod index_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_file::test_dir;
    use std::fs;

    #[test]
    fn filters_known_and_ignored_before_sorting_limit_and_recovers_ignored() {
        let dir = test_dir("自学习 слова");
        let initial = "\u{feff}甲\t99\r\n乙\t98\r\n丙\t97\r\n";
        let mut learned = initial.to_owned();
        for count in 1..=25 {
            learned.push_str(&format!("词{}\t{count}\r\n", char::from_u32(0x4e00 + count).unwrap()));
        }
        fs::write(dir.join(LEARNED_FILE), &learned).unwrap();
        fs::write(dir.join("user-dict.tsv"), "\u{feff}甲\tjia\tпервый\r\n").unwrap();
        fs::write(dir.join("user-gloss.tsv"), "乙\tyi\tвторой\r\n").unwrap();
        fs::write(dir.join(IGNORED_FILE), "\u{feff}丙\r\n").unwrap();
        assert!(has_ignored(&dir));
        let words = load(&dir);
        let result = suggestions(&dir, &words);
        assert_eq!(result.len(), 20);
        assert_eq!(result.iter().map(|word| word.count).collect::<Vec<_>>(), (6..=25).rev().collect::<Vec<_>>());
        ignore(&dir, &result[0].head).unwrap();
        assert_eq!(suggestions(&dir, &words)[0].count, 24);
        restore_ignored(&dir).unwrap();
        assert!(!has_ignored(&dir));
        assert_eq!(suggestions(&dir, &words)[0].head, "丙");
        assert_eq!(fs::read(dir.join(LEARNED_FILE)).unwrap(), learned.as_bytes());
        assert_eq!(fs::read(dir.join("user-dict.tsv")).unwrap(), "\u{feff}甲\tjia\tпервый\r\n".as_bytes());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_empty_or_damaged_learned_log_yields_empty_list() {
        let dir = test_dir("自学习 empty");
        assert!(load(&dir).is_empty());
        for bytes in [b"".as_slice(), b"\xff", "甲\t2\n乙\tnot-a-count\n".as_bytes()] {
            fs::write(dir.join(LEARNED_FILE), bytes).unwrap();
            assert!(load(&dir).is_empty());
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn occupied_learned_and_ignored_files_leave_existing_data_intact() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = test_dir("自学习 занято");
        let path = dir.join(LEARNED_FILE);
        fs::write(&path, "甲\t2\r\n").unwrap();
        let held = fs::OpenOptions::new().read(true).share_mode(0).open(&path).unwrap();
        assert!(load(&dir).is_empty());
        drop(held);
        assert_eq!(load(&dir)[0].count, 2);

        let path = dir.join(IGNORED_FILE);
        ignore(&dir, "甲").unwrap();
        let held = fs::OpenOptions::new().read(true).share_mode(0).open(&path).unwrap();
        assert!(ignore(&dir, "乙").is_err());
        assert!(restore_ignored(&dir).is_err());
        drop(held);
        assert_eq!(fs::read_to_string(&path).unwrap(), "甲\n");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unique_reading_retains_shipped_gloss_and_ambiguous_words_open_editor() {
        let base = "header\n甲\t甲\tjia\tjiǎ\t\t1\tпервый\n行\t行\txing\txíng\t\t2\tходить\n行\t行\thang\tháng\t\t1\tряд\n行\t行\thang\tháng\t\t1\tряд\n";
        let index = index_tests::unique_readings(base, "甲\tjiǎ\tновый\t说明");
        let Preparation::Ready(entry) = prepare_from("甲", &index) else { panic!("unique reading missing") };
        assert_eq!(entry.reading, "jia");
        assert_eq!(entry.russian, "новый");
        for head in ["行", "未收词"] {
            let Preparation::NeedsReading(draft) = prepare_from(head, &index) else { panic!("unexpected guessed reading") };
            assert_eq!(draft.head, head);
            assert!(draft.reading.is_empty());
        }
        assert!(matches!(
            prepare_from("长词", &format!("长词\t{}\tсмысл\n", "shan".repeat(13))),
            Preparation::NeedsReading(_)
        ));
    }

    #[test]
    fn direct_add_detects_duplicate_and_undo_restores_original() {
        let dir = test_dir("学词 add");
        let path = dir.join("user-dict.tsv");
        let original = "\u{feff}# 自定义\r\n甲\tjia\r\n";
        fs::write(&path, original).unwrap();
        let Preparation::Ready(entry) = prepare("朋友") else { panic!("missing built-in reading") };
        assert_eq!(entry.reading, "pengyou");
        assert!(!entry.russian.is_empty());
        assert!(add(&dir, entry.clone()).unwrap());
        assert!(!add(&dir, entry).unwrap());
        let action = crate::undo_action(&dir).unwrap();
        assert_eq!(action.description, "添加 1 条");
        assert_eq!(crate::undo_last(&dir, Some(&action.id)).unwrap().as_deref(), Some("添加 1 条"));
        assert_eq!(fs::read(&path).unwrap(), original.as_bytes());
        fs::remove_dir_all(dir).unwrap();
    }
}
