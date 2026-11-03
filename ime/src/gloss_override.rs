//! 手写俄语释义覆盖。
//!
//! `data/gloss-overrides.tsv` 在编译期嵌进来，查词和逐词翻译取 `ru` 时整条替换。
//! 不改词头、拼音、词频、来源档，因此：
//! - 自学习仍按中文词头记在 learned.txt
//! - 自定义短语仍只读 phrases.txt
//! - 缩写键的行号不用重算
//!
//! 列：简体、读音（pinyin_display 或 pinyin_ascii；空 = 该词全部读音）、俄语、中文说明。
//! 匹配顺序：读音列对上 display，其次对上 ascii，最后才用读音为空的整词行。
//! 同一键有多行时，文件里后一行生效。提交的这份表不允许重复、不允许空读音，
//! 并且每个词头和读音都必须在 dict.tsv 里；对不上就报错，不悄悄跳过。

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

const SHIPPED: &str = include_str!("../../data/gloss-overrides.tsv");

#[derive(Debug)]
struct Entry<'a> {
    line_no: usize,
    simp: &'a str,
    /// 空字符串表示这个简体的每一个读音。
    reading: &'a str,
    ru: &'a str,
    /// 给人看的中文说明，不进候选窗。
    #[allow(dead_code)]
    note: &'a str,
}

#[derive(Debug)]
struct Table<'a> {
    entries: Vec<Entry<'a>>,
    /// 后一行覆盖前一行。
    map: HashMap<(&'a str, &'a str), &'a str>,
}

#[derive(Debug, PartialEq)]
enum IssueKind {
    FieldCount {
        got: usize,
    },
    EmptyHeadword,
    EmptyGloss,
    EmptyNote,
    NoCyrillic,
    UnknownHeadword {
        word: String,
    },
    UnknownReading {
        word: String,
        reading: String,
    },
    DuplicateKey {
        word: String,
        reading: String,
        first_line: usize,
    },
    EmptyReading {
        word: String,
    },
}

#[derive(Debug, PartialEq)]
struct Issue {
    line_no: usize,
    kind: IssueKind,
}

impl std::fmt::Display for Issue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use IssueKind::*;
        write!(f, "第{}行: ", self.line_no)?;
        match &self.kind {
            FieldCount { got } => write!(f, "应为 4 列（简体、读音、俄语、说明），实际 {got} 列"),
            EmptyHeadword => write!(f, "简体为空"),
            EmptyGloss => write!(f, "俄语为空"),
            EmptyNote => write!(f, "说明为空"),
            NoCyrillic => write!(f, "俄语里没有西里尔字母"),
            UnknownHeadword { word } => write!(f, "词头「{word}」不在词典里"),
            UnknownReading { word, reading } => write!(f, "「{word}」没有读音「{reading}」"),
            DuplicateKey {
                word,
                reading,
                first_line,
            } => {
                let reading = if reading.is_empty() {
                    "（所有读音）"
                } else {
                    reading.as_str()
                };
                write!(f, "「{word} / {reading}」与第 {first_line} 行重复")
            }
            EmptyReading { word } => {
                write!(f, "「{word}」没有写读音；提交的覆盖表必须逐个读音写")
            }
        }
    }
}

fn has_cyrillic(s: &str) -> bool {
    s.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c))
}

