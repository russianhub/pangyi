use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pangyi_settings::learned::{self, LearnedWord, Preparation};
use pangyi_settings::{
    commit_import, crowded_reading, delete_entries, edit_lexicon, export_backup, lexicon_entries,
    load_counts, move_weight, preview_import, read_import, undo_action, undo_last, DuplicatePolicy,
    ImportPreview, LexiconEntry, LexiconKind,
};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use crate::{LearnedChoice, LexChoice, MainWindow};

const THEMES: [&str; 3] = [
    include_str!("../../dictionaries/travel.tsv"),
    include_str!("../../dictionaries/food.tsv"),
    include_str!("../../dictionaries/work.tsv"),
];

#[derive(Default)]
struct Editor {
    visible: Vec<LexiconEntry>,
    original: Option<LexiconEntry>,
    import: Option<(LexiconKind, Vec<u8>, String)>,
    all: Vec<LexiconEntry>,
    selected: HashSet<LexiconEntry>,
    preview: Option<ImportPreview>,
    learned: Vec<LearnedWord>,
    learned_draft: bool,
}

impl Editor {
    fn save_learned_draft(&self, dir: &Path, entry: &LexiconEntry) -> Option<std::io::Result<bool>> {
        self.learned_draft.then(|| learned::add(dir, entry.clone()))
    }
}

