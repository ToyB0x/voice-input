// Opt-in controlled fixture. No screen capture or access to other applications.
import AppKit
import Vision
import CoreFoundation

func emit(_ value: [String: Any]) {
    guard let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]), data.count <= 32768 else { exit(2) }
    FileHandle.standardOutput.write(data); FileHandle.standardOutput.write(Data("\n".utf8))
}
func finish(_ reason: String) -> Never { emit(["error": reason]); exit(1) }
// Japanese locale token readings, not generic Any-Latin (which can read kanji as Chinese).
// Only Japanese tokens change; numbers, Latin identifiers and separators remain byte-for-byte.
func japaneseReading(_ text: String) -> String? {
    let ns = text as NSString
    // NSLocale and CFLocale are toll-free bridged. Avoid SDK 27's newly typed
    // CFLocaleIdentifier constructor while passing an explicit Japanese locale.
    let locale = unsafeBitCast(NSLocale(localeIdentifier: "ja_JP"), to: CFLocale.self)
    let tokenizer = CFStringTokenizerCreate(kCFAllocatorDefault, text as CFString,
        CFRange(location: 0, length: ns.length), kCFStringTokenizerUnitWord, locale)!
    var kind = CFStringTokenizerGoToTokenAtIndex(tokenizer, 0)
    var end = 0, output = ""
    while kind.rawValue != 0 {
        let range = CFStringTokenizerGetCurrentTokenRange(tokenizer)
        guard range.location >= end, range.length > 0, range.location + range.length <= ns.length else { return nil }
        output += ns.substring(with: NSRange(location: end, length: range.location-end))
        let token = ns.substring(with: NSRange(location: range.location, length: range.length))
        let japanese = token.unicodeScalars.contains { (0x3040...0x30ff).contains($0.value) || (0x3400...0x9fff).contains($0.value) }
        let identifier = token.unicodeScalars.contains { CharacterSet.latinLettersForReading.contains($0) || (0x30...0x39).contains($0.value) }
        if japanese && !identifier, let latin = CFStringTokenizerCopyCurrentTokenAttribute(tokenizer, kCFStringTokenizerAttributeLatinTranscription) as? String,
           let kana = latin.applyingTransform(.latinToHiragana, reverse: false),
           !kana.unicodeScalars.contains(where: { CharacterSet.latinLettersForReading.contains($0) }) {
            output += kana
        } else { output += token }
        end = range.location + range.length
        kind = CFStringTokenizerAdvanceToNextToken(tokenizer)
    }
    output += ns.substring(from: end)
    return output
}
extension CharacterSet {
    static let latinLettersForReading = CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ")
}
let readingArgs = Array(CommandLine.arguments.dropFirst())
if readingArgs == ["--reading"] {
    let data = FileHandle.standardInput.readData(ofLength: 24577)
    guard data.count <= 24576, let input = try? JSONSerialization.jsonObject(with: data) as? [String: String],
          let text = input["text"], text.utf8.count <= 8192, !text.contains("\0"),
          let reading = japaneseReading(text), reading.utf8.count <= 8192 else { finish("reading-input-failed") }
    emit(["text": reading, "method": "CFStringTokenizer-ja_JP-latinToHiragana-v1"])
    exit(0)
}
if readingArgs == ["--reading-self-test"] {
    guard japaneseReading("雨が降っています") == "あめがふっています",
          japaneseReading("飴が降っています") == "あめがふっています",
          japaneseReading("会議室Bで10人")?.contains("B") == true,
          japaneseReading("会議室Bで10人")?.contains("10") == true else { finish("reading-self-test-failed") }
    emit(["status": "passed", "method": "CFStringTokenizer-ja_JP-latinToHiragana-v1", "inferenceTested": false]); exit(0)
}
let args = CommandLine.arguments
if args.count != 3 || !["storm", "candy", "rainfall", "candy-shower"].contains(args[2]) { finish("invalid-arguments") }
let scene = args[2], assets = URL(fileURLWithPath: args[1])
let text = [
    "storm": "気象情報。大型の台風が接近中。強い雨と風、降水量の増加に注意してください。",
    "candy": "お菓子の新商品。カラフルな飴、キャンディ、砂糖菓子を紹介します。",
    "rainfall": "住宅街で雨が降っています。屋根から雨水が滴り、道路に水たまりができています。",
    "candy-shower": "お祭りの飴まきです。屋根の上からカラフルな飴やキャンディをばらまいています。"
][scene]!
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
// Fixed multiscale grid, independent of scene ID and expected answer. Small
// objects can disappear in whole-frame classifications dominated by people/sky.
func regionalClassification(_ image: CGImage) -> [String: Any] {
    let started = ProcessInfo.processInfo.systemUptime
    var regions = [[String: Any]](), best = [String: (Float, Int)]()
    var failures = 0
    for grid in [3, 5] {
        for y in 0..<grid { for x in 0..<grid {
            let x0 = image.width*x/grid, y0 = image.height*y/grid
            let rect = CGRect(x: CGFloat(x0), y: CGFloat(y0), width: CGFloat(image.width*(x+1)/grid-x0), height: CGFloat(image.height*(y+1)/grid-y0))
            guard let crop = image.cropping(to: rect) else { failures += 1; continue }
            let request = VNClassifyImageRequest()
            do {
                try VNImageRequestHandler(cgImage: crop, options: [:]).perform([request])
                let labels = Array((request.results ?? []).prefix(3))
                regions.append(["grid": grid, "x": x, "y": y,
                    "labels": labels.map { ["label": $0.identifier, "confidence": $0.confidence] as [String: Any] }])
                for label in labels {
                    let previous = best[label.identifier] ?? (0, 0)
                    best[label.identifier] = (max(previous.0, label.confidence), previous.1 + 1)
                }
            } catch { failures += 1 }
        }}
    }
    let labels = best.sorted { $0.value.0 == $1.value.0 ? $0.key < $1.key : $0.value.0 > $1.value.0 }.prefix(24)
        .map { ["label": $0.key, "confidence": $0.value.0, "regions": $0.value.1] as [String: Any] }
    return ["status": regions.isEmpty ? "unavailable" : "completed", "labels": labels, "regions": regions,
        "failedRegions": failures, "method": "fixed-grid-3x3-5x5-top3-maxpool24-v1",
        "confidenceMeaning": "maximum region classifier score, not whole-scene or word-selection probability",
        "elapsedMs": Int((ProcessInfo.processInfo.systemUptime-started)*1000)]
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
emit(["scene": scene, "fixtureText": text, "ocr": textOCR, "imageOCR": ocr(image), "vision": vision, "visionRegions": regionalClassification(image),
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
