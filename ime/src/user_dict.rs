//! 用户自己的词和俄语释义。
//!
//! 两个 UTF-8 文本放在 `%LOCALAPPDATA%\Pangyi\`（和短语、自学习、设置同一个文件夹）：
//! - `user-dict.tsv`：添加词（词头、拼音、可选俄语、可选权重）
//! - `user-gloss.tsv`：改已有词的整条俄语
//!
//! 启动时读一次。新的一次拼音开始时再看文件修改时间，大约一秒最多看一次。
//! 坏行跳过，不让输入法崩溃；最近的警告写在 `user-dict-warnings.txt`。
//! 这里不用 Windows API，方便单测。真正的按键和候选窗仍在 `lib.rs` / `dict.rs`。

use std::borrow::Cow;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use crate::dict::Hit;

/// 每个文件最多读这么多字节。再大就截断并记一条警告，避免一个巨大文件拖住启动。
pub(crate) const MAX_BYTES: usize = 256 * 1024;
/// 每个文件最多收下这么多数据行（空行和 # 注释不计）。
pub(crate) const MAX_DATA_LINES: usize = 5_000;
/// 警告列表最多留这么多条具体说明，再多就加一行「还有更多」。
pub(crate) const MAX_WARNINGS: usize = 32;
/// 词头最多多少个 Unicode 字符。
const MAX_HEAD_CHARS: usize = 32;
/// 俄语列最多多少个 Unicode 字符。
pub(crate) const MAX_GLOSS_CHARS: usize = 400;
/// 去掉声调之后的拼音字母最多多少个。
pub(crate) const MAX_READING_CHARS: usize = 48;
/// 权重要落在这个闭区间里。
pub(crate) const MIN_WEIGHT: i64 = -9_999;
pub(crate) const MAX_WEIGHT: i64 = 9_999;
/// 大于等于这个数，用户词才能排到「该拼音内置精确第一候选」前面。
pub(crate) const LEAD_WEIGHT: i64 = 100;
/// 两次查看文件修改时间的最小间隔。
pub(crate) const CHECK_INTERVAL_MS: u64 = 1_000;

const DICT_FILE: &str = "user-dict.tsv";
const GLOSS_FILE: &str = "user-gloss.tsv";
const WARN_FILE: &str = "user-dict-warnings.txt";

pub(crate) const DICT_TEMPLATE: &str = "\
# 旁译用户词库（user-dict.tsv）\r\n\
# 这个文件只在你自己的电脑上，不会上传，也不用登录。\r\n\
# 请用 UTF-8 保存。列与列之间用 TAB，不要用空格代替 TAB。\r\n\
# 以 # 开头的整行是说明。删掉行首的 # 才会生效。\r\n\
#\r\n\
# 列：词头<TAB>拼音<TAB>俄语（可以空着）<TAB>权重（可以空着）\r\n\
# 拼音写你要敲的字母。ü 用 v（女 = nv）。可以不标声调，也可以写 cháng 或 chang2。\r\n\
# 多音字分成两行：长 的 chang 和 zhang 各写一行。读音不能空。\r\n\
#\r\n\
# 权重不写、写 0，或写 1–99：排在这个拼音「内置精确第一候选」的后面。\r\n\
# 权重写 100 或更大：可以排到那个第一候选的前面（越大越靠前）。\r\n\
# 负数：排到所有内置精确候选的后面。自定义短语和整句结果始终在最前。\r\n\
#\r\n\
# 例子（去掉行首的 # 才生效）：\r\n\
# 张三\tzhangsan\tЧжан Сань\r\n\
# 旁译\tpangyi\tПанъи\t100\r\n\
";

pub(crate) const GLOSS_TEMPLATE: &str = "\
# 旁译用户俄语释义（user-gloss.tsv）\r\n\
# 用来改「已经在词库里的词」的整条俄语。\r\n\
# 比内置词典优先，也比随软件发布的手写覆盖优先。\r\n\
# 这个文件只在你自己的电脑上，不会上传，也不用登录。\r\n\
# 请用 UTF-8 保存。列与列之间用 TAB。\r\n\
# 以 # 开头的整行是说明。\r\n\
#\r\n\
# 列：词头<TAB>拼音<TAB>俄语<TAB>说明（可选，不进候选窗）\r\n\
# 读音不能空。多音字一行一个读音，否则两条会串。\r\n\
# 俄语整条换掉原来的释义，不会接在旧释义后面。几个义项用「 | 」分开。\r\n\
#\r\n\
# 例子（去掉行首的 # 才生效）：\r\n\
# 的\tde\t（структурная частица）\t结构助词\r\n\
# 长\tcháng\tдлинный\t长，不是生长\r\n\
# 长\tzhǎng\tрасти | старший\t生长、年长\r\n\
";

