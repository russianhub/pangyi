//! 组合回调和「没有候选时确认」走真实的编辑会话，但不用激活 TSF、也不建窗口。
use super::*;
use std::sync::Arc;
use windows::core::{IUnknown, PCWSTR, PWSTR};
use windows::Win32::System::Com::IDataObject;
use windows::Win32::UI::TextServices::{
    IEnumTfContextViews, IEnumTfProperties, ITfCompositionSink_Impl, ITfComposition_Impl,
    ITfContext_Impl, ITfContextView, ITfDocumentMgr, ITfEditSession_Impl, ITfKeyEventSink_Impl,
    ITfProperty, ITfRange, ITfRangeBackup, ITfRange_Impl, ITfReadOnlyProperty, TfAnchor, TfGravity,
    TfShiftDir, TF_HALTCOND, TS_STATUS,
};

#[implement(ITfComposition)]
struct TestComposition {
    unlocked: Arc<AtomicBool>,
    sink: Option<ITfCompositionSink>,
    range: Option<ITfRange>,
    ended: Arc<AtomicU32>,
}

impl ITfComposition_Impl for TestComposition {
    fn GetRange(&self) -> Result<ITfRange> {
        self.unlocked
            .fetch_and(COMPOSITION.0.try_lock().is_ok(), Ordering::SeqCst);
        self.range.clone().ok_or_else(|| E_FAIL.into())
    }

    fn ShiftStart(&self, _ec: u32, _range: Option<&ITfRange>) -> Result<()> {
        Ok(())
    }
    fn ShiftEnd(&self, _ec: u32, _range: Option<&ITfRange>) -> Result<()> {
        Ok(())
    }

    fn EndComposition(&self, ec: u32) -> Result<()> {
        self.ended.fetch_add(1, Ordering::SeqCst);
        let unlocked = COMPOSITION.0.try_lock().is_ok();
        self.unlocked.fetch_and(unlocked, Ordering::SeqCst);
        // 旧实现拿着锁回调会把自己卡死。锁还在就不要再进去。
        if unlocked {
            if let Some(sink) = &self.sink {
                let this: ITfComposition = unsafe { self.cast()? };
                unsafe { sink.OnCompositionTerminated(ec, Some(&this))? };
            }
        }
        Ok(())
    }
}

fn composition(sink: Option<ITfCompositionSink>) -> (ITfComposition, Arc<AtomicBool>) {
    let unlocked = Arc::new(AtomicBool::new(true));
    (
        TestComposition {
            unlocked: unlocked.clone(),
            sink,
            range: None,
            ended: Arc::new(AtomicU32::new(0)),
        }
        .into(),
        unlocked,
    )
}

/// 在内存里跑真正的编辑会话，不激活 TSF，也不创建窗口。
#[implement(ITfContext)]
struct TestContext;

impl ITfContext_Impl for TestContext {
    fn RequestEditSession(
        &self,
        _tid: u32,
        session: Option<&ITfEditSession>,
        flags: TF_CONTEXT_EDIT_CONTEXT_FLAGS,
    ) -> Result<HRESULT> {
        assert_eq!(flags.0, TF_ES_SYNC.0 | TF_ES_READWRITE.0);
        unsafe {
            session
                .ok_or_else(|| windows::core::Error::from(E_POINTER))?
                .DoEditSession(7)?;
        }
        Ok(S_OK)
    }
    fn InWriteSession(&self, _tid: u32) -> Result<BOOL> {
        Err(E_FAIL.into())
    }
    fn GetSelection(
        &self,
        _ec: u32,
        _index: u32,
        _count: u32,
        _selection: *mut TF_SELECTION,
        _fetched: *mut u32,
    ) -> Result<()> {
        Err(E_FAIL.into())
    }
    fn SetSelection(&self, _ec: u32, _count: u32, _selection: *const TF_SELECTION) -> Result<()> {
        Err(E_FAIL.into())
    }
    fn GetStart(&self, _ec: u32) -> Result<ITfRange> {
        Err(E_FAIL.into())
    }
    fn GetEnd(&self, _ec: u32) -> Result<ITfRange> {
        Err(E_FAIL.into())
    }
    fn GetActiveView(&self) -> Result<ITfContextView> {
        Err(E_FAIL.into())
    }
    fn EnumViews(&self) -> Result<IEnumTfContextViews> {
        Err(E_FAIL.into())
    }
    fn GetStatus(&self) -> Result<TS_STATUS> {
        Err(E_FAIL.into())
    }
    fn GetProperty(&self, _guid: *const GUID) -> Result<ITfProperty> {
        Err(E_FAIL.into())
    }
    fn GetAppProperty(&self, _guid: *const GUID) -> Result<ITfReadOnlyProperty> {
        Err(E_FAIL.into())
    }
    fn TrackProperties(
        &self,
        _props: *const *const GUID,
        _count: u32,
        _app_props: *const *const GUID,
        _app_count: u32,
    ) -> Result<ITfReadOnlyProperty> {
        Err(E_FAIL.into())
    }
    fn EnumProperties(&self) -> Result<IEnumTfProperties> {
        Err(E_FAIL.into())
    }
    fn GetDocumentMgr(&self) -> Result<ITfDocumentMgr> {
        Err(E_FAIL.into())
    }
    fn CreateRangeBackup(&self, _ec: u32, _range: Option<&ITfRange>) -> Result<ITfRangeBackup> {
        Err(E_FAIL.into())
    }
}

