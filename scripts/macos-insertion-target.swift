// Synthetic native fields for testing guards. Never linked into the product.
import AppKit
import Foundation

final class FocusButton: NSButton {
    override var acceptsFirstResponder: Bool { true }
    @objc func focusSelf() { window?.makeFirstResponder(self) }
}
let app = NSApplication.shared
app.setActivationPolicy(.regular)
let window = NSWindow(contentRect: NSRect(x: 120, y: 150, width: 560, height: 420), styleMask: [.titled, .closable], backing: .buffered, defer: false)
window.title = "Insertion Guard Fixture"
func textArea(_ label: String, _ value: String, y: CGFloat, editable: Bool = true) -> NSTextView {
    let view = NSTextView(frame: NSRect(x: 20, y: y, width: 510, height: 70))
    view.string = value
    view.isEditable = editable
    view.isRichText = false
    view.setAccessibilityLabel(label)
    window.contentView!.addSubview(view)
    return view
}
let first = textArea("First input", "first-sentinel", y: 320)
let second = textArea("Second input", "second-sentinel", y: 230)
let readonly = textArea("Read only input", "readonly-sentinel", y: 140, editable: false)
let secure = NSSecureTextField(frame: NSRect(x: 20, y: 90, width: 510, height: 30))
secure.stringValue = "password-sentinel"
secure.setAccessibilityLabel("Protected input")
window.contentView!.addSubview(secure)
let button = FocusButton(title: "Non text target", target: nil, action: #selector(FocusButton.focusSelf))
button.target = button
button.frame = NSRect(x: 20, y: 30, width: 200, height: 35)
window.contentView!.addSubview(button)
let stateURL = URL(fileURLWithPath: CommandLine.arguments[1])
let timer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { _ in
    let state: [String: Any] = ["first": first.string, "second": second.string, "readonly": readonly.string,
                               "secureLength": secure.stringValue.utf16.count]
    if let data = try? JSONSerialization.data(withJSONObject: state, options: [.sortedKeys]) { try? data.write(to: stateURL, options: .atomic) }
}
window.makeKeyAndOrderFront(nil)
app.activate(ignoringOtherApps: true)
app.run()
