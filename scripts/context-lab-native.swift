// Opt-in controlled fixture. No screen capture or access to other applications.
import AppKit
import Vision

func emit(_ value: [String: Any]) {
    guard let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]), data.count <= 32768 else { exit(2) }
    FileHandle.standardOutput.write(data); FileHandle.standardOutput.write(Data("\n".utf8))
}
func finish(_ reason: String) -> Never { emit(["error": reason]); exit(1) }
let args = CommandLine.arguments
if args.count != 3 || !["storm", "candy"].contains(args[2]) { finish("invalid-arguments") }
let scene = args[2], assets = URL(fileURLWithPath: args[1])
let text = scene == "storm" ? "気象情報。大型の台風が接近中。強い雨と風、降水量の増加に注意してください。" : "お菓子の新商品。カラフルな飴、キャンディ、砂糖菓子を紹介します。"
let app = NSApplication.shared
app.setActivationPolicy(.regular)
let window = NSWindow(contentRect: NSRect(x: 100, y: 120, width: 960, height: 640), styleMask: [.titled], backing: .buffered, defer: false)
window.title = "Voice Input — 同梱サンプルの文脈取得"
window.contentView!.wantsLayer = true; window.contentView!.layer?.backgroundColor = NSColor.white.cgColor
let field = NSTextView(frame: NSRect(x: 30, y: 100, width: 900, height: 420))
field.font = NSFont.systemFont(ofSize: 36); field.backgroundColor = .white; field.textColor = .black
field.string = text; field.isEditable = true; field.isRichText = false
field.setAccessibilityLabel("Voice Input controlled context")
window.contentView!.addSubview(field)
func render(_ view: NSView) -> CGImage? {
    guard let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { return nil }
    view.cacheDisplay(in: view.bounds, to: rep); return rep.cgImage
}
func ocr(_ image: CGImage) -> [String: Any] {
    let start = ProcessInfo.processInfo.systemUptime
    var attempts = [[String: Any]]()
    for cpu in [false, true] {
        let request = VNRecognizeTextRequest(); request.recognitionLevel = .accurate
        request.recognitionLanguages = ["ja-JP", "en-US"]; request.usesLanguageCorrection = false; request.usesCPUOnly = cpu
        do {
            try VNImageRequestHandler(cgImage: image, options: [:]).perform([request])
            let lines = request.results?.compactMap { $0.topCandidates(1).first?.string } ?? []
            let result = lines.joined(separator: "\n")
            guard result.utf8.count <= 6144 else { return ["status": "unavailable", "text": "", "reason": "ocr-output-limit"] }
            attempts.append(["cpuOnly": cpu, "status": "completed"])
            return ["status": "completed", "text": result, "attempts": attempts, "elapsedMs": Int((ProcessInfo.processInfo.systemUptime-start)*1000)]
        } catch {
            let error = error as NSError
            attempts.append(["cpuOnly": cpu, "status": "failed", "code": error.code])
        }
    }
    return ["status": "unavailable", "text": "", "reason": "vision-ocr-failed", "attempts": attempts]
}
func captureAndFinish() {
guard let textImage = render(window.contentView!) else { finish("fixture-render-failed") }
let textOCR = ocr(textImage)
field.removeFromSuperview()
let imageView = NSImageView(frame: window.contentView!.bounds)
guard let picture = NSImage(contentsOf: assets.appendingPathComponent(scene+".png")) else { finish("fixture-image-missing") }
imageView.image = picture; imageView.imageScaling = .scaleProportionallyUpOrDown
window.contentView!.addSubview(imageView)
RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.1))
guard let image = render(window.contentView!) else { finish("fixture-render-failed") }
let started = ProcessInfo.processInfo.systemUptime
let request = VNClassifyImageRequest()
var vision: [String: Any]
do {
    try VNImageRequestHandler(cgImage: image, options: [:]).perform([request])
    let labels = (request.results ?? []).prefix(8).map { ["label": $0.identifier, "confidence": $0.confidence] as [String: Any] }
    vision = ["status": "completed", "labels": labels, "elapsedMs": Int((ProcessInfo.processInfo.systemUptime-started)*1000)]
} catch { vision = ["status": "unavailable", "labels": [], "reason": "vision-classification-failed"] }
emit(["scene": scene, "fixtureText": text, "ocr": textOCR, "imageOCR": ocr(image), "vision": vision,
      "renderMethod": "Controlled AppKit window bitmap; no ScreenCaptureKit or other app capture"])
window.orderOut(nil)

    exit(0)
}

// A real NSApplication run loop serves cross-process AX; a bare RunLoop did
// not register/respond reliably on the hosted macOS 27 fixture.
final class FixtureDelegate: NSObject, NSApplicationDelegate {
    var timeout: Timer?
    func applicationDidFinishLaunching(_ notification: Notification) {
        window.makeKeyAndOrderFront(nil); window.makeFirstResponder(field)
        app.activate(ignoringOtherApps: true)
        timeout = Timer.scheduledTimer(withTimeInterval: 15, repeats: false) { _ in finish("capture-handshake-timeout") }
        DispatchQueue.main.asyncAfter(deadline: .now()+0.3) {
            emit(["phase": "ready", "protocolVersion": 1])
            DispatchQueue.global().async {
                var bytes = Data()
                while let chunk = try? FileHandle.standardInput.read(upToCount: 1), !chunk.isEmpty, bytes.count <= 128 {
                    bytes.append(chunk); if chunk.first == 10 { break }
                }
                let valid = bytes == Data("continue\n".utf8)
                DispatchQueue.main.async {
                    self.timeout?.invalidate()
                    if !valid { finish("invalid-acknowledgement") }
                    captureAndFinish()
                }
            }
        }
    }
}
let delegate = FixtureDelegate()
app.delegate = delegate
app.run()