pub(super) fn wire(ui: &MainWindow, dir: &Path) {
    let editor = Rc::new(RefCell::new(Editor::default()));
    update_undo(ui, dir);
    let choices = Rc::new(VecModel::<LexChoice>::default());
    ui.set_lex_choices(ModelRc::from(choices.clone()));

    let weak = ui.as_weak();
    let state = editor.clone();
    let folder = dir.to_path_buf();
    let list = choices.clone();
    ui.on_lex_filter(move || {
        let Some(ui) = weak.upgrade() else { return };
        let mut state = state.borrow_mut();
        match lexicon_entries(&folder, kind(&ui)) {
            Ok(mut entries) => {
                state.all = entries.clone();
                let live: HashSet<_> = entries.iter().collect();
                state.selected.retain(|entry| live.contains(entry));
                let mut themes: Vec<_> = entries
                    .iter()
                    .filter(|row| !row.theme.is_empty())
                    .map(|row| row.theme.clone())
                    .collect();
                themes.sort_unstable();
                themes.dedup();
                let old = ui.get_lex_theme_filter();
                let names = std::iter::once(SharedString::from("所有主题"))
                    .chain(themes.into_iter().map(Into::into))
                    .collect::<Vec<_>>();
                let theme_index = names
                    .iter()
                    .skip(1)
                    .position(|name| name == &old)
                    .map_or(0, |index| index + 1);
                let theme = if theme_index > 0 {
                    names[theme_index].as_str()
                } else {
                    ""
                };
                let search = ui.get_lex_search().trim().to_lowercase();
                entries.retain(|row| {
                    (theme.is_empty() || row.theme == theme)
                        && (search.is_empty()
                            || [&row.head, &row.reading, &row.russian]
                                .iter()
                                .any(|field| field.to_lowercase().contains(&search))
                            || row.trigger().contains(&search))
                });
                ui.set_lex_theme_filter(theme.into());
                ui.set_lex_themes(ModelRc::new(VecModel::from(names)));
                ui.set_lex_theme_index(theme_index as i32);
                state.visible = entries;
            }
            Err(error) => {
                state.visible.clear();
                state.all.clear();
                state.selected.clear();
                ui.invoke_show_lex_status(format!("无法读取词库：{error}").into(), false);
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
        list.set_vec(
            state
                .visible
                .iter()
                .zip(labels.iter().skip(1))
                .map(|(row, label)| LexChoice {
                    label: label.clone(),
                    checked: state.selected.contains(row),
                })
                .collect::<Vec<_>>(),
        );
        ui.set_lex_selected_count(state.selected.len() as i32);
        ui.set_lex_labels(ModelRc::new(VecModel::from(labels)));
        ui.set_lex_index(0);
        if ui.get_learned_open() {
            show_learned(&ui, &folder, &state.learned);
        }
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
        let mut state = state.borrow_mut();
        state.original = Some(entry);
        state.learned_draft = false;
    });

    let weak = ui.as_weak();
    let state = editor.clone();
    ui.on_lex_clear(move || {
        if let Some(ui) = weak.upgrade() {
            let mut state = state.borrow_mut();
            state.original = None;
            state.learned_draft = false;
            set_fields(&ui, &LexiconEntry::default());
            ui.set_lex_editing(false);
            ui.set_lex_crowded(false);
            ui.set_lex_index(0);
        }
    });

    let weak = ui.as_weak();
    let state = editor.clone();
    ui.on_lex_kind_changed(move || {
        if let Some(ui) = weak.upgrade() {
            state.borrow_mut().selected.clear();
            ui.set_lex_theme_filter("".into());
            ui.invoke_lex_clear();
            ui.invoke_lex_filter();
        }
    });

    let weak = ui.as_weak();
    let state = editor.clone();
    ui.on_lex_reading_changed(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_lex_crowded(
                kind(&ui) == LexiconKind::Words
                    && !ui.get_lex_editing()
                    && crowded_reading(ui.get_lex_reading().as_str(), &state.borrow().all),
            );
        }
    });

    let weak = ui.as_weak();
    let state = editor.clone();
    let list = choices;
    ui.on_lex_mark(move |index, checked| {
        let Some(ui) = weak.upgrade() else { return };
        let mut state = state.borrow_mut();
        let Some(entry) = state.visible.get(index as usize).cloned() else {
            return;
        };
        if checked && !state.selected.contains(&entry) {
            state.selected.insert(entry);
        } else if !checked {
            state.selected.remove(&entry);
        }
        let Some(mut row) = list.row_data(index as usize) else {
            return;
        };
        row.checked = checked;
        list.set_row_data(index as usize, row);
        ui.set_lex_selected_count(state.selected.len() as i32);
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
            theme: ui.get_lex_theme().into(),
        };
        let learned_result = state.borrow().save_learned_draft(&folder, &replacement);
        if let Some(result) = learned_result {
            report_learned_add(&ui, &folder, result, true);
            return;
        }
        let original = state.borrow().original.clone();
        let previous_undo = ui.get_lex_undo_id();
        match edit_lexicon(&folder, kind(&ui), original.as_ref(), Some(replacement)) {
            Ok(()) => {
                ui.invoke_lex_clear();
                refresh(&ui, &folder);
                if ui.get_lex_can_undo() && ui.get_lex_undo_id() != previous_undo {
                    ui.invoke_show_lex_status(
                        "权重已调整。重新输入拼音即可看到变化。".into(),
                        true,
                    );
                } else {
                    ui.invoke_show_lex_status(
                        "词条已保存，已生效，无需重启。请重新输入拼音查看候选。".into(),
                        false,
                    );
                }
            }
            Err(error) => ui.invoke_show_lex_status(format!("词条未保存：{error}").into(), false),
        }
    });

    let weak = ui.as_weak();
    let state = editor.clone();
    let folder = dir.to_path_buf();
    ui.on_lex_move_weight(move |up| {
        let Some(ui) = weak.upgrade() else { return };
        let Some(original) = state.borrow().original.clone() else {
            return;
        };
        if kind(&ui) != LexiconKind::Words
            || original.head != ui.get_lex_head().as_str()
            || original.reading != ui.get_lex_reading().as_str()
            || original.russian != ui.get_lex_russian().as_str()
            || original.extra != ui.get_lex_extra().as_str()
            || original.theme != ui.get_lex_theme().as_str()
        {
            ui.invoke_show_lex_status("请先保存词条修改，再调整顺序。".into(), false);
            return;
        }
        match move_weight(&folder, &original, up) {
            Ok(Some(entry)) => {
                set_fields(&ui, &entry);
                state.borrow_mut().original = Some(entry);
                refresh(&ui, &folder);
                ui.invoke_show_lex_status("权重已调整。重新输入拼音即可看到变化。".into(), true);
            }
            Ok(None) => ui.invoke_show_lex_status("已经到达这个读音的自定义词条边界。".into(), false),
            Err(error) => ui.invoke_show_lex_status(format!("无法调整顺序：{error}").into(), false),
        }
    });

    for bulk in [false, true] {
        let weak = ui.as_weak();
        let state = editor.clone();
        let folder = dir.to_path_buf();
        let delete = move || {
            let Some(ui) = weak.upgrade() else { return };
            let selected = {
                let state = state.borrow();
                if bulk {
                    state.selected.iter().cloned().collect::<Vec<_>>()
                } else {
                    state.original.iter().cloned().collect()
                }
            };
            match delete_entries(&folder, kind(&ui), &selected) {
                Ok(()) => {
                    ui.invoke_lex_clear();
                    refresh(&ui, &folder);
                    ui.invoke_show_lex_status(
                        format!("已删除 {} 条词条。", selected.len()).into(),
                        !selected.is_empty(),
                    );
                }
                Err(error) => ui.invoke_show_lex_status(format!("词条未删除：{error}").into(), false),
            }
        };
        if bulk {
            ui.on_lex_delete_selected(delete);
        } else {
            ui.on_lex_delete(delete);
        }
    }

    let weak = ui.as_weak();
    let folder = dir.to_path_buf();
    ui.on_lex_undo(move || {
        let Some(ui) = weak.upgrade() else { return };
        match undo_last(&folder, Some(ui.get_lex_undo_id().as_str())) {
            Ok(Some(description)) => {
                ui.invoke_lex_clear();
                ui.invoke_show_lex_status(format!("已撤销「{description}」。无需重启。").into(), false);
                refresh(&ui, &folder);
            }
            Ok(None) => {
                ui.invoke_show_lex_status("没有可撤销的操作。".into(), false);
                update_undo(&ui, &folder);
            }
            Err(error) => {
                ui.invoke_show_lex_status(format!("无法撤销：{error}").into(), false);
                update_undo(&ui, &folder);
            }
        }
    });

    let weak = ui.as_weak();
    let state = editor.clone();
    ui.on_lex_import(move || {
        let Some(ui) = weak.upgrade() else { return };
        match choose_tsv() {
            Ok(Some(path)) => match read_import(&path) {
                Ok(bytes) => {
                    let theme = path
                        .file_stem()
                        .and_then(|name| name.to_str())
                        .filter(|name| {
                            !name.is_empty()
                                && name.chars().count() <= 80
                                && !name.chars().any(char::is_control)
                        })
                        .unwrap_or("文件导入");
                    start_import(&ui, &state, kind(&ui), bytes, theme);
                }
                Err(error) => ui.invoke_show_lex_status(format!("无法读取导入文件：{error}").into(), false),
            },
            Ok(None) => {}
            Err(error) => ui.invoke_show_lex_status(format!("无法打开导入文件：{error}").into(), false),
        }
    });

    let weak = ui.as_weak();
    let state = editor.clone();
    ui.on_lex_add_pack(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        if let Some(text) = THEMES.get(index as usize) {
            start_import(
                &ui,
                &state,
                LexiconKind::Words,
                text.as_bytes().to_vec(),
                ["出行俄语", "点餐俄语", "办公俄语"][index as usize],
            );
        }
    });

    let weak = ui.as_weak();
    let state = editor.clone();
    let folder = dir.to_path_buf();
    ui.on_lex_preview(move || {
        let Some(ui) = weak.upgrade() else { return };
        let mut state = state.borrow_mut();
        let Some((kind, bytes, theme)) = state.import.as_ref() else {
            return;
        };
        let policy = match ui.get_lex_duplicate_policy() {
            1 => DuplicatePolicy::Replace,
            2 => DuplicatePolicy::Merge,
            _ => DuplicatePolicy::Skip,
        };
        state.preview = match preview_import(&folder, *kind, bytes, policy, theme) {
            Ok(plan) => {
                ui.set_lex_preview_summary(
                    format!(
                        "将新增 {} 条、跳过 {} 条、覆盖 {} 条、合并 {} 条。{} 行使用默认权重。",
                        plan.added,
                        plan.skipped,
                        plan.replaced,
                        plan.merged,
                        plan.defaulted_weights
                    )
                    .into(),
                );
                let metadata = &plan.metadata;
                let details = [
                    ("名称", &metadata.name),
                    ("作者", &metadata.author),
                    ("版本", &metadata.version),
                    ("语言", &metadata.language),
                    ("说明", &metadata.description),
                ]
                .into_iter()
                .filter(|(_, value)| !value.is_empty())
                .map(|(label, value)| format!("{label}  {value}"))
                .collect::<Vec<_>>();
                ui.set_lex_metadata(details.join("\n").into());
                ui.set_lex_issues(plan.issues.join("\n").into());
                ui.set_lex_can_import(plan.has_changes());
                Some(plan)
            }
            Err(error) => {
                ui.set_lex_preview_summary(format!("无法预览：{error}").into());
                ui.set_lex_metadata("".into());
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
            commit_import(&folder, plan).map(|()| plan.added + plan.replaced + plan.merged)
        };
        match result {
            Ok(changed) => {
                ui.invoke_lex_cancel_import();
                refresh(&ui, &folder);
                ui.invoke_show_lex_status(
                    format!("已导入 {changed} 条词条，已生效，无需重启。").into(),
                    changed > 0,
                );
            }
            Err(error) => {
                ui.invoke_show_lex_status(format!("导入失败：{error}").into(), false);
                ui.invoke_lex_preview();
            }
        }
    });

    let weak = ui.as_weak();
    let folder = dir.to_path_buf();
    ui.on_lex_export(move || {
        let Some(ui) = weak.upgrade() else { return };
        match export_backup(&folder) {
            Ok(path) => {
                let _ = crate::open_target(&pangyi_settings::OpenTarget::Directory(path.clone()));
                ui.invoke_show_lex_status(format!("备份已导出到 {}", path.display()).into(), false);
            }
            Err(error) => ui.invoke_show_lex_status(format!("导出备份失败：{error}").into(), false),
        }
    });

    let weak = ui.as_weak();
    ui.on_lex_catalog(move || {
        let Some(ui) = weak.upgrade() else { return };
        if let Err(error) = open_catalog() {
            ui.invoke_show_lex_status(format!("无法打开词典下载目录：{error}").into(), false);
        }
    });

    wire_learned(ui, &editor, dir);
}

