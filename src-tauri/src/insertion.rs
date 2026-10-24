//! AX references and target snapshots never leave this owning thread.
//! No keystroke synthesis, clipboard, full-value writes or text IPC.
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
struct Range {
    location: isize,
    length: isize,
}
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
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
    fn AXValueGetValue(value: CFTypeRef, kind: i32, output: *mut c_void) -> bool;
}

pub fn trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}
#[tauri::command]
pub fn request_accessibility() -> Result<bool, String> {
    let options = CFDictionary::from_CFType_pairs(&[(
        CFString::new("AXTrustedCheckOptionPrompt").as_CFType(),
        CFBoolean::true_value().as_CFType(),
    )]);
    let granted = unsafe { AXIsProcessTrustedWithOptions(options.as_CFTypeRef()) };
    if !granted {
        std::process::Command::new("/usr/bin/open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .status()
            .map_err(|e| e.to_string())?;
    }
    Ok(granted)
}
fn attribute(element: &CFType, name: &str) -> Result<CFType, String> {
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
fn selected_range(element: &CFType) -> Result<Range, String> {
    let value = attribute(element, "AXSelectedTextRange")?;
    let mut range = Range {
        location: 0,
        length: 0,
    };
    // AXValue enum 4 is kAXValueCFRangeType. Check type before reading memory.
    if value.type_of() != unsafe { AXValueGetTypeID() }
        || unsafe { AXValueGetType(value.as_CFTypeRef()) } != 4
        || !unsafe { AXValueGetValue(value.as_CFTypeRef(), 4, (&mut range as *mut Range).cast()) }
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
fn focused() -> Result<(i32, CFType), String> {
    if !trusted() {
        return Err("アクセシビリティが未許可です。結果を確認してコピーしてください。".into());
    }
    let system = unsafe { CFType::wrap_under_create_rule(AXUIElementCreateSystemWide()) };
    unsafe {
        AXUIElementSetMessagingTimeout(system.as_CFTypeRef(), 0.2);
    }
    let app = attribute(&system, "AXFocusedApplication")?;
    let element = attribute(&app, "AXFocusedUIElement")?;
    if app.type_of() != unsafe { AXUIElementGetTypeID() }
        || element.type_of() != unsafe { AXUIElementGetTypeID() }
    {
        return Err("入力先を識別できません。".into());
    }
    unsafe {
        AXUIElementSetMessagingTimeout(app.as_CFTypeRef(), 0.2);
        AXUIElementSetMessagingTimeout(element.as_CFTypeRef(), 0.2);
    }
    let (mut pid, mut field_pid) = (0, 0);
    if unsafe { AXUIElementGetPid(app.as_CFTypeRef(), &mut pid) } != 0
        || unsafe { AXUIElementGetPid(element.as_CFTypeRef(), &mut field_pid) } != 0
        || pid != field_pid
        || pid == std::process::id() as i32
    {
        return Err("別アプリの入力欄で録音を開始してください。".into());
    }
    editable(&element)?;
    Ok((pid, element))
}
struct Target {
    id: u64,
    epoch: u64,
    pid: i32,
    element: CFType,
    range: Range,
    fingerprint: u64,
}
impl Target {
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
                            match Target::capture(sequence, worker.epoch.load(Ordering::Acquire)) {
                                Ok(captured) => {
                                    target = Some(captured);
                                    worker.report("armed", "録音開始時の入力先を記録しました。認識完了時に再確認します。");
                                }
                                Err(error) => worker.report("blocked", &error),
                            }
                        }
                        let _ = reply.send(target.as_ref().map(|t| t.id));
                    }
                    Command::Apply(id, value, reply) => {
                        if let Some(captured) = target.take().filter(|t| t.id == id) {
                            if value.is_empty() {
                                worker.report("blocked", "認識結果が空のため自動入力しません。");
                            } else {
                                match captured.insert(&value, &worker) {
                                Ok(()) => worker.report("inserted", "入力先へ挿入しました。再試行した認識結果は自動入力しません。"),
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
    pub fn configure(&self, enabled: bool) {
        if self.shared.enabled.swap(enabled, Ordering::AcqRel) != enabled {
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
        let _ = self.tx.send(Command::Cancel);
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
