//! 旁译设置程序。只读写本机配置，并启动已经装好的主程序或更新程序。
//! 打不开 OpenGL 时直接说明，不用软件渲染器画缺字的中文界面。

#![cfg_attr(all(not(test), target_os = "windows"), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::io::Read;

mod lexicon_ui;

use pangyi_settings::{
    accent_strong, data_dir, effective_dark, ensure_phrases_file, existing_file_or_dir,
    format_count, load_counts, load_dir, packs_dir, phrase_path, preview_palette, scaled_px,
    scheme_kind, LocalCounts, OpenTarget, PreviewPalette, Rgb, SchemeKind, Settings, ACCENTS,
    ACCOUNT_ARG, ACCOUNT_EXE, ANNO_PX, INDEX_PX, MSG_NO_ACCOUNT, MSG_NO_UPDATER, TEXT_PX,
    UPDATER_DISABLE, UPDATER_ENABLE, UPDATER_EXE, UPDATER_MANUAL,
};
use slint::ComponentHandle;
use slint::winit_030::{winit::event::WindowEvent, EventResult, WinitWindowAccessor};
use pangyi_settings::quick_add;

slint::include_modules!();

fn main() {
    if std::env::args().nth(1).as_deref() == Some(quick_add::COMMAND) {
        let result = serde_json::from_reader(std::io::stdin().lock().take(quick_add::MAX_REQUEST_BYTES + 1))
            .map_err(std::io::Error::other)
            .and_then(|candidate| pangyi_settings::quick_add_word(&data_dir(), candidate));
        std::process::exit(match result {
            Ok(true) => 0,
            Ok(false) => quick_add::EXISTS,
            Err(_) => quick_add::FAILED,
        });
    }
    if let Err(err) = run() {
        let message = format!(
            "旁译设置打不开。这台电脑没有可用的 OpenGL，界面不能退回到缺字的软件渲染。\n\n{err}"
        );
        eprintln!("{message}");
        report_fatal(&message);
        std::process::exit(1);
    }
}

fn run() -> Result<(), slint::PlatformError> {
    let ui = MainWindow::new()?;
    let dir = data_dir();
    let focus_dir = dir.clone();
    let weak = ui.as_weak();
    ui.window().on_winit_window_event(move |_, event| {
        if let Some(ui) = weak.upgrade() {
            match event {
                WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => {
                    ui.set_lex_keyboard_focus(true);
                }
                WindowEvent::MouseInput { state, .. } if state.is_pressed() => {
                    ui.set_lex_keyboard_focus(false);
                }
                WindowEvent::Focused(true) if ui.get_page() == 3 => {
                    lexicon_ui::refresh_external(&ui, &focus_dir);
                }
                _ => {}
            }
        }
        EventResult::Propagate
    });
    ui.set_version(env!("CARGO_PKG_VERSION").into());
    wire(&ui, dir.clone());
    if let Err(err) = refresh(&ui, &dir) {
        ui.set_status(format!("没能读取设置：{err}").into());
    }
    push_stats(&ui, &load_counts(&dir));
    ui.run()
}

