//! A bounded, local undo journal. The dictionary remains the source of truth.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::settings_file::unique_stamp;

use super::{
    check_size, invalid, lock, read_capped, read_local, write_atomic, LexiconKind, MAX_BYTES,
};

const FILE: &str = "lexicon-undo.json";
const VERSION: u8 = 1;
const KEEP: usize = 10;
// JSON can escape each byte of a snapshot into at most six bytes.
const MAX_JOURNAL_BYTES: usize = (KEEP + 1) * (12 * MAX_BYTES + 1024);

#[derive(Clone, Serialize, Deserialize)]
struct Change {
    id: String,
    kind: LexiconKind,
    description: String,
    before: String,
    after: String,
}

impl Change {
    fn valid(&self) -> bool {
        !self.id.is_empty()
            && self.id.len() <= 64
            && !self.description.is_empty()
            && self.description.len() <= 256
            && !self.description.chars().any(char::is_control)
            && self.before.len() <= MAX_BYTES
            && self.after.len() <= MAX_BYTES
            && self.before != self.after
    }

    fn path(&self, dir: &Path) -> std::path::PathBuf {
        dir.join(self.kind.filename())
    }
}

#[derive(Serialize, Deserialize)]
enum Pending {
    Record(Change),
    Undo,
}

#[derive(Serialize, Deserialize)]
struct History {
    version: u8,
    entries: Vec<Change>,
    pending: Option<Pending>,
}

impl Default for History {
    fn default() -> Self {
        Self {
            version: VERSION,
            entries: Vec::new(),
            pending: None,
        }
    }
}

impl History {
    fn load(dir: &Path) -> io::Result<Self> {
        let path = dir.join(FILE);
        let parsed = match read_capped(&path, MAX_JOURNAL_BYTES) {
            Ok(bytes) => serde_json::from_slice::<Self>(
                bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes),
            )
            .ok(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(_) => None,
        };
        let Some(mut history) = parsed.filter(Self::valid) else {
            let empty = Self::default();
            let _ = empty.save(dir);
            return Ok(empty);
        };
        if let Some(pending) = history.pending.take() {
            // The pending record was written before the dictionary. Its exact contents
            // tell us which side of the atomic dictionary replacement survived.
            match pending {
                Pending::Record(change) => {
                    if read_local(&change.path(dir))? == change.after {
                        history.push(change);
                    }
                }
                Pending::Undo => {
                    if let Some(change) = history.entries.last() {
                        if read_local(&change.path(dir))? == change.before {
                            history.entries.pop();
                        }
                    }
                }
            }
            let _ = history.save(dir);
        }
        Ok(history)
    }

    fn valid(&self) -> bool {
        self.version == VERSION
            && self.entries.len() <= KEEP
            && self.entries.iter().all(Change::valid)
            && match &self.pending {
                Some(Pending::Record(change)) => change.valid(),
                Some(Pending::Undo) => !self.entries.is_empty(),
                None => true,
            }
    }

    fn push(&mut self, change: Change) {
        if self.entries.len() == KEEP {
            self.entries.remove(0);
        }
        self.entries.push(change);
    }

    fn save(&self, dir: &Path) -> io::Result<()> {
        let body = serde_json::to_string(self).map_err(io::Error::other)?;
        write_atomic(&dir.join(FILE), &body)
    }
}

// Mutation callers already hold the shared lexicon lock.
pub(super) fn save_change(
    dir: &Path,
    kind: LexiconKind,
    description: String,
    before: String,
    after: String,
) -> io::Result<()> {
    save(dir, kind, description, before, after, false)
}

pub(super) fn save_required_change(
    dir: &Path,
    kind: LexiconKind,
    description: String,
    before: String,
    after: String,
) -> io::Result<()> {
    save(dir, kind, description, before, after, true)
}

fn save(
    dir: &Path,
    kind: LexiconKind,
    description: String,
    before: String,
    after: String,
    require_undo: bool,
) -> io::Result<()> {
    let path = dir.join(kind.filename());
    if before == after {
        return Ok(());
    }
    let change = Change {
        id: format!("{}-{}", std::process::id(), unique_stamp()),
        kind,
        description,
        before,
        after,
    };
    // An unavailable journal must never prevent the user from saving their words.
    let loaded = History::load(dir);
    let mut journal = if require_undo { Some(loaded?) } else { loaded.ok() };
    let staged = if let Some(history) = journal.as_mut() {
        history.pending = Some(Pending::Record(change.clone()));
        let saved = history.save(dir);
        if require_undo {
            saved?;
            true
        } else {
            saved.is_ok()
        }
    } else {
        false
    };
    write_atomic(&path, &change.after)?;
    if let Some(mut history) = journal.filter(|_| staged) {
        history.pending = None;
        history.push(change);
        let _ = history.save(dir);
    }
    Ok(())
}

