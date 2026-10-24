// CI-only observable AX server. Never compiled into Voice Input.
import AppKit
import Foundation

var valueReads = [String: Int]()
var rangeReads = [String: Int]()
var maximumRange = 0
var lastRange = NSRange(location: 0, length: 0)
final class ContextTextView: NSTextView {
    var label = ""
    var supported = true
    override func accessibilityValue() -> String? {
        valueReads[label, default: 0] += 1
        return super.accessibilityValue()
    }
    override func accessibilityString(for range: NSRange) -> String? {
        rangeReads[label, default: 0] += 1
        maximumRange = max(maximumRange, range.length)
        lastRange = range
        return supported ? super.accessibilityString(for: range) : nil
    }
}
final class ProtectedField: NSSecureTextField {
    override func accessibilityValue() -> String? {
        valueReads["secure", default: 0] += 1
        return super.accessibilityValue()
    }
}
let app = NSApplication.shared
app.setActivationPolicy(.regular)
let window = NSWindow(contentRect: NSRect(x: 100, y: 120, width: 600, height: 400), styleMask: [.titled, .closable], backing: .buffered, defer: false)
window.title = "Local Context Fixture"
func area(_ label: String, _ text: String, _ y: CGFloat, editable: Bool = true, supported: Bool = true) -> ContextTextView {
    let view = ContextTextView(frame: NSRect(x: 20, y: y, width: 550, height: 65))
    view.label = label
    view.string = text
    view.isEditable = editable
    view.isRichText = false
    view.supported = supported
    view.setAccessibilityLabel(label)
    window.contentView!.addSubview(view)
    return view
}
let prefix = String(repeating: "遠方の文章😀。", count: 500)
let nearby = "月日、百代、過客、旅人。"
let first = area("Context input", prefix + nearby + String(repeating: "別の文章。", count: 500), 300)
let readonly = area("Read only context", "readonly-sentinel", 210, editable: false)
let unsupported = area("Unsupported context", "unsupported-sentinel", 120, supported: false)
let secure = ProtectedField(frame: NSRect(x: 20, y: 50, width: 550, height: 30))
secure.stringValue = "password-sentinel"
secure.setAccessibilityLabel("Protected context")
window.contentView!.addSubview(secure)
let stateURL = URL(fileURLWithPath: CommandLine.arguments[1])
let timer = Timer.scheduledTimer(withTimeInterval: 0.05, repeats: true) { _ in
    let state: [String: Any] = ["valueReads": valueReads, "rangeReads": rangeReads, "maximumRange": maximumRange, "lastRangeStart": lastRange.location, "lastRangeLength": lastRange.length,
        "caret": prefix.utf16.count + nearby.utf16.count, "firstUnchanged": first.string == prefix + nearby + String(repeating: "別の文章。", count: 500),
        "secureUnchanged": secure.stringValue == "password-sentinel"]
    if let data = try? JSONSerialization.data(withJSONObject: state, options: [.sortedKeys]) { try? data.write(to: stateURL, options: .atomic) }
}
window.makeKeyAndOrderFront(nil)
app.activate(ignoringOtherApps: true)
app.run()