fn wire_learned(ui: &MainWindow, editor: &Rc<RefCell<Editor>>, dir: &Path) {
    let weak = ui.as_weak();
    let state = editor.clone();
    let folder = dir.to_path_buf();
    ui.on_learned_visibility_changed(move |open| {
        let Some(ui) = weak.upgrade() else { return };
        let mut state = state.borrow_mut();
        if open {
            state.learned = learned::load(&folder);
            show_learned(&ui, &folder, &state.learned);
        } else {
            state.learned.clear();
            ui.set_learned_choices(ModelRc::default());
        }
    });

    let weak = ui.as_weak();
    let state = editor.clone();
    let folder = dir.to_path_buf();
    ui.on_learned_add(move |head| {
        let Some(ui) = weak.upgrade() else { return false };
        match learned::prepare(head.as_str()) {
            Preparation::Ready(entry) => {
                report_learned_add(&ui, &folder, learned::add(&folder, entry), false);
                false
            }
            Preparation::NeedsReading(entry) => {
                ui.set_lex_kind(0);
                ui.invoke_lex_kind_changed();
                set_fields(&ui, &entry);
                state.borrow_mut().learned_draft = true;
                ui.invoke_show_lex_status("请填写触发拼音，再添加词条。".into(), false);
                true
            }
        }
    });

    let weak = ui.as_weak();
    let state = editor.clone();
    let folder = dir.to_path_buf();
    ui.on_learned_ignore(move |head| {
        let Some(ui) = weak.upgrade() else { return };
        match learned::ignore(&folder, head.as_str()) {
            Ok(()) => show_learned(&ui, &folder, &state.borrow().learned),
            Err(error) => ui.invoke_show_lex_status(format!("无法忽略词条：{error}").into(), false),
        }
    });

    let weak = ui.as_weak();
    let state = editor.clone();
    let folder = dir.to_path_buf();
    ui.on_learned_restore(move || {
        let Some(ui) = weak.upgrade() else { return };
        match learned::restore_ignored(&folder) {
            Ok(()) => {
                show_learned(&ui, &folder, &state.borrow().learned);
                ui.invoke_show_lex_status("已恢复忽略的词。".into(), false);
            }
            Err(error) => ui.invoke_show_lex_status(format!("无法恢复忽略的词：{error}").into(), false),
        }
    });
}