#[implement(ITfRange)]
struct TestRange {
    text: Arc<Mutex<String>>,
}

impl ITfRange_Impl for TestRange {
    fn SetText(&self, _ec: u32, _flags: u32, text: &PCWSTR, len: i32) -> Result<()> {
        assert!(
            COMPOSITION.0.try_lock().is_ok(),
            "SetText called while composition mutex was locked"
        );
        assert!(len >= 0);
        let utf16 = if len == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(text.0, len as usize) }
        };
        *self.text.lock().unwrap() = String::from_utf16(utf16).unwrap();
        Ok(())
    }
    fn GetText(&self, _ec: u32, _flags: u32, _text: PWSTR, _max: u32, _len: *mut u32) -> Result<()> {
        Err(E_FAIL.into())
    }
    fn GetFormattedText(&self, _ec: u32) -> Result<IDataObject> {
        Err(E_FAIL.into())
    }
    fn GetEmbedded(&self, _ec: u32, _service: *const GUID, _iid: *const GUID) -> Result<IUnknown> {
        Err(E_FAIL.into())
    }
    fn InsertEmbedded(&self, _ec: u32, _flags: u32, _object: Option<&IDataObject>) -> Result<()> {
        Err(E_FAIL.into())
    }
    fn ShiftStart(
        &self,
        _ec: u32,
        _count: i32,
        _shifted: *mut i32,
        _halt: *const TF_HALTCOND,
    ) -> Result<()> {
        Err(E_FAIL.into())
    }
    fn ShiftEnd(
        &self,
        _ec: u32,
        _count: i32,
        _shifted: *mut i32,
        _halt: *const TF_HALTCOND,
    ) -> Result<()> {
        Err(E_FAIL.into())
    }
    fn ShiftStartToRange(&self, _ec: u32, _range: Option<&ITfRange>, _anchor: TfAnchor) -> Result<()> {
        Err(E_FAIL.into())
    }
    fn ShiftEndToRange(&self, _ec: u32, _range: Option<&ITfRange>, _anchor: TfAnchor) -> Result<()> {
        Err(E_FAIL.into())
    }
    fn ShiftStartRegion(&self, _ec: u32, _dir: TfShiftDir) -> Result<BOOL> {
        Err(E_FAIL.into())
    }
    fn ShiftEndRegion(&self, _ec: u32, _dir: TfShiftDir) -> Result<BOOL> {
        Err(E_FAIL.into())
    }
    fn IsEmpty(&self, _ec: u32) -> Result<BOOL> {
        Err(E_FAIL.into())
    }
    fn Collapse(&self, _ec: u32, _anchor: TfAnchor) -> Result<()> {
        Err(E_FAIL.into())
    }
    fn IsEqualStart(&self, _ec: u32, _range: Option<&ITfRange>, _anchor: TfAnchor) -> Result<BOOL> {
        Err(E_FAIL.into())
    }
    fn IsEqualEnd(&self, _ec: u32, _range: Option<&ITfRange>, _anchor: TfAnchor) -> Result<BOOL> {
        Err(E_FAIL.into())
    }
    fn CompareStart(&self, _ec: u32, _range: Option<&ITfRange>, _anchor: TfAnchor) -> Result<i32> {
        Err(E_FAIL.into())
    }
    fn CompareEnd(&self, _ec: u32, _range: Option<&ITfRange>, _anchor: TfAnchor) -> Result<i32> {
        Err(E_FAIL.into())
    }
    fn AdjustForInsert(&self, _ec: u32, _count: u32) -> Result<BOOL> {
        Err(E_FAIL.into())
    }
    fn GetGravity(&self, _start: *mut TfGravity, _end: *mut TfGravity) -> Result<()> {
        Err(E_FAIL.into())
    }
    fn SetGravity(&self, _ec: u32, _start: TfGravity, _end: TfGravity) -> Result<()> {
        Err(E_FAIL.into())
    }
    fn Clone(&self) -> Result<ITfRange> {
        Err(E_FAIL.into())
    }
    fn GetContext(&self) -> Result<ITfContext> {
        Err(E_FAIL.into())
    }
}

