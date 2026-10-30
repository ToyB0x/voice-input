//! AX references and target snapshots never leave this owning thread.
//! Optional clipboard fallback is selected at capture, never after an AX write.
use core_foundation::{
    base::{CFType, CFTypeRef, TCFType},
    boolean::CFBoolean,
    dictionary::CFDictionary,
    string::{CFString, CFStringRef},
};
use serde::Serialize;
use std::{
    ffi::c_void,
    hash::{DefaultHasher, Hash, Hasher},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
};

type Element = *const c_void;
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Range {
    pub(crate) location: isize,
    pub(crate) length: isize,
}
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> u8;
    fn AXUIElementCreateSystemWide() -> Element;
    fn AXUIElementGetTypeID() -> usize;
    fn AXUIElementGetPid(element: Element, pid: *mut i32) -> i32;
    fn AXUIElementSetMessagingTimeout(element: Element, timeout: f32) -> i32;
    fn AXUIElementCopyAttributeValue(
        element: Element,
        attr: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementIsAttributeSettable(element: Element, attr: CFStringRef, value: *mut u8) -> i32;
    fn AXUIElementSetAttributeValue(element: Element, attr: CFStringRef, value: CFTypeRef) -> i32;
    fn AXValueGetTypeID() -> usize;
    fn AXValueGetType(value: CFTypeRef) -> i32;
    fn AXValueGetValue(value: CFTypeRef, kind: i32, output: *mut c_void) -> u8;
}

