import AppKit
import ApplicationServices
import Foundation

func axValue(_ element: AXUIElement, _ attribute: String) -> CFTypeRef? {
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, attribute as CFString, &value) == .success else { return nil }
    return value
}

func axText(_ element: AXUIElement) -> String {
    [kAXTitleAttribute, kAXDescriptionAttribute, kAXValueAttribute]
        .compactMap { axValue(element, $0) as? String }.joined(separator: " ")
}

func axElements(_ root: AXUIElement) -> [AXUIElement] {
    var queue = [root]
    var offset = 0
    while offset < queue.count && queue.count < 2_000 {
        let current = queue[offset]
        offset += 1
        if let children = axValue(current, kAXChildrenAttribute) as? [AXUIElement] {
            queue.append(contentsOf: children)
        }
    }
    return queue
}

func uiText(_ root: AXUIElement) -> String {
    axElements(root).map(axText).joined(separator: "\n")
}

func uiButton(_ root: AXUIElement, _ names: [String]) -> AXUIElement? {
    axElements(root).first { element in
        guard axValue(element, kAXRoleAttribute) as? String == kAXButtonRole,
              (axValue(element, kAXEnabledAttribute) as? Bool) == true else { return false }
        let fields = [kAXTitleAttribute, kAXDescriptionAttribute, kAXValueAttribute]
            .compactMap { axValue(element, $0) as? String }
        return fields.contains { field in names.contains { field.caseInsensitiveCompare($0) == .orderedSame } }
    }
}

func press(_ button: AXUIElement) throws {
    let result = AXUIElementPerformAction(button, kAXPressAction as CFString)
    guard result == .success else { throw SmokeFailure(description: "AXPress failed: \(result.rawValue)") }
}

func waitForUI(_ description: String, seconds: Double = 20, _ predicate: () throws -> Bool) throws {
    let deadline = Date().addingTimeInterval(seconds)
    while Date() < deadline {
        if try predicate() { return }
        RunLoop.current.run(until: Date().addingTimeInterval(0.25))
    }
    throw SmokeFailure(description: "Timed out: \(description)")
}

func interactionScreenshot(_ output: URL, name: String, windowID: NSNumber? = nil) throws {
    let capture = Process()
    capture.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
    capture.arguments = ["-x", "-o"]
    if let windowID { capture.arguments! += ["-l", windowID.stringValue] }
    capture.arguments!.append(output.appendingPathComponent(name).path)
    try capture.run()
    capture.waitUntilExit()
    guard capture.terminationStatus == 0 else { throw SmokeFailure(description: "Interaction screenshot failed") }
}

func recordingInteraction(pid: pid_t, windowID: NSNumber, scenario: String, output: URL) throws {
    let root = AXUIElementCreateApplication(pid)
    AXUIElementSetMessagingTimeout(root, 1)
    try writeJSON(["accessibilityTrusted": AXIsProcessTrusted(), "canPostEvents": CGPreflightPostEventAccess()],
                  to: output.appendingPathComponent("automation-access.json"))
    guard AXIsProcessTrusted() else {
        throw SmokeFailure(description: "Native permission UI automation requires Accessibility access for the CI driver")
    }

    try waitForUI("initial microphone permission state") { uiText(root).contains("未確認") }
    try uiText(root).write(to: output.appendingPathComponent("before.txt"), atomically: true, encoding: .utf8)
    guard let start = uiButton(root, ["Start recording", "録音を開始"]) else {
        throw SmokeFailure(description: "Start recording button was not available")
    }
    try press(start)

    // TCC may host the alert in the requesting app or a system agent. Only
    // act on an alert identifying this app and microphone access.
    var permissionButton: AXUIElement?
    let choices = scenario == "deny" ? ["Don't Allow", "Don’t Allow", "許可しない"] : ["OK", "Allow", "許可"]
    try waitForUI("real macOS microphone dialog") {
        let candidates = [pid] + NSWorkspace.shared.runningApplications.map(\.processIdentifier).filter { $0 != pid }
        for candidate in candidates {
            let app = AXUIElementCreateApplication(candidate)
            AXUIElementSetMessagingTimeout(app, 0.25)
            let text = uiText(app)
            if text.localizedCaseInsensitiveContains("Voice Input")
                && (text.localizedCaseInsensitiveContains("microphone") || text.contains("マイク")),
               let button = uiButton(app, choices) {
                permissionButton = button
                try text.write(to: output.appendingPathComponent("permission-dialog.txt"), atomically: true, encoding: .utf8)
                return true
            }
        }
        return false
    }
    try interactionScreenshot(output, name: "permission-dialog.png")
    try press(permissionButton!)

    if scenario == "deny" {
        try waitForUI("denied state and settings action") {
            uiText(root).contains("拒否済み") && uiButton(root, ["Open microphone settings"]) != nil
        }
        guard uiButton(root, ["Start recording"]) == nil, uiButton(root, ["Stop recording"]) == nil else {
            throw SmokeFailure(description: "Recording controls remained enabled after permission denial")
        }
        try interactionScreenshot(output, name: "denied.png", windowID: windowID)
        try uiText(root).write(to: output.appendingPathComponent("after.txt"), atomically: true, encoding: .utf8)
        try writeJSON(["status": "passed", "scenario": "deny", "realPermissionDialog": true], to: output.appendingPathComponent("interaction.json"))
        return
    }

    try waitForUI("authorized recording state") {
        let text = uiText(root)
        if text.contains("録音できませんでした") {
            throw SmokeFailure(description: "Recording failed after permission grant: \(text)")
        }
        return text.contains("許可済み") && text.contains("録音中") && uiButton(root, ["Stop recording"]) != nil
    }
    var peak = 0.0
    try waitForUI("nonzero signal from virtual microphone") {
        for element in axElements(root) {
            if let value = axValue(element, kAXValueAttribute) as? NSNumber,
               let role = axValue(element, kAXRoleAttribute) as? String,
               role == "AXLevelIndicator" || role == "AXProgressIndicator" {
                peak = max(peak, value.doubleValue)
            }
        }
        return peak > 0.001
    }
    try interactionScreenshot(output, name: "recording.png", windowID: windowID)
    guard let stop = uiButton(root, ["Stop recording"]) else { throw SmokeFailure(description: "Stop control missing") }
    try press(stop)
    try waitForUI("recorded samples") { uiText(root).contains("サンプルを Mac 内に保持") }
    let recorded = uiText(root)
    try recorded.write(to: output.appendingPathComponent("recorded.txt"), atomically: true, encoding: .utf8)
    try interactionScreenshot(output, name: "recorded.png", windowID: windowID)
    guard let clear = uiButton(root, ["Discard recording"]) else { throw SmokeFailure(description: "Discard control missing") }
    try press(clear)
    try waitForUI("discarded recording") {
        uiText(root).contains("録音を開始できます") && uiButton(root, ["Discard recording"]) == nil
    }
    try interactionScreenshot(output, name: "cleared.png", windowID: windowID)
    try writeJSON(["status": "passed", "scenario": "allow", "realPermissionDialog": true,
                   "inputLevel": peak, "recordingStopped": true, "recordingDiscarded": true],
                  to: output.appendingPathComponent("interaction.json"))
}