#[derive(Clone, Debug)]
struct GlossSlot {
    tone: String,
    raw: String,
    ru: String,
    from_gloss_file: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct UserWord {
    pub(crate) word: String,
    ascii: String,
    display: String,
    /// 候选上要显示的俄语。`user-gloss.tsv` 优先于本行的俄语列。空表示没写。
    pub(crate) gloss: String,
    pub(crate) weight: Option<i64>,
    pub(crate) line_no: usize,
}

#[derive(Clone, Debug, Default)]
struct Snapshot {
    /// 无声调、ü 已换成 v 的拼音 → 这个读音下的用户词。
    words: HashMap<String, Vec<UserWord>>,
    /// 词头 → 拼音 → 释义槽。供候选和翻译按读音取俄语。
    gloss: HashMap<String, HashMap<String, Vec<GlossSlot>>>,
    /// 词头只有一种用户俄语时，整句翻译没有读音也能用。多种则不猜。
    lone: HashMap<String, String>,
}

impl Snapshot {
    fn empty() -> Self {
        Self::default()
    }
}

struct LoadState {
    snap: std::sync::Arc<Snapshot>,
    dict_mtime: Option<SystemTime>,
    gloss_mtime: Option<SystemTime>,
    last_check_ms: u64,
    loaded: bool,
}

impl LoadState {
    fn empty() -> Self {
        Self {
            snap: std::sync::Arc::new(Snapshot::empty()),
            dict_mtime: None,
            gloss_mtime: None,
            last_check_ms: 0,
            loaded: false,
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

/// 一次查词期间钉住快照，避免读到一半文件被重载，也不必每个候选都抢锁。
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

fn active_snap() -> std::sync::Arc<Snapshot> {
    if let Some(s) = ACTIVE.with(|c| c.borrow().clone()) {
        return s;
    }
    current_snap()
}

/// 用户目录：`%LOCALAPPDATA%\Pangyi`。没有这个环境变量时用相对路径 `Pangyi`。
pub(crate) fn user_dir() -> PathBuf {
    match std::env::var_os("LOCALAPPDATA") {
        Some(base) if !base.is_empty() => PathBuf::from(base).join("Pangyi"),
        _ => PathBuf::from("Pangyi"),
    }
}

/// 输入法激活时调用：没有文件就写下带注释的模板，然后读入。
pub(crate) fn load_at_startup() {
    let dir = user_dir();
    let mut g = store().lock().unwrap_or_else(|e| e.into_inner());
    poll(&mut g, &dir, now_ms(), true);
}

/// 一次新的拼音开始时调用（缓冲从空变成第一个字母）。间隔不够就什么都不做。
pub(crate) fn on_composition_start() {
    let dir = user_dir();
    let mut g = store().lock().unwrap_or_else(|e| e.into_inner());
    poll(&mut g, &dir, now_ms(), false);
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
    let dict_path = dir.join(DICT_FILE);
    let gloss_path = dir.join(GLOSS_FILE);
    let mut warnings = Vec::new();
    ensure_template(&dict_path, DICT_TEMPLATE, &mut warnings);
    ensure_template(&gloss_path, GLOSS_TEMPLATE, &mut warnings);
    let dict_mtime = mtime(&dict_path);
    let gloss_mtime = mtime(&gloss_path);
    if state.loaded && !force && state.dict_mtime == dict_mtime && state.gloss_mtime == gloss_mtime
    {
        if !warnings.is_empty() {
            // 模板没写成，但仍把这条警告留下。已有快照不动。
            write_warnings(dir, &warnings);
        }
        return;
    }
    match read_both(&dict_path, &gloss_path) {
        Ok(report) => {
            warnings.extend(report.warnings);
            state.snap = std::sync::Arc::new(report.snap);
            state.dict_mtime = dict_mtime;
            state.gloss_mtime = gloss_mtime;
            state.loaded = true;
            write_warnings(dir, &warnings);
            crate::log(&format!(
                "user-dict 已读入：{} 个拼音，{} 条警告",
                state.snap.words.len(),
                warnings.len()
            ));
        }
        Err(err) => {
            warnings.extend(err);
            // 读失败时留着上一份，避免用户正保存文件时把词库清空。
            state.loaded = true;
            state.dict_mtime = dict_mtime;
            state.gloss_mtime = gloss_mtime;
            write_warnings(dir, &warnings);
            crate::log("user-dict 读取失败，继续用上一份");
        }
    }
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// 文件不存在时写模板。已存在（含用户改过的）不动。返回是否新写了。
fn ensure_template(path: &Path, template: &str, warnings: &mut Vec<String>) -> bool {
    if path.exists() {
        return false;
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                push_warning(
                    warnings,
                    format!("无法创建文件夹 {}（{e}）", parent.display()),
                );
                return false;
            }
        }
    }
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut f) => {
            if let Err(e) = f.write_all(template.as_bytes()) {
                push_warning(
                    warnings,
                    format!("{}: 没能写上模板（{e}）", file_label(path)),
                );
                return false;
            }
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(e) => {
            push_warning(
                warnings,
                format!("{}: 没能创建模板（{e}）", file_label(path)),
            );
            false
        }
    }
}

fn write_warnings(dir: &Path, warnings: &[String]) {
    let path = dir.join(WARN_FILE);
    if warnings.is_empty() {
        let _ = std::fs::remove_file(path);
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut body = String::from(
        "# 旁译读取用户词库时跳过的行。改完 tsv，重新打一个拼音后这里会更新。\n# 没有问题时这个文件会被删掉。\n",
    );
    for w in warnings {
        body.push_str(w);
        body.push('\n');
    }
    let _ = std::fs::write(path, body);
}

struct Report {
    snap: Snapshot,
    warnings: Vec<String>,
}

fn read_both(dict_path: &Path, gloss_path: &Path) -> Result<Report, Vec<String>> {
    let dict = read_capped(dict_path)?;
    let gloss = read_capped(gloss_path)?;
    let mut warnings = Vec::new();
    if dict.truncated {
        push_warning(
            &mut warnings,
            format!(
                "{}: 文件有 {} 字节，超过 {MAX_BYTES} 字节，多出来的部分没有读",
                DICT_FILE, dict.total_len
            ),
        );
    }
    if gloss.truncated {
        push_warning(
            &mut warnings,
            format!(
                "{}: 文件有 {} 字节，超过 {MAX_BYTES} 字节，多出来的部分没有读",
                GLOSS_FILE, gloss.total_len
            ),
        );
    }
    let dict_text = decode(DICT_FILE, &dict.bytes, &mut warnings);
    let gloss_text = decode(GLOSS_FILE, &gloss.bytes, &mut warnings);
    let mut report = parse_texts(&dict_text, &gloss_text);
    // 文件级警告放前面，但总数仍封顶：先拼再截。
    let mut merged = Vec::new();
    for w in warnings {
        push_warning(&mut merged, w);
    }
    for w in report.warnings {
        push_warning(&mut merged, w);
    }
    report.warnings = merged;
    Ok(report)
}

pub(crate) struct FileBytes {
    pub(crate) bytes: Vec<u8>,
    pub(crate) truncated: bool,
    pub(crate) total_len: u64,
}

pub(crate) fn read_capped(path: &Path) -> Result<FileBytes, Vec<String>> {
    let label = file_label(path);
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) => return Err(vec![format!("{label}: 无法读取（{e}）")]),
    };
    if !meta.is_file() {
        return Err(vec![format!("{label}: 不是文件")]);
    }
    let total = meta.len();
    let truncated = total > MAX_BYTES as u64;
    let want = (total as usize).min(MAX_BYTES);
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => return Err(vec![format!("{label}: 无法打开（{e}）")]),
    };
    let mut buf = vec![0u8; want];
    let mut got = 0;
    while got < want {
        match f.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) => return Err(vec![format!("{label}: 读取中断（{e}）")]),
        }
    }
    buf.truncate(got);
    if truncated {
        buf = trim_partial_utf8(&buf).to_vec();
    }
    Ok(FileBytes {
        bytes: buf,
        truncated,
        total_len: total,
    })
}

