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

#[derive(Default)]
struct Inner {
    enabled: bool,
    epoch: u64,
    hint: Option<String>,
    message: String,
}
#[derive(Clone)]
pub struct Reader(Arc<Mutex<Inner>>);
/// Only an opaque token enters the audio capture/job. It is never serialized.
pub(crate) struct Hint {
    epoch: u64,
    reader: Reader,
}
impl Hint {
    pub fn take(self) -> Option<String> {
        let mut state = self.reader.0.lock().ok()?;
        if !state.enabled || state.epoch != self.epoch {
            return None;
        }
        let text = state.hint.take();
        if text.is_some() {
            state.message =
                "認識補助: 今回の認識にローカルヒントを使用しました。再試行はヒントなしです。"
                    .into();
        }
        text
    }
}
#[derive(Serialize)]
pub struct Status {
    enabled: bool,
    message: String,
}
impl Reader {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(Inner {
            message: "認識補助: 無効".into(),
            ..Inner::default()
        })))
    }
    pub fn configure(&self, enabled: bool) {
        if let Ok(mut state) = self.0.lock() {
            if state.enabled == enabled {
                return;
            }
            state.enabled = enabled;
            state.epoch += 1;
            state.hint = None;
            state.message = if enabled {
                "認識補助: 有効。次の録音で入力欄を確認します。"
            } else {
                "認識補助: 無効"
            }
            .into();
        }
    }
    pub fn cancel(&self) {
        if let Ok(mut state) = self.0.lock() {
            state.epoch += 1;
            state.hint = None;
            state.message = if state.enabled {
                "認識補助: 次の録音で入力欄を確認します。"
            } else {
                "認識補助: 無効"
            }
            .into();
        }
    }
    pub fn capture(&self) -> Option<Hint> {
        let epoch = {
            let state = self.0.lock().ok()?;
            // No AX calls, including trust checks, when disabled.
            if !state.enabled {
                return None;
            }
            state.epoch
        };
        let result = read_near_cursor();
        let mut state = self.0.lock().ok()?;
        if !state.enabled || state.epoch != epoch {
            return None;
        }
        match result {
            Ok(text) => {
                state.message = format!(
                    "認識補助: 取得済み（{}文字）。この録音に一度だけ使用します。",
                    text.chars().count()
                );
                state.hint = Some(text);
                Some(Hint {
                    epoch,
                    reader: self.clone(),
                })
            }
            Err(reason) => {
                state.message = format!("認識補助: ヒントなしで続行。{reason}");
                None
            }
        }
    }
    fn status(&self) -> Status {
        let state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        Status {
            enabled: state.enabled,
            message: state.message.clone(),
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
        let mut state = reader.0.lock().unwrap();
        state.hint = Some("private vocabulary".into());
        Hint {
            epoch: state.epoch,
            reader: reader.clone(),
        }
    }
    #[test]
    fn cancellation_and_setting_changes_destroy_pending_text() {
        let reader = Reader::new();
        assert!(reader.capture().is_none()); // disabled path performs no AX calls
        reader.configure(true);
        let hint = seed(&reader);
        reader.configure(false);
        reader.configure(true);
        assert!(reader.0.lock().unwrap().hint.is_none());
        assert!(hint.take().is_none());
        let hint = seed(&reader);
        reader.cancel();
        assert!(reader.0.lock().unwrap().hint.is_none());
        assert!(hint.take().is_none());
        let hint = seed(&reader);
        assert_eq!(hint.take().as_deref(), Some("private vocabulary"));
        assert!(reader.0.lock().unwrap().hint.is_none());
    }
}
