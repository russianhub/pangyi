//! 设置程序和输入法共用的词条格式。
/// 每个文件最多读这么多字节。再大就截断并记一条警告，避免一个巨大文件拖住启动。
pub(crate) const MAX_BYTES: usize = 256 * 1024;
/// 每个文件最多收下这么多数据行（空行和 # 注释不计）。
pub(crate) const MAX_DATA_LINES: usize = 5_000;
/// 词头最多多少个 Unicode 字符。
const MAX_HEAD_CHARS: usize = 32;
/// 俄语列最多多少个 Unicode 字符。
pub(crate) const MAX_GLOSS_CHARS: usize = 400;
/// 去掉声调之后的拼音字母最多多少个。
pub(crate) const MAX_READING_CHARS: usize = 48;
/// 权重要落在这个闭区间里。
pub(crate) const MIN_WEIGHT: i64 = -9_999;
pub(crate) const MAX_WEIGHT: i64 = 9_999;

pub(crate) fn validate_head(head: &str) -> Result<(), &'static str> {
    if head.is_empty() {
        return Err("词头为空");
    }
    if head.chars().count() > MAX_HEAD_CHARS {
        return Err("词头超过 32 个字符");
    }
    if !has_han(head) {
        return Err("词头里要有汉字");
    }
    Ok(())
}

pub(crate) fn parse_weight(s: &str) -> Result<Option<i64>, &'static str> {
    if s.is_empty() {
        return Ok(None);
    }
    let n: i64 = match s.parse() {
        Ok(n) => n,
        Err(_) => return Err("权重不是整数"),
    };
    if !(MIN_WEIGHT..=MAX_WEIGHT).contains(&n) {
        return Err("权重超出 -9999 到 9999");
    }
    Ok(Some(n))
}

pub(crate) struct Reading {
    pub(crate) ascii: String,
    pub(crate) tone: String,
    #[allow(dead_code)] // 输入法用原始读音显示候选，设置只用归一化的键。
    pub(crate) raw: String,
}

pub(crate) fn analyze_reading(raw: &str) -> Result<Reading, &'static str> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("读音为空");
    }
    if raw.chars().count() > 64 {
        return Err("读音太长");
    }
    let mut ascii = String::new();
    let mut tone = String::new();
    for c in raw.chars() {
        if c == '\'' || c == '\u{2019}' {
            continue;
        }
        if matches!(c, '0'..='5') {
            tone.push(if c == '0' { '5' } else { c });
            continue;
        }
        if c.is_ascii_digit() {
            return Err("读音里的数字只能是声调 0–5");
        }
        if c == '\u{0308}' {
            // u + 分音符 = ü，输入时用 v。
            if ascii.pop() == Some('u') {
                ascii.push('v');
                continue;
            }
            return Err("读音里有无法识别的字符");
        }
        if let Some(t) = combining_tone(c) {
            tone.push(char::from(b'0' + t));
            continue;
        }
        match tone_letter(c) {
            Some((base, t)) => {
                ascii.push(base);
                if t > 0 {
                    tone.push(char::from(b'0' + t));
                }
            }
            None => return Err("读音里有无法识别的字符"),
        }
    }
    if ascii.is_empty()
        || ascii.len() > MAX_READING_CHARS
        || !ascii.bytes().all(|b| b.is_ascii_lowercase())
    {
        return Err("读音不是拼音");
    }
    Ok(Reading {
        ascii,
        tone,
        raw: raw.to_string(),
    })
}

fn tone_letter(c: char) -> Option<(char, u8)> {
    Some(match c {
        'a' | 'A' => ('a', 0),
        'ā' | 'Ā' => ('a', 1),
        'á' | 'Á' => ('a', 2),
        'ǎ' | 'Ǎ' => ('a', 3),
        'à' | 'À' => ('a', 4),
        'e' | 'E' => ('e', 0),
        'ē' | 'Ē' => ('e', 1),
        'é' | 'É' => ('e', 2),
        'ě' | 'Ě' => ('e', 3),
        'è' | 'È' => ('e', 4),
        'i' | 'I' => ('i', 0),
        'ī' | 'Ī' => ('i', 1),
        'í' | 'Í' => ('i', 2),
        'ǐ' | 'Ǐ' => ('i', 3),
        'ì' | 'Ì' => ('i', 4),
        'o' | 'O' => ('o', 0),
        'ō' | 'Ō' => ('o', 1),
        'ó' | 'Ó' => ('o', 2),
        'ǒ' | 'Ǒ' => ('o', 3),
        'ò' | 'Ò' => ('o', 4),
        'u' | 'U' => ('u', 0),
        'ū' | 'Ū' => ('u', 1),
        'ú' | 'Ú' => ('u', 2),
        'ǔ' | 'Ǔ' => ('u', 3),
        'ù' | 'Ù' => ('u', 4),
        'ü' | 'Ü' | 'v' | 'V' => ('v', 0),
        'ǖ' | 'Ǖ' => ('v', 1),
        'ǘ' | 'Ǘ' => ('v', 2),
        'ǚ' | 'Ǚ' => ('v', 3),
        'ǜ' | 'Ǜ' => ('v', 4),
        'n' | 'N' => ('n', 0),
        'ń' | 'Ń' => ('n', 2),
        'ň' | 'Ň' => ('n', 3),
        'ǹ' | 'Ǹ' => ('n', 4),
        'm' | 'M' => ('m', 0),
        'ḿ' | 'Ḿ' => ('m', 2),
        'b' | 'B' => ('b', 0),
        'c' | 'C' => ('c', 0),
        'd' | 'D' => ('d', 0),
        'f' | 'F' => ('f', 0),
        'g' | 'G' => ('g', 0),
        'h' | 'H' => ('h', 0),
        'j' | 'J' => ('j', 0),
        'k' | 'K' => ('k', 0),
        'l' | 'L' => ('l', 0),
        'p' | 'P' => ('p', 0),
        'q' | 'Q' => ('q', 0),
        'r' | 'R' => ('r', 0),
        's' | 'S' => ('s', 0),
        't' | 'T' => ('t', 0),
        'w' | 'W' => ('w', 0),
        'x' | 'X' => ('x', 0),
        'y' | 'Y' => ('y', 0),
        'z' | 'Z' => ('z', 0),
        _ => return None,
    })
}

fn combining_tone(c: char) -> Option<u8> {
    Some(match c {
        '\u{0304}' => 1,
        '\u{0301}' => 2,
        '\u{030c}' => 3,
        '\u{0300}' => 4,
        _ => return None,
    })
}

fn has_han(s: &str) -> bool {
    s.chars().any(|c| {
        ('\u{3400}'..='\u{9FFF}').contains(&c)
            || ('\u{F900}'..='\u{FAFF}').contains(&c)
            || ('\u{20000}'..='\u{2CEAF}').contains(&c)
            || c == '\u{3007}'
    })
}