pub fn trusted() -> bool {
    unsafe { AXIsProcessTrusted() != 0 }
}
#[tauri::command]
pub fn request_accessibility() -> Result<bool, String> {
    let options = CFDictionary::from_CFType_pairs(&[(
        CFString::new("AXTrustedCheckOptionPrompt").as_CFType(),
        CFBoolean::true_value().as_CFType(),
    )]);
    let granted = unsafe { AXIsProcessTrustedWithOptions(options.as_CFTypeRef()) != 0 };
    if !granted {
        std::process::Command::new("/usr/bin/open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .status()
            .map_err(|e| e.to_string())?;
    }
    Ok(granted)
}
pub(crate) fn attribute(element: &CFType, name: &str) -> Result<CFType, String> {
    let mut value = std::ptr::null();
    let code = unsafe {
        AXUIElementCopyAttributeValue(
            element.as_CFTypeRef(),
            CFString::new(name).as_concrete_TypeRef(),
            &mut value,
        )
    };
    if code != 0 || value.is_null() {
        return Err("入力先の情報を確認できません。".into());
    }
    // Copy API transfers one retain, released by CFType on this thread.
    Ok(unsafe { CFType::wrap_under_create_rule(value) })
}
fn text(element: &CFType, name: &str) -> Result<String, String> {
    let value = attribute(element, name)?
        .downcast::<CFString>()
        .ok_or("入力欄の形式に対応していません。")?;
    if value.char_len() > 1_000_000 {
        return Err("大きすぎる入力欄には自動入力しません。".into());
    }
    Ok(value.to_string())
}
pub(crate) fn selected_range(element: &CFType) -> Result<Range, String> {
    let value = attribute(element, "AXSelectedTextRange")?;
    let mut range = Range {
        location: 0,
        length: 0,
    };
    // AXValue enum 4 is kAXValueCFRangeType. Check type before reading memory.
    if value.type_of() != unsafe { AXValueGetTypeID() }
        || unsafe { AXValueGetType(value.as_CFTypeRef()) } != 4
        || unsafe { AXValueGetValue(value.as_CFTypeRef(), 4, (&mut range as *mut Range).cast()) }
            == 0
        || range.location < 0
        || range.length < 0
    {
        return Err("入力位置を確認できません。".into());
    }
    Ok(range)
}
fn fingerprint(element: &CFType) -> Result<u64, String> {
    let value = text(element, "AXValue")?;
    let mut hash = DefaultHasher::new();
    value.hash(&mut hash);
    Ok(hash.finish())
}
fn editable(element: &CFType) -> Result<(), String> {
    let role = text(element, "AXRole")?;
    let subrole = text(element, "AXSubrole");
    if role == "AXTextField" && subrole.is_err() {
        return Err("入力欄の安全性を確認できません。".into());
    }
    let subrole = subrole.unwrap_or_default();
    if !["AXTextArea", "AXTextField"].contains(&role.as_str()) || subrole.contains("Secure") {
        return Err("パスワード欄や非対応の入力先には自動入力しません。".into());
    }
    let mut settable = 0;
    if unsafe {
        AXUIElementIsAttributeSettable(
            element.as_CFTypeRef(),
            CFString::new("AXSelectedText").as_concrete_TypeRef(),
            &mut settable,
        )
    } != 0
        || settable == 0
    {
        return Err("この入力欄は直接入力に対応していません。".into());
    }
    Ok(())
}
fn focused_app() -> Result<(i32, CFType), String> {
    if !trusted() {
        return Err("アクセシビリティが未許可です。結果を確認してコピーしてください。".into());
    }
    let system = unsafe { CFType::wrap_under_create_rule(AXUIElementCreateSystemWide()) };
    unsafe {
        AXUIElementSetMessagingTimeout(system.as_CFTypeRef(), 0.2);
    }
    let app = attribute(&system, "AXFocusedApplication")?;
    if app.type_of() != unsafe { AXUIElementGetTypeID() } {
        return Err("入力先を識別できません。".into());
    }
    let mut pid = 0;
    if unsafe { AXUIElementGetPid(app.as_CFTypeRef(), &mut pid) } != 0
        || pid == std::process::id() as i32
    {
        return Err("別アプリの入力欄で録音を開始してください。".into());
    }
    // Reject our own app before querying its UI; never wait for the main
    // thread while it may be polling the capture mutex held by the ASR worker.
    unsafe {
        AXUIElementSetMessagingTimeout(app.as_CFTypeRef(), 0.2);
    }
    Ok((pid, app))
}
fn focused_element(app: &CFType, pid: i32) -> Result<CFType, String> {
    let element = attribute(app, "AXFocusedUIElement")?;
    if element.type_of() != unsafe { AXUIElementGetTypeID() } {
        return Err("入力先を識別できません。".into());
    }
    unsafe {
        AXUIElementSetMessagingTimeout(element.as_CFTypeRef(), 0.2);
    }
    let mut field_pid = 0;
    if unsafe { AXUIElementGetPid(element.as_CFTypeRef(), &mut field_pid) } != 0 || pid != field_pid
    {
        return Err("入力先を識別できません。".into());
    }
    Ok(element)
}
pub(crate) fn focused() -> Result<(i32, CFType), String> {
    let (pid, app) = focused_app()?;
    let element = focused_element(&app, pid)?;
    editable(&element)?;
    Ok((pid, element))
}
struct DirectTarget {
    id: u64,
    epoch: u64,
    pid: i32,
    element: CFType,
    range: Range,
    fingerprint: u64,
}
impl DirectTarget {
    fn capture(id: u64, epoch: u64) -> Result<Self, String> {
        let (pid, element) = focused()?;
        let range = selected_range(&element)?;
        let fingerprint = fingerprint(&element)?;
        Ok(Self {
            id,
            epoch,
            pid,
            element,
            range,
            fingerprint,
        })
    }
    fn insert(&self, value: &str, shared: &Shared) -> Result<(), String> {
        let (pid, element) = focused()?;
        if self.pid != pid
            || self.element != element
            || self.range != selected_range(&element)?
            || self.fingerprint != fingerprint(&element)?
        {
            return Err(
                "入力先・位置・内容が変わったため自動入力を見送りました。結果をコピーできます。"
                    .into(),
            );
        }
        // Recheck focus after reading the value, then perform exactly one
        // selection write to the captured element. Never replace the full value.
        let (pid, current) = focused()?;
        if pid != self.pid
            || current != self.element
            || !shared.enabled.load(Ordering::Acquire)
            || shared.epoch.load(Ordering::Acquire) != self.epoch
        {
            return Err("入力先または設定が変わったため自動入力を見送りました。".into());
        }
        if unsafe {
            AXUIElementSetAttributeValue(
                self.element.as_CFTypeRef(),
                CFString::new("AXSelectedText").as_concrete_TypeRef(),
                CFString::new(value).as_CFTypeRef(),
            )
        } != 0
        {
            return Err("入力先が直接入力を受け付けませんでした。自動再試行はしません。結果を確認してください。".into());
        }
        Ok(())
    }
}
// The fallback is intentionally explicit: without AX field metadata we can
// verify the application, but cannot promise the original caret/field.
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn IsSecureEventInputEnabled() -> u8;
}
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn CGEventCreateKeyboardEvent(source: *const c_void, key: u16, down: bool) -> CFTypeRef;
    fn CGEventSetFlags(event: CFTypeRef, flags: u64);
    fn CGEventPostToPid(pid: i32, event: CFTypeRef);
}
fn paste_events() -> Result<(CFType, CFType), String> {
    fn event(down: bool) -> Result<CFType, String> {
        // kVK_ANSI_V, kCGEventFlagMaskCommand. Do not inherit shortcut modifiers.
        let ptr = unsafe { CGEventCreateKeyboardEvent(std::ptr::null(), 9, down) };
        if ptr.is_null() {
            return Err("貼り付けキーを作成できません。".into());
        }
        let event = unsafe { CFType::wrap_under_create_rule(ptr) };
        unsafe {
            CGEventSetFlags(event.as_CFTypeRef(), 1 << 20);
        }
        Ok(event)
    }
    Ok((event(true)?, event(false)?))
}
fn paste_field_allowed(element: &CFType) -> Result<(), String> {
    let role = text(element, "AXRole")?;
    let subrole = text(element, "AXSubrole").unwrap_or_default();
    if role.contains("Secure") || subrole.contains("Secure") {
        return Err("パスワード欄には貼り付けません。".into());
    }
    if ![
        "AXTextArea",
        "AXTextField",
        "AXComboBox",
        "AXGroup",
        "AXWebArea",
        "AXUnknown",
    ]
    .contains(&role.as_str())
    {
        return Err("テキスト入力先を確認できないため貼り付けません。".into());
    }
    if let Ok(value) = attribute(element, "AXEnabled")
        && value
            .downcast::<CFBoolean>()
            .is_some_and(|v| !bool::from(v))
    {
        return Err("無効な入力欄には貼り付けません。".into());
    }
    Ok(())
}
struct PasteTarget {
    id: u64,
    epoch: u64,
    pid: i32,
    element: Option<CFType>,
    range: Option<Range>,
    fingerprint: Option<u64>,
}
impl PasteTarget {
    fn capture(id: u64, epoch: u64) -> Result<Self, String> {
        if unsafe { IsSecureEventInputEnabled() } != 0 {
            return Err("保護された入力中は貼り付けません。".into());
        }
        let (pid, app) = focused_app()?;
        let element = focused_element(&app, pid).ok();
        if let Some(element) = &element {
            paste_field_allowed(element)?;
        }
        let range = element.as_ref().and_then(|e| selected_range(e).ok());
        let fingerprint = element.as_ref().and_then(|e| fingerprint(e).ok());
        Ok(Self {
            id,
            epoch,
            pid,
            element,
            range,
            fingerprint,
        })
    }
    fn same_destination(&self, current: &Self) -> bool {
        self.pid == current.pid
            && self.element == current.element
            && self.range == current.range
            && self.fingerprint == current.fingerprint
    }
    fn ensure_current(&self, shared: &Shared) -> Result<(), String> {
        if !shared.enabled.load(Ordering::Acquire)
            || !shared.clipboard_fallback.load(Ordering::Acquire)
            || shared.epoch.load(Ordering::Acquire) != self.epoch
        {
            return Err("設定変更・取り消しのため貼り付けを見送りました。".into());
        }
        Ok(())
    }
    fn validate_using(
        &self,
        shared: &Shared,
        capture: impl FnOnce() -> Result<Self, String>,
    ) -> Result<(), String> {
        self.ensure_current(shared)?;
        let current = capture()?;
        // AX IPC may block while another thread invalidates this recording.
        self.ensure_current(shared)?;
        if !self.same_destination(&current) {
            return Err("入力先・位置・内容が変わったため貼り付けを見送りました。".into());
        }
        Ok(())
    }
    fn validate(&self, shared: &Shared) -> Result<(), String> {
        self.validate_using(shared, || Self::capture(self.id, self.epoch))
    }
    fn insert(&self, value: &str, shared: &Shared) -> Result<(), String> {
        self.validate(shared)?;
        let (down, up) = paste_events()?;
        let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        clipboard.set_text(value).map_err(|e| e.to_string())?;
        // Recheck after clipboard IPC. Never restore on a timer: the target may
        // read asynchronously, and restoring could overwrite a user's new copy.
        self.validate(shared)?;
        self.ensure_current(shared)?;
        unsafe {
            CGEventPostToPid(self.pid, down.as_CFTypeRef());
            CGEventPostToPid(self.pid, up.as_CFTypeRef());
        }
        Ok(())
    }
}
enum Target {
    Direct(DirectTarget),
    Paste(PasteTarget),
}
impl Target {
    fn capture(id: u64, epoch: u64, fallback: bool) -> Result<Self, String> {
        match DirectTarget::capture(id, epoch) {
            Ok(target) => Ok(Self::Direct(target)),
            Err(_) if fallback => PasteTarget::capture(id, epoch).map(Self::Paste),
            Err(error) => Err(error),
        }
    }
    fn id(&self) -> u64 {
        match self {
            Self::Direct(t) => t.id,
            Self::Paste(t) => t.id,
        }
    }
    fn epoch(&self) -> u64 {
        match self {
            Self::Direct(t) => t.epoch,
            Self::Paste(t) => t.epoch,
        }
    }
    fn insert(&self, value: &str, shared: &Shared) -> Result<(), String> {
        match self {
            Self::Direct(t) => t.insert(value, shared),
            Self::Paste(t) => t.insert(value, shared),
        }
    }
    fn success_message(&self) -> &'static str {
        match self {
            Self::Direct(_) => "入力先へ挿入しました。再試行した認識結果は自動入力しません。",
            Self::Paste(_) => {
                "録音開始時のアプリへ貼り付けキーを送信しました。挿入結果を確認してください。クリップボードに結果を保持しています。"
            }
        }
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub enabled: bool,
    pub trusted: bool,
    pub state: String,
    pub message: String,
}
struct Shared {
    enabled: AtomicBool,
    clipboard_fallback: AtomicBool,
    epoch: AtomicU64,
    status: Mutex<(String, String)>,
}
impl Shared {
    fn report(&self, state: &str, message: &str) {
        if let Ok(mut status) = self.status.lock() {
            *status = (state.into(), message.into());
        }
    }
}
enum Command {
    Begin(mpsc::Sender<Option<u64>>),
    Apply(u64, String, mpsc::Sender<()>),
    Cancel,
}
#[derive(Clone)]
pub struct Inserter {
    tx: mpsc::Sender<Command>,
    shared: Arc<Shared>,
}
impl Inserter {
    pub fn new() -> Self {
        let shared = Arc::new(Shared {
            enabled: AtomicBool::new(false),
            clipboard_fallback: AtomicBool::new(false),
            epoch: AtomicU64::new(0),
            status: Mutex::new(("disabled".into(), "自動入力は無効です。".into())),
        });
        let (tx, rx) = mpsc::channel();
        let worker = shared.clone();
        std::thread::spawn(move || {
            let mut target: Option<Target> = None;
            let mut sequence = 0;
            for command in rx {
                match command {
                    Command::Cancel => {
                        target = None;
                    }
                    Command::Begin(reply) => {
                        target = None;
                        sequence += 1;
                        if worker.enabled.load(Ordering::Acquire) {
                            match Target::capture(
                                sequence,
                                worker.epoch.load(Ordering::Acquire),
                                worker.clipboard_fallback.load(Ordering::Acquire),
                            ) {
                                Ok(captured)
                                    if worker.enabled.load(Ordering::Acquire)
                                        && captured.epoch()
                                            == worker.epoch.load(Ordering::Acquire) =>
                                {
                                    target = Some(captured);
                                    worker.report("armed", "録音開始時の入力先を記録しました。認識完了時に再確認します。");
                                }
                                Ok(_) => {}
                                Err(error) => worker.report("blocked", &error),
                            }
                        }
                        let _ = reply.send(target.as_ref().map(|t| t.id()));
                    }
                    Command::Apply(id, value, reply) => {
                        if let Some(captured) = target.take().filter(|t| t.id() == id) {
                            if value.is_empty() {
                                worker.report("blocked", "認識結果が空のため自動入力しません。");
                            } else {
                                match captured.insert(&value, &worker) {
                                    Ok(()) => worker.report("inserted", captured.success_message()),
                                    Err(error) => worker.report("blocked", &error),
                                }
                            }
                        }
                        let _ = reply.send(());
                    }
                }
            }
        });
        Self { tx, shared }
    }
    pub fn configure(&self, enabled: bool, clipboard_fallback: bool) {
        let changed = self
            .shared
            .clipboard_fallback
            .swap(clipboard_fallback, Ordering::AcqRel)
            != clipboard_fallback;
        if self.shared.enabled.swap(enabled, Ordering::AcqRel) != enabled || changed {
            self.shared.epoch.fetch_add(1, Ordering::AcqRel);
            self.cancel();
            self.shared.report(
                if enabled { "idle" } else { "disabled" },
                if enabled {
                    "次の録音から自動入力します。入力先でショートカットを押してください。"
                } else {
                    "自動入力は無効です。"
                },
            );
        }
    }
    pub fn begin(&self) -> Option<u64> {
        if !self.shared.enabled.load(Ordering::Acquire) {
            return None;
        }
        let (reply, rx) = mpsc::channel();
        self.tx.send(Command::Begin(reply)).ok()?;
        rx.recv().ok().flatten()
    }
    pub fn apply(&self, id: u64, value: &str) {
        let (reply, rx) = mpsc::channel();
        if self
            .tx
            .send(Command::Apply(id, value.into(), reply))
            .is_ok()
        {
            // Do not detach a timed-out write: discard must wait until the AX
            // attempt is finished. AX IPC has a per-element 200 ms timeout.
            let _ = rx.recv();
        }
    }
    pub fn cancel(&self) {
        self.shared.epoch.fetch_add(1, Ordering::AcqRel);
        let _ = self.tx.send(Command::Cancel);
        let enabled = self.shared.enabled.load(Ordering::Acquire);
        self.shared.report(
            if enabled { "idle" } else { "disabled" },
            if enabled {
                "自動入力を取り消しました。次の録音で入力先を記録します。"
            } else {
                "自動入力は無効です。"
            },
        );
    }
    pub(crate) fn feedback(&self) -> (bool, String) {
        (
            self.shared.enabled.load(Ordering::Acquire),
            self.shared
                .status
                .lock()
                .map(|status| status.0.clone())
                .unwrap_or_default(),
        )
    }
    pub fn status(&self) -> Status {
        let (state, message) = self
            .shared
            .status
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default();
        Status {
            enabled: self.shared.enabled.load(Ordering::Acquire),
            trusted: trusted(),
            state,
            message,
        }
    }
}
#[tauri::command]
pub fn insertion_status(inserter: tauri::State<'_, Inserter>) -> Status {
    inserter.status()
}