/// 解析覆盖表。任何坏行都让整个结果失败，并把每一行的问题都放进错误列表。
fn parse(text: &str) -> Result<Table<'_>, Vec<Issue>> {
    let mut errors = Vec::new();
    let mut entries = Vec::new();
    for (idx, raw) in text.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // 不能 trim 整行：行首的 TAB 是空的简体列。
        let fields: Vec<&str> = line.split('\t').map(str::trim).collect();
        if fields.len() != 4 {
            errors.push(Issue {
                line_no,
                kind: IssueKind::FieldCount { got: fields.len() },
            });
            continue;
        }
        let (simp, reading, ru, note) = (fields[0], fields[1], fields[2], fields[3]);
        if simp.is_empty() {
            errors.push(Issue {
                line_no,
                kind: IssueKind::EmptyHeadword,
            });
            continue;
        }
        if ru.is_empty() {
            errors.push(Issue {
                line_no,
                kind: IssueKind::EmptyGloss,
            });
            continue;
        }
        if note.is_empty() {
            errors.push(Issue {
                line_no,
                kind: IssueKind::EmptyNote,
            });
            continue;
        }
        if !has_cyrillic(ru) {
            errors.push(Issue {
                line_no,
                kind: IssueKind::NoCyrillic,
            });
            continue;
        }
        entries.push(Entry {
            line_no,
            simp,
            reading,
            ru,
            note,
        });
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let mut map = HashMap::with_capacity(entries.len());
    for e in &entries {
        map.insert((e.simp, e.reading), e.ru);
    }
    Ok(Table { entries, map })
}

/// 读音更具体的行优先。都没有则原样返回 base（不拼接）。
fn apply<'a, 't: 'a>(
    table: &Table<'t>,
    simp: &str,
    py_ascii: &str,
    py_display: &str,
    base: &'a str,
) -> &'a str {
    // display 为空时不能拿它去对「读音列为空」的整词行，那是第三条规则。
    if !py_display.is_empty() {
        if let Some(ru) = table.map.get(&(simp, py_display)).copied() {
            return ru;
        }
    }
    if !py_ascii.is_empty() && py_ascii != py_display {
        if let Some(ru) = table.map.get(&(simp, py_ascii)).copied() {
            return ru;
        }
    }
    if let Some(ru) = table.map.get(&(simp, "")).copied() {
        return ru;
    }
    base
}

