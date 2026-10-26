// Research fixture only: renders its own public window; never captures other apps.
import AppKit
import ApplicationServices
import Vision

func writeJSON(_ value: [String: Any], _ url: URL) throws {
    try JSONSerialization.data(withJSONObject: value, options: [.prettyPrinted, .sortedKeys]).write(to: url)
}
let args = CommandLine.arguments
if args.count != 4 { fatalError("usage: context-evidence-native ASSET_DIR OUTPUT_DIR SCENARIO") }
let assets = URL(fileURLWithPath: args[1]), output = URL(fileURLWithPath: args[2])
let scenario = args[3]
let text = scenario == "storm" ? "気象情報。大型の台風が接近中。強い雨と風、降水量の増加に注意してください。" : "お菓子の新商品。カラフルな飴、キャンディ、砂糖菓子を紹介します。"
try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
let app = NSApplication.shared
app.setActivationPolicy(.regular)
let window = NSWindow(contentRect: NSRect(x: 80, y: 100, width: 960, height: 640), styleMask: [.titled], backing: .buffered, defer: false)
window.title = "Context experiment fixture"
let field = NSTextView(frame: NSRect(x: 30, y: 100, width: 900, height: 420))
field.font = NSFont.systemFont(ofSize: 36)
field.backgroundColor = .white; field.textColor = .black
field.string = text; field.isEditable = true; field.isRichText = false
field.setAccessibilityLabel("Experiment context")
window.contentView!.addSubview(field)
window.makeKeyAndOrderFront(nil); window.makeFirstResponder(field); app.activate(ignoringOtherApps: true)
RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.5))
func render(_ view: NSView, _ name: String) throws -> URL {
    let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds)!
    view.cacheDisplay(in: view.bounds, to: rep)
    let url = output.appendingPathComponent(name)
    try rep.representation(using: .png, properties: [:])!.write(to: url)
    return url
}
let textImage = try render(window.contentView!, "text-window.png")
// Real consumer AX call, no TCC DB writes, no prompt automation, no provider shortcut.
var value: CFTypeRef?
let own = AXUIElementCreateApplication(ProcessInfo.processInfo.processIdentifier)
let focusCode = AXUIElementCopyAttributeValue(own, kAXFocusedUIElementAttribute as CFString, &value)
var axText = "", rangeCode: AXError = .failure
if focusCode == .success, let value = value {
    let element = unsafeBitCast(value, to: AXUIElement.self)
    var range = CFRange(location: 0, length: min(256, text.utf16.count))
    if let parameter = AXValueCreate(.cfRange, &range) {
        var result: CFTypeRef?
        rangeCode = AXUIElementCopyParameterizedAttributeValue(element, kAXStringForRangeParameterizedAttribute as CFString, parameter, &result)
        if rangeCode == .success { axText = result as? String ?? "" }
    }
}
func ocr(_ url: URL) -> [String: Any] {
    let start = ProcessInfo.processInfo.systemUptime
    let request = VNRecognizeTextRequest(); request.recognitionLevel = .accurate
    request.recognitionLanguages = ["ja-JP", "en-US"]; request.usesLanguageCorrection = false
    do {
        try VNImageRequestHandler(url: url, options: [:]).perform([request])
        let lines = request.results?.compactMap { $0.topCandidates(1).first?.string } ?? []
        return ["status": "completed", "text": lines.joined(separator: "\n"), "elapsedMs": Int((ProcessInfo.processInfo.systemUptime-start)*1000)]
    } catch { return ["status": "unavailable", "reason": "vision-ocr-failed", "text": ""] }
}
field.removeFromSuperview()
let picture = NSImageView(frame: window.contentView!.bounds)
picture.image = NSImage(contentsOf: assets.appendingPathComponent(scenario + ".png"))
picture.imageScaling = .scaleProportionallyUpOrDown
window.contentView!.addSubview(picture)
RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.2))
let image = try render(window.contentView!, "image-window.png")
let start = ProcessInfo.processInfo.systemUptime
let request = VNClassifyImageRequest()
var vision: [String: Any]
do {
    try VNImageRequestHandler(url: image, options: [:]).perform([request])
    // No rain/candy lookup or manual category mapping. Forward actual top labels.
    let labels = (request.results ?? []).prefix(8).map { ["label": $0.identifier, "confidence": $0.confidence] as [String: Any] }
    vision = ["status": "completed", "labels": labels, "elapsedMs": Int((ProcessInfo.processInfo.systemUptime-start)*1000)]
} catch { vision = ["status": "unavailable", "reason": "vision-classification-failed", "labels": []] }
try writeJSON(["scenario": scenario, "fixtureText": text,
    "renderMethod": "AppKit view bitmap; not ScreenCaptureKit; no screen-recording permission tested",
    "ax": ["status": axText.isEmpty ? "unavailable" : "completed", "text": axText,
           "trusted": AXIsProcessTrusted(), "focusCode": focusCode.rawValue, "rangeCode": rangeCode.rawValue],
    "ocr": ocr(textImage), "imageOCR": ocr(image), "vision": vision], output.appendingPathComponent("capture.json"))
window.orderOut(nil)
