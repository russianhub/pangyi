//! 本机表情包和符号包。
//!
//! `%LOCALAPPDATA%\Pangyi\packs\` 里的 `*.tsv` 放进去就生效，删掉就关闭。
//! 输入法只读这个文件夹，不联网，也不写模板。
//! 格式和用户词库同一套限制：UTF-8、`#` 注释、坏行跳过、按修改时间热重载。
//! 列是：关键词<TAB>候选内容<TAB>俄语（可空）<TAB>权重（可空）。
//!
//! 排序和用户词同一档：默认紧跟内置精确第一名，权重 ≥ 100 才能排到它前面。
//! 同一档里用户词更靠前。包不改词典俄语。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use crate::dict::Hit;
use crate::user_dict::{
    self, analyze_reading, data_lines, decode, parse_weight, push_warning, read_capped, FileBytes,
    CHECK_INTERVAL_MS, MAX_BYTES, MAX_GLOSS_CHARS,
};

/// 最多读这么多个包文件。再多就按文件名只取前面的。
const MAX_PACK_FILES: usize = 32;
/// 候选内容最多多少个 Unicode 字符。比用户词词头宽，是为了放下颜文字。
const MAX_SURFACE_CHARS: usize = 64;
const PACKS_DIR: &str = "packs";
const WARN_FILE: &str = "packs-warnings.txt";

#[derive(Clone, Debug)]
struct PackItem {
    surface: String,
    ascii: String,
    display: String,
    gloss: String,
    weight: Option<i64>,
    file_ord: usize,
    line_no: usize,
    file: String,
}

#[derive(Clone, Debug, Default)]
struct Snapshot {
    by_key: HashMap<String, Vec<PackItem>>,
    /// 选中后不写入自学习的候选（表情、颜文字、符号）。纯汉字不在这里。
    skip_learn: HashSet<String>,
}

struct LoadState {
    snap: std::sync::Arc<Snapshot>,
    /// 上次读入时每个文件的修改时间和长度。空表示目录不存在或里面没有 tsv。
    sig: Vec<FileSig>,
    last_check_ms: u64,
    loaded: bool,
    /// 某个文件这次没读成时，沿用它上一份解析结果。
    prev_files: HashMap<String, Vec<PackItem>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileSig {
    name: String,
    mtime: Option<SystemTime>,
    len: u64,
}

impl LoadState {
    fn empty() -> Self {
        Self {
            snap: std::sync::Arc::new(Snapshot::default()),
            sig: Vec::new(),
            last_check_ms: 0,
            loaded: false,
            prev_files: HashMap::new(),
        }
    }
}

static STORE: OnceLock<Mutex<LoadState>> = OnceLock::new();

fn store() -> &'static Mutex<LoadState> {
    STORE.get_or_init(|| Mutex::new(LoadState::empty()))
}

