//! ScreenCaptureKit crops only the focused editable field; Vision runs in memory.
use crate::insertion::{Range, attribute, focused, selected_range};
use core_foundation::base::{CFType, CFTypeRef, TCFType};
use objc2::{AnyThread, rc::autoreleasepool};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::CGImage;
use objc2_foundation::{NSArray, NSDictionary, NSError, NSString};
use objc2_screen_capture_kit::{
    SCContentFilter, SCScreenshotManager, SCShareableContent, SCStreamConfiguration,
};
use objc2_vision::{
    VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel,
};
use std::{ffi::c_void, sync::mpsc, time::Duration};

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXValueGetTypeID() -> usize;
    fn AXValueGetType(value: CFTypeRef) -> i32;
    fn AXValueGetValue(value: CFTypeRef, kind: i32, output: *mut c_void) -> u8;
    fn AXUIElementGetTypeID() -> usize;
}
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}
pub fn permitted() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() }
}
#[tauri::command]
pub async fn request_screen_permission() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(|| {
        if unsafe { CGRequestScreenCaptureAccess() } {
            return Ok(true);
        }
        std::process::Command::new("/usr/bin/open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
            .status()
            .map_err(|_| "画面収録の設定を開けません。".to_owned())?;
        Ok(false)
    })
    .await
    .map_err(|_| "画面収録の許可を確認できません。".to_owned())?
}
#[derive(Clone, Copy, PartialEq)]
pub struct Region {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}
impl Region {
    fn read(element: &CFType) -> Result<Self, &'static str> {
        fn pair(element: &CFType, name: &str, kind: i32) -> Result<[f64; 2], &'static str> {
            let value = attribute(element, name).map_err(|_| "入力欄の位置を確認できません。")?;
            let mut pair = [0.0; 2];
            if value.type_of() != unsafe { AXValueGetTypeID() }
                || unsafe { AXValueGetType(value.as_CFTypeRef()) } != kind
                || unsafe { AXValueGetValue(value.as_CFTypeRef(), kind, pair.as_mut_ptr().cast()) }
                    == 0
            {
                return Err("入力欄の位置を確認できません。");
            }
            Ok(pair)
        }
        let [x, y] = pair(element, "AXPosition", 1)?;
        let [w, h] = pair(element, "AXSize", 2)?;
        if ![x, y, w, h].iter().all(|v| v.is_finite())
            || !(1.0..=8192.0).contains(&w)
            || !(1.0..=8192.0).contains(&h)
        {
            return Err("撮影範囲に対応していません。");
        }
        Ok(Self { x, y, w, h })
    }
    fn same(&self, rect: CGRect) -> bool {
        (self.x - rect.origin.x).abs() < 2.0
            && (self.y - rect.origin.y).abs() < 2.0
            && (self.w - rect.size.width).abs() < 2.0
            && (self.h - rect.size.height).abs() < 2.0
    }
    fn inside(&self, outer: &Self) -> bool {
        self.x >= outer.x
            && self.y >= outer.y
            && self.x + self.w <= outer.x + outer.w + 1.0
            && self.y + self.h <= outer.y + outer.h + 1.0
    }
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFHash(value: CFTypeRef) -> usize;
}
#[derive(Clone, Copy)]
pub struct Target {
    selection: Range,
    identity: usize,
    pid: i32,
    field: Region,
    window: Region,
}
impl Target {
    pub fn capture() -> Result<Self, &'static str> {
        // The shared focus check rejects secure/readonly/non-text/own-app first.
        let (pid, field) = focused().map_err(|_| "安全な撮影対象を確認できません。")?;
        let field_bounds = Region::read(&field)?;
        let window =
            attribute(&field, "AXWindow").map_err(|_| "入力先ウィンドウを確認できません。")?;
        if window.type_of() != unsafe { AXUIElementGetTypeID() } {
            return Err("入力先ウィンドウを確認できません。");
        }
        let window_bounds = Region::read(&window)?;
        if !field_bounds.inside(&window_bounds) {
            return Err("入力欄がウィンドウ内にありません。");
        }
        Ok(Self {
            selection: selected_range(&field).map_err(|_| "入力位置を確認できません。")?,
            identity: unsafe { CFHash(field.as_CFTypeRef()) },
            pid,
            field: field_bounds,
            window: window_bounds,
        })
    }
    fn still_focused(&self) -> bool {
        Self::capture().is_ok_and(|new| {
            self.pid == new.pid
                && self.selection == new.selection
                && self.identity == new.identity
                && self.field == new.field
                && self.window == new.window
        })
    }
}
fn recognize(image: &CGImage) -> Result<String, &'static str> {
    autoreleasepool(|_| unsafe {
        let request = VNRecognizeTextRequest::new();
        request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        request.setRecognitionLanguages(&NSArray::from_retained_slice(&[
            NSString::from_str("ja-JP"),
            NSString::from_str("en-US"),
        ]));
        request.setUsesLanguageCorrection(false);
        let handler = VNImageRequestHandler::initWithCGImage_options(
            VNImageRequestHandler::alloc(),
            image,
            &NSDictionary::new(),
        );
        let requests: objc2::rc::Retained<NSArray<VNRequest>> = NSArray::from_slice(&[&request]);
        handler
            .performRequests_error(&requests)
            .map_err(|_| "ローカルOCRに失敗しました。")?;
        let mut result = String::new();
        if let Some(observations) = request.results() {
            for observation in observations.iter() {
                if let Some(best) = observation.topCandidates(1).firstObject() {
                    for c in best
                        .string()
                        .to_string()
                        .chars()
                        .filter(|c| !c.is_control())
                    {
                        if result.len() + c.len_utf8() > 4096 {
                            return Ok(result);
                        }
                        result.push(c);
                    }
                    if result.len() < 4096 {
                        result.push('\n');
                    }
                }
            }
        }
        if result.trim().is_empty() {
            Err("OCRでテキストを取得できませんでした。")
        } else {
            Ok(result.trim().into())
        }
    })
}
/// Called from a native worker; only a bounded String crosses threads. No image files.
pub fn capture_text(
    target: Target,
    active: std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<String, &'static str> {
    if !active() {
        return Err("OCRを取り消しました。");
    }
    if !permitted() {
        return Err("画面収録が未許可です。OCRなしで続行します。");
    }
    if !target.still_focused() {
        return Err("撮影前に入力先が変わりました。");
    }
    let (tx, rx) = mpsc::channel();
    let callback = block2::RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut NSError| {
            let send = tx.clone();
            let Some(content) = (unsafe { content.as_ref() }) else {
                let _ = send.send(Err("撮影対象を取得できません。"));
                return;
            };
            if !error.is_null() || !active() || !target.still_focused() {
                let _ = send.send(Err("撮影対象が変わりました。"));
                return;
            }
            unsafe {
                let windows = content.windows();
                let matches: Vec<_> = windows
                    .iter()
                    .filter(|window| {
                        window
                            .owningApplication()
                            .is_some_and(|app| app.processID() == target.pid)
                            && target.window.same(window.frame())
                    })
                    .collect();
                if matches.len() != 1 {
                    let _ = send.send(Err("入力先ウィンドウを一意に特定できません。"));
                    return;
                }
                let filter = SCContentFilter::initWithDesktopIndependentWindow(
                    SCContentFilter::alloc(),
                    &matches[0],
                );
                let config = SCStreamConfiguration::new();
                let scale = 2.0_f64
                    .min(1600.0 / target.field.w)
                    .min(900.0 / target.field.h);
                config.setWidth((target.field.w * scale).ceil() as usize);
                config.setHeight((target.field.h * scale).ceil() as usize);
                config.setSourceRect(CGRect::new(
                    CGPoint::new(
                        target.field.x - target.window.x,
                        target.field.y - target.window.y,
                    ),
                    CGSize::new(target.field.w, target.field.h),
                ));
                config.setShowsCursor(false);
                config.setCapturesAudio(false);
                config.setIgnoreShadowsSingleWindow(true);
                let active = active.clone();
                let image_callback =
                    block2::RcBlock::new(move |image: *mut CGImage, error: *mut NSError| {
                        let result = if error.is_null() && active() && target.still_focused() {
                            image
                                .as_ref()
                                .ok_or("画像を取得できません。")
                                .and_then(recognize)
                        } else {
                            Err("撮影後に入力先が変わりました。")
                        };
                        let _ = send.send(result);
                    });
                SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(
                    &filter,
                    &config,
                    Some(&image_callback),
                );
            }
        },
    );
    unsafe {
        SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(true,true,&callback);
    }
    rx.recv_timeout(Duration::from_secs(3))
        .map_err(|_| "OCRが時間内に完了しませんでした。")?
}
