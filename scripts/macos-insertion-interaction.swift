import AppKit
import ApplicationServices
import Foundation

func insertionCheckbox(_ root: AXUIElement) -> AXUIElement? {
    axElements(root).first { axValue($0, kAXRoleAttribute) as? String == "AXCheckBox" && axText($0).contains("Automatic text insertion") }
}
func configureInsertion(_ root: AXUIElement) throws {
    try waitForUI("restored model", seconds: 120) { uiText(root).contains("モデル準備完了") }
    try typeField(root, label: "Recording shortcut", value: "Control+Alt+KeyV")
    guard let checkbox = insertionCheckbox(root) else { throw SmokeFailure(description: "Direct insertion checkbox missing") }
    if (axValue(checkbox, kAXValueAttribute) as? NSNumber)?.intValue != 1 { try press(checkbox) }
    try press(uiButton(root, ["Save preferences"])!)
    try waitForUI("insertion explicitly enabled") { uiText(root).contains("直接入力: 有効") }
}
func hideInsertionWindow(_ root: AXUIElement) throws {
    guard let window = (axValue(root, kAXWindowsAttribute) as? [AXUIElement])?.first,
          let close = axValue(window, kAXCloseButtonAttribute) else { throw SmokeFailure(description: "No window close control") }
    try press(unsafeBitCast(close, to: AXUIElement.self))
}
func focusElement(_ root: AXUIElement, _ element: AXUIElement, pid: pid_t) throws {
    NSRunningApplication(processIdentifier: pid)?.activate(options: [.activateAllWindows])
    guard AXUIElementSetAttributeValue(element, kAXFocusedAttribute as CFString, kCFBooleanTrue) == .success else {
        throw SmokeFailure(description: "Could not focus native target")
    }
    try waitForUI("target focus") { NSWorkspace.shared.frontmostApplication?.processIdentifier == pid }
}
func setRange(_ field: AXUIElement, _ location: Int, _ length: Int) throws {
    var range = CFRange(location: location, length: length)
    guard let value = AXValueCreate(.cfRange, &range),
          AXUIElementSetAttributeValue(field, kAXSelectedTextRangeAttribute as CFString, value) == .success else {
        throw SmokeFailure(description: "Could not set native selection")
    }
}
func openTextTarget(_ output: URL, name: String, text: String) throws -> (pid_t, AXUIElement) {
    let file = output.appendingPathComponent(name + ".txt")
    try text.write(to: file, atomically: true, encoding: .utf8)
    let process = Process()
    process.executableURL = URL(fileURLWithPath: "/usr/bin/open")
    process.arguments = ["-a", "TextEdit", file.path]
    try process.run(); process.waitUntilExit()
    var pid: pid_t = 0
    try waitForUI("TextEdit foreground target") {
        guard let front = NSWorkspace.shared.frontmostApplication, front.bundleIdentifier == "com.apple.TextEdit" else { return false }
        pid = front.processIdentifier
        return true
    }
    let root = AXUIElementCreateApplication(pid)
    var field: AXUIElement?
    try waitForUI("focused TextEdit field") {
        guard let value = axValue(root, kAXFocusedUIElementAttribute), CFGetTypeID(value) == AXUIElementGetTypeID() else { return false }
        let candidate = unsafeBitCast(value, to: AXUIElement.self)
        guard axValue(candidate, kAXRoleAttribute) as? String == "AXTextArea",
              axValue(candidate, kAXValueAttribute) as? String == text else { return false }
        field = candidate
        return true
    }
    return (pid, field!)
}
func recordForInsertion(_ voice: AXUIElement, afterStop: () throws -> Void = {}) throws {
    guard let audio = ProcessInfo.processInfo.environment["VOICE_INPUT_TEST_AUDIO"] else { throw SmokeFailure(description: "Missing audio fixture") }
    NSPasteboard.general.clearContents()
    NSPasteboard.general.setString("insertion-clipboard-sentinel", forType: .string)
    try key(9, flags: [.maskControl, .maskAlternate])
    try waitForUI("shortcut recording") { trayItem(voice).map(axText)?.contains("録音中") == true }
    let playback = Process()
    playback.executableURL = URL(fileURLWithPath: "/usr/bin/afplay")
    playback.arguments = [audio]
    try playback.run()
    defer { if playback.isRunning { playback.terminate() } }
    try waitForUI("insertion audio playback", seconds: 30) { !playback.isRunning }
    guard playback.terminationStatus == 0 else { throw SmokeFailure(description: "Playback failed") }
    try key(9, flags: [.maskControl, .maskAlternate])
    try waitForUI("insertion inference") { trayItem(voice).map(axText)?.contains("処理中") == true }
    try afterStop()
    try waitForUI("insertion inference completed", seconds: 120) {
        guard let text = trayItem(voice).map(axText) else { return false }
        return text.contains("待機") || text.contains("要確認")
    }
    guard NSPasteboard.general.string(forType: .string) == "insertion-clipboard-sentinel" else {
        throw SmokeFailure(description: "Insertion changed clipboard")
    }
}
func inspectInsertion(_ root: AXUIElement, output: URL, name: String, inserted: Bool) throws -> String {
    try trayAction(root, title: "設定・認識結果を表示")
    try waitForUI("insertion result and delivery status") {
        let text = uiText(root)
        return fixtureRecognized(resultText(root)) && (inserted ? text.contains("入力先へ挿入しました") : text.contains("直接入力: 有効"))
    }
    try uiText(root).write(to: output.appendingPathComponent(name + "-state.txt"), atomically: true, encoding: .utf8)
    try interactionScreenshot(output, name: name + ".png")
    return resultText(root)
}

