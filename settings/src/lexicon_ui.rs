use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pangyi_settings::{
    commit_import, edit_lexicon, lexicon_entries, load_counts, preview_import, read_import,
    DuplicatePolicy, ImportPreview, LexiconEntry, LexiconKind,
};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::MainWindow;

const THEMES: [&str; 3] = [
    include_str!("../../dictionaries/travel.tsv"),
    include_str!("../../dictionaries/food.tsv"),
    include_str!("../../dictionaries/work.tsv"),
];

#[derive(Default)]
struct Editor {
    visible: Vec<LexiconEntry>,
    original: Option<LexiconEntry>,
    import: Option<(LexiconKind, Vec<u8>)>,
    preview: Option<ImportPreview>,
}

pub(super) fn wire(ui: &MainWindow, dir: &Path) {
    let editor = Rc::new(RefCell::new(Editor::default()));

    let weak = ui.as_weak();
    let state = editor.clone();
    let folder = dir.to_path_buf();
    ui.on_lex_filter(move || {
        let Some(ui) = weak.upgrade() else { return };
        let mut state = state.borrow_mut();
        match lexicon_entries(&folder, kind(&ui)) {
            Ok(mut entries) => {
                let search = ui.get_lex_search().trim().to_lowercase();
                if !search.is_empty() {
                    entries.retain(|row| {
                        [&row.head, &row.reading, &row.russian]
                            .iter()
                            .any(|field| field.to_lowercase().contains(&search))
                            || row.trigger().contains(&search)
                    });
                }
                state.visible = entries;
            }
            Err(error) => {
                state.visible.clear();
                ui.set_status(format!("无法读取词库：{error}").into());
            }
        }
        let labels: Vec<SharedString> = std::iter::once("选择已保存的词条".into())
            .chain(state.visible.iter().map(|row| {
                let trigger = row.trigger();
                let reading = if trigger == row.reading {
                    trigger
                } else {
                    format!("{} → {trigger}", row.reading)
                };
                format!("{} · {reading} · {}", row.head, row.russian).into()
            }))
            .collect();
        ui.set_lex_labels(ModelRc::new(VecModel::from(labels)));
        ui.set_lex_index(0);
    });

    let weak = ui.as_weak();
    let state = editor.clone();
    ui.on_lex_select(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        if index < 1 {
            return;
        }
        let Some(entry) = state.borrow().visible.get((index - 1) as usize).cloned() else {
            return;
        };
        set_fields(&ui, &entry);
        ui.set_lex_editing(true);
        state.borrow_mut().original = Some(entry);
    });

    let weak = ui.as_weak();
    let state = editor.clone();
    ui.on_lex_clear(move || {
        if let Some(ui) = weak.upgrade() {
            state.borrow_mut().original = None;
            set_fields(&ui, &LexiconEntry::default());
            ui.set_lex_editing(false);
            ui.set_lex_index(0);
        }
    });

    let weak = ui.as_weak();
    ui.on_lex_kind_changed(move || {
        if let Some(ui) = weak.upgrade() {
            ui.invoke_lex_clear();
            ui.invoke_lex_filter();
        }
    });

    let weak = ui.as_weak();
    let state = editor.clone();
    let folder = dir.to_path_buf();
    ui.on_lex_save(move || {
        let Some(ui) = weak.upgrade() else { return };
        let replacement = LexiconEntry {
            head: ui.get_lex_head().into(),
            reading: ui.get_lex_reading().into(),
            russian: ui.get_lex_russian().into(),
            extra: ui.get_lex_extra().into(),
        };
        let original = state.borrow().original.clone();
        match edit_lexicon(&folder, kind(&ui), original.as_ref(), Some(replacement)) {
            Ok(()) => {
                ui.set_status("词条已保存。请重新输入完整拼音查看候选。".into());
                ui.invoke_lex_clear();
                refresh(&ui, &folder);
            }
            Err(error) => ui.set_status(format!("词条未保存：{error}").into()),
        }
    });

    let weak = ui.as_weak();
    let state = editor.clone();
    let folder = dir.to_path_buf();
    ui.on_lex_delete(move || {
        let Some(ui) = weak.upgrade() else { return };
        let Some(original) = state.borrow().original.clone() else {
            return;
        };
        match edit_lexicon(&folder, kind(&ui), Some(&original), None) {
            Ok(()) => {
                ui.set_status("已删除所选词条。".into());
                ui.invoke_lex_clear();
                refresh(&ui, &folder);
            }
            Err(error) => ui.set_status(format!("词条未删除：{error}").into()),
        }
    });

    let weak = ui.as_weak();
    let state = editor.clone();
    ui.on_lex_import(move || {
        let Some(ui) = weak.upgrade() else { return };
        match choose_tsv().and_then(|path| {
            path.map(|path| read_import(&path))
                .transpose()
                .map_err(|err| err.to_string())
        }) {
            Ok(Some(bytes)) => start_import(&ui, &state, kind(&ui), bytes),
            Ok(None) => {}
            Err(error) => ui.set_status(format!("无法读取导入文件：{error}").into()),
        }
    });

    let weak = ui.as_weak();
    let state = editor.clone();
    ui.on_lex_add_pack(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        if let Some(text) = THEMES.get(index as usize) {
            start_import(&ui, &state, LexiconKind::Words, text.as_bytes().to_vec());
        }
    });

    let weak = ui.as_weak();
    let state = editor.clone();
    let folder = dir.to_path_buf();
    ui.on_lex_preview(move || {
        let Some(ui) = weak.upgrade() else { return };
        let mut state = state.borrow_mut();
        let Some((kind, bytes)) = state.import.as_ref() else {
            return;
        };
        let policy = match ui.get_lex_duplicate_policy() {
            1 => DuplicatePolicy::Replace,
            2 => DuplicatePolicy::Merge,
            _ => DuplicatePolicy::Skip,
        };
        state.preview = match preview_import(&folder, *kind, bytes, policy) {
            Ok(plan) => {
                ui.set_lex_preview_summary(
                    format!(
                        "将新增 {} 条、跳过 {} 条、覆盖 {} 条、合并 {} 条",
                        plan.added, plan.skipped, plan.replaced, plan.merged
                    )
                    .into(),
                );
                ui.set_lex_issues(plan.issues.join("\n").into());
                ui.set_lex_can_import(plan.has_changes());
                Some(plan)
            }
            Err(error) => {
                ui.set_lex_preview_summary(format!("无法预览：{error}").into());
                ui.set_lex_issues("".into());
                ui.set_lex_can_import(false);
                None
            }
        };
    });

    let weak = ui.as_weak();
    let state = editor.clone();
    ui.on_lex_cancel_import(move || {
        if let Some(ui) = weak.upgrade() {
            state.borrow_mut().import = None;
            state.borrow_mut().preview = None;
            ui.set_lex_preview_open(false);
        }
    });

    let weak = ui.as_weak();
    let state = editor.clone();
    let folder = dir.to_path_buf();
    ui.on_lex_confirm_import(move || {
        let Some(ui) = weak.upgrade() else { return };
        let result = {
            let state = state.borrow();
            let Some(plan) = state.preview.as_ref() else {
                return;
            };
            commit_import(&folder, plan)
        };
        match result {
            Ok(()) => {
                ui.invoke_lex_cancel_import();
                ui.set_status("词典已导入。重新输入拼音查看候选。".into());
                refresh(&ui, &folder);
            }
            Err(error) => {
                ui.set_status(format!("导入失败：{error}").into());
                ui.invoke_lex_preview();
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_lex_catalog(move || {
        let Some(ui) = weak.upgrade() else { return };
        if let Err(error) = open_catalog() {
            ui.set_status(format!("无法打开词典下载目录：{error}").into());
        }
    });
}

fn kind(ui: &MainWindow) -> LexiconKind {
    if ui.get_lex_kind() == 1 {
        LexiconKind::Glosses
    } else {
        LexiconKind::Words
    }
}

fn set_fields(ui: &MainWindow, entry: &LexiconEntry) {
    ui.set_lex_head(entry.head.as_str().into());
    ui.set_lex_reading(entry.reading.as_str().into());
    ui.set_lex_russian(entry.russian.as_str().into());
    ui.set_lex_extra(entry.extra.as_str().into());
}

fn refresh(ui: &MainWindow, folder: &Path) {
    ui.invoke_lex_filter();
    crate::push_stats(ui, &load_counts(folder));
}

fn start_import(ui: &MainWindow, state: &RefCell<Editor>, kind: LexiconKind, bytes: Vec<u8>) {
    state.borrow_mut().import = Some((kind, bytes));
    ui.set_lex_duplicate_policy(0);
    ui.set_lex_preview_open(true);
    ui.invoke_lex_preview();
}

#[cfg(windows)]
fn choose_tsv() -> Result<Option<PathBuf>, String> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::UI::Controls::Dialogs::{
        CommDlgExtendedError, GetOpenFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR,
        OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };

    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;

    let filter: Vec<u16> = "TSV 词典\0*.tsv\0所有文件\0*.*\0\0"
        .encode_utf16()
        .collect();
    let mut path = vec![0u16; 32_768];
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: unsafe { GetActiveWindow() },
        lpstrFilter: filter.as_ptr(),
        lpstrFile: path.as_mut_ptr(),
        nMaxFile: path.len() as u32,
        Flags: OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_NOCHANGEDIR | OFN_PATHMUSTEXIST,
        ..Default::default()
    };
    // 对话框返回前，过滤器和路径缓冲区一直存在。
    if unsafe { GetOpenFileNameW(&mut dialog) } != 0 {
        let end = path
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(path.len());
        return Ok(Some(std::ffi::OsString::from_wide(&path[..end]).into()));
    }
    match unsafe { CommDlgExtendedError() } {
        0 => Ok(None),
        code => Err(format!("文件选择窗口错误 {code:#x}")),
    }
}

#[cfg(not(windows))]
fn choose_tsv() -> Result<Option<PathBuf>, String> {
    Err("文件选择窗口仅在 Windows 上可用".into())
}

#[cfg(windows)]
fn open_catalog() -> Result<(), String> {
    crate::shell_open(Path::new(
        "https://github.com/russianhub/pangyi/tree/main/dictionaries",
    ))
}

#[cfg(not(windows))]
fn open_catalog() -> Result<(), String> {
    Err("请访问 GitHub 上的 russianhub/pangyi 词典目录".into())
}