pub(crate) fn decode(label: &str, bytes: &[u8], warnings: &mut Vec<String>) -> String {
    match String::from_utf8(bytes.to_vec()) {
        Ok(s) => s,
        Err(e) => {
            push_warning(
                warnings,
                format!("{label}: 不是合法的 UTF-8，无法识别的字节已替换"),
            );
            String::from_utf8_lossy(e.as_bytes()).into_owned()
        }
    }
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn parse_texts(dict_text: &str, gloss_text: &str) -> Report {
    let dict_text = limit_text(dict_text);
    let gloss_text = limit_text(gloss_text);
    let mut warnings = Vec::new();
    if dict_text.truncated {
        push_warning(
            &mut warnings,
            format!("{DICT_FILE}: 超过 {MAX_BYTES} 字节，多出来的部分没有读"),
        );
    }
    if gloss_text.truncated {
        push_warning(
            &mut warnings,
            format!("{GLOSS_FILE}: 超过 {MAX_BYTES} 字节，多出来的部分没有读"),
        );
    }
    let mut words_index: HashMap<(String, String), usize> = HashMap::new();
    let mut words_flat: Vec<UserWord> = Vec::new();
    let mut gloss: HashMap<String, HashMap<String, Vec<GlossSlot>>> = HashMap::new();

    let dict_lines = data_lines(DICT_FILE, dict_text.text, &mut warnings);
    for (line_no, line) in dict_lines {
        ingest_dict_line(
            line_no,
            line,
            &mut words_flat,
            &mut words_index,
            &mut gloss,
            &mut warnings,
        );
    }
    let gloss_lines = data_lines(GLOSS_FILE, gloss_text.text, &mut warnings);
    for (line_no, line) in gloss_lines {
        ingest_gloss_line(line_no, line, &mut gloss, &mut warnings);
    }

    for w in &mut words_flat {
        if let Some(ru) = gloss_file_ru(&gloss, w) {
            w.gloss = ru;
        }
    }

    let mut words: HashMap<String, Vec<UserWord>> = HashMap::new();
    for w in words_flat {
        words.entry(w.ascii.clone()).or_default().push(w);
    }
    let lone = build_lone(&gloss);
    Report {
        snap: Snapshot { words, gloss, lone },
        warnings,
    }
}

struct Limited<'a> {
    text: &'a str,
    truncated: bool,
}

fn limit_text(text: &str) -> Limited<'_> {
    if text.len() <= MAX_BYTES {
        return Limited {
            text: strip_bom(text),
            truncated: false,
        };
    }
    let cut = cut_utf8(text.as_bytes(), MAX_BYTES);
    let text = std::str::from_utf8(cut).unwrap_or("");
    Limited {
        text: strip_bom(text),
        truncated: true,
    }
}

fn strip_bom(s: &str) -> &str {
    s.strip_prefix('\u{feff}').unwrap_or(s)
}

pub(crate) fn data_lines<'a>(
    file: &str,
    text: &'a str,
    warnings: &mut Vec<String>,
) -> Vec<(usize, &'a str)> {
    let mut out = Vec::new();
    let mut count = 0usize;
    for (idx, line) in text.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        count += 1;
        if count > MAX_DATA_LINES {
            push_warning(
                warnings,
                format!("{file}: 数据行超过 {MAX_DATA_LINES} 行，其余已忽略"),
            );
            break;
        }
        out.push((line_no, line));
    }
    out
}

fn ingest_dict_line(
    line_no: usize,
    line: &str,
    words: &mut Vec<UserWord>,
    index: &mut HashMap<(String, String), usize>,
    gloss: &mut HashMap<String, HashMap<String, Vec<GlossSlot>>>,
    warnings: &mut Vec<String>,
) {
    let fields = split_fields(line);
    if !(2..=4).contains(&fields.len()) {
        push_warning(
            warnings,
            format!(
                "{DICT_FILE}:{line_no}: 应为 2 到 4 列（词头、拼音、俄语、权重），实际 {} 列",
                fields.len()
            ),
        );
        return;
    }
    let head = fields[0];
    let reading = fields[1];
    let gloss_col = if fields.len() >= 3 { fields[2] } else { "" };
    let weight_col = if fields.len() == 4 { fields[3] } else { "" };
    let Some(head) = check_head(DICT_FILE, line_no, head, warnings) else {
        return;
    };
    let Some(reading) = check_reading(DICT_FILE, line_no, reading, warnings) else {
        return;
    };
    if gloss_col.chars().count() > MAX_GLOSS_CHARS {
        push_warning(
            warnings,
            format!("{DICT_FILE}:{line_no}: 俄语超过 {MAX_GLOSS_CHARS} 个字符"),
        );
        return;
    }
    let weight = match parse_weight(weight_col) {
        Ok(w) => w,
        Err(msg) => {
            push_warning(warnings, format!("{DICT_FILE}:{line_no}: {msg}"));
            return;
        }
    };
    let key = (head.clone(), reading.ascii.clone());
    if let Some(&prev) = index.get(&key) {
        let first = words[prev].line_no;
        push_warning(
            warnings,
            format!(
                "{DICT_FILE}:{line_no}: 「{head} / {}」与第 {first} 行重复，以后一行生效",
                reading.raw
            ),
        );
        words[prev] = UserWord {
            word: head.clone(),
            ascii: reading.ascii.clone(),
            display: reading.raw.clone(),
            gloss: gloss_col.to_string(),
            weight,
            line_no,
        };
    } else {
        index.insert(key, words.len());
        words.push(UserWord {
            word: head.clone(),
            ascii: reading.ascii.clone(),
            display: reading.raw.clone(),
            gloss: gloss_col.to_string(),
            weight,
            line_no,
        });
    }
    if !gloss_col.is_empty() {
        upsert_gloss(
            gloss,
            &head,
            &reading,
            gloss_col.to_string(),
            false,
            warnings,
            DICT_FILE,
            line_no,
        );
    }
}

fn ingest_gloss_line(
    line_no: usize,
    line: &str,
    gloss: &mut HashMap<String, HashMap<String, Vec<GlossSlot>>>,
    warnings: &mut Vec<String>,
) {
    let fields = split_fields(line);
    // 第 4 列是给人看的说明，和手写覆盖表一样，不进候选窗。再多就是坏行。
    if !(3..=4).contains(&fields.len()) {
        push_warning(
            warnings,
            format!(
                "{GLOSS_FILE}:{line_no}: 应为 3 或 4 列（词头、拼音、俄语、可选说明），实际 {} 列",
                fields.len()
            ),
        );
        return;
    }
    let head = fields[0];
    let reading = fields[1];
    let ru = fields[2];
    let Some(head) = check_head(GLOSS_FILE, line_no, head, warnings) else {
        return;
    };
    let Some(reading) = check_reading(GLOSS_FILE, line_no, reading, warnings) else {
        return;
    };
    if ru.is_empty() {
        push_warning(warnings, format!("{GLOSS_FILE}:{line_no}: 俄语为空"));
        return;
    }
    if ru.chars().count() > MAX_GLOSS_CHARS {
        push_warning(
            warnings,
            format!("{GLOSS_FILE}:{line_no}: 俄语超过 {MAX_GLOSS_CHARS} 个字符"),
        );
        return;
    }
    upsert_gloss(
        gloss,
        &head,
        &reading,
        ru.to_string(),
        true,
        warnings,
        GLOSS_FILE,
        line_no,
    );
}

fn upsert_gloss(
    gloss: &mut HashMap<String, HashMap<String, Vec<GlossSlot>>>,
    head: &str,
    reading: &Reading,
    ru: String,
    from_gloss_file: bool,
    warnings: &mut Vec<String>,
    file: &str,
    line_no: usize,
) {
    let slots = gloss
        .entry(head.to_string())
        .or_default()
        .entry(reading.ascii.clone())
        .or_default();
    let slot = GlossSlot {
        tone: reading.tone.clone(),
        raw: reading.raw.clone(),
        ru,
        from_gloss_file,
    };
    if let Some(prev) = slots.iter_mut().find(|s| s.tone == slot.tone) {
        // 释义文件盖住词库文件是预定的优先级，不叫重复。
        if prev.from_gloss_file == from_gloss_file {
            push_warning(
                warnings,
                format!(
                    "{file}:{line_no}: 「{head} / {}」与前面同一读音重复，以后一行生效",
                    reading.raw
                ),
            );
        }
        *prev = slot;
    } else {
        slots.push(slot);
    }
}