func grantInsertionPermission(_ voice: AXUIElement, output: URL) throws {
    guard let request = uiButton(voice, ["Request accessibility permission"]) else { throw SmokeFailure(description: "Permission request control missing") }
    try press(request)
    var settings: AXUIElement?
    try waitForUI("Accessibility settings", seconds: 30) {
        guard let process = NSWorkspace.shared.runningApplications.first(where: { $0.bundleIdentifier == "com.apple.systempreferences" }) else { return false }
        settings = AXUIElementCreateApplication(process.processIdentifier)
        AXUIElementSetMessagingTimeout(settings!, 0.25)
        return true
    }
    do {
        var toggle: AXUIElement?
        try waitForUI("Voice Input accessibility switch", seconds: 45) {
            let elements = axElements(settings!)
            toggle = elements.first { ["AXCheckBox", "AXSwitch"].contains(axValue($0, kAXRoleAttribute) as? String ?? "") && axText($0).contains("Voice Input") }
            if toggle != nil { return true }
            // SwiftUI may label the row rather than the switch itself.
            for container in elements.reversed() {
                let children = axElements(container)
                if children.count < 40 && children.map(axText).joined().contains("Voice Input"),
                   let control = children.first(where: { ["AXCheckBox", "AXSwitch"].contains(axValue($0, kAXRoleAttribute) as? String ?? "") }) {
                    toggle = control
                    return true
                }
            }
            // Dismiss the product's real permission alert only by opening settings.
            for app in NSWorkspace.shared.runningApplications {
                let root = AXUIElementCreateApplication(app.processIdentifier)
                AXUIElementSetMessagingTimeout(root, 0.2)
                let text = uiText(root)
                if text.contains("Voice Input") && text.localizedCaseInsensitiveContains("accessibility"),
                   let open = uiButton(root, ["Open System Settings", "Open System Preferences", "システム設定を開く"]) { try press(open) }
            }
            return false
        }
        if (axValue(toggle!, kAXValueAttribute) as? NSNumber)?.intValue != 1 { try press(toggle!) }
        try waitForUI("accessibility switch enabled", seconds: 30) { (axValue(toggle!, kAXValueAttribute) as? NSNumber)?.intValue == 1 }
        // The switch changes before TCC necessarily commits/invalidates its cache.
        RunLoop.current.run(until: Date().addingTimeInterval(3))
        try uiText(settings!).write(to: output.appendingPathComponent("system-settings.txt"), atomically: true, encoding: .utf8)
        try uiText(voice).write(to: output.appendingPathComponent("product-after-grant.txt"), atomically: true, encoding: .utf8)
        try interactionScreenshot(output, name: "accessibility-granted.png")
        try writeJSON(["status": "passed", "method": "System Settings UI", "productSwitchEnabled": true], to: output.appendingPathComponent("permission.json"))
    } catch {
        try? uiText(settings!).write(to: output.appendingPathComponent("system-settings.txt"), atomically: true, encoding: .utf8)
        throw error
    }
}