thread_local! {
    static ACTIVE: std::cell::RefCell<Option<std::sync::Arc<Snapshot>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
thread_local! {
    static TEST_SNAP: std::cell::RefCell<Option<std::sync::Arc<Snapshot>>> =
        const { std::cell::RefCell::new(None) };
}

pub(crate) struct SnapGuard;

impl Drop for SnapGuard {
    fn drop(&mut self) {
        ACTIVE.with(|c| *c.borrow_mut() = None);
    }
}

pub(crate) fn activate_snap() -> SnapGuard {
    let snap = current_snap();
    ACTIVE.with(|c| *c.borrow_mut() = Some(snap));
    SnapGuard
}

fn current_snap() -> std::sync::Arc<Snapshot> {
    #[cfg(test)]
    if let Some(s) = TEST_SNAP.with(|c| c.borrow().clone()) {
        return s;
    }
    if let Some(s) = ACTIVE.with(|c| c.borrow().clone()) {
        return s;
    }
    store()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .snap
        .clone()
}

fn packs_dir() -> PathBuf {
    user_dict::user_dir().join(PACKS_DIR)
}

pub(crate) fn load_at_startup() {
    let dir = packs_dir();
    let mut g = store().lock().unwrap_or_else(|e| e.into_inner());
    poll(&mut g, &dir, now_ms(), true);
}

pub(crate) fn on_composition_start() {
    let dir = packs_dir();
    let mut g = store().lock().unwrap_or_else(|e| e.into_inner());
    poll(&mut g, &dir, now_ms(), false);
}

/// 这个上屏串是表情包里的符号或颜文字，不要写进自学习。
pub(crate) fn blocks_learning(word: &str) -> bool {
    current_snap().skip_learn.contains(word)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn poll(state: &mut LoadState, dir: &Path, now: u64, force: bool) {
    if state.loaded && !force && now.saturating_sub(state.last_check_ms) < CHECK_INTERVAL_MS {
        return;
    }
    state.last_check_ms = now;
    let sig = match signature(dir) {
        Ok(sig) => sig,
        Err(msg) => {
            let mut warnings = Vec::new();
            push_warning(&mut warnings, msg);
            write_warnings(dir, &warnings);
            state.loaded = true;
            crate::log("packs 目录读失败，继续用上一份");
            return;
        }
    };
    if state.loaded && !force && state.sig == sig {
        return;
    }
    let loaded = load_dir(dir, &sig, &state.prev_files);
    state.snap = std::sync::Arc::new(loaded.snap);
    state.prev_files = loaded.prev_files;
    state.sig = sig;
    state.loaded = true;
    write_warnings(dir, &loaded.warnings);
    crate::log(&format!(
        "packs 已读入：{} 个拼音，{} 条警告",
        state.snap.by_key.len(),
        loaded.warnings.len()
    ));
}

fn signature(dir: &Path) -> Result<Vec<FileSig>, String> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let meta = std::fs::metadata(dir).map_err(|e| format!("packs: 无法读取（{e}）"))?;
    if !meta.is_dir() {
        return Err("packs: 不是文件夹".to_string());
    }
    let rd = std::fs::read_dir(dir).map_err(|e| format!("packs: 无法列出文件（{e}）"))?;
    let mut names: Vec<String> = Vec::new();
    for ent in rd {
        let ent = match ent {
            Ok(ent) => ent,
            Err(e) => return Err(format!("packs: 列目录中断（{e}）")),
        };
        let name = ent.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !is_pack_name(name) {
            continue;
        }
        names.push(name.to_string());
    }
    names.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()).then(a.cmp(b)));
    let mut out = Vec::new();
    for name in names {
        let path = dir.join(&name);
        let meta = std::fs::metadata(&path).ok();
        let (mtime, len) = match meta {
            Some(m) if m.is_file() => (m.modified().ok(), m.len()),
            _ => (None, 0),
        };
        out.push(FileSig { name, mtime, len });
    }
    Ok(out)
}

fn is_pack_name(name: &str) -> bool {
    if name.starts_with('.') || name.len() < 5 {
        return false;
    }
    let (stem, ext) = name.split_at(name.len() - 4);
    ext.eq_ignore_ascii_case(".tsv")
        && !stem.is_empty()
        && !stem.contains(['/', '\\', '.'])
}

struct Loaded {
    snap: Snapshot,
    warnings: Vec<String>,
    prev_files: HashMap<String, Vec<PackItem>>,
}

fn load_dir(dir: &Path, sig: &[FileSig], prev: &HashMap<String, Vec<PackItem>>) -> Loaded {
    let mut warnings = Vec::new();
    let mut files: Vec<(String, Vec<PackItem>)> = Vec::new();
    let mut next_prev: HashMap<String, Vec<PackItem>> = HashMap::new();
    let listed = if sig.len() > MAX_PACK_FILES {
        push_warning(
            &mut warnings,
            format!(
                "packs: 超过 {MAX_PACK_FILES} 个文件，只读按文件名排在前面的 {MAX_PACK_FILES} 个"
            ),
        );
        &sig[..MAX_PACK_FILES]
    } else {
        sig
    };
    for file in listed {
        let path = dir.join(&file.name);
        match read_one(&path, &file.name) {
            Ok(one) => {
                warnings_extend(&mut warnings, one.warnings);
                next_prev.insert(file.name.clone(), one.items.clone());
                files.push((file.name.clone(), one.items));
            }
            Err(msg) => {
                push_warning(&mut warnings, msg);
                if let Some(old) = prev.get(&file.name) {
                    push_warning(
                        &mut warnings,
                        format!("{}: 这次没读成，暂时沿用上一份", file.name),
                    );
                    next_prev.insert(file.name.clone(), old.clone());
                    files.push((file.name.clone(), old.clone()));
                }
            }
        }
    }
    let (snap, more) = assemble(files);
    warnings_extend(&mut warnings, more);
    Loaded {
        snap,
        warnings,
        prev_files: next_prev,
    }
}

struct OneFile {
    items: Vec<PackItem>,
    warnings: Vec<String>,
}

