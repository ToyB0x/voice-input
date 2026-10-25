//! Read only a bounded range of the focused editable field, on the recorder thread.
//! The vault is native-only; IPC exposes a generic message and never field text.
use crate::insertion::{Range, attribute, focused, selected_range};
use core_foundation::{
    base::{CFType, CFTypeRef, TCFType},
    number::CFNumber,
    string::{CFString, CFStringRef},
};
use serde::Serialize;
use std::{
    ffi::c_void,
    sync::{Arc, Mutex},
};

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXValueCreate(kind: i32, value: *const c_void) -> CFTypeRef;
    fn AXUIElementCopyParameterizedAttributeValue(
        element: CFTypeRef,
        attribute: CFStringRef,
        parameter: CFTypeRef,
        value: *mut CFTypeRef,
    ) -> i32;
}

fn read_near_cursor() -> Result<String, &'static str> {
    // focused() rejects own app, secure/non-text and non-editable fields BEFORE
    // any input value read. No parent/window traversal and no AXValue fallback.
    let (_, element) =
        focused().map_err(|_| "入力欄またはアクセシビリティの許可を確認できません。")?;
    let selection = selected_range(&element).map_err(|_| "入力位置を確認できません。")?;
    let total = attribute(&element, "AXNumberOfCharacters")
        .ok()
        .and_then(|n| n.downcast::<CFNumber>())
        .and_then(|n| n.to_i64())
        .filter(|n| *n >= 0)
        .ok_or("文字数を確認できません。")?;
    if selection.location as i64 > total
        || selection.length as i64 > total - selection.location as i64
    {
        return Err("入力位置が変わりました。");
    }
    // At most 256 UTF-16 code units before and after the selection START.
    // Large selections do not expand the read. Even huge documents stay bounded.
    let start = (selection.location as i64 - 256).max(0);
    let end = (selection.location as i64).saturating_add(256).min(total);
    if end == start {
        return Err("入力欄が空です。");
    }
    let range = Range {
        location: start as isize,
        length: (end - start) as isize,
    };
    let parameter = unsafe { AXValueCreate(4, (&range as *const Range).cast()) };
    if parameter.is_null() {
        return Err("取得範囲を準備できません。");
    }
    let parameter = unsafe { CFType::wrap_under_create_rule(parameter) };
    let mut value = std::ptr::null();
    let code = unsafe {
        AXUIElementCopyParameterizedAttributeValue(
            element.as_CFTypeRef(),
            CFString::new("AXStringForRange").as_concrete_TypeRef(),
            parameter.as_CFTypeRef(),
            &mut value,
        )
    };
    if code != 0 || value.is_null() {
        return Err("この入力欄は範囲取得に対応していません。");
    }
    let value = unsafe { CFType::wrap_under_create_rule(value) };
    let value = value
        .downcast::<CFString>()
        .ok_or("テキストを取得できません。")?;
    // Reject misbehaving apps before converting their response to a Rust String.
    if value.char_len() > 512 {
        return Err("取得範囲が上限を超えました。");
    }
    // If focus/selection changed during IPC, do not use a different field's text.
    let (_, current) = focused().map_err(|_| "入力先が変わりました。")?;
    if current != element || selected_range(&current).ok() != Some(selection) {
        return Err("入力先または位置が変わりました。");
    }
    let text = local_transcription::recognition_hint(&value.to_string());
    if text.is_empty() {
        return Err("利用できるテキストがありません。");
    }
    Ok(text)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Config {
    pub ax: bool,
    pub ocr: bool,
    pub whisper: bool,
    pub correction: bool,
    pub comparison: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            ax: false,
            ocr: false,
            whisper: true,
            correction: false,
            comparison: false,
        }
    }
}
impl Config {
    fn enabled(self) -> bool {
        (self.ax || self.ocr) && (self.whisper || self.correction || self.comparison)
    }
}
#[derive(Default)]
struct Evidence {
    ax: String,
    ocr: String,
    pending: bool,
    sealed: bool,
}
struct Inner {
    config: Config,
    epoch: u64,
    evidence: Option<Evidence>,
    message: String,
    ocr_message: String,
}
#[derive(Clone)]
pub struct Reader(Arc<(Mutex<Inner>, std::sync::Condvar)>);
struct Lease {
    epoch: u64,
    reader: Reader,
}
#[derive(Clone, Default)]
pub(crate) struct Sources {
    pub ax: String,
    pub ocr: String,
}
impl Sources {
    pub fn for_source(&self, source: &str) -> Option<String> {
        match source {
            "none" => Some(String::new()),
            "ax" if !self.ax.is_empty() => Some(self.ax.clone()),
            "ocr" if !self.ocr.is_empty() => Some(self.ocr.clone()),
            "both" if !self.ax.is_empty() && !self.ocr.is_empty() => Some(format!(
                "入力欄: {}\n画面OCR: {}",
                bounded(&self.ax, 350),
                bounded(&self.ocr, 350)
            )),
            _ => None,
        }
    }
}
/// Opaque shared ownership for one recording. No source text enters IPC.
#[derive(Clone)]
pub(crate) struct Hint(Arc<Lease>);
fn bounded(text: &str, bytes: usize) -> String {
    let mut end = text.len().min(bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}
impl Hint {
    fn text(&self, correction: bool) -> Option<String> {
        let (lock, condition) = &*self.0.reader.0;
        let mut state = lock.lock().ok()?;
        if state.epoch != self.0.epoch
            || !state.config.enabled()
            || !(if correction {
                state.config.correction
            } else {
                state.config.whisper
            })
        {
            return None;
        }
        if state
            .evidence
            .as_ref()
            .is_some_and(|e| e.pending && !e.sealed)
        {
            state = condition
                .wait_timeout_while(state, std::time::Duration::from_millis(3200), |s| {
                    s.epoch == self.0.epoch
                        && s.evidence.as_ref().is_some_and(|e| e.pending && !e.sealed)
                })
                .ok()?
                .0;
        }
        if state.epoch != self.0.epoch {
            return None;
        }
        let evidence = state.evidence.as_mut()?;
        // Freeze once: Whisper and correction see the same captured evidence.
        let timed_out = evidence.pending;
        evidence.pending = false;
        evidence.sealed = true;
        let combined = match (evidence.ax.is_empty(), evidence.ocr.is_empty()) {
            (false, false) => format!(
                "入力欄: {}\n画面OCR: {}",
                bounded(&evidence.ax, 350),
                bounded(&evidence.ocr, 350)
            ),
            (false, true) => evidence.ax.clone(),
            (true, false) => bounded(&evidence.ocr, 768),
            (true, true) => String::new(),
        };
        if timed_out {
            state.ocr_message = "OCR: 時間内に取得できず、OCRなしで続行します。".into();
        }
        if combined.is_empty() {
            return None;
        }
        state.message =
            "認識補助: 今回の認識にローカルヒントを使用しました。再試行はヒントなしです。".into();
        Some(combined)
    }
    pub(crate) fn comparison_sources(self) -> Sources {
        let (lock, condition) = &*self.0.reader.0;
        let Ok(mut state) = lock.lock() else {
            return Sources::default();
        };
        if state.epoch != self.0.epoch || !state.config.comparison {
            return Sources::default();
        }
        if state
            .evidence
            .as_ref()
            .is_some_and(|e| e.pending && !e.sealed)
        {
            let Ok(waited) =
                condition.wait_timeout_while(state, std::time::Duration::from_millis(3200), |s| {
                    s.epoch == self.0.epoch
                        && s.evidence.as_ref().is_some_and(|e| e.pending && !e.sealed)
                })
            else {
                return Sources::default();
            };
            state = waited.0;
        }
        if state.epoch != self.0.epoch || !state.config.comparison {
            return Sources::default();
        }
        let Some(evidence) = state.evidence.as_mut() else {
            return Sources::default();
        };
        evidence.pending = false;
        evidence.sealed = true;
        Sources {
            ax: bounded(&evidence.ax, 768),
            ocr: bounded(&evidence.ocr, 768),
        }
    }
    pub fn take(self) -> Option<String> {
        self.text(false)
    }
    pub fn correction(self) -> Option<String> {
        self.text(true)
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        if let Ok(mut state) = self.reader.0.0.lock()
            && state.epoch == self.epoch
            && state.evidence.take().is_some()
        {
            if state.ocr_message.contains("処理中") {
                state.ocr_message = "OCR: 取得を取り消しました。".into();
            }
            // Keep a completed delivery message while destroying the source.
            if state.message.contains("取得済み") {
                state.message = "認識補助: ヒントを破棄しました。再試行はヒントなしです。".into();
            }
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    enabled: bool,
    message: String,
    ocr_message: String,
    screen_permitted: bool,
}
impl Reader {
    pub fn new() -> Self {
        Self(Arc::new((
            Mutex::new(Inner {
                config: Config::default(),
                epoch: 0,
                evidence: None,
                message: "認識補助: 無効".into(),
                ocr_message: "OCR: 無効".into(),
            }),
            std::sync::Condvar::new(),
        )))
    }
    pub fn configure(&self, config: Config) {
        if let Ok(mut state) = self.0.0.lock() {
            if state.config == config {
                return;
            }
            state.config = config;
            state.epoch += 1;
            state.evidence = None;
            state.message = if config.enabled() {
                "認識補助: 有効。次の録音で入力欄を確認します。"
            } else {
                "認識補助: 無効"
            }
            .into();
            state.ocr_message = if config.ocr {
                "OCR: 有効。録音開始時の入力欄だけを撮影します。"
            } else {
                "OCR: 無効"
            }
            .into();
            self.0.1.notify_all();
        }
    }
    pub(crate) fn set_comparison(&self, enabled: bool) {
        let config = self
            .0
            .0
            .lock()
            .map(|state| Config {
                comparison: enabled,
                ..state.config
            })
            .unwrap_or_default();
        self.configure(config);
    }
    pub fn cancel(&self) {
        if let Ok(mut state) = self.0.0.lock() {
            state.epoch += 1;
            state.evidence = None;
            state.message = if state.config.enabled() {
                "認識補助: 次の録音で入力欄を確認します。"
            } else {
                "認識補助: 無効"
            }
            .into();
            state.ocr_message = if state.config.ocr {
                "OCR: 次の録音で確認します。"
            } else {
                "OCR: 無効"
            }
            .into();
            self.0.1.notify_all();
        }
    }
    pub fn retain_for_correction(&self) -> bool {
        self.0.0.lock().is_ok_and(|s| s.config.correction)
    }
    pub fn capture(&self) -> Option<Hint> {
        let (epoch, config) = {
            let state = self.0.0.lock().ok()?;
            if !state.config.enabled() {
                return None;
            }
            (state.epoch, state.config)
        };
        let ax = if config.ax {
            read_near_cursor()
        } else {
            Err("Accessibility取得は無効です。")
        };
        let target = if config.ocr {
            crate::ocr::Target::capture()
        } else {
            Err("OCRは無効です。")
        };
        let mut state = self.0.0.lock().ok()?;
        if state.epoch != epoch {
            return None;
        }
        let ax = ax.ok().unwrap_or_default();
        let pending = config.ocr && target.is_ok();
        state.message = if ax.is_empty() {
            "認識補助: ヒントなしで続行。入力欄またはアクセシビリティの許可を確認できません。"
                .into()
        } else {
            format!(
                "認識補助: 取得済み（{}文字）。この録音に一度だけ使用します。",
                ax.chars().count()
            )
        };
        state.ocr_message = if !config.ocr {
            "OCR: 無効".into()
        } else if pending {
            "OCR: 処理中（画像はメモリ内のみ）".into()
        } else {
            "OCR: 安全な撮影対象がないため取得しません。".into()
        };
        if ax.is_empty() && !pending {
            return None;
        }
        state.evidence = Some(Evidence {
            ax,
            pending,
            ..Evidence::default()
        });
        drop(state);
        if let Ok(target) = target {
            let reader = self.clone();
            std::thread::spawn(move || {
                let guard_reader = reader.clone();
                let active: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || {
                    guard_reader.0.0.lock().is_ok_and(|state| {
                        state.epoch == epoch
                            && state.config.ocr
                            && state
                                .evidence
                                .as_ref()
                                .is_some_and(|evidence| !evidence.sealed)
                    })
                });
                let result = crate::ocr::capture_text(target, active);
                if let Ok(mut state) = reader.0.0.lock()
                    && state.epoch == epoch
                {
                    if let Some(evidence) = state.evidence.as_mut()
                        && !evidence.sealed
                    {
                        evidence.pending = false;
                        let message = match result {
                            Ok(text) => {
                                let count = text.chars().count();
                                evidence.ocr = text;
                                format!("OCR: 取得済み（{count}文字）。画像は保存しません。")
                            }
                            Err(reason) => format!("OCR: 取得なし。{reason}"),
                        };
                        state.ocr_message = message;
                    }
                    reader.0.1.notify_all();
                }
            });
        }
        Some(Hint(Arc::new(Lease {
            epoch,
            reader: self.clone(),
        })))
    }
    fn status(&self) -> Status {
        let state = self.0.0.lock().unwrap_or_else(|e| e.into_inner());
        Status {
            enabled: state.config.enabled(),
            message: state.message.clone(),
            ocr_message: state.ocr_message.clone(),
            screen_permitted: crate::ocr::permitted(),
        }
    }
}
#[tauri::command]
pub fn context_status(reader: tauri::State<'_, Reader>) -> Status {
    reader.status()
}
#[cfg(test)]
mod tests {
    use super::*;
    fn seed(reader: &Reader) -> Hint {
        let mut state = reader.0.0.lock().unwrap();
        state.evidence = Some(Evidence {
            ax: "private vocabulary".into(),
            ..Evidence::default()
        });
        Hint(Arc::new(Lease {
            epoch: state.epoch,
            reader: reader.clone(),
        }))
    }
    fn enabled() -> Config {
        Config {
            ax: true,
            correction: true,
            comparison: false,
            ..Config::default()
        }
    }
    #[test]
    fn shared_evidence_is_released_after_both_consumers() {
        let reader = Reader::new();
        reader.configure(enabled());
        let asr = seed(&reader);
        let correction = asr.clone();
        assert_eq!(asr.take().as_deref(), Some("private vocabulary"));
        assert!(reader.0.0.lock().unwrap().evidence.is_some());
        assert_eq!(
            correction.correction().as_deref(),
            Some("private vocabulary")
        );
        assert!(reader.0.0.lock().unwrap().evidence.is_none());
    }
    #[test]
    fn old_jobs_cannot_erase_a_new_recording_and_disabled_paths_do_no_reads() {
        let reader = Reader::new();
        assert!(reader.capture().is_none());
        reader.configure(enabled());
        let old = seed(&reader);
        reader.cancel();
        let new = seed(&reader);
        drop(old);
        assert!(new.take().is_some());
        assert!(reader.0.0.lock().unwrap().evidence.is_none());
        let old = seed(&reader);
        reader.configure(Config::default());
        reader.configure(enabled());
        assert!(old.correction().is_none());
    }
    #[test]
    fn routing_is_independent() {
        for whisper in [false, true] {
            for correction in [false, true] {
                let reader = Reader::new();
                reader.configure(Config {
                    ax: true,
                    whisper,
                    correction,
                    ocr: false,
                });
                let asr = seed(&reader);
                let final_stage = asr.clone();
                assert_eq!(asr.take().is_some(), whisper);
                assert_eq!(final_stage.correction().is_some(), correction);
                assert!(reader.0.0.lock().unwrap().evidence.is_none());
            }
        }
    }
}
