use std::ffi::{c_void, OsString};
use std::os::windows::{ffi::OsStringExt, process::CommandExt};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, FreeLibrary, HMODULE, HWND, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::{
    FreeLibraryAndExitThread, GetModuleFileNameW, GetModuleHandleExW,
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
};
use windows::Win32::System::Threading::{CreateThread, CREATE_NO_WINDOW, THREAD_CREATION_FLAGS};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

#[path = "../../shared/quick_add.rs"]
mod protocol;
pub(crate) use protocol::{parse_key, Candidate, DEFAULT_KEY};

pub(crate) const COMPLETE: u32 = WM_APP + 41;
static RUNNING: AtomicBool = AtomicBool::new(false);

pub(crate) fn matches_chord(
    key: u8,
    vk: usize,
    control: bool,
    shift: bool,
    alt_or_win: bool,
    repeat: bool,
) -> bool {
    key != 0 && vk == 0x6f + usize::from(key) && control && shift && !alt_or_win && !repeat
}

#[derive(Clone, Copy)]
pub(crate) enum Feedback {
    Added = 1,
    Exists,
    Busy,
}

impl Feedback {
    pub(crate) fn from_message(value: isize) -> Option<Self> {
        match value {
            1 => Some(Self::Added),
            2 => Some(Self::Exists),
            3 => Some(Self::Busy),
            _ => None,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Added => "已加入我的词库",
            Self::Exists => "已存在",
            Self::Busy => "词库正被占用",
        }
    }
}

struct Task {
    module: HMODULE,
    window: HWND,
    serial: usize,
    candidate: Candidate,
}

pub(crate) fn start(window: HWND, serial: usize, candidate: Candidate) -> bool {
    if RUNNING.swap(true, Ordering::AcqRel) {
        return false;
    }
    unsafe {
        // TSF can release the DLL when the user switches input methods. Retain a
        // module reference until the worker posts its result and exits.
        let mut module = HMODULE::default();
        if GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
            PCWSTR(worker as *const () as *const u16),
            &mut module,
        )
        .is_err()
        {
            RUNNING.store(false, Ordering::Release);
            return false;
        }
        let task = Box::into_raw(Box::new(Task { module, window, serial, candidate }));
        match CreateThread(None, 0, Some(worker), Some(task.cast()), THREAD_CREATION_FLAGS(0), None) {
            Ok(handle) => {
                let _ = CloseHandle(handle);
                true
            }
            Err(_) => {
                drop(Box::from_raw(task));
                let _ = FreeLibrary(module);
                RUNNING.store(false, Ordering::Release);
                false
            }
        }
    }
}

unsafe extern "system" fn worker(param: *mut c_void) -> u32 {
    let task = Box::from_raw(param.cast::<Task>());
    let feedback = write_candidate(&task);
    RUNNING.store(false, Ordering::Release);
    let _ = PostMessageW(task.window, COMPLETE, WPARAM(task.serial), LPARAM(feedback as isize));
    let module = task.module;
    drop(task);
    FreeLibraryAndExitThread(module, 0)
}

fn write_candidate(task: &Task) -> Feedback {
    let mut buffer = vec![0; 32768];
    let length = unsafe { GetModuleFileNameW(task.module, &mut buffer) } as usize;
    if length == 0 || length >= buffer.len() {
        return Feedback::Busy;
    }
    let location = OsString::from_wide(&buffer[..length]);
    let program = Path::new(&location).with_file_name("pangyi-settings.exe");
    let Ok(request) = serde_json::to_vec(&task.candidate) else { return Feedback::Busy };
    if request.len() as u64 > protocol::MAX_REQUEST_BYTES {
        return Feedback::Busy;
    }
    let Ok(mut child) = Command::new(program)
        .arg(protocol::COMMAND)
        .creation_flags(CREATE_NO_WINDOW.0)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return Feedback::Busy;
    };
    use std::io::Write;
    let written = child.stdin.take().is_some_and(|mut input| input.write_all(&request).is_ok());
    let status = child.wait();
    if !written {
        return Feedback::Busy;
    }
    match status.ok().and_then(|status| status.code()).unwrap_or(protocol::FAILED) {
        0 => Feedback::Added,
        protocol::EXISTS => Feedback::Exists,
        _ => Feedback::Busy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_respects_preferences_modifiers_and_repeats() {
        let f2 = 0x71;
        assert!(matches_chord(DEFAULT_KEY, f2, true, true, false, false));
        assert!(!matches_chord(0, f2, true, true, false, false));
        assert!(!matches_chord(3, f2, true, true, false, false));
        assert!(matches_chord(3, 0x72, true, true, false, false));
        assert!(!matches_chord(DEFAULT_KEY, f2, false, true, false, false));
        assert!(!matches_chord(DEFAULT_KEY, f2, true, false, false, false));
        assert!(!matches_chord(DEFAULT_KEY, f2, true, true, true, false));
        assert!(!matches_chord(DEFAULT_KEY, f2, true, true, false, true));
    }
}