fn read_one(path: &Path, name: &str) -> Result<OneFile, String> {
    let bytes: FileBytes = read_capped(path).map_err(|errs| {
        errs.into_iter()
            .next()
            .unwrap_or_else(|| format!("{name}: 无法读取"))
    })?;
    let mut warnings = Vec::new();
    if bytes.truncated {
        push_warning(
            &mut warnings,
            format!(
                "{name}: 文件有 {} 字节，超过 {MAX_BYTES} 字节，多出来的部分没有读",
                bytes.total_len
            ),
        );
    }
    let text = decode(name, &bytes.bytes, &mut warnings);
    let parsed = parse_pack_text(name, &text);
    warnings_extend(&mut warnings, parsed.warnings);
    Ok(OneFile {
        items: parsed.items,
        warnings,
    })
}

struct Parsed {
    items: Vec<PackItem>,
    warnings: Vec<String>,
}

fn parse_pack_text(file: &str, text: &str) -> Parsed {
    let mut warnings = Vec::new();
    let text = limit_text(file, text, &mut warnings);
    let lines = data_lines(file, text, &mut warnings);
    let mut items: Vec<PackItem> = Vec::new();
    let mut index: HashMap<(String, String), usize> = HashMap::new();
    for (line_no, line) in lines {
        let Some(item) = ingest_line(file, line_no, line, &mut warnings) else {
            continue;
        };
        let key = (item.ascii.clone(), item.surface.clone());
        if let Some(&prev) = index.get(&key) {
            let first = items[prev].line_no;
            push_warning(
                &mut warnings,
                format!(
                    "{file}:{line_no}: 「{} / {}」与第 {first} 行重复，以后一行生效",
                    item.surface, item.display
                ),
            );
            items[prev] = item;
        } else {
            index.insert(key, items.len());
            items.push(item);
        }
    }
    Parsed { items, warnings }
}

fn limit_text<'a>(file: &str, text: &'a str, warnings: &mut Vec<String>) -> &'a str {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.len() <= MAX_BYTES {
        return text;
    }
    push_warning(
        warnings,
        format!("{file}: 超过 {MAX_BYTES} 字节，多出来的部分没有读"),
    );
    let mut i = MAX_BYTES.min(text.len());
    while i > 0 && text.as_bytes()[i] & 0b1100_0000 == 0b1000_0000 {
        i -= 1;
    }
    &text[..i]
}

fn ingest_line(file: &str, line_no: usize, line: &str, warnings: &mut Vec<String>) -> Option<PackItem> {
    let fields: Vec<&str> = line.split('\t').map(str::trim).collect();
    if !(2..=4).contains(&fields.len()) {
        push_warning(
            warnings,
            format!(
                "{file}:{line_no}: 应为 2 到 4 列（关键词、候选、俄语、权重），实际 {} 列",
                fields.len()
            ),
        );
        return None;
    }
    let keyword = fields[0];
    let surface = fields[1];
    let gloss = if fields.len() >= 3 { fields[2] } else { "" };
    let weight_col = if fields.len() == 4 { fields[3] } else { "" };
    let reading = match analyze_reading(keyword) {
        Ok(r) => r,
        Err(msg) => {
            push_warning(warnings, format!("{file}:{line_no}: {msg}"));
            return None;
        }
    };
    if surface.is_empty() {
        push_warning(warnings, format!("{file}:{line_no}: 候选内容为空"));
        return None;
    }
    if surface.chars().count() > MAX_SURFACE_CHARS {
        push_warning(
            warnings,
            format!("{file}:{line_no}: 候选超过 {MAX_SURFACE_CHARS} 个字符"),
        );
        return None;
    }
    if surface.chars().any(|c| c.is_control()) {
        push_warning(
            warnings,
            format!("{file}:{line_no}: 候选里有控制字符"),
        );
        return None;
    }
    if gloss.chars().count() > MAX_GLOSS_CHARS {
        push_warning(
            warnings,
            format!("{file}:{line_no}: 俄语超过 {MAX_GLOSS_CHARS} 个字符"),
        );
        return None;
    }
    let weight = match parse_weight(weight_col) {
        Ok(w) => w,
        Err(msg) => {
            push_warning(warnings, format!("{file}:{line_no}: {msg}"));
            return None;
        }
    };
    Some(PackItem {
        surface: surface.to_string(),
        ascii: reading.ascii,
        display: reading.raw,
        gloss: gloss.to_string(),
        weight,
        file_ord: 0,
        line_no,
        file: file.to_string(),
    })
}