fn wire(ui: &MainWindow, dir: PathBuf) {
    let ui_weak = ui.as_weak();
    lexicon_ui::wire(ui, &dir);

    let path = dir.clone();
    let weak = ui_weak.clone();
    ui.on_quick_add_toggled(move |enabled| {
        act(&weak, &path, save_key(&path, "quick_add", bit(enabled), |s| s.quick_add == enabled));
    });
    let path = dir.clone();
    let weak = ui_weak.clone();
    ui.on_quick_add_key_selected(move |index| {
        let key = index + 1;
        act(&weak, &path, save_key(&path, "quick_add_key", &key.to_string(), |s| i32::from(s.quick_add_key) == key));
    });
    let dir_max = dir.clone();
    let weak = ui_weak.clone();
    ui.on_max_selected(move |index| {
        let Some(value) = pangyi_settings::max_value_for_segment(index) else {
            return;
        };
        let weak = weak.clone();
        let dir = dir_max.clone();
        act(
            &weak,
            &dir,
            save_key(&dir, "max", &value.to_string(), |s| s.max == value),
        );
    });

    let dir_scale = dir.clone();
    let weak = ui_weak.clone();
    ui.on_scale_selected(move |index| {
        let Some(value) = pangyi_settings::scale_value_for_segment(index) else {
            return;
        };
        act(
            &weak,
            &dir_scale,
            save_key(&dir_scale, "scale", &value.to_string(), |s| {
                s.scale == value
            }),
        );
    });

    let dir_cn = dir.clone();
    let weak = ui_weak.clone();
    ui.on_cnpunct_toggled(move |on| {
        act(
            &weak,
            &dir_cn,
            save_key(&dir_cn, "cnpunct", bit(on), |s| s.cnpunct == on),
        );
    });

    let dir_en = dir.clone();
    let weak = ui_weak.clone();
    ui.on_english_toggled(move |on| {
        act(
            &weak,
            &dir_en,
            save_key(&dir_en, "english", bit(on), |s| s.english == on),
        );
    });

    let dir_emoji = dir.clone();
    let weak = ui_weak.clone();
    ui.on_emoji_toggled(move |on| {
        act(
            &weak,
            &dir_emoji,
            save_key(&dir_emoji, "emoji", bit(on), |s| s.emoji == on),
        );
    });

    let dir_layout = dir.clone();
    let weak = ui_weak.clone();
    ui.on_layout_selected(move |index| {
        if index != 0 && index != 1 {
            return;
        }
        let horizontal = index == 1;
        let value = if horizontal { "h" } else { "v" };
        act(
            &weak,
            &dir_layout,
            save_key(&dir_layout, "layout", value, |s| s.horizontal == horizontal),
        );
    });

    let dir_theme = dir.clone();
    let weak = ui_weak.clone();
    ui.on_theme_selected(move |index| {
        if !(0..=2).contains(&index) {
            return;
        }
        act(
            &weak,
            &dir_theme,
            save_key(&dir_theme, "theme", &index.to_string(), |s| {
                s.theme == index
            }),
        );
    });

    let dir_accent = dir.clone();
    let weak = ui_weak.clone();
    ui.on_accent_selected(move |index| {
        if !(0..=4).contains(&index) {
            return;
        }
        act(
            &weak,
            &dir_accent,
            save_key(&dir_accent, "accent", &index.to_string(), |s| {
                s.accent == index
            }),
        );
    });

    let dir_tr = dir.clone();
    let weak = ui_weak.clone();
    ui.on_trmode_selected(move |index| {
        if !(0..=2).contains(&index) {
            return;
        }
        act(
            &weak,
            &dir_tr,
            save_key(&dir_tr, "trmode", &index.to_string(), |s| s.trmode == index),
        );
    });

    let dir_phrase = dir.clone();
    let weak = ui_weak.clone();
    ui.on_open_phrases(move || {
        let result = (|| {
            let path = phrase_path(&dir_phrase);
            ensure_phrases_file(&path).map_err(|err| format!("没能准备短语文件：{err}"))?;
            open_target(&OpenTarget::File(path))
        })();
        if let Some(ui) = weak.upgrade() {
            set_action_status(&ui, result);
            push_stats(&ui, &load_counts(&dir_phrase));
        }
    });

    let dir_dict = dir.clone();
    let weak = ui_weak.clone();
    ui.on_open_user_dict(move || {
        let result = open_user_file(&dir_dict, "user-dict.tsv");
        if let Some(ui) = weak.upgrade() {
            set_action_status(&ui, result);
        }
    });

    let dir_gloss = dir.clone();
    let weak = ui_weak.clone();
    ui.on_open_user_gloss(move || {
        let result = open_user_file(&dir_gloss, "user-gloss.tsv");
        if let Some(ui) = weak.upgrade() {
            set_action_status(&ui, result);
        }
    });

    let dir_packs = dir.clone();
    let weak = ui_weak.clone();
    ui.on_open_packs(move || {
        let result = (|| {
            let folder = packs_dir(&dir_packs);
            std::fs::create_dir_all(&folder)
                .map_err(|err| format!("没能打开表情包文件夹：{err}"))?;
            open_target(&OpenTarget::Directory(folder))
        })();
        if let Some(ui) = weak.upgrade() {
            set_action_status(&ui, result);
            push_stats(&ui, &load_counts(&dir_packs));
        }
    });

    let dir_stats = dir.clone();
    let weak = ui_weak.clone();
    ui.on_lexicon_shown(move || {
        if let Some(ui) = weak.upgrade() {
            lexicon_ui::update_undo(&ui, &dir_stats);
            push_stats(&ui, &load_counts(&dir_stats));
            ui.invoke_lex_filter();
        }
    });

    let weak = ui_weak.clone();
    ui.on_open_account(move || {
        let result = spawn_program(ACCOUNT_EXE, &[ACCOUNT_ARG], false)
            .map_err(|_| MSG_NO_ACCOUNT.to_string());
        if let Some(ui) = weak.upgrade() {
            set_action_status(&ui, result);
        }
    });

    let weak = ui_weak.clone();
    ui.on_check_update(move || {
        let result = spawn_program(UPDATER_EXE, &[UPDATER_MANUAL], false)
            .map_err(|_| MSG_NO_UPDATER.to_string());
        if let Some(ui) = weak.upgrade() {
            set_action_status(&ui, result);
        }
    });

    let dir_updater = dir.clone();
    let weak = ui_weak.clone();
    ui.on_updater_toggled(move |on| {
        let arg = if on { UPDATER_ENABLE } else { UPDATER_DISABLE };
        let result =
            spawn_program(UPDATER_EXE, &[arg], true).map_err(|_| MSG_NO_UPDATER.to_string());
        if let Some(ui) = weak.upgrade() {
            ui.set_updater_on(pangyi_settings::read_updater_enabled(&dir_updater));
            set_action_status(&ui, result);
        }
    });

    let weak = ui_weak.clone();
    ui.on_request_close(move || {
        if let Some(ui) = weak.upgrade() {
            let _ = ui.hide();
        }
    });
}