fn reset() {
    let previous = COMPOSITION.lock().unwrap().take();
    drop(previous);
    UI_CANDS.lock().unwrap().clear();
    ALL_CANDS.lock().unwrap().clear();
    SENT.lock().unwrap().clear();
    PAGE.store(0, Ordering::Relaxed);
    *PENDING_CAND.lock().unwrap() = None;
    unsafe {
        cand::set_preedit("");
        cand::set_sentence(None);
        cand::set_items(Vec::new());
        cand::set_page(0, 1);
    }
}

#[test]
fn non_letter_keydown_does_not_change_pinyin() {
    let _serial = crate::lock_state();
    reset();
    let service = TextService::new();
    service.cursor.set(1);
    *service.buf.borrow_mut() = "ni".to_string();
    for vk in [0x61, 0x69, 0x70, 0x74, 0x7A, 0x141] {
        let eaten = service.OnKeyDown(None, WPARAM(vk), LPARAM(0)).unwrap();
        assert!(!eaten.as_bool(), "VK {vk:#x}");
        assert_eq!(&*service.buf.borrow(), "ni");
        assert_eq!(service.cursor.get(), 1);
    }
    reset();
}

#[test]
fn host_termination_clears_preedit_and_candidates() {
    let _serial = crate::lock_state();
    reset();
    let service = TextService::new();
    *service.buf.borrow_mut() = "ni".to_string();
    service.cursor.set(2);
    let (comp, _) = composition(None);
    *COMPOSITION.lock().unwrap() = Some(comp.clone());
    UI_CANDS.lock().unwrap().push(("你".into(), "ты".into()));
    ALL_CANDS.lock().unwrap().push(("你".into(), "ты".into()));
    SENT.lock().unwrap().push("你好".into());
    PAGE.store(3, Ordering::Relaxed);
    *PENDING_CAND.lock().unwrap() = Some((42, 100));
    unsafe {
        cand::set_preedit("ni");
        cand::set_items(vec![cand::Item {
            word: "你".into(),
            ru: "ты".into(),
            fresh: false,
        }]);
    }
    service.OnCompositionTerminated(0, Some(&comp)).unwrap();
    assert!(service.buf.borrow().is_empty());
    assert_eq!(service.cursor.get(), 0);
    assert!(COMPOSITION.lock().unwrap().is_none());
    assert!(UI_CANDS.lock().unwrap().is_empty());
    assert!(ALL_CANDS.lock().unwrap().is_empty());
    assert!(SENT.lock().unwrap().is_empty());
    assert_eq!(PAGE.load(Ordering::Relaxed), 0);
    assert_eq!(*PENDING_CAND.lock().unwrap(), None);
    assert_eq!(unsafe { cand::count() }, 0);
    reset();
}

#[test]
fn old_termination_does_not_cancel_new_composition() {
    let _serial = crate::lock_state();
    reset();
    let service = TextService::new();
    *service.buf.borrow_mut() = "hao".to_string();
    service.cursor.set(3);
    let (current, _) = composition(None);
    let (old, _) = composition(None);
    *COMPOSITION.lock().unwrap() = Some(current.clone());
    service.OnCompositionTerminated(0, Some(&old)).unwrap();
    let remaining = COMPOSITION.lock().unwrap().clone();
    assert_eq!(remaining.as_ref(), Some(&current));
    assert_eq!(&*service.buf.borrow(), "hao");
    assert_eq!(service.cursor.get(), 3);
    reset();
}

#[test]
fn cancel_composition_allows_synchronous_callback() {
    let _serial = crate::lock_state();
    reset();
    let sink: ITfCompositionSink = TextService::new().into();
    let (comp, unlocked) = composition(Some(sink));
    *COMPOSITION.lock().unwrap() = Some(comp);
    SENT.lock().unwrap().push("你好".into());
    // 自己结束组合时回调看见的是已经 take() 掉的槽，不能再清句子缓冲。
    EndSession.DoEditSession(0).unwrap();
    assert!(
        unlocked.load(Ordering::SeqCst),
        "COM was called while composition mutex was locked"
    );
    assert!(COMPOSITION.lock().unwrap().is_none());
    assert_eq!(&*SENT.lock().unwrap(), &["你好"]);
    reset();
}