pub struct UndoAction {
    pub id: String,
    pub description: String,
}

pub fn undo_action(dir: &Path) -> Option<UndoAction> {
    // Avoid creating a directory or lock file merely by opening settings for the first time.
    fs::metadata(dir.join(FILE)).ok()?;
    let _lock = lock(dir).ok()?;
    let history = History::load(dir).ok()?;
    let change = history.entries.last()?;
    if read_local(&change.path(dir)).ok()? != change.after {
        return None;
    }
    Some(UndoAction {
        id: change.id.clone(),
        description: change.description.clone(),
    })
}

pub fn undo_last(dir: &Path, expected: Option<&str>) -> io::Result<Option<String>> {
    let _lock = lock(dir)?;
    let mut history = History::load(dir)?;
    let Some(change) = history.entries.last() else {
        return Ok(None);
    };
    if expected.is_some_and(|id| id != change.id) {
        return Err(invalid("撤销记录已更新，请核对当前按钮后重试"));
    }
    let path = change.path(dir);
    if read_local(&path)? != change.after {
        return Err(invalid("词库在操作后有新的修改，撤销会覆盖这些修改"));
    }
    check_size(&change.before)?;
    history.pending = Some(Pending::Undo);
    history.save(dir)?;
    write_atomic(&path, &change.before)?;
    let change = history.entries.pop().unwrap();
    history.pending = None;
    let _ = history.save(dir);
    Ok(Some(change.description))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexicon::{
        commit_import, delete_entries, edit_lexicon, lexicon_entries, move_weight, preview_import,
        DuplicatePolicy,
    };
    use crate::settings_file::test_dir;

    const WORDS: LexiconKind = LexiconKind::Words;

    fn import(dir: &Path, source: &str) {
        let preview =
            preview_import(dir, WORDS, source.as_bytes(), DuplicatePolicy::Skip, "").unwrap();
        commit_import(dir, &preview).unwrap();
    }

    #[test]
    fn reopened_history_undoes_import_weight_moves_and_single_or_bulk_deletions() {
        let root = test_dir("undo-unicode");
        let dir = root.join("用户 词库 · Русский");
        fs::create_dir(&dir).unwrap();
        let path = dir.join(WORDS.filename());
        let original = "\u{feff}# 旧词库\r\n";
        fs::write(&path, original).unwrap();
        import(
            &dir,
            "甲\tjia\tодин\t100\r\n乙\tjia\tдва\t200\r\n丙\tbing\tтри",
        );
        let imported = fs::read_to_string(&path).unwrap();
        let rows = lexicon_entries(&dir, WORDS).unwrap();
        let first = rows.iter().find(|entry| entry.head == "甲").unwrap();
        let moved = move_weight(&dir, first, true).unwrap().unwrap();
        let mut weighted = moved.clone();
        weighted.extra = "9999".into();
        edit_lexicon(&dir, WORDS, Some(&moved), Some(weighted)).unwrap();
        let rows = lexicon_entries(&dir, WORDS).unwrap();
        delete_entries(&dir, WORDS, &rows[..1]).unwrap();
        let remaining = lexicon_entries(&dir, WORDS).unwrap();
        delete_entries(&dir, WORDS, &remaining).unwrap();

        // All operations load from disk, with no process-local history to pass in.
        let file = dir.join(FILE);
        let data = fs::read_to_string(&file).unwrap();
        fs::write(&file, format!("\u{feff}{data}\r\n")).unwrap();
        for action in [
            "删除 2 条",
            "删除 1 条",
            "调整权重",
            "调整权重顺序",
            "导入 3 条",
        ] {
            assert_eq!(
                undo_action(&dir)
                    .map(|action| action.description)
                    .as_deref(),
                Some(action)
            );
            assert_eq!(undo_last(&dir, None).unwrap().as_deref(), Some(action));
            if action == "调整权重顺序" {
                assert_eq!(fs::read_to_string(&path).unwrap(), imported);
            }
        }
        assert_eq!(fs::read_to_string(path).unwrap(), original);
        assert!(undo_last(&dir, None).unwrap().is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn journal_keeps_only_the_latest_ten_operations() {
        let dir = test_dir("undo-retention");
        for index in 0..12 {
            import(&dir, &format!("词{index}\tci\tслово"));
        }
        for _ in 0..10 {
            assert_eq!(undo_last(&dir, None).unwrap().as_deref(), Some("导入 1 条"));
        }
        assert!(undo_last(&dir, None).unwrap().is_none());
        let rows = lexicon_entries(&dir, WORDS).unwrap();
        assert_eq!(
            rows.iter().map(|row| row.head.as_str()).collect::<Vec<_>>(),
            ["词0", "词1"]
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_journal_is_reset_without_touching_dictionary() {
        let dir = test_dir("undo-corrupt");
        import(&dir, "甲\tjia\tодин");
        let path = dir.join(WORDS.filename());
        let original = fs::read(&path).unwrap();
        fs::write(dir.join(FILE), [0xff, 0xfe, 0, 1]).unwrap();
        assert!(undo_action(&dir).map(|action| action.description).is_none());
        assert!(undo_last(&dir, None).unwrap().is_none());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(serde_json::from_slice::<History>(&fs::read(dir.join(FILE)).unwrap()).is_ok());
        import(&dir, "乙\tyi\tдва");
        assert_eq!(undo_last(&dir, None).unwrap().as_deref(), Some("导入 1 条"));
        assert_eq!(fs::read(&path).unwrap(), original);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unreadable_history_path_does_not_block_writes() {
        let dir = test_dir("undo-unreadable");
        let journal = dir.join(FILE);
        fs::create_dir(&journal).unwrap();
        import(&dir, "甲\tjia");
        assert_eq!(lexicon_entries(&dir, WORDS).unwrap().len(), 1);
        assert!(undo_action(&dir).is_none());
        assert!(undo_last(&dir, None).unwrap().is_none());
        fs::remove_dir(&journal).unwrap();
        import(&dir, "乙\tyi");
        assert!(undo_last(&dir, None).unwrap().is_some());
        assert_eq!(lexicon_entries(&dir, WORDS).unwrap().len(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn interrupted_record_and_undo_are_reconciled_with_dictionary_bytes() {
        let dir = test_dir("undo-interrupted");
        let change = Change {
            id: "journal-recovery".into(),
            kind: WORDS,
            description: "导入 1 条".into(),
            before: String::new(),
            after: "甲\tjia\r\n".into(),
        };
        let path = change.path(&dir);
        let staged = History {
            pending: Some(Pending::Record(change.clone())),
            ..History::default()
        };
        staged.save(&dir).unwrap();
        assert!(undo_action(&dir).map(|action| action.description).is_none());
        staged.save(&dir).unwrap();
        write_atomic(&path, &change.after).unwrap();
        assert_eq!(
            undo_action(&dir)
                .map(|action| action.description)
                .as_deref(),
            Some("导入 1 条")
        );
        let mut journal = History::load(&dir).unwrap();
        journal.pending = Some(Pending::Undo);
        journal.save(&dir).unwrap();
        assert!(undo_action(&dir).map(|action| action.description).is_some());
        journal.save(&dir).unwrap();
        write_atomic(&path, &change.before).unwrap();
        assert!(undo_action(&dir).map(|action| action.description).is_none());
        assert!(undo_last(&dir, None).unwrap().is_none());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn stale_window_cannot_undo_another_operation_with_the_same_label() {
        let dir = test_dir("undo-stale-label");
        import(&dir, "甲\tjia");
        let stale = undo_action(&dir).unwrap();
        import(&dir, "乙\tyi");
        let current = undo_action(&dir).unwrap();
        assert_eq!(stale.description, current.description);
        assert!(undo_last(&dir, Some(&stale.id)).is_err());
        assert_eq!(lexicon_entries(&dir, WORDS).unwrap().len(), 2);
        assert!(undo_last(&dir, Some(&current.id)).unwrap().is_some());
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn exclusively_open_history_does_not_prevent_a_dictionary_save() {
        use std::os::windows::fs::OpenOptionsExt;

        let dir = test_dir("undo-file-lock");
        import(&dir, "甲\tjia\tодин");
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(dir.join(FILE))
            .unwrap();
        import(&dir, "乙\tyi\tдва");
        assert_eq!(lexicon_entries(&dir, WORDS).unwrap().len(), 2);
        assert!(fs::read_dir(&dir).unwrap().all(|entry| !entry
            .unwrap()
            .path()
            .extension()
            .is_some_and(|ext| ext == "tmp")));
        drop(held);
        // A volume may allow replacement even when the previous handle was held.
        // Only the new operation can be offered, since the old snapshot is stale.
        if let Some(action) = undo_action(&dir) {
            undo_last(&dir, Some(&action.id)).unwrap();
            let rows = lexicon_entries(&dir, WORDS).unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].head, "甲");
        } else {
            assert_eq!(lexicon_entries(&dir, WORDS).unwrap().len(), 2);
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