fn act(weak: &slint::Weak<MainWindow>, dir: &Path, result: Result<(), String>) {
    let Some(ui) = weak.upgrade() else {
        return;
    };
    if let Err(err) = refresh(&ui, dir) {
        ui.set_status(format!("没能读取设置：{err}").into());
        return;
    }
    set_action_status(&ui, result);
}

fn set_action_status(ui: &MainWindow, result: Result<(), String>) {
    ui.set_status(match result {
        Ok(()) => "".into(),
        Err(err) => err.into(),
    });
}

fn save_key(
    dir: &Path,
    key: &str,
    value: &str,
    unchanged: impl Fn(&Settings) -> bool,
) -> Result<(), String> {
    let loaded = load_dir(dir).map_err(|err| err.to_string())?;
    if !loaded.can_write {
        return Err("settings.txt 不是合法的 UTF-8，没有写回".into());
    }
    if unchanged(&loaded.settings) {
        return Ok(());
    }
    pangyi_settings::commit_key(dir, key, value).map_err(|err| err.to_string())
}

fn bit(on: bool) -> &'static str {
    if on {
        "1"
    } else {
        "0"
    }
}

fn refresh(ui: &MainWindow, dir: &Path) -> Result<(), String> {
    let loaded = load_dir(dir).map_err(|err| err.to_string())?;
    push_settings(ui, &loaded.settings, system_is_dark());
    ui.set_updater_on(pangyi_settings::read_updater_enabled(dir));
    Ok(())
}

fn push_settings(ui: &MainWindow, settings: &Settings, system_dark: bool) {
    ui.set_max_index(pangyi_settings::max_segment(settings.max));
    ui.set_scale_index(pangyi_settings::scale_segment(settings.scale));
    ui.set_cnpunct(settings.cnpunct);
    ui.set_english(settings.english);
    ui.set_emoji(settings.emoji);
    ui.set_quick_add(settings.quick_add);
    ui.set_quick_add_key_index(i32::from(settings.quick_add_key) - 1);
    ui.set_layout_index(pangyi_settings::layout_segment(settings.horizontal));
    ui.set_theme_index(pangyi_settings::theme_segment(settings.theme));
    ui.set_accent_index(settings.accent);
    ui.set_trmode(pangyi_settings::trmode_segment(settings.trmode));
    let dark = effective_dark(settings.theme, system_dark);
    apply_scheme(ui, scheme_kind(settings.theme));
    let palette = preview_palette(settings.accent, dark);
    push_preview(ui, &palette, settings.scale, settings.horizontal);
    ui.set_mascot_dark(dark);
    ui.set_dot0(color(accent_strong(0, dark)));
    ui.set_dot1(color(accent_strong(1, dark)));
    ui.set_dot2(color(accent_strong(2, dark)));
    ui.set_dot3(color(accent_strong(3, dark)));
    ui.set_dot4(color(accent_strong(4, dark)));
    let accent_pick = usize::try_from(settings.accent.clamp(0, 4)).unwrap_or(0);
    ui.set_on_accent(color(ACCENTS[accent_pick][usize::from(dark)].on_strong));
}

fn push_preview(ui: &MainWindow, palette: &PreviewPalette, scale: i32, horizontal: bool) {
    ui.set_pv_card(color(palette.card));
    ui.set_pv_border(color(palette.border));
    ui.set_pv_text(color(palette.text));
    ui.set_pv_faint(color(palette.faint));
    ui.set_pv_fill(color(palette.fill));
    ui.set_pv_bar_text(color(palette.bar_text));
    ui.set_pv_text_px(px(scaled_px(TEXT_PX, scale)));
    ui.set_pv_anno_px(px(scaled_px(ANNO_PX, scale)));
    ui.set_pv_index_px(px(scaled_px(INDEX_PX, scale)));
    ui.set_pv_horizontal(horizontal);
}