func insertionInteraction(pid: pid_t, windowID: NSNumber, scenario: String, output: URL) throws {
    let voice = AXUIElementCreateApplication(pid)
    try configureInsertion(voice)
    if scenario == "insertion-grant" { try grantInsertionPermission(voice, output: output); return }
    let shouldBeTrusted = scenario == "insertion"
    try waitForUI("product accessibility trust") { uiText(voice).contains(shouldBeTrusted ? "アクセシビリティ許可済み" : "アクセシビリティ未許可") }
    try hideInsertionWindow(voice)
    if !shouldBeTrusted {
        let (_, field) = try openTextTarget(output, name: "denied-target", text: "denied-sentinel")
        try setRange(field, 0, 0)
        try recordForInsertion(voice)
        _ = try inspectInsertion(voice, output: output, name: "permission-denied", inserted: false)
        guard axValue(field, kAXValueAttribute) as? String == "denied-sentinel", uiText(voice).contains("アクセシビリティが未許可") else {
            throw SmokeFailure(description: "Untrusted product wrote to target or did not explain denial")
        }
        // Register the product in System Settings through its real prompt API.
        try press(uiButton(voice, ["Request accessibility permission"])!)
        try interactionScreenshot(output, name: "accessibility-request.png")
        try writeJSON(["status": "passed", "productTrusted": false, "targetUnchanged": true, "resultRetained": true], to: output.appendingPathComponent("interaction.json"))
        return
    }
    var passed = [String]()
    for (name, initial, location, length, prefix, suffix) in [
        ("cursor", "前😀後", 3, 0, "前😀", "後"),
        ("selection", "前😀置換後", 3, 2, "前😀", "後")
    ] {
        let (targetPID, field) = try openTextTarget(output, name: name, text: initial)
        try setRange(field, location, length)
        try recordForInsertion(voice)
        guard NSWorkspace.shared.frontmostApplication?.processIdentifier == targetPID else { throw SmokeFailure(description: "Insertion stole focus") }
        let transcript = try inspectInsertion(voice, output: output, name: name, inserted: true)
        guard axValue(field, kAXValueAttribute) as? String == prefix + transcript + suffix else {
            throw SmokeFailure(description: "Cursor/selection insertion mismatch")
        }
        passed.append(name)
        try hideInsertionWindow(voice)
    }
    guard let fixturePath = ProcessInfo.processInfo.environment["VOICE_INPUT_INSERTION_TARGET"] else { throw SmokeFailure(description: "Missing native guard fixture") }
    let state = output.appendingPathComponent("guard-fixture.json")
    let launch = Process()
    launch.executableURL = URL(fileURLWithPath: "/usr/bin/open")
    launch.arguments = ["-n", "-a", fixturePath, "--args", state.path]
    try launch.run(); launch.waitUntilExit()
    var fixture: NSRunningApplication?
    try waitForUI("guard fixture") {
        fixture = NSWorkspace.shared.runningApplications.first { $0.bundleIdentifier == "dev.toyb0x.insertionfixture" }
        return fixture != nil && FileManager.default.fileExists(atPath: state.path)
    }
    defer { fixture?.forceTerminate() }
    let target = AXUIElementCreateApplication(fixture!.processIdentifier)
    func field(_ label: String) throws -> AXUIElement {
        guard let element = axElements(target).first(where: { axValue($0, kAXDescriptionAttribute) as? String == label || axValue($0, kAXTitleAttribute) as? String == label }) else {
            throw SmokeFailure(description: "Missing native field \(label): \(uiText(target))")
        }
        return element
    }
    for mode in ["field-change", "selection-change", "content-change", "app-change", "secure", "readonly", "nontext", "discard"] {
        let label = mode == "secure" ? "Protected input" : mode == "readonly" ? "Read only input" : mode == "nontext" ? "Non text target" : "First input"
        let element = try field(label)
        try focusElement(target, element, pid: fixture!.processIdentifier)
        if mode == "nontext" { try press(element) }
        if !["secure", "nontext"].contains(mode) { try setRange(element, 0, 0) }
        try recordForInsertion(voice, afterStop: {
            if mode == "field-change" { try focusElement(target, field("Second input"), pid: fixture!.processIdentifier) }
            if mode == "selection-change" { try setRange(element, 1, 0) }
            if mode == "content-change" {
                guard AXUIElementSetAttributeValue(element, kAXValueAttribute as CFString, "edited-sentinel" as CFString) == .success else { throw SmokeFailure(description: "Could not edit synthetic field") }
                try setRange(element, 0, 0)
            }
            if mode == "app-change" { _ = try openTextTarget(output, name: "changed-app", text: "other-app-sentinel") }
            if mode == "discard" {
                try trayAction(voice, title: "設定・認識結果を表示")
                try waitForUI("discard enabled") { uiButton(voice, ["Discard recording"]) != nil }
                try press(uiButton(voice, ["Discard recording"])!)
            }
        })
        if mode != "discard" {
            _ = try inspectInsertion(voice, output: output, name: mode, inserted: false)
            guard !uiText(voice).contains("入力先へ挿入しました") else { throw SmokeFailure(description: "Guard did not block \(mode)") }
        } else {
            try waitForUI("discarded result") { resultText(voice).isEmpty && uiButton(voice, ["Copy transcript"]) == nil }
            try interactionScreenshot(output, name: "discard.png")
        }
        let values = try JSONSerialization.jsonObject(with: Data(contentsOf: state)) as! [String: Any]
        let expected = mode == "content-change" || ["app-change", "secure", "readonly", "nontext", "discard"].contains(mode) ? "edited-sentinel" : "first-sentinel"
        guard values["first"] as? String == expected, values["second"] as? String == "second-sentinel",
              values["readonly"] as? String == "readonly-sentinel", values["secureLength"] as? Int == "password-sentinel".utf16.count else {
            throw SmokeFailure(description: "Guard allowed an unexpected target mutation: \(mode)")
        }
        passed.append(mode)
        try hideInsertionWindow(voice)
    }
    try writeJSON(["status": "passed", "productTrusted": true, "cases": passed, "clipboardUnchanged": true,
                   "realAudioAndRecognition": true], to: output.appendingPathComponent("interaction.json"))
}