fn fmt_issues(issues: &[Issue]) -> String {
    issues
        .iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn dict_index() -> (HashSet<&'static str>, HashSet<(&'static str, &'static str)>) {
    let mut words = HashSet::new();
    let mut readings = HashSet::new();
    for line in crate::dict::raw().lines().skip(1) {
        let mut it = line.split('\t');
        let Some(simp) = it.next() else { continue };
        if simp.is_empty() {
            continue;
        }
        let _trad = it.next();
        let ascii = it.next().unwrap_or("");
        let disp = it.next().unwrap_or("");
        words.insert(simp);
        if !ascii.is_empty() {
            readings.insert((simp, ascii));
        }
        if !disp.is_empty() {
            readings.insert((simp, disp));
        }
    }
    (words, readings)
}

/// 提交的这份表：词头和读音必须在词典里，不能重复，也不能留空读音。
fn shipped_issues(table: &Table<'_>) -> Vec<Issue> {
    let (words, readings) = dict_index();
    let mut seen: HashMap<(&str, &str), usize> = HashMap::new();
    let mut out = Vec::new();
    for e in &table.entries {
        if e.reading.is_empty() {
            out.push(Issue {
                line_no: e.line_no,
                kind: IssueKind::EmptyReading {
                    word: e.simp.to_string(),
                },
            });
        }
        if let Some(first) = seen.insert((e.simp, e.reading), e.line_no) {
            out.push(Issue {
                line_no: e.line_no,
                kind: IssueKind::DuplicateKey {
                    word: e.simp.to_string(),
                    reading: e.reading.to_string(),
                    first_line: first,
                },
            });
        }
        if !words.contains(e.simp) {
            out.push(Issue {
                line_no: e.line_no,
                kind: IssueKind::UnknownHeadword {
                    word: e.simp.to_string(),
                },
            });
            continue;
        }
        if !e.reading.is_empty() && !readings.contains(&(e.simp, e.reading)) {
            out.push(Issue {
                line_no: e.line_no,
                kind: IssueKind::UnknownReading {
                    word: e.simp.to_string(),
                    reading: e.reading.to_string(),
                },
            });
        }
    }
    out
}

fn global() -> &'static Table<'static> {
    static T: OnceLock<Table<'static>> = OnceLock::new();
    T.get_or_init(|| {
        let table = parse(SHIPPED)
            .unwrap_or_else(|errs| panic!("gloss-overrides.tsv 无法解析:\n{}", fmt_issues(&errs)));
        let problems = shipped_issues(&table);
        if !problems.is_empty() {
            panic!(
                "gloss-overrides.tsv 和词典不一致:\n{}",
                fmt_issues(&problems)
            );
        }
        table
    })
}

/// 用覆盖表替换这一行的整条俄语。没有覆盖时返回原来的 `base`。
pub(crate) fn ru_for<'a>(simp: &str, py_ascii: &str, py_display: &str, base: &'a str) -> &'a str {
    apply(global(), simp, py_ascii, py_display, base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_keeps_comment_and_blank_lines_out() {
        let text = "\n# 注释\n  # 也是注释\n的\tde\tчасти́ца\t助词\n";
        let t = parse(text).unwrap();
        assert_eq!(t.entries.len(), 1);
        assert_eq!(t.entries[0].line_no, 4);
        assert_eq!(t.entries[0].simp, "的");
        assert_eq!(t.entries[0].reading, "de");
        assert_eq!(t.entries[0].ru, "части́ца");
        assert_eq!(t.entries[0].note, "助词");
        assert_eq!(apply(&t, "的", "de", "de", "яркий"), "части́ца");
    }

    #[test]
    fn malformed_lines_are_all_reported() {
        let text = "\
# 注释\n\
\n\
只有一列\n\
\t读音\t释义\t说明\n\
词\tde\t\t说明\n\
词\tde\tка́мень\t\n\
词\tde\tbright\t说明\n\
词\tde\tка́мень\t说明\t多一列\n\
词\tde\tка́мень\t这条是好的\n\
";
        let err = parse(text).expect_err("坏行不能当成成功");
        let lines: Vec<usize> = err.iter().map(|i| i.line_no).collect();
        assert_eq!(lines, vec![3, 4, 5, 6, 7, 8], "{err:?}");
        assert!(matches!(err[0].kind, IssueKind::FieldCount { got: 1 }));
        assert_eq!(err[1].kind, IssueKind::EmptyHeadword);
        assert_eq!(err[2].kind, IssueKind::EmptyGloss);
        assert_eq!(err[3].kind, IssueKind::EmptyNote);
        assert_eq!(err[4].kind, IssueKind::NoCyrillic);
        assert!(matches!(err[5].kind, IssueKind::FieldCount { got: 5 }));
        let msg = fmt_issues(&err);
        assert!(msg.contains("第3行"), "错误信息里要有行号: {msg}");
        assert!(!msg.contains("第1行") && !msg.contains("第2行"));
        assert!(!msg.contains("第9行"));
    }

    #[test]
    fn later_line_wins_and_specific_reading_beats_blanket() {
        let text = "\
长\t\tо́бщий\t整词兜底\n\
长\tcháng\tпе́рвый\t先写的长音\n\
长\tcháng\tдли́нный\t后写的长音\n\
";
        let t = parse(text).unwrap();
        assert_eq!(apply(&t, "长", "chang", "cháng", "старое"), "дли́нный");
        assert_eq!(apply(&t, "长", "zhang", "zhǎng", "старое"), "о́бщий");
        let replaced = apply(&t, "长", "chang", "cháng", "старое | хвост");
        assert_eq!(replaced, "дли́нный");
        assert!(
            !replaced.contains('旧') && !replaced.contains("старое") && !replaced.contains('|')
        );
    }

    #[test]
    fn ascii_reading_matches_when_it_differs_from_display() {
        let text = "的\tdi\tтакси́\t用无声调拼音\n";
        let t = parse(text).unwrap();
        assert_eq!(apply(&t, "的", "di", "dī", "яркий"), "такси́");
        assert_eq!(apply(&t, "的", "de", "de", "яркий"), "яркий");
    }

    #[test]
    fn unknown_headword_and_reading_are_reported() {
        let text = "\
没有这个词\tde\tка́мень\t词典里没有\n\
的\tnot-a-reading\tка́мень\t读音对不上\n\
的\tde\tчасти́ца\t这条对\n\
";
        let t = parse(text).unwrap();
        let issues = shipped_issues(&t);
        assert!(
            issues
                .iter()
                .any(|i| i.line_no == 1 && matches!(i.kind, IssueKind::UnknownHeadword { .. })),
            "{issues:?}"
        );
        assert!(
            issues
                .iter()
                .any(|i| i.line_no == 2 && matches!(i.kind, IssueKind::UnknownReading { .. })),
            "{issues:?}"
        );
        assert!(
            !issues.iter().any(|i| i.line_no == 3),
            "对得上的行不应报错: {issues:?}"
        );
        let msg = fmt_issues(&issues);
        assert!(msg.contains("没有这个词"), "{msg}");
        assert!(msg.contains("not-a-reading"), "{msg}");
    }

    #[test]
    fn shipped_table_parses_and_binds_to_dict() {
        let t = parse(SHIPPED).unwrap_or_else(|e| panic!("{}", fmt_issues(&e)));
        assert!(t.entries.len() >= 60, "覆盖太少: {}", t.entries.len());
        let issues = shipped_issues(&t);
        assert!(issues.is_empty(), "{}", fmt_issues(&issues));
        let de = t
            .entries
            .iter()
            .find(|e| e.simp == "的" && e.reading == "de")
            .unwrap();
        assert!(de.ru.contains("частица"), "{}", de.ru);
        assert!(!de.note.is_empty());
    }

    fn gloss(ascii: &str, word: &str) -> String {
        crate::dict::lookup(ascii)
            .into_iter()
            .find(|h| h.word == word)
            .unwrap_or_else(|| panic!("lookup({ascii}) 里没有「{word}」"))
            .ru
    }

    #[test]
    fn lookup_applies_overrides_without_touching_word_or_freq() {
        let de = crate::dict::lookup("de")
            .into_iter()
            .find(|h| h.word == "的")
            .expect("de 组里应有「的」");
        assert!(de.ru.contains("частица"), "{}", de.ru);
        assert!(!de.ru.contains("яркий"), "{}", de.ru);
        assert_eq!(de.freq, 318825, "覆盖不能改词频，也不能变成短语的 MAX");
        assert_ne!(de.freq, u64::MAX);
        assert_eq!(de.word, "的");

        assert!(gloss("di", "的").contains("такси"), "{}", gloss("di", "的"));
        let mei = gloss("mei", "没");
        assert!(mei.contains("не"), "{mei}");
        assert!(!mei.contains("умер") && !mei.contains("сконч"), "{mei}");
        assert!(
            gloss("mo", "没").contains("погруж"),
            "{}",
            gloss("mo", "没")
        );
        // 重音是 и 后面的 U+0301，「длин」不再是连续子串。
        assert_eq!(gloss("chang", "长"), "дли́нный");
        let zhang = gloss("zhang", "长");
        assert!(zhang.contains("ста"), "{zhang}");
        assert!(!zhang.contains("длин"), "{zhang}");
        assert!(
            gloss("kuai", "会").contains("бухгалт"),
            "{}",
            gloss("kuai", "会")
        );

        let shicong = crate::dict::ru_of("是从").expect("空释义应被覆盖");
        assert!(
            shicong.contains("след") || shicong.contains("подчин"),
            "{shicong}"
        );
        assert_eq!(
            crate::dict::ru_of("我"),
            Some("я"),
            "没写进覆盖表的词保持原释义"
        );
    }
}