fn split_fields(line: &str) -> Vec<&str> {
    line.split('\t').map(str::trim).collect()
}

fn check_head(
    file: &str,
    line_no: usize,
    head: &str,
    warnings: &mut Vec<String>,
) -> Option<String> {
    if head.is_empty() {
        push_warning(warnings, format!("{file}:{line_no}: 词头为空"));
        return None;
    }
    if head.chars().count() > MAX_HEAD_CHARS {
        push_warning(
            warnings,
            format!("{file}:{line_no}: 词头超过 {MAX_HEAD_CHARS} 个字符"),
        );
        return None;
    }
    if !has_han(head) {
        push_warning(warnings, format!("{file}:{line_no}: 词头里要有汉字"));
        return None;
    }
    Some(head.to_string())
}

fn check_reading(
    file: &str,
    line_no: usize,
    reading: &str,
    warnings: &mut Vec<String>,
) -> Option<Reading> {
    match analyze_reading(reading) {
        Ok(r) => Some(r),
        Err(msg) => {
            push_warning(warnings, format!("{file}:{line_no}: {msg}"));
            None
        }
    }
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

fn gloss_file_ru(
    gloss: &HashMap<String, HashMap<String, Vec<GlossSlot>>>,
    w: &UserWord,
) -> Option<String> {
    let slots = gloss.get(&w.word)?.get(&w.ascii)?;
    let file_slots: Vec<GlossSlot> = slots
        .iter()
        .filter(|s| s.from_gloss_file)
        .cloned()
        .collect();
    if let Some(ru) = pick_ru(&file_slots, &w.ascii, &w.display) {
        return Some(ru.to_string());
    }
    if file_slots.len() == 1 {
        return Some(file_slots[0].ru.clone());
    }
    None
}

fn build_lone(gloss: &HashMap<String, HashMap<String, Vec<GlossSlot>>>) -> HashMap<String, String> {
    let mut tmp: HashMap<String, Option<String>> = HashMap::new();
    for (word, by_ascii) in gloss {
        for slots in by_ascii.values() {
            for s in slots {
                match tmp.get(word) {
                    None => {
                        tmp.insert(word.clone(), Some(s.ru.clone()));
                    }
                    Some(Some(prev)) if prev == &s.ru => {}
                    Some(Some(_)) => {
                        tmp.insert(word.clone(), None);
                    }
                    Some(None) => {}
                }
            }
        }
    }
    tmp.into_iter()
        .filter_map(|(w, ru)| ru.map(|r| (w, r)))
        .collect()
}

pub(crate) fn push_warning(out: &mut Vec<String>, msg: String) {
    if out.len() < MAX_WARNINGS {
        out.push(msg);
    } else if out.len() == MAX_WARNINGS {
        out.push("还有更多问题没有列出。".to_string());
    }
}

pub(crate) struct Reading {
    pub(crate) ascii: String,
    pub(crate) tone: String,
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

fn ascii_key(py: &str) -> Cow<'_, str> {
    if py.bytes().all(|b| b.is_ascii_lowercase()) {
        return Cow::Borrowed(py);
    }
    match analyze_reading(py) {
        Ok(r) => Cow::Owned(r.ascii),
        Err(_) => Cow::Owned(py.to_string()),
    }
}

fn tone_sig(py: &str) -> String {
    analyze_reading(py).map(|r| r.tone).unwrap_or_default()
}

/// 在这一组释义槽里选一条。声调对得上的优先，其次才是没写声调的那条。
/// 没写声调的条目只作用于这个拼写（chang 不会套到 zhang）。
fn pick_ru<'a>(slots: &'a [GlossSlot], py_ascii: &str, py_display: &str) -> Option<&'a str> {
    if let Some(s) = slots
        .iter()
        .find(|s| !py_display.is_empty() && s.raw == py_display)
    {
        return Some(s.ru.as_str());
    }
    let want = tone_sig(py_display);
    if !want.is_empty() {
        if let Some(s) = slots.iter().find(|s| s.tone == want) {
            return Some(s.ru.as_str());
        }
    }
    if let Some(s) = slots
        .iter()
        .find(|s| s.tone.is_empty() && s.raw == py_ascii)
    {
        return Some(s.ru.as_str());
    }
    slots
        .iter()
        .find(|s| s.tone.is_empty())
        .map(|s| s.ru.as_str())
}

fn overlay_in<'a>(
    snap: &Snapshot,
    word: &str,
    py_ascii: &str,
    py_display: &str,
    base: &'a str,
) -> Cow<'a, str> {
    if snap.gloss.is_empty() {
        return Cow::Borrowed(base);
    }
    let Some(by_ascii) = snap.gloss.get(word) else {
        return Cow::Borrowed(base);
    };
    let key = ascii_key(py_ascii);
    let Some(slots) = by_ascii.get(key.as_ref()) else {
        return Cow::Borrowed(base);
    };
    match pick_ru(slots, py_ascii, py_display) {
        Some(ru) => Cow::Owned(ru.to_string()),
        None => Cow::Borrowed(base),
    }
}

/// 用户释义盖住 `base`（base 应已含内置手写覆盖）。没有就原样返回。
pub(crate) fn overlay_ru<'a>(
    word: &str,
    py_ascii: &str,
    py_display: &str,
    base: &'a str,
) -> Cow<'a, str> {
    let snap = active_snap();
    overlay_in(&snap, word, py_ascii, py_display, base)
}

/// 词不在内置词典里、且用户只给了一种俄语时，整句翻译用它。
pub(crate) fn lone_gloss(word: &str) -> Option<String> {
    active_snap().lone.get(word).cloned()
}

fn band(weight: Option<i64>) -> i32 {
    match weight.unwrap_or(0) {
        n if n >= LEAD_WEIGHT => 0,
        n if n < 0 => 2,
        _ => 1,
    }
}

fn sort_band(v: &mut [UserWord]) {
    v.sort_by(|a, b| {
        let wa = a.weight.unwrap_or(0);
        let wb = b.weight.unwrap_or(0);
        wb.cmp(&wa).then_with(|| a.line_no.cmp(&b.line_no))
    });
}

