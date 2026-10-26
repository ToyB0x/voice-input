//! An owned non-activating AppKit panel. No secondary renderer or IPC surface.
use crate::overlay_state::Status;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSFloatingWindowLevel, NSFont, NSPanel, NSProgressIndicator,
    NSScreen, NSTextField, NSView, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use std::cell::RefCell;

define_class!(
    // SAFETY: this is our own NSPanel subclass, initialized through NSPanel's
    // designated initializer. No foreign class/ivars are modified.
    #[unsafe(super = NSPanel)]
    #[thread_kind = MainThreadOnly]
    #[ivars = ()]
    struct StatusPanel;
    unsafe impl NSObjectProtocol for StatusPanel {}
    impl StatusPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self)->bool {false}
        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main(&self)->bool {false}
    }
);
define_class!(
    // SAFETY: an owned NSView subclass; drawRect is called with AppKit's drawing context.
    #[unsafe(super = NSView)]
    #[thread_kind = MainThreadOnly]
    #[ivars = ()]
    struct StatusView;
    unsafe impl NSObjectProtocol for StatusView {}
    impl StatusView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self,_rect:NSRect) {
            NSColor::colorWithSRGBRed_green_blue_alpha(0.094,0.137,0.2,1.).setFill();
            objc2_app_kit::NSRectFill(self.bounds());
        }
    }
);
struct Panel {
    window: Retained<StatusPanel>,
    root: Retained<StatusView>,
    phase: Retained<NSTextField>,
    timing: Retained<NSTextField>,
    meter: Retained<NSProgressIndicator>,
    last_message: String,
    last_time: String,
}
thread_local! {static PANEL:RefCell<Option<Panel>>=const{RefCell::new(None)};}
fn with_panel<T>(work: impl FnOnce(&mut Panel) -> Result<T, String>) -> Result<T, String> {
    let mtm = MainThreadMarker::new().ok_or("overlay requires AppKit main thread")?;
    PANEL.with(|stored| {
        let mut stored = stored.borrow_mut();
        if stored.is_none() {
            *stored = Some(Panel::new(mtm));
        }
        work(stored.as_mut().ok_or("overlay unavailable")?)
    })
}
impl Panel {
    fn new(mtm: MainThreadMarker) -> Self {
        // SAFETY: only main-thread-owned objects are created here; views are
        // retained by this struct and by the panel's content hierarchy. The panel
        // is not auto-released on close and never becomes key/main.
        unsafe {
            let this = StatusPanel::alloc(mtm).set_ivars(());
            let window: Retained<StatusPanel> = msg_send![super(this),initWithContentRect:NSRect::new(NSPoint::new(0.,0.),NSSize::new(400.,144.)),styleMask:NSWindowStyleMask::Borderless|NSWindowStyleMask::NonactivatingPanel,backing:NSBackingStoreType::Buffered,defer:false];
            window.setReleasedWhenClosed(false);
            window.setIgnoresMouseEvents(true);
            window.setFloatingPanel(true);
            window.setHidesOnDeactivate(false);
            window.setLevel(NSFloatingWindowLevel);
            window.setCollectionBehavior(
                NSWindowCollectionBehavior::CanJoinAllSpaces
                    | NSWindowCollectionBehavior::FullScreenAuxiliary,
            );
            window.setBackgroundColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(
                0.094, 0.137, 0.2, 1.,
            )));
            window.setHasShadow(true);
            window.setTitle(&NSString::from_str("Voice Input Status"));
            let root: Retained<StatusView> = msg_send![super(StatusView::alloc(mtm).set_ivars(())),initWithFrame:NSRect::new(NSPoint::new(0.,0.),NSSize::new(400.,144.))];
            fn label(
                mtm: MainThreadMarker,
                text: &str,
                y: f64,
                size: f64,
            ) -> Retained<NSTextField> {
                let field = NSTextField::labelWithString(&NSString::from_str(text), mtm);
                field.setFrame(NSRect::new(NSPoint::new(16., y), NSSize::new(368., 20.)));
                field.setFont(Some(&NSFont::systemFontOfSize(size)));
                field.setTextColor(Some(&NSColor::whiteColor()));
                field
            }
            let brand = label(mtm, "VOICE INPUT", 116., 10.);
            let phase = label(mtm, "", 88., 13.);
            phase.setFont(Some(&NSFont::boldSystemFontOfSize(13.)));
            let timing = label(mtm, "", 64., 11.);
            let hint = label(mtm, "操作・取り消しはショートカットか設定画面から", 8., 10.);
            hint.setTextColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(
                0.65, 0.73, 0.82, 1.,
            )));
            let meter = NSProgressIndicator::initWithFrame(
                NSProgressIndicator::alloc(mtm),
                NSRect::new(NSPoint::new(16., 40.), NSSize::new(368., 10.)),
            );
            meter.setIndeterminate(false);
            meter.setMinValue(0.);
            meter.setMaxValue(100.);
            meter.setDoubleValue(0.);
            root.addSubview(&brand);
            root.addSubview(&phase);
            root.addSubview(&timing);
            root.addSubview(&hint);
            root.addSubview(&meter);
            window.setContentView(Some(&root));
            Self {
                window,
                root,
                phase,
                timing,
                meter,
                last_message: String::new(),
                last_time: String::new(),
            }
        }
    }
    fn position(&self) -> Result<(), String> {
        let screen = NSScreen::screens(self.window.mtm())
            .firstObject()
            .ok_or("primary screen unavailable")?;
        let area = screen.visibleFrame();
        self.window.setFrameOrigin(NSPoint::new(
            area.origin.x + ((area.size.width - 400.).max(0.) / 2.),
            area.origin.y + 24.,
        ));
        Ok(())
    }
}
pub(crate) struct Properties {
    pub visible: bool,
    pub key: bool,
    pub can_key: bool,
    pub can_main: bool,
    pub ignores_mouse: bool,
    pub floating: bool,
    pub non_activating: bool,
}
pub(crate) fn properties() -> Result<Properties, String> {
    with_panel(|panel| {
        Ok(Properties {
            visible: panel.window.isVisible(),
            key: panel.window.isKeyWindow(),
            can_key: panel.window.canBecomeKeyWindow(),
            can_main: panel.window.canBecomeMainWindow(),
            ignores_mouse: panel.window.ignoresMouseEvents(),
            floating: panel.window.isFloatingPanel(),
            non_activating: panel
                .window
                .styleMask()
                .contains(NSWindowStyleMask::NonactivatingPanel),
        })
    })
}
pub(crate) fn initialize() -> Result<(), String> {
    let p = properties()?;
    if p.visible
        || p.key
        || p.can_key
        || p.can_main
        || !p.ignores_mouse
        || !p.floating
        || !p.non_activating
    {
        return Err("invalid passive overlay properties".into());
    }
    Ok(())
}
pub(crate) fn render(status: &Status) -> Result<(), String> {
    with_panel(|panel| {
        if !status.visible {
            panel.window.orderOut(None);
            return Ok(());
        }
        if panel.last_message != status.message {
            panel
                .phase
                .setStringValue(&NSString::from_str(status.message));
            panel.last_message = status.message.into();
        }
        let extra = match status.phase {
            "correcting" => format!(" / 補正 {:.1}秒", status.stage_ms as f64 / 1000.),
            "recognizing" => format!(" / 認識 {:.1}秒", status.stage_ms as f64 / 1000.),
            _ => String::new(),
        };
        let timing = format!("録音 {:.1}秒{extra}", status.recording_ms as f64 / 1000.);
        if panel.last_time != timing {
            panel.timing.setStringValue(&NSString::from_str(&timing));
            panel.last_time = timing;
        }
        panel
            .meter
            .setDoubleValue(crate::overlay_state::level_percent(
                if status.phase == "recording" {
                    status.level
                } else {
                    0.
                },
            ));
        if !panel.window.isVisible() {
            panel.position()?;
            panel.window.orderFront(None);
        }
        panel.root.displayIfNeeded();
        Ok(())
    })
}
pub(crate) fn hide() -> Result<(), String> {
    with_panel(|panel| {
        panel.window.orderOut(None);
        Ok(())
    })
}
// CI-only helper for this owned view, never exposed as a command or user-data export.
#[cfg(debug_assertions)]
#[allow(dead_code)]
pub(crate) fn capture(path: &std::path::Path) -> Result<(), String> {
    use objc2_app_kit::NSBitmapImageFileType;
    use objc2_foundation::NSDictionary;
    with_panel(|panel| {
        let bounds = panel.root.bounds();
        let bitmap = panel
            .root
            .bitmapImageRepForCachingDisplayInRect(bounds)
            .ok_or("overlay bitmap unavailable")?;
        panel
            .root
            .cacheDisplayInRect_toBitmapImageRep(bounds, &bitmap);
        // SAFETY: an empty dictionary provides no untyped image properties.
        let data = unsafe {
            bitmap.representationUsingType_properties(
                NSBitmapImageFileType::PNG,
                &NSDictionary::new(),
            )
        }
        .ok_or("overlay PNG unavailable")?;
        if !data.writeToFile_atomically(&NSString::from_str(&path.to_string_lossy()), true) {
            return Err("overlay PNG write failed".into());
        }
        Ok(())
    })
}