fn show_learned(ui: &MainWindow, dir: &Path, words: &[LearnedWord]) {
    let choices: Vec<_> = learned::suggestions(dir, words).into_iter()
        .map(|word| LearnedChoice {
            head: word.head.as_str().into(),
            usage: format!("使用 {} 次", word.count).into(),
        })
        .collect();
    ui.set_learned_choices(ModelRc::new(VecModel::from(choices)));
    ui.set_learned_has_ignored(learned::has_ignored(dir));
}

fn report_learned_add(
    ui: &MainWindow,
    dir: &Path,
    result: std::io::Result<bool>,
    clear_editor: bool,
) {
    match result {
        Ok(added) => {
            if added && clear_editor {
                ui.invoke_lex_clear();
            }
            refresh(ui, dir);
            let message = if added { "已加入我的词库。" } else { "已存在。" };
            ui.invoke_show_lex_status(message.into(), added);
        }
        Err(error) => ui.invoke_show_lex_status(format!("词条未保存：{error}").into(), false),
    }
}

#[cfg(test)]
mod learned_editor_tests {
    use super::*;

    #[test]
    fn a_learned_draft_requires_a_confirmed_reading_and_uses_reversible_add() {
        let dir = std::env::temp_dir().join(format!(
            "pangyi-learned-editor-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let Preparation::NeedsReading(mut draft) = learned::prepare("行") else {
            panic!("a polyphonic word must open the editor");
        };
        assert_eq!(draft.head, "行");
        assert!(draft.reading.is_empty());
        assert!(Editor::default().save_learned_draft(&dir, &draft).is_none());

        let editor = Editor { learned_draft: true, ..Default::default() };
        assert!(editor.save_learned_draft(&dir, &draft).unwrap().is_err());
        assert!(!dir.join("user-dict.tsv").exists());
        draft.reading = "hang".into();
        draft.extra = "7".into();
        draft.theme = "我的主题".into();
        assert!(editor.save_learned_draft(&dir, &draft).unwrap().unwrap());
        assert!(!editor.save_learned_draft(&dir, &draft).unwrap().unwrap());
        assert!(lexicon_entries(&dir, LexiconKind::Words).unwrap() == [draft]);
        assert_eq!(undo_last(&dir, None).unwrap().as_deref(), Some("添加 1 条"));
        assert!(lexicon_entries(&dir, LexiconKind::Words).unwrap().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
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
    ui.set_lex_theme(entry.theme.as_str().into());
    ui.set_lex_crowded(false);
}

pub(super) fn update_undo(ui: &MainWindow, folder: &Path) {
    let action = undo_action(folder);
    ui.set_lex_can_undo(action.is_some());
    ui.set_lex_undo_id(
        action
            .as_ref()
            .map_or_else(|| "".into(), |action| action.id.as_str().into()),
    );
    ui.set_lex_undo_label(action.map_or_else(
        || "撤销".into(),
        |action| format!("撤销：{}", action.description).into(),
    ));
}

fn refresh(ui: &MainWindow, folder: &Path) {
    update_undo(ui, folder);
    ui.invoke_lex_filter();
    crate::push_stats(ui, &load_counts(folder));
}

pub(super) fn refresh_external(ui: &MainWindow, folder: &Path) {
    let previous = ui.get_lex_undo_id();
    update_undo(ui, folder);
    if ui.get_lex_undo_id() != previous {
        ui.invoke_lex_filter();
        crate::push_stats(ui, &load_counts(folder));
    }
}

fn start_import(
    ui: &MainWindow,
    state: &RefCell<Editor>,
    kind: LexiconKind,
    bytes: Vec<u8>,
    theme: &str,
) {
    state.borrow_mut().import = Some((kind, bytes, theme.into()));
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
