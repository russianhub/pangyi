//! 本机词库快照，供设置程序和安装器共用。

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) const WORDS_FILE: &str = "user-dict.tsv";
pub(crate) const GLOSSES_FILE: &str = "user-gloss.tsv";
const FILES: [&str; 2] = [WORDS_FILE, GLOSSES_FILE];
const KEEP_AUTOMATIC: usize = 5;

pub(crate) fn lock(dir: &Path) -> io::Result<File> {
    fs::create_dir_all(dir)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join("lexicon-edit.lock"))?;
    file.lock()?;
    Ok(file)
}

#[allow(dead_code)] // 安装器调用；设置侧在测试中复用。
pub(crate) fn automatic_upgrade_backup(dir: &Path) -> io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    let _lock = lock(dir)?;
    snapshot_locked(dir, false).map(|_| ())
}

// 调用者持有词库锁，导入备份与提交共用这把锁。
pub(crate) fn snapshot_locked(dir: &Path, manual: bool) -> io::Result<Option<PathBuf>> {
    let mut files = Vec::new();
    for name in FILES {
        match File::open(dir.join(name)) {
            Ok(file) => files.push((name, file)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
    }
    if files.is_empty() && !manual {
        return Ok(None);
    }
    let root = dir.join("lexicon-backups");
    fs::create_dir_all(&root)?;
    let prefix = if manual { "export-" } else { "auto-" };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for counter in 0..100 {
        let name = format!("{prefix}{now:020}-{:010}-{counter:02}", std::process::id());
        let pending = root.join(format!(".{name}.pending"));
        match fs::create_dir(&pending) {
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            result => result?,
        }
        let destination = root.join(name);
        let result = (|| {
            for (name, source) in &mut files {
                let mut target = OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(pending.join(name))?;
                io::copy(source, &mut target)?;
                target.sync_all()?;
            }
            if files.is_empty() {
                let mut file = File::create(pending.join("空词库.txt"))?;
                file.write_all("备份时还没有个人词条或释义覆盖。\n".as_bytes())?;
                file.sync_all()?;
            }
            fs::rename(&pending, &destination)
        })();
        if let Err(err) = result {
            let _ = fs::remove_dir_all(&pending);
            return Err(err);
        }
        if !manual {
            prune_automatic(&root);
        }
        return Ok(Some(destination));
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "无法创建词库备份目录",
    ))
}

fn prune_automatic(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let mut backups: Vec<_> = entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let Some(stamp) = name.to_str().and_then(|name| name.strip_prefix("auto-")) else {
                return false;
            };
            let parts: Vec<_> = stamp.split('-').collect();
            parts.len() == 3
                && parts.iter().zip([20, 10, 2]).all(|(part, size)| {
                    part.len() == size && part.bytes().all(|byte| byte.is_ascii_digit())
                })
                && entry.file_type().is_ok_and(|kind| kind.is_dir())
        })
        .map(|entry| entry.path())
        .collect();
    backups.sort_unstable();
    let remove = backups.len().saturating_sub(KEEP_AUTOMATIC);
    for backup in backups.into_iter().take(remove) {
        let _ = fs::remove_dir_all(backup);
    }
}