fn push_stats(ui: &MainWindow, counts: &LocalCounts) {
    ui.set_stat_phrases(format_count(counts.phrases, "行").into());
    ui.set_stat_dict(format_count(counts.user_dict, "行").into());
    ui.set_stat_gloss(format_count(counts.user_gloss, "行").into());
    ui.set_stat_packs(format_count(counts.packs, "个").into());
    ui.set_stat_learned(format_count(counts.learned, "行").into());
}

fn apply_scheme(ui: &MainWindow, kind: SchemeKind) {
    // 生成代码的 setter 用的是这套枚举。数值和设置文件里的 theme 不是同一套。
    use slint::private_unstable_api::re_exports::ColorScheme;
    let scheme = match kind {
        SchemeKind::Light => ColorScheme::Light,
        SchemeKind::Dark => ColorScheme::Dark,
        SchemeKind::Follow => ColorScheme::Unknown,
    };
    ui.global::<Palette>().set_color_scheme(scheme);
}

fn color(rgb: Rgb) -> slint::Color {
    slint::Color::from_rgb_u8(rgb.r, rgb.g, rgb.b)
}

fn px(value: i32) -> f32 {
    value as f32
}

fn open_user_file(dir: &Path, name: &str) -> Result<(), String> {
    let file = dir.join(name);
    let target = existing_file_or_dir(&file, dir);
    if let OpenTarget::Directory(folder) = &target {
        std::fs::create_dir_all(folder).map_err(|err| format!("没能打开文件夹：{err}"))?;
    }
    open_target(&target)
}

fn spawn_program(program: &str, args: &[&str], wait: bool) -> Result<(), String> {
    if !Path::new(program).is_file() {
        return Err("missing".into());
    }
    let mut child = Command::new(program)
        .args(args)
        .spawn()
        .map_err(|err| err.to_string())?;
    if wait {
        let status = child.wait().map_err(|err| err.to_string())?;
        if !status.success() {
            return Err(format!("exit {status}"));
        }
        Ok(())
    } else {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
}

#[cfg(windows)]
fn open_target(target: &OpenTarget) -> Result<(), String> {
    let path = match target {
        OpenTarget::File(path) | OpenTarget::Directory(path) => path,
    };
    shell_open(path)
}

#[cfg(not(windows))]
fn open_target(_target: &OpenTarget) -> Result<(), String> {
    Err("只有 Windows 能打开本机文件".into())
}

#[cfg(windows)]
fn shell_open(path: &Path) -> Result<(), String> {
    use pangyi_settings::OPEN_VERB;
    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteW(
            hwnd: *mut core::ffi::c_void,
            verb: *const u16,
            file: *const u16,
            params: *const u16,
            dir: *const u16,
            show: i32,
        ) -> isize;
    }
    let verb = wide(OPEN_VERB);
    let file = wide_path(path);
    let rc = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    };
    if (rc as usize) <= 32 {
        Err(format!("打不开（错误码 {rc}）"))
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn system_is_dark() -> bool {
    #[link(name = "advapi32")]
    extern "system" {
        fn RegGetValueW(
            hkey: isize,
            sub: *const u16,
            name: *const u16,
            flags: u32,
            ty: *mut u32,
            data: *mut core::ffi::c_void,
            len: *mut u32,
        ) -> i32;
    }
    const HKEY_CURRENT_USER: isize = 0x8000_0001u32 as i32 as isize;
    const RRF_RT_REG_DWORD: u32 = 0x0000_0010;
    let sub = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let name = wide("AppsUseLightTheme");
    let mut value: u32 = 1;
    let mut len: u32 = 4;
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            sub.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            &mut value as *mut u32 as *mut core::ffi::c_void,
            &mut len,
        )
    };
    rc == 0 && value == 0
}

#[cfg(not(windows))]
fn system_is_dark() -> bool {
    false
}

#[cfg(windows)]
fn report_fatal(message: &str) {
    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(
            hwnd: *mut core::ffi::c_void,
            text: *const u16,
            caption: *const u16,
            flags: u32,
        ) -> i32;
    }
    let text = wide(message);
    let title = wide("旁译设置");
    unsafe {
        MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), 0x10);
    }
}

#[cfg(not(windows))]
fn report_fatal(_message: &str) {}

#[cfg(windows)]
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn wide_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}