/// 把用户词插进已经排好序的候选。
///
/// `head` 之前是自定义短语和整句，不动。
/// 默认权重紧挨在内置精确第一名后面；权重 ≥ 100 可以插到它前面；负数排到精确候选之后。
/// 已经是精确第一名的内置词，默认权重不会把它拉下来。
pub(crate) fn merge_hits(hits: &mut Vec<Hit>, head: usize, ascii: &str) {
    let snap = active_snap();
    let Some(words) = snap.words.get(ascii) else {
        return;
    };
    if words.is_empty() {
        return;
    }
    // 克隆这一小列，好让后面能改 hits，同时仍能读词条。
    let words = words.clone();
    merge_prepared(hits, head, &words);
}

fn merge_prepared(hits: &mut Vec<Hit>, head: usize, words: &[UserWord]) {
    if words.is_empty() {
        return;
    }
    let head = head.min(hits.len());
    let top_idx = hits
        .iter()
        .enumerate()
        .skip(head)
        .find(|(_, h)| h.exact)
        .map(|(i, _)| i);
    let top_word = top_idx.map(|i| hits[i].word.clone());

    let mut lead = Vec::new();
    let mut normal = Vec::new();
    let mut low = Vec::new();
    let mut drop_words: std::collections::HashSet<String> = std::collections::HashSet::new();

    for u in words {
        let in_head = hits.iter().take(head).any(|h| h.word == u.word);
        if in_head {
            drop_words.insert(u.word.clone());
            continue;
        }
        if top_word.as_deref() == Some(u.word.as_str()) && band(u.weight) != 2 {
            if !u.gloss.is_empty() {
                if let Some(h) = hits.iter_mut().find(|h| h.exact && h.word == u.word) {
                    h.ru = u.gloss.clone();
                }
            }
            continue;
        }
        drop_words.insert(u.word.clone());
        match band(u.weight) {
            0 => lead.push(u.clone()),
            2 => low.push(u.clone()),
            _ => normal.push(u.clone()),
        }
    }

    let mut pulled: HashMap<String, Hit> = HashMap::new();
    let mut i = hits.len();
    while i > head {
        i -= 1;
        if drop_words.contains(&hits[i].word) {
            let h = hits.remove(i);
            pulled.entry(h.word.clone()).or_insert(h);
        }
    }

    sort_band(&mut lead);
    sort_band(&mut normal);
    sort_band(&mut low);

    let take = |pulled: &mut HashMap<String, Hit>, u: UserWord, rank_band: u8| -> Hit {
        match pulled.remove(&u.word) {
            Some(mut h) => {
                if !u.gloss.is_empty() {
                    h.ru = u.gloss.clone();
                }
                h.exact = true;
                h.rank_band = rank_band;
                h
            }
            None => Hit {
                word: u.word,
                pinyin: u.display,
                ru: u.gloss,
                freq: 0,
                tier: 0,
                exact: true,
                rank_band,
            },
        }
    };

    let lead_hits: Vec<Hit> = lead.into_iter().map(|u| take(&mut pulled, u, 1)).collect();
    let normal_hits: Vec<Hit> = normal
        .into_iter()
        .map(|u| take(&mut pulled, u, 2))
        .collect();
    let low_hits: Vec<Hit> = low.into_iter().map(|u| take(&mut pulled, u, 3)).collect();
    let lead_n = lead_hits.len();

    let top = hits
        .iter()
        .enumerate()
        .skip(head.min(hits.len()))
        .find(|(_, h)| h.exact)
        .map(|(i, _)| i);

    let lead_at = top.unwrap_or(head.min(hits.len()));
    insert_rev(hits, lead_at, lead_hits);

    let top_after = top.map(|i| i + lead_n);
    let normal_at = match top_after {
        Some(i) => (i + 1).min(hits.len()),
        None => (head + lead_n).min(hits.len()),
    };
    insert_rev(hits, normal_at, normal_hits);

    // 普通/领先的用户词已经标成精确，所以「最后一个精确」会落在它们后面。
    // 一个精确候选都没有时，负数词放到列表末尾。
    let low_at = match hits.iter().rposition(|h| h.exact) {
        Some(i) => i + 1,
        None => hits.len(),
    };
    insert_rev(hits, low_at, low_hits);
}

fn insert_rev(hits: &mut Vec<Hit>, at: usize, items: Vec<Hit>) {
    let at = at.min(hits.len());
    for h in items.into_iter().rev() {
        hits.insert(at, h);
    }
}

fn cut_utf8(bytes: &[u8], max: usize) -> &[u8] {
    if bytes.len() <= max {
        return bytes;
    }
    let mut i = max;
    while i > 0 && bytes[i] & 0b1100_0000 == 0b1000_0000 {
        i -= 1;
    }
    if i < bytes.len() {
        let width = utf8_width(bytes[i]);
        if i + width > max {
            return &bytes[..i];
        }
    }
    &bytes[..i]
}

fn trim_partial_utf8(bytes: &[u8]) -> &[u8] {
    if bytes.is_empty() {
        return bytes;
    }
    let mut i = bytes.len() - 1;
    let mut cont = 0usize;
    while bytes[i] & 0b1100_0000 == 0b1000_0000 {
        if i == 0 {
            return &[];
        }
        i -= 1;
        cont += 1;
        if cont > 3 {
            break;
        }
    }
    let width = utf8_width(bytes[i]);
    if i + width > bytes.len() {
        &bytes[..i]
    } else {
        bytes
    }
}

fn utf8_width(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b & 0b1110_0000 == 0b1100_0000 {
        2
    } else if b & 0b1111_0000 == 0b1110_0000 {
        3
    } else if b & 0b1111_1000 == 0b1111_0000 {
        4
    } else {
        1
    }
}

#[cfg(test)]
pub(crate) struct TestInstall;

#[cfg(test)]
impl Drop for TestInstall {
    fn drop(&mut self) {
        TEST_SNAP.with(|c| *c.borrow_mut() = None);
    }
}