fn assemble(files: Vec<(String, Vec<PackItem>)>) -> (Snapshot, Vec<String>) {
    let mut warnings = Vec::new();
    let mut flat: Vec<PackItem> = Vec::new();
    let mut index: HashMap<(String, String), usize> = HashMap::new();
    for (ord, (_name, items)) in files.into_iter().enumerate() {
        for mut item in items {
            item.file_ord = ord;
            let key = (item.ascii.clone(), item.surface.clone());
            if let Some(&prev) = index.get(&key) {
                push_warning(
                    &mut warnings,
                    format!(
                        "{}:{line}: 「{} / {}」与 {} 第 {} 行重复，以后一行生效",
                        item.file,
                        item.surface,
                        item.display,
                        flat[prev].file,
                        flat[prev].line_no,
                        line = item.line_no,
                    ),
                );
                flat[prev] = item;
            } else {
                index.insert(key, flat.len());
                flat.push(item);
            }
        }
    }
    let mut skip_learn = HashSet::new();
    let mut by_key: HashMap<String, Vec<PackItem>> = HashMap::new();
    for item in flat {
        if !all_han(&item.surface) {
            skip_learn.insert(item.surface.clone());
        }
        by_key.entry(item.ascii.clone()).or_default().push(item);
    }
    (Snapshot { by_key, skip_learn }, warnings)
}

fn all_han(s: &str) -> bool {
    !s.is_empty()
        && s.chars().all(|c| {
            ('\u{3400}'..='\u{9FFF}').contains(&c)
                || ('\u{F900}'..='\u{FAFF}').contains(&c)
                || ('\u{20000}'..='\u{2CEAF}').contains(&c)
                || c == '\u{3007}'
        })
}

fn warnings_extend(out: &mut Vec<String>, more: Vec<String>) {
    for w in more {
        push_warning(out, w);
    }
}

fn warn_path(dir: &Path) -> PathBuf {
    match dir.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.join(WARN_FILE),
        _ => PathBuf::from(WARN_FILE),
    }
}

