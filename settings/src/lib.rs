//! 旁译设置程序的纯逻辑。
//!
//! 界面在 `main.rs`。这里不链接输入法，不读词库表，也不联网。
//! 设置真源仍是输入法里的 `ime/src/settings.rs`；本包抄了同一套读入规则，
//! 写回时比旧的整文件 `save()` 更窄：改前重读，只改一个键。

mod colors;
mod lexicon;
mod local_files;
mod settings_file;
mod updater_cfg;

pub use colors::{
    accent_name, accent_strong, base_palette, effective_dark, mascot_palette, preview_palette,
    scaled_px, scheme_kind, Accent, BasePalette, MascotPalette, PreviewPalette, Rgb, SchemeKind,
    ACCENTS, ANNO_PX, INDEX_PX, TEXT_PX,
};
pub use lexicon::{
    commit_import, crowded_reading, delete_entries, edit_lexicon, export_backup, lexicon_entries,
    move_weight, preview_import, read_import, undo_change, DuplicatePolicy, ImportPreview,
    LexiconEntry, LexiconKind, UndoChange,
};
pub use local_files::{
    count_data_lines, count_pack_files, ensure_phrases_file, existing_file_or_dir, format_count,
    load_counts, packs_dir, phrase_path, LocalCounts, OpenTarget, PHRASE_TEMPLATE,
};
pub use settings_file::{
    apply_one_key, commit_key, data_dir, default_body, layout_segment, load_dir, load_from_parts,
    max_segment, max_value_for_segment, pangyi_dir, parse_settings, read_text_file, scale_segment,
    scale_value_for_segment, settings_path, theme_segment, trmode_segment, write_atomic, Loaded,
    Settings, TextFile, DEFAULT_BODY,
};
pub use updater_cfg::{
    read_updater_enabled, updater_enabled_text, ACCOUNT_ARG, ACCOUNT_EXE, MSG_NO_ACCOUNT,
    MSG_NO_UPDATER, OPEN_VERB, UPDATER_DISABLE, UPDATER_ENABLE, UPDATER_EXE, UPDATER_MANUAL,
};

#[cfg(test)]
mod guard {
    use std::fs;
    use std::path::Path;

    fn read_tree(dir: &Path, out: &mut String) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name == "target") {
                    continue;
                }
                read_tree(&path, out);
            } else if path.is_file() {
                if let Ok(text) = fs::read_to_string(&path) {
                    out.push_str(&text);
                    out.push('\n');
                }
            }
        }
    }

    #[test]
    fn package_does_not_add_network_or_the_ime_dictionary() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut blob = String::new();
        read_tree(&root.join("src"), &mut blob);
        read_tree(&root.join("ui"), &mut blob);
        blob.push_str(&fs::read_to_string(root.join("Cargo.toml")).unwrap());
        // 针分成两段，避免本测试自己的源码被扫进去。
        let needles = [
            ["ure", "q"],
            ["req", "west"],
            ["hy", "per"],
            ["Tcp", "Stream"],
            ["Win", "Http"],
            ["win", "http"],
            ["account", ".dat"],
            ["dict", ".tsv"],
            ["lm", ".tsv"],
            ["457", "138"],
            ["renderer-", "software"],
            ["renderer-", "skia"],
            ["backend-", "qt"],
            ["pangyi_", "ime"],
            ["pangyi_", "account"],
        ];
        for parts in needles {
            let needle = format!("{}{}", parts[0], parts[1]);
            assert!(
                !contains_as_its_own_token(&blob, &needle),
                "设置程序里不该出现 {needle}"
            );
        }
    }

    /// `user-dict.tsv` 是本机用户词库，不能被「内置词库文件名」这条规则误伤。
    fn contains_as_its_own_token(blob: &str, needle: &str) -> bool {
        let mut rest = blob;
        while let Some(index) = rest.find(needle) {
            let own = rest[..index]
                .chars()
                .next_back()
                .is_none_or(|ch| !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'));
            if own {
                return true;
            }
            rest = &rest[index + needle.len()..];
        }
        false
    }
}