#[test]
fn updating_composition_calls_com_without_lock() {
    let _serial = crate::lock_state();
    reset();
    let text = Arc::new(Mutex::new(String::new()));
    let range: ITfRange = TestRange { text: text.clone() }.into();
    let unlocked = Arc::new(AtomicBool::new(true));
    let comp: ITfComposition = TestComposition {
        unlocked: unlocked.clone(),
        sink: None,
        range: Some(range),
        ended: Arc::new(AtomicU32::new(0)),
    }
    .into();
    *COMPOSITION.lock().unwrap() = Some(comp);
    let ctx: ITfContext = TestContext.into();
    UpdateSession {
        ctx,
        text: vec![b'n' as u16],
        sel: 1,
    }
    .DoEditSession(0)
    .unwrap();
    assert!(unlocked.load(Ordering::SeqCst));
    assert_eq!(&*text.lock().unwrap(), "n");
    reset();
}

#[test]
fn tab_only_selects_when_candidates_exist() {
    let _serial = crate::lock_state();
    reset();
    let service = TextService::new();
    *service.buf.borrow_mut() = "zzzzzz".to_string();
    assert!(!claim(WPARAM(0x09), true, false));
    assert!(!service
        .OnKeyDown(None, WPARAM(0x09), LPARAM(0))
        .unwrap()
        .as_bool());
    assert_eq!(&*service.buf.borrow(), "zzzzzz");

    UI_CANDS.lock().unwrap().extend([
        ("你".into(), "ты".into()),
        ("尼".into(), String::new()),
    ]);
    unsafe {
        cand::set_items(vec![
            cand::Item {
                word: "你".into(),
                ru: "ты".into(),
                fresh: false,
            },
            cand::Item {
                word: "尼".into(),
                ru: String::new(),
                fresh: false,
            },
        ]);
    }
    assert!(claim(WPARAM(0x09), true, false));
    assert!(service
        .OnKeyDown(None, WPARAM(0x09), LPARAM(0))
        .unwrap()
        .as_bool());
    assert_eq!(unsafe { cand::current_index() }, 1);
    assert_eq!(&*service.buf.borrow(), "zzzzzz");
    reset();
}

#[test]
fn confirming_without_candidates_preserves_letters_and_ends_composition() {
    let _serial = crate::lock_state();
    let learned_path = std::path::PathBuf::from(settings::cfg_file()).with_file_name("learned.txt");
    let learned_before = std::fs::read(&learned_path).ok();
    for key in [0x0D, 0x20] {
        reset();
        let service = TextService::new();
        let raw = "zzzzzz";
        *service.buf.borrow_mut() = raw.to_string();
        service.cursor.set(6);
        SENT.lock().unwrap().push("已有句子".into());
        let text = Arc::new(Mutex::new(raw.to_string()));
        let range: ITfRange = TestRange { text: text.clone() }.into();
        let ended = Arc::new(AtomicU32::new(0));
        let unlocked = Arc::new(AtomicBool::new(true));
        let comp: ITfComposition = TestComposition {
            unlocked: unlocked.clone(),
            sink: None,
            range: Some(range),
            ended: ended.clone(),
        }
        .into();
        *COMPOSITION.lock().unwrap() = Some(comp);
        let ctx: ITfContext = TestContext.into();

        assert!(claim(WPARAM(key), true, false));
        assert!(service
            .OnKeyDown(Some(&ctx), WPARAM(key), LPARAM(0))
            .unwrap()
            .as_bool());
        assert_eq!(&*text.lock().unwrap(), raw, "VK {key:#x} changed the raw input");
        assert_eq!(ended.load(Ordering::SeqCst), 1, "VK {key:#x} left a composition alive");
        assert!(unlocked.load(Ordering::SeqCst));
        assert!(COMPOSITION.lock().unwrap().is_none());
        assert!(service.buf.borrow().is_empty());
        assert_eq!(service.cursor.get(), 0);
        assert!(UI_CANDS.lock().unwrap().is_empty());
        assert_eq!(&*SENT.lock().unwrap(), &["已有句子"]);
        assert_eq!(std::fs::read(&learned_path).ok(), learned_before);
    }
    reset();
}