/// 测试里装上一份用户词库。离开作用域就清掉，避免串到别的测试。
#[cfg(test)]
pub(crate) fn testing_install(dict: &str, gloss: &str) -> TestInstall {
    let report = parse_texts(dict, gloss);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    TEST_SNAP.with(|c| *c.borrow_mut() = Some(std::sync::Arc::new(report.snap)));
    TestInstall
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
            let path =
                std::env::temp_dir().join(format!("pangyi-user-dict-{}-{}", std::process::id(), n));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
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

    fn parse(dict: &str, gloss: &str) -> Report {
        parse_texts(dict, gloss)
    }

    fn h(word: &str, exact: bool) -> Hit {
        Hit {
            word: word.to_string(),
            pinyin: String::new(),
            ru: String::new(),
            freq: 1,
            tier: 0,
            exact,
            rank_band: 0,
        }
    }

    fn uw(word: &str, weight: Option<i64>, line: usize) -> UserWord {
        UserWord {
            word: word.to_string(),
            ascii: "de".into(),
            display: "de".into(),
            gloss: String::new(),
            weight,
            line_no: line,
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
    fn valid_lines_keep_reading_weight_and_gloss() {
        let text = "\
# 注释\n\
\n\
  # 缩进也是注释\n\
两列\tlianglie\n\
三列\tsanlie\tТРИ\n\
好词\tcháng\tДЛИННЫЙ\t50\n\
女\tnǚ\tЖЕНЩИНА\n\
数字声调\tchang2\tВТОРОЙ\n\
边界\tjie\tГРАН\t9999\n\
负界\tfujie\tМИН\t-9999\n\
";
        let r = parse(text, "");
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        assert_eq!(r.snap.words.get("lianglie").unwrap()[0].gloss, "");
        assert_eq!(r.snap.words.get("lianglie").unwrap()[0].weight, None);
        assert_eq!(r.snap.words.get("sanlie").unwrap()[0].gloss, "ТРИ");
        let good = &r.snap.words.get("chang").unwrap()[0];
        assert_eq!(good.word, "好词");
        assert_eq!(good.gloss, "ДЛИННЫЙ");
        assert_eq!(good.weight, Some(50));
        assert!(r.snap.words.get("nv").is_some(), "nǚ 要收成敲键用的 nv");
        assert!(r
            .snap
            .words
            .get("chang")
            .unwrap()
            .iter()
            .any(|w| w.word == "数字声调"));
        assert_eq!(r.snap.words.get("jie").unwrap()[0].weight, Some(9999));
        assert_eq!(r.snap.words.get("fujie").unwrap()[0].weight, Some(-9999));
        let show = overlay_in(&r.snap, "女", "nü", "nǚ", "старое");
        assert_eq!(show.as_ref(), "ЖЕНЩИНА");
    }

    #[test]
    fn malformed_lines_are_skipped_and_neighbors_kept() {
        let text = "\
# 注释\n\
\n\
只有一列\n\
\tchang\t俄语\n\
词\t\t俄语\n\
abc\tchang\t俄语\n\
词\tchang bad\t俄语\n\
词\tchang\t俄语\t不是数字\n\
词\tchang\t俄语\t100\t多余\n\
词\tchang\t俄语\t10000\n\
好词\tchang\t留下\n\
";
        let r = parse(text, "");
        let lines: Vec<&str> = r
            .warnings
            .iter()
            .filter_map(|w| w.split(':').nth(1))
            .collect();
        assert!(
            lines.iter().any(|n| *n == "3"),
            "第 3 行应报错: {:?}",
            r.warnings
        );
        assert!(
            !r.warnings.iter().any(|w| w.starts_with("user-dict.tsv:1:")),
            "{:?}",
            r.warnings
        );
        assert!(!r.warnings.iter().any(|w| w.starts_with("user-dict.tsv:2:")));
        assert_eq!(r.warnings.len(), 8, "{:?}", r.warnings);
        assert_eq!(r.snap.words.get("chang").unwrap().len(), 1);
        assert_eq!(r.snap.words.get("chang").unwrap()[0].word, "好词");
        assert_eq!(r.snap.words.get("chang").unwrap()[0].gloss, "留下");
    }

    #[test]
    fn gloss_file_rejects_bad_rows_and_ignores_note() {
        let gloss = "\
# c\n\
的\tde\t来自释义\t说明不进候选\n\
\tde\tнет\n\
的\t\tнет\n\
的\tde\n\
长\tchang\tЧАН\n\
";
        let r = parse("", gloss);
        assert!(r.warnings.len() >= 3, "{:?}", r.warnings);
        assert_eq!(
            overlay_in(&r.snap, "的", "de", "de", "яркий").as_ref(),
            "来自释义"
        );
        assert_eq!(
            overlay_in(&r.snap, "长", "chang", "cháng", "старое").as_ref(),
            "ЧАН"
        );
        assert_eq!(
            overlay_in(&r.snap, "长", "zhang", "zhǎng", "старое").as_ref(),
            "старое"
        );
    }

    #[test]
    fn user_gloss_beats_user_dict_and_shipped_override() {
        let shipped = crate::gloss_override::ru_for("的", "de", "de", "яркий");
        assert!(
            shipped.contains("частица"),
            "测试前提：内置覆盖还在。实际 {shipped}"
        );
        let only_dict = parse("的\tde\t来自词库\n", "");
        assert_eq!(
            overlay_in(&only_dict.snap, "的", "de", "de", shipped).as_ref(),
            "来自词库"
        );
        let both = parse("的\tde\t来自词库\n", "的\tde\t来自释义\n");
        assert_eq!(
            overlay_in(&both.snap, "的", "de", "de", shipped).as_ref(),
            "来自释义"
        );
        let empty = parse("", "");
        assert_eq!(
            overlay_in(&empty.snap, "的", "de", "de", shipped).as_ref(),
            shipped
        );
    }

    #[test]
    fn polyphone_readings_do_not_share_a_gloss() {
        let gloss = "\
长\tchang\tПОЛЬЗОВАТЕЛЬ_ЧАН\n\
长\tzhang\tПОЛЬЗОВАТЕЛЬ_ЧЖАН\n\
长\tcháng\tПОЛЬЗОВАТЕЛЬ_ЧАН2\n\
坷\tkě\tПОЛЬЗОВАТЕЛЬ_КЕ3\n\
坷\tkē\tПОЛЬЗОВАТЕЛЬ_КЕ1\n\
";
        // cháng 比不标调的 chang 更具体，只改长音这一读；zhang 不受影响。
        let r = parse("", gloss);
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        assert_eq!(
            overlay_in(&r.snap, "长", "chang", "cháng", "старое").as_ref(),
            "ПОЛЬЗОВАТЕЛЬ_ЧАН2"
        );
        assert_eq!(
            overlay_in(&r.snap, "长", "zhang", "zhǎng", "старое").as_ref(),
            "ПОЛЬЗОВАТЕЛЬ_ЧЖАН"
        );
        assert_eq!(
            overlay_in(&r.snap, "坷", "ke", "kě", "старое").as_ref(),
            "ПОЛЬЗОВАТЕЛЬ_КЕ3"
        );
        assert_eq!(
            overlay_in(&r.snap, "坷", "ke", "kē", "старое").as_ref(),
            "ПОЛЬЗОВАТЕЛЬ_КЕ1"
        );
        // 没写声调的「长 / chang」不能流到 zhang。
        let one = parse("", "长\tchang\t只改长音\n");
        assert_eq!(
            overlay_in(&one.snap, "长", "zhang", "zhǎng", "原来").as_ref(),
            "原来"
        );
        assert_eq!(
            overlay_in(&one.snap, "长", "chang", "cháng", "原来").as_ref(),
            "只改长音"
        );
    }

    #[test]
    fn bom_and_crlf_parse() {
        let text = "\u{feff}# 注释\r\n甲\tjia\tОДИН\r\n乙\tyi\tДВА\r\n";
        let r = parse(text, "");
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        assert_eq!(r.snap.words.get("jia").unwrap()[0].word, "甲");
        assert_eq!(r.snap.words.get("yi").unwrap()[0].gloss, "ДВА");
        let raw = "\u{feff}甲\tjia\tОК\r\n";
        let r = parse(raw, "");
        assert_eq!(r.snap.words.get("jia").unwrap()[0].word, "甲");
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    }

    #[test]
    fn duplicate_later_line_wins() {
        let text = "\
测重\tchong\t第一\t10\n\
测重\tchong2\t第二\n\
测重\tchong\t第三\t100\n\
测长\tchang\tЧАН\n\
测张\tzhang\tЧЖАН\n\
";
        let r = parse(text, "");
        assert!(
            r.warnings.iter().any(|w| w.contains("重复")),
            "{:?}",
            r.warnings
        );
        let w = &r.snap.words.get("chong").unwrap();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].gloss, "第三");
        assert_eq!(w[0].weight, Some(100));
        assert_eq!(w[0].line_no, 3);
        assert_eq!(r.snap.words.get("chang").unwrap()[0].word, "测长");
        assert_eq!(r.snap.words.get("zhang").unwrap()[0].gloss, "ЧЖАН");
    }

    #[test]
    fn huge_file_is_cut_and_warning_list_is_capped() {
        let mut big = String::from("甲\tjia\tОДИН\n");
        while big.len() < MAX_BYTES {
            big.push_str("# pad\n");
        }
        big.push_str("乙\tyi\tДВА\n");
        assert!(big.len() > MAX_BYTES);
        let r = parse(&big, "");
        assert!(
            r.snap.words.contains_key("jia"),
            "{:?}",
            r.snap.words.keys()
        );
        assert!(!r.snap.words.contains_key("yi"));
        assert!(
            r.warnings.iter().any(|w| w.contains("超过")),
            "{:?}",
            r.warnings
        );

        let mut many = String::from("甲\tjia\tОК\n");
        for _ in 0..40 {
            many.push_str("坏\n");
        }
        let r = parse(&many, "");
        assert!(r.snap.words.contains_key("jia"));
        assert_eq!(r.warnings.len(), MAX_WARNINGS + 1, "{:?}", r.warnings.len());
        assert!(r.warnings.last().unwrap().contains("更多"));
    }

    #[test]
    fn templates_have_no_live_entries() {
        let d = parse(DICT_TEMPLATE, GLOSS_TEMPLATE);
        assert!(d.snap.words.is_empty(), "{:?}", d.snap.words);
        assert!(d.snap.gloss.is_empty());
        assert!(d.warnings.is_empty(), "{:?}", d.warnings);
    }

    #[test]
    fn default_weight_sits_behind_top_exact_not_ahead_of_phrase() {
        let mut hits = vec![
            h("短语", false),
            h("的", true),
            h("得", true),
            h("缩", false),
        ];
        merge_prepared(&mut hits, 1, &[uw("旁译自造", None, 1)]);
        assert_eq!(names(&hits), vec!["短语", "的", "旁译自造", "得", "缩"]);
    }

    #[test]
    fn lead_weight_passes_top_exact_but_not_phrase() {
        let mut hits = vec![h("短语", false), h("的", true), h("得", true)];
        let users = vec![uw("旁译领先", Some(100), 1), uw("旁译更高", Some(150), 2)];
        merge_prepared(&mut hits, 1, &users);
        assert_eq!(
            names(&hits),
            vec!["短语", "旁译更高", "旁译领先", "的", "得"]
        );
    }

    #[test]
    fn weight_99_stays_behind_top_and_outranks_omitted_weight() {
        let mut hits = vec![h("的", true), h("得", true)];
        let users = vec![uw("甲", Some(99), 2), uw("乙", None, 1)];
        merge_prepared(&mut hits, 0, &users);
        assert_eq!(names(&hits), vec!["的", "甲", "乙", "得"]);
    }

    #[test]
    fn same_default_weight_keeps_file_order() {
        let mut hits = vec![h("的", true), h("得", true)];
        merge_prepared(&mut hits, 0, &[uw("先", None, 1), uw("后", None, 2)]);
        assert_eq!(names(&hits), vec!["的", "先", "后", "得"]);
    }

    #[test]
    fn negative_weight_goes_after_exact_hits() {
        let mut hits = vec![h("的", true), h("缩", false), h("得", true)];
        merge_prepared(
            &mut hits,
            0,
            &[uw("低", Some(-1), 1), uw("更低", Some(-5), 2)],
        );
        assert_eq!(names(&hits), vec!["的", "缩", "得", "低", "更低"]);
    }

    #[test]
    fn negative_weight_demotes_the_current_top() {
        let mut hits = vec![h("的", true), h("得", true)];
        merge_prepared(&mut hits, 0, &[uw("的", Some(-1), 1)]);
        assert_eq!(names(&hits), vec!["得", "的"]);
    }

    #[test]
    fn existing_top_is_not_demoted_and_gloss_updates() {
        let mut hits = vec![h("的", true), h("得", true)];
        hits[0].ru = "旧".into();
        hits[0].freq = 318825;
        let mut u = uw("的", None, 1);
        u.gloss = "新".into();
        merge_prepared(&mut hits, 0, &[u]);
        assert_eq!(names(&hits), vec!["的", "得"]);
        assert_eq!(hits[0].ru, "新");
        assert_eq!(hits[0].freq, 318825);
    }

    #[test]
    fn existing_word_with_lead_weight_moves_ahead() {
        let mut hits = vec![h("的", true), h("得", true)];
        hits[1].ru = "旧得".into();
        let mut u = uw("得", Some(100), 1);
        u.gloss = "新得".into();
        merge_prepared(&mut hits, 0, &[u]);
        assert_eq!(names(&hits), vec!["得", "的"]);
        assert_eq!(hits[0].ru, "新得");
        assert!(hits[0].exact);
    }

    #[test]
    fn no_exact_builtin_puts_default_first_among_the_rest() {
        let mut hits = vec![h("缩", false)];
        merge_prepared(&mut hits, 0, &[uw("新", None, 1)]);
        assert_eq!(names(&hits), vec!["新", "缩"]);
    }

    #[test]
    fn word_already_in_the_head_is_not_duplicated() {
        let mut hits = vec![h("整句", false), h("别的", true)];
        merge_prepared(&mut hits, 1, &[uw("整句", Some(100), 1)]);
        assert_eq!(names(&hits), vec!["整句", "别的"]);
    }

    #[test]
    fn empty_input_does_not_panic() {
        let mut hits = Vec::new();
        merge_prepared(&mut hits, 0, &[uw("新", Some(100), 1)]);
        assert_eq!(names(&hits), vec!["新"]);
        let mut hits = vec![h("的", true)];
        merge_prepared(&mut hits, 0, &[]);
        assert_eq!(names(&hits), vec!["的"]);
    }

    #[test]
    fn lookup_applies_user_gloss_rank_and_polyphones() {
        let r = parse(
            "旁译新词\tde\tСАМОДЕЛ\n",
            "的\tde\tПОЛЬЗОВАТЕЛЬ_ДЕ\n长\tchang\tПОЛЬЗОВАТЕЛЬ_ЧАН\n长\tzhang\tПОЛЬЗОВАТЕЛЬ_ЧЖАН\n",
        );
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        let _g = install(r.snap);
        let hits = crate::dict::lookup("de");
        let de = hits
            .iter()
            .find(|h| h.word == "的")
            .expect("de 组应有「的」");
        assert_eq!(de.ru, "ПОЛЬЗОВАТЕЛЬ_ДЕ");
        assert_eq!(de.freq, 318825);
        let top = hits.iter().position(|h| h.exact).unwrap();
        let neu = hits.iter().position(|h| h.word == "旁译新词").unwrap();
        assert_eq!(neu, top + 1, "默认权重应紧跟内置精确第一名");
        let chang = crate::dict::lookup("chang");
        assert_eq!(
            chang.iter().find(|h| h.word == "长").unwrap().ru,
            "ПОЛЬЗОВАТЕЛЬ_ЧАН"
        );
        let zhang = crate::dict::lookup("zhang");
        assert_eq!(
            zhang.iter().find(|h| h.word == "长").unwrap().ru,
            "ПОЛЬЗОВАТЕЛЬ_ЧЖАН"
        );
        assert!(!zhang.iter().any(|h| h.word == "旁译新词"));
        assert_eq!(crate::dict::ru_of("的").as_deref(), Some("ПОЛЬЗОВАТЕЛЬ_ДЕ"));
        assert_eq!(crate::dict::ru_of("旁译新词").as_deref(), Some("САМОДЕЛ"));
        assert_eq!(crate::dict::ru_of("我").as_deref(), Some("я"));
    }

    #[test]
    fn lookup_lead_weight_passes_only_the_exact_top() {
        let r = parse("旁译领先\tde\tЛИД\t100\n旁译跟随\tde\tСЛЕД\t99\n", "");
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        let _g = install(r.snap);
        let hits = crate::dict::lookup("de");
        let lead = hits.iter().position(|h| h.word == "旁译领先").unwrap();
        let follow = hits.iter().position(|h| h.word == "旁译跟随").unwrap();
        let top = hits
            .iter()
            .enumerate()
            .find(|(_, h)| h.exact && h.word != "旁译领先" && h.word != "旁译跟随")
            .unwrap()
            .0;
        assert_eq!(lead + 1, top);
        assert_eq!(follow, top + 1);
        assert!(hits[..lead].iter().all(|h| !h.exact));
    }

    #[test]
    fn lookup_nv_uses_user_gloss() {
        let r = parse("女\tnǚ\tЖЕНЩИНА_ТЕСТ\n", "");
        let _g = install(r.snap);
        let hits = crate::dict::lookup("nv");
        let nu = hits.iter().find(|h| h.word == "女").expect("nv 应有「女」");
        assert_eq!(nu.ru, "ЖЕНЩИНА_ТЕСТ");
    }

    #[test]
    fn template_is_created_once_and_not_overwritten() {
        let tmp = Tmp::new();
        let path = tmp.path.join(DICT_FILE);
        let mut warnings = Vec::new();
        assert!(ensure_template(&path, DICT_TEMPLATE, &mut warnings));
        assert!(warnings.is_empty(), "{warnings:?}");
        std::fs::write(&path, "KEEP\n").unwrap();
        assert!(!ensure_template(&path, DICT_TEMPLATE, &mut warnings));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "KEEP\n");
    }

    #[test]
    fn reload_respects_throttle_and_mtime() {
        let tmp = Tmp::new();
        let mut state = LoadState::empty();
        poll(&mut state, &tmp.path, 1_000, true);
        assert!(state.snap.words.is_empty());
        assert!(tmp.path.join(DICT_FILE).is_file());
        assert!(tmp.path.join(GLOSS_FILE).is_file());
        let parsed = parse(
            &std::fs::read_to_string(tmp.path.join(DICT_FILE)).unwrap(),
            &std::fs::read_to_string(tmp.path.join(GLOSS_FILE)).unwrap(),
        );
        assert!(parsed.snap.words.is_empty());
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);

        let dict = tmp.path.join(DICT_FILE);
        std::fs::write(&dict, "甲\tjia\tОДИН\n").unwrap();
        set_mtime(&dict, 10_000);
        poll(&mut state, &tmp.path, 1_500, false);
        assert!(state.snap.words.is_empty(), "一秒内不应因为改了文件就重读");
        poll(&mut state, &tmp.path, 2_500, false);
        assert!(state.snap.words.contains_key("jia"));

        std::fs::write(&dict, "乙\tyi\tДВА\n").unwrap();
        set_mtime(&dict, 20_000);
        poll(&mut state, &tmp.path, 3_000, false);
        assert!(state.snap.words.contains_key("jia"));
        assert!(!state.snap.words.contains_key("yi"));
        poll(&mut state, &tmp.path, 4_500, false);
        assert!(state.snap.words.contains_key("yi"));
        assert!(!state.snap.words.contains_key("jia"));
    }

    #[test]
    fn bad_lines_warn_without_dropping_good_ones_and_clear_when_fixed() {
        let tmp = Tmp::new();
        let mut state = LoadState::empty();
        poll(&mut state, &tmp.path, 0, true);
        let dict = tmp.path.join(DICT_FILE);
        std::fs::write(&dict, "好行\thao\tОК\n坏行\n").unwrap();
        set_mtime(&dict, 30_000);
        poll(&mut state, &tmp.path, 5_000, true);
        assert_eq!(state.snap.words.get("hao").unwrap()[0].word, "好行");
        let warn_path = tmp.path.join(WARN_FILE);
        let text = std::fs::read_to_string(&warn_path).unwrap();
        assert!(text.contains("坏行") || text.contains("列"), "{text}");
        std::fs::write(&dict, "好行\thao\tОК\n").unwrap();
        set_mtime(&dict, 40_000);
        poll(&mut state, &tmp.path, 9_000, true);
        assert!(!warn_path.exists(), "修好之后警告文件应删掉");
        assert!(state.snap.words.contains_key("hao"));
    }

    #[test]
    fn unreadable_file_keeps_the_previous_snapshot() {
        let tmp = Tmp::new();
        let mut state = LoadState::empty();
        poll(&mut state, &tmp.path, 0, true);
        let dict = tmp.path.join(DICT_FILE);
        std::fs::write(&dict, "甲\tjia\tОДИН\n").unwrap();
        set_mtime(&dict, 50_000);
        poll(&mut state, &tmp.path, 2_000, true);
        assert!(state.snap.words.contains_key("jia"));
        std::fs::remove_file(&dict).unwrap();
        std::fs::create_dir(&dict).unwrap();
        poll(&mut state, &tmp.path, 4_000, true);
        assert!(
            state.snap.words.contains_key("jia"),
            "读失败不能把已经载入的词丢掉"
        );
        let text = std::fs::read_to_string(tmp.path.join(WARN_FILE)).unwrap();
        assert!(text.contains("不是文件") || text.contains("无法"), "{text}");
    }
}