#[cfg(test)]
mod paste_tests {
    use super::*;
    fn target() -> PasteTarget {
        PasteTarget {
            id: 1,
            epoch: 7,
            pid: 42,
            element: None,
            range: None,
            fingerprint: None,
        }
    }
    #[test]
    fn paste_rejects_changed_app_or_newly_available_field_metadata() {
        let before = target();
        let mut after = target();
        assert!(before.same_destination(&after));
        after.pid = 43;
        assert!(!before.same_destination(&after));
        after.pid = 42;
        after.range = Some(Range {
            location: 1,
            length: 0,
        });
        assert!(!before.same_destination(&after));
        after.range = None;
        after.fingerprint = Some(123);
        assert!(!before.same_destination(&after));
    }
    #[test]
    fn paste_rejects_cancelled_or_disabled_jobs_before_os_access() {
        let shared = Shared {
            enabled: AtomicBool::new(true),
            clipboard_fallback: AtomicBool::new(true),
            epoch: AtomicU64::new(8),
            status: Mutex::new((String::new(), String::new())),
        };
        assert!(target().validate(&shared).unwrap_err().contains("取り消し"));
        shared.epoch.store(7, Ordering::Release);
        shared.clipboard_fallback.store(false, Ordering::Release);
        assert!(target().validate(&shared).unwrap_err().contains("設定変更"));
        shared.clipboard_fallback.store(true, Ordering::Release);
        shared.enabled.store(false, Ordering::Release);
        assert!(target().validate(&shared).is_err());
    }
    #[test]
    fn paste_rechecks_cancellation_after_destination_query() {
        let shared = Shared {
            enabled: AtomicBool::new(true),
            clipboard_fallback: AtomicBool::new(true),
            epoch: AtomicU64::new(7),
            status: Mutex::new((String::new(), String::new())),
        };
        let error = target()
            .validate_using(&shared, || {
                shared.epoch.fetch_add(1, Ordering::AcqRel);
                Ok(target())
            })
            .unwrap_err();
        assert!(error.contains("取り消し"));
    }
    #[test]
    fn paste_events_are_command_v_down_and_up_without_shift() {
        unsafe extern "C" {
            fn CGEventGetFlags(event: CFTypeRef) -> u64;
            fn CGEventGetType(event: CFTypeRef) -> u32;
            fn CGEventGetIntegerValueField(event: CFTypeRef, field: u32) -> i64;
        }
        let (down, up) = paste_events().unwrap();
        unsafe {
            assert_eq!(CGEventGetType(down.as_CFTypeRef()), 10);
            assert_eq!(CGEventGetType(up.as_CFTypeRef()), 11);
            for event in [&down, &up] {
                assert_eq!(CGEventGetFlags(event.as_CFTypeRef()), 1 << 20);
                assert_eq!(CGEventGetIntegerValueField(event.as_CFTypeRef(), 9), 9);
            }
        }
    }
}