fn write_warnings(dir: &Path, warnings: &[String]) {
    let path = warn_path(dir);
    if warnings.is_empty() {
        let _ = std::fs::remove_file(path);
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut body = String::from(
        "# 旁译读取表情包时跳过的行。改完 tsv，重新打一个拼音后这里会更新。\n# 没有问题时这个文件会被删掉。\n",
    );
    for w in warnings {
        body.push_str(w);
        body.push('\n');
    }
    let _ = std::fs::write(path, body);
}

fn band(weight: Option<i64>) -> i32 {
    match weight.unwrap_or(0) {
        n if n >= user_dict::LEAD_WEIGHT => 0,
        n if n < 0 => 2,
        _ => 1,
    }
}

fn sort_band(v: &mut [PackItem]) {
    v.sort_by(|a, b| {
        let wa = a.weight.unwrap_or(0);
        let wb = b.weight.unwrap_or(0);
        wb.cmp(&wa)
            .then(a.file_ord.cmp(&b.file_ord))
            .then(a.line_no.cmp(&b.line_no))
    });
}

/// 把表情包候选插进已经排好序的列表。先插过用户词再调这个。
///
/// `head` 之前是短语和整句，不动。默认档紧挨内置精确第一名，并且排在同一档用户词后面。
/// 已经在列表里的词头不另插，也不改它的俄语。
pub(crate) fn merge_hits(hits: &mut Vec<Hit>, head: usize, ascii: &str) {
    let snap = if let Some(s) = ACTIVE.with(|c| c.borrow().clone()) {
        s
    } else {
        current_snap()
    };
    let Some(items) = snap.by_key.get(ascii) else {
        return;
    };
    if items.is_empty() {
        return;
    }
    let items = items.clone();
    merge_prepared(hits, head, &items);
}

fn merge_prepared(hits: &mut Vec<Hit>, head: usize, items: &[PackItem]) {
    if items.is_empty() {
        return;
    }
    let head = head.min(hits.len());
    let mut lead = Vec::new();
    let mut normal = Vec::new();
    let mut low = Vec::new();
    for it in items {
        if hits.iter().any(|h| h.word == it.surface) {
            continue;
        }
        match band(it.weight) {
            0 => lead.push(it.clone()),
            2 => low.push(it.clone()),
            _ => normal.push(it.clone()),
        }
    }
    sort_band(&mut lead);
    sort_band(&mut normal);
    sort_band(&mut low);
    insert_band(hits, slot_lead(hits, head), lead, 1);
    insert_band(hits, slot_normal(hits, head), normal, 2);
    insert_band(hits, slot_low(hits), low, 3);
}

fn slot_lead(hits: &[Hit], head: usize) -> usize {
    if let Some(i) = hits
        .iter()
        .enumerate()
        .skip(head)
        .find(|(_, h)| h.exact && h.rank_band == 0)
        .map(|(i, _)| i)
    {
        return i;
    }
    hits.iter()
        .enumerate()
        .skip(head)
        .find(|(_, h)| h.rank_band != 1)
        .map(|(i, _)| i)
        .unwrap_or(hits.len())
}

fn slot_normal(hits: &[Hit], head: usize) -> usize {
    if let Some(i) = hits
        .iter()
        .enumerate()
        .skip(head)
        .find(|(_, h)| h.exact && h.rank_band == 0)
        .map(|(i, _)| i)
    {
        let mut j = i + 1;
        while j < hits.len() && hits[j].rank_band == 2 {
            j += 1;
        }
        return j;
    }
    let mut j = head;
    while j < hits.len() && (hits[j].rank_band == 1 || hits[j].rank_band == 2) {
        j += 1;
    }
    j
}

fn slot_low(hits: &[Hit]) -> usize {
    match hits.iter().rposition(|h| h.exact) {
        Some(i) => i + 1,
        None => hits.len(),
    }
}

fn insert_band(hits: &mut Vec<Hit>, at: usize, items: Vec<PackItem>, rank_band: u8) {
    if items.is_empty() {
        return;
    }
    let at = at.min(hits.len());
    for it in items.into_iter().rev() {
        hits.insert(at, to_hit(it, rank_band));
    }
}

fn to_hit(it: PackItem, rank_band: u8) -> Hit {
    Hit {
        word: it.surface,
        pinyin: it.display,
        ru: it.gloss,
        freq: 0,
        tier: 0,
        exact: true,
        rank_band,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    struct Tmp {
        path: PathBuf,
    }

    impl Tmp {
        fn new() -> Self {
            static N: AtomicU64 = AtomicU64::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("pangyi-packs-{}-{}", std::process::id(), n));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn packs(&self) -> PathBuf {
            self.path.join(PACKS_DIR)
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            TEST_SNAP.with(|c| *c.borrow_mut() = None);
        }
    }

    fn install(snap: Snapshot) -> Guard {
        TEST_SNAP.with(|c| *c.borrow_mut() = Some(std::sync::Arc::new(snap)));
        Guard
    }

    fn h(word: &str, exact: bool, rank_band: u8) -> Hit {
        Hit {
            word: word.to_string(),
            pinyin: String::new(),
            ru: String::new(),
            freq: 1,
            tier: 0,
            exact,
            rank_band,
        }
    }

    fn item(surface: &str, weight: Option<i64>, line: usize) -> PackItem {
        PackItem {
            surface: surface.to_string(),
            ascii: "de".into(),
            display: "de".into(),
            gloss: String::new(),
            weight,
            file_ord: 0,
            line_no: line,
            file: "t.tsv".into(),
        }
    }

    fn names(hits: &[Hit]) -> Vec<&str> {
        hits.iter().map(|h| h.word.as_str()).collect()
    }

    fn set_mtime(path: &Path, secs: u64) {
        let f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
            .unwrap();
    }

    #[test]
    fn valid_lines_keep_keyword_gloss_and_weight() {
        let text = "\
# 注释\n\
\n\
  # 缩进也是注释\n\
xiao\t😄\tулыбка\n\
nǚ\t女旁\tЖЕНЩИНА\t50\n\
chang2\t数字\tВТОРОЙ\n\
jie\t★\tзвезда\t9999\n\
fu\t↓\t\t-9999\n\
";
        let r = parse_pack_text("t.tsv", text);
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        let xiao = r.items.iter().find(|i| i.ascii == "xiao").unwrap();
        assert_eq!(xiao.surface, "😄");
        assert_eq!(xiao.gloss, "улыбка");
        assert_eq!(xiao.weight, None);
        let nv = r.items.iter().find(|i| i.ascii == "nv").unwrap();
        assert_eq!(nv.surface, "女旁");
        assert_eq!(nv.gloss, "ЖЕНЩИНА");
        assert_eq!(nv.weight, Some(50));
        assert!(r.items.iter().any(|i| i.ascii == "chang" && i.surface == "数字"));
        assert_eq!(
            r.items.iter().find(|i| i.ascii == "jie").unwrap().weight,
            Some(9999)
        );
        assert_eq!(
            r.items.iter().find(|i| i.ascii == "fu").unwrap().weight,
            Some(-9999)
        );
        let raw = "\u{feff}xiao\t😄\tОК\r\n";
        let r = parse_pack_text("t.tsv", raw);
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        assert_eq!(r.items[0].surface, "😄");
    }

    #[test]
    fn malformed_lines_are_skipped_and_neighbors_kept() {
        let text = "\
hao\t★\tзвезда\n\
只有一列\n\
\t空关键词\t★\n\
坏读音\t★\n\
kong\t\t俄语\n\
taizhong\t★\t\t10000\n\
bushishu\t★\t\tabc\n\
zai\t☆\n\
";
        let r = parse_pack_text("a.tsv", text);
        assert_eq!(r.items.len(), 2, "{:?}", r.items);
        assert!(r.items.iter().any(|i| i.surface == "★"));
        assert!(r.items.iter().any(|i| i.surface == "☆"));
        assert!(r.warnings.len() >= 5, "{:?}", r.warnings);
        let long = format!("xiao\t{}\n", "颜".repeat(MAX_SURFACE_CHARS + 1));
        let r = parse_pack_text("a.tsv", &long);
        assert!(r.items.is_empty());
        assert!(r.warnings.iter().any(|w| w.contains("候选超过")), "{:?}", r.warnings);
    }

    #[test]
    fn duplicate_later_line_wins_and_byte_cap_warns() {
        let text = "\
xiao\t😄\t第一\t10\n\
xiao\t😄\t第二\t100\n\
ku\t😭\tслёзы\n\
";
        let r = parse_pack_text("a.tsv", text);
        assert!(r.warnings.iter().any(|w| w.contains("重复")), "{:?}", r.warnings);
        let xiao: Vec<_> = r.items.iter().filter(|i| i.ascii == "xiao").collect();
        assert_eq!(xiao.len(), 1);
        assert_eq!(xiao[0].gloss, "第二");
        assert_eq!(xiao[0].weight, Some(100));
        assert!(r.items.iter().any(|i| i.ascii == "ku"));

        let mut big = String::from("xiao\t😄\tОДИН\n");
        while big.len() < MAX_BYTES {
            big.push_str("# pad\n");
        }
        big.push_str("ku\t😭\tДВА\n");
        let r = parse_pack_text("a.tsv", &big);
        assert!(r.items.iter().any(|i| i.ascii == "xiao"));
        assert!(!r.items.iter().any(|i| i.ascii == "ku"));
        assert!(r.warnings.iter().any(|w| w.contains("超过")), "{:?}", r.warnings);
    }

    #[test]
    fn warning_list_is_capped() {
        let mut many = String::from("xiao\t😄\n");
        for _ in 0..40 {
            many.push_str("坏\n");
        }
        let r = parse_pack_text("a.tsv", &many);
        assert!(r.items.iter().any(|i| i.ascii == "xiao"));
        assert_eq!(
            r.warnings.len(),
            crate::user_dict::MAX_WARNINGS + 1,
            "{:?}",
            r.warnings.len()
        );
        assert!(r.warnings.last().unwrap().contains("更多"));
    }

    #[test]
    fn default_weight_sits_behind_top_exact_not_ahead_of_phrase() {
        let mut hits = vec![
            h("短语", false, 0),
            h("的", true, 0),
            h("得", true, 0),
            h("缩", false, 0),
        ];
        merge_prepared(&mut hits, 1, &[item("😀", None, 1)]);
        assert_eq!(names(&hits), vec!["短语", "的", "😀", "得", "缩"]);
    }

    #[test]
    fn lead_weight_passes_top_exact_but_not_phrase_or_user_lead() {
        let mut hits = vec![
            h("短语", false, 0),
            h("用户领先", true, 1),
            h("的", true, 0),
            h("得", true, 0),
        ];
        merge_prepared(
            &mut hits,
            1,
            &[item("更高", Some(150), 2), item("领先", Some(100), 1)],
        );
        assert_eq!(
            names(&hits),
            vec!["短语", "用户领先", "更高", "领先", "的", "得"]
        );
    }

    #[test]
    fn normal_pack_follows_user_normal_and_weight_orders_inside() {
        let mut hits = vec![
            h("的", true, 0),
            h("用户", true, 2),
            h("得", true, 0),
        ];
        let mut a = item("甲", Some(99), 2);
        a.gloss = "甲俄".into();
        let b = item("乙", None, 1);
        merge_prepared(&mut hits, 0, &[a, b]);
        assert_eq!(names(&hits), vec!["的", "用户", "甲", "乙", "得"]);
        assert_eq!(hits[2].ru, "甲俄");
        assert!(hits[2].exact);
        assert_eq!(hits[2].rank_band, 2);
    }

    #[test]
    fn negative_goes_after_exact_and_after_user_low() {
        let mut hits = vec![
            h("的", true, 0),
            h("缩", false, 0),
            h("得", true, 0),
            h("用户低", true, 3),
        ];
        merge_prepared(
            &mut hits,
            0,
            &[item("低", Some(-1), 1), item("更低", Some(-5), 2)],
        );
        assert_eq!(
            names(&hits),
            vec!["的", "缩", "得", "用户低", "低", "更低"]
        );
    }

    #[test]
    fn same_surface_as_existing_word_is_not_duplicated_or_reglossed() {
        let mut hits = vec![h("的", true, 0), h("得", true, 0)];
        hits[0].ru = "用户俄语".into();
        hits[0].freq = 318825;
        let mut dup = item("的", Some(100), 1);
        dup.gloss = "不该盖住".into();
        merge_prepared(&mut hits, 0, &[dup, item("😀", None, 2)]);
        assert_eq!(names(&hits), vec!["的", "😀", "得"]);
        assert_eq!(hits[0].ru, "用户俄语");
        assert_eq!(hits[0].freq, 318825);
        assert_eq!(hits[0].rank_band, 0);
    }

    #[test]
    fn no_exact_builtin_puts_default_before_the_rest() {
        let mut hits = vec![h("缩", false, 0)];
        merge_prepared(&mut hits, 0, &[item("新", None, 1)]);
        assert_eq!(names(&hits), vec!["新", "缩"]);
    }

    #[test]
    fn same_weight_keeps_file_then_line_order() {
        let mut hits = vec![h("的", true, 0)];
        let mut a = item("后文件", None, 1);
        a.file_ord = 1;
        let mut b = item("先文件", None, 1);
        b.file_ord = 0;
        let mut c = item("先文件后行", None, 3);
        c.file_ord = 0;
        merge_prepared(&mut hits, 0, &[a, c, b]);
        assert_eq!(names(&hits), vec!["的", "先文件", "先文件后行", "后文件"]);
    }

    #[test]
    fn blocks_learning_skips_symbols_but_not_pure_han() {
        let parsed = parse_pack_text("a.tsv", "de\t😀\tулыбка\nde\t词头\tСЛОВО\n");
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        let (snap, warn) = assemble(vec![("a.tsv".into(), parsed.items)]);
        assert!(warn.is_empty(), "{warn:?}");
        let _g = install(snap);
        assert!(blocks_learning("😀"));
        assert!(!blocks_learning("词头"));
        assert!(!blocks_learning("的"));
    }

    #[test]
    fn lookup_keeps_user_gloss_and_ranks_pack_after_user_word() {
        let _user = crate::user_dict::testing_install(
            "旁译新词\tde\tСАМОДЕЛ\t50\n旁译领先\tde\tЛИД\t100\n",
            "的\tde\tПОЛЬЗОВАТЕЛЬ_ДЕ\n",
        );
        let parsed = parse_pack_text(
            "emoji.tsv",
            "de\t😀\tулыбка\nde\t🔥\tогонь\t100\n",
        );
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        let (snap, _) = assemble(vec![("emoji.tsv".into(), parsed.items)]);
        let _pack = install(snap);
        let hits = crate::dict::lookup("de");
        let de = hits.iter().find(|h| h.word == "的").expect("应有「的」");
        assert_eq!(de.ru, "ПОЛЬЗОВАТЕЛЬ_ДЕ");
        assert_eq!(de.freq, 318825);
        let builtin = hits
            .iter()
            .position(|h| h.exact && h.rank_band == 0)
            .expect("应有内置精确候选");
        let lead = hits.iter().position(|h| h.word == "旁译领先").unwrap();
        let fire = hits.iter().position(|h| h.word == "🔥").unwrap();
        let neu = hits.iter().position(|h| h.word == "旁译新词").unwrap();
        let face = hits.iter().position(|h| h.word == "😀").unwrap();
        assert!(lead < fire && fire < builtin, "{:?}", names(&hits));
        assert_eq!(neu, builtin + 1, "用户词默认档仍紧跟内置第一名");
        assert_eq!(face, neu + 1, "表情包默认档排在同一档用户词后面");
        assert_eq!(hits[face].ru, "улыбка");
        assert_eq!(hits[fire].rank_band, 1);
        assert_eq!(
            crate::dict::ru_of("的").as_deref(),
            Some("ПОЛЬЗОВАТЕЛЬ_ДЕ"),
            "表情包不能改词典俄语"
        );
    }

    #[test]
    fn reload_respects_throttle_mtime_and_delete() {
        let tmp = Tmp::new();
        let dir = tmp.packs();
        let mut state = LoadState::empty();
        poll(&mut state, &dir, 1_000, true);
        assert!(state.snap.by_key.is_empty());
        assert!(!dir.exists(), "没有包时不要自己建目录");

        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.txt"), "xiao\t😄\n").unwrap();
        let pack = dir.join("emoji.tsv");
        std::fs::write(&pack, "xiao\t😄\tулыбка\n坏行\n").unwrap();
        set_mtime(&pack, 10_000);
        poll(&mut state, &dir, 1_500, false);
        assert!(
            state.snap.by_key.is_empty(),
            "一秒内不应因为新文件就重读"
        );
        poll(&mut state, &dir, 2_500, false);
        assert!(state.snap.by_key.contains_key("xiao"));
        assert!(!state.snap.by_key.contains_key("notes"));
        let warn = tmp.path.join(WARN_FILE);
        let text = std::fs::read_to_string(&warn).unwrap();
        assert!(text.contains("坏行") || text.contains("列"), "{text}");

        std::fs::write(&pack, "ku\t😭\tслёзы\n").unwrap();
        set_mtime(&pack, 20_000);
        poll(&mut state, &dir, 3_000, false);
        assert!(state.snap.by_key.contains_key("xiao"));
        poll(&mut state, &dir, 4_500, false);
        assert!(state.snap.by_key.contains_key("ku"));
        assert!(!state.snap.by_key.contains_key("xiao"));
        assert!(!warn.exists(), "修好之后警告文件应删掉");

        std::fs::remove_file(&pack).unwrap();
        poll(&mut state, &dir, 6_000, false);
        assert!(
            state.snap.by_key.is_empty(),
            "删掉文件就不再出这条候选"
        );
    }

    #[test]
    fn unreadable_file_keeps_previous_items() {
        let tmp = Tmp::new();
        let dir = tmp.packs();
        std::fs::create_dir_all(&dir).unwrap();
        let pack = dir.join("emoji.tsv");
        std::fs::write(&pack, "xiao\t😄\n").unwrap();
        set_mtime(&pack, 10_000);
        let mut state = LoadState::empty();
        poll(&mut state, &dir, 1_000, true);
        assert!(state.snap.by_key.contains_key("xiao"));
        std::fs::remove_file(&pack).unwrap();
        std::fs::create_dir(&pack).unwrap();
        poll(&mut state, &dir, 3_000, true);
        assert!(
            state.snap.by_key.contains_key("xiao"),
            "单个文件没读成时不能把上一份丢掉"
        );
        let text = std::fs::read_to_string(tmp.path.join(WARN_FILE)).unwrap();
        assert!(text.contains("不是文件") || text.contains("无法"), "{text}");
    }

    #[test]
    fn shipped_packs_parse_clean_and_cover_the_four_kinds() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../packs");
        let mut files = 0usize;
        let mut saw_emoji = false;
        let mut saw_kaomoji = false;
        let mut saw_symbol = false;
        let mut saw_ruble = false;
        let mut saw_numero = false;
        let mut saw_quote = false;
        for ent in std::fs::read_dir(&root).unwrap() {
            let path = ent.unwrap().path();
            if path.extension().and_then(|s| s.to_str()) != Some("tsv") {
                continue;
            }
            files += 1;
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(
                text.lines().any(|l| l.starts_with("#") && (l.contains("许可") || l.contains("CC"))),
                "{name} 头部要写许可"
            );
            let parsed = parse_pack_text(&name, &text);
            assert!(parsed.warnings.is_empty(), "{name} {:?}", parsed.warnings);
            assert!(parsed.items.len() >= 8, "{name} 条数太少");
            for it in &parsed.items {
                assert!(
                    !it.surface.chars().any(|c| c == '\u{FE0F}' || c == '\u{200D}'),
                    "{name} 含有 VS16 或 ZWJ：{}",
                    it.surface
                );
                assert!(it.weight.is_none(), "{name} 官方包不要抢在内置第一名前面");
                if it.surface == "😄" {
                    saw_emoji = true;
                }
                if it.surface.contains('▽') || it.surface.contains('ツ') {
                    saw_kaomoji = true;
                }
                if it.surface == "€" || it.surface == "≠" {
                    saw_symbol = true;
                }
                if it.surface == "\u{20BD}" {
                    saw_ruble = true;
                    assert_eq!(it.gloss, "рубль");
                    assert!(
                        it.ascii == "lubu" || it.ascii == "lubufuhao",
                        "{}",
                        it.ascii
                    );
                }
                if it.surface == "\u{2116}" {
                    saw_numero = true;
                    assert!(it.gloss.contains("номер"), "{}", it.gloss);
                }
                if it.surface == "\u{00AB}" {
                    saw_quote = true;
                }
            }
        }
        assert!(files >= 4, "至少四个官方包，实际 {files}");
        assert!(saw_emoji && saw_kaomoji && saw_symbol && saw_ruble && saw_numero && saw_quote);
    }
}
