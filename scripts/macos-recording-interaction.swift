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
    _ = AXUIElementPerformAction(button, "AXScrollToVisible" as CFString)
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
    do {
        try runRecordingInteraction(pid: pid, windowID: windowID, scenario: scenario, output: output)
    } catch {
        try? interactionScreenshot(output, name: "interaction-failure.png")
        try? uiText(AXUIElementCreateApplication(pid)).write(
            to: output.appendingPathComponent("failure-app-state.txt"), atomically: true, encoding: .utf8)
        throw error
    }
}

func runRecordingInteraction(pid: pid_t, windowID: NSNumber, scenario: String, output: URL) throws {
    let root = AXUIElementCreateApplication(pid)
    AXUIElementSetMessagingTimeout(root, 1)
    try writeJSON(["accessibilityTrusted": AXIsProcessTrusted(), "canPostEvents": CGPreflightPostEventAccess()],
                  to: output.appendingPathComponent("automation-access.json"))
    guard AXIsProcessTrusted() else {
        throw SmokeFailure(description: "Native permission UI automation requires Accessibility access for the CI driver")
    }

    if scenario == "desktop" { try desktopInteraction(pid: pid, windowID: windowID, output: output); return }

    if scenario == "transcribe" { try configureModel(root: root, windowID: windowID, output: output) }

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
    var playback: Process?
    defer { if let playback, playback.isRunning { playback.terminate() } }
    if scenario == "transcribe" {
        guard let audio = ProcessInfo.processInfo.environment["VOICE_INPUT_TEST_AUDIO"] else {
            throw SmokeFailure(description: "VOICE_INPUT_TEST_AUDIO is required")
        }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/afplay")
        process.arguments = [audio]
        try process.run()
        playback = process
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
    if let playback {
        try waitForUI("public Japanese fixture playback", seconds: 30) { !playback.isRunning }
        guard playback.terminationStatus == 0 else { throw SmokeFailure(description: "Fixture playback failed") }
    }
    guard let stop = uiButton(root, ["Stop recording"]) else { throw SmokeFailure(description: "Stop control missing") }
    try press(stop)
    if scenario == "transcribe" {
        try verifyTranscription(root: root, windowID: windowID, output: output)
        return
    }
    var recorded = ""
    try waitForUI("recorded samples") {
        let text = uiText(root)
        if text.contains("サンプルを Mac 内に保持") { recorded = text; return true }
        return false
    }
    let pattern = try NSRegularExpression(pattern: "([0-9,]+)\\s+サンプル")
    let fullRange = NSRange(recorded.startIndex..<recorded.endIndex, in: recorded)
    guard let match = pattern.firstMatch(in: recorded, range: fullRange),
          let range = Range(match.range(at: 1), in: recorded),
          let sampleCount = Int(recorded[range].replacingOccurrences(of: ",", with: "")), sampleCount > 0
    else { throw SmokeFailure(description: "No positive recorded sample count in the UI") }
    try recorded.write(to: output.appendingPathComponent("recorded.txt"), atomically: true, encoding: .utf8)
    try interactionScreenshot(output, name: "recorded.png", windowID: windowID)
    guard let clear = uiButton(root, ["Discard recording"]) else { throw SmokeFailure(description: "Discard control missing") }
    try press(clear)
    try waitForUI("discarded recording") {
        uiText(root).contains("録音を開始できます") && uiButton(root, ["Discard recording"]) == nil
    }
    try interactionScreenshot(output, name: "cleared.png", windowID: windowID)
    try writeJSON(["status": "passed", "scenario": "allow", "realPermissionDialog": true,
                   "inputLevel": peak, "sampleCount": sampleCount, "recordingStopped": true, "recordingDiscarded": true],
                  to: output.appendingPathComponent("interaction.json"))
}

func textField(_ root: AXUIElement, label: String) -> AXUIElement? {
    axElements(root).first { element in
        guard let role = axValue(element, kAXRoleAttribute) as? String,
              ["AXTextField", "AXTextArea"].contains(role) else { return false }
        return [kAXTitleAttribute, kAXDescriptionAttribute].contains {
            axValue(element, $0) as? String == label
        }
    }
}

func typeField(_ root: AXUIElement, label: String, value path: String) throws {
    guard CGPreflightPostEventAccess(), let field = textField(root, label: label),
          AXUIElementSetAttributeValue(field, kAXFocusedAttribute as CFString, kCFBooleanTrue) == .success else {
        throw SmokeFailure(description: "Could not focus model path for native keyboard input")
    }
    // WebKit can acknowledge AXValue writes without updating the DOM input.
    // Exercise the same keyboard route as the user instead of injecting JS.
    let source = CGEventSource(stateID: .hidSystemState)
    for down in [true, false] {
        guard let event = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: down) else {
            throw SmokeFailure(description: "Could not create Select All keyboard event")
        }
        event.flags = .maskCommand
        event.post(tap: .cghidEventTap)
    }
    // Selecting all alone does not clear the field when the replacement is empty.
    if path.isEmpty { try key(51) } // Backspace
    let units = Array(path.utf16)
    for offset in stride(from: 0, to: units.count, by: 16) {
        let chunk = Array(units[offset..<min(offset + 16, units.count)])
        for down in [true, false] {
            guard let event = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: down) else {
                throw SmokeFailure(description: "Could not create text keyboard event")
            }
            event.flags = []
            chunk.withUnsafeBufferPointer { buffer in
                event.keyboardSetUnicodeString(stringLength: buffer.count, unicodeString: buffer.baseAddress!)
            }
            event.post(tap: .cghidEventTap)
        }
    }
    try waitForUI("typed model path and enabled load control") {
        axValue(field, kAXValueAttribute) as? String == path
    }
}

func setModelPath(_ root: AXUIElement, _ path: String) throws {
    try typeField(root, label: "Model path", value: path)
    try waitForUI("enabled load control") { uiButton(root, ["Load local model"]) != nil }
}

func configureModel(root: AXUIElement, windowID: NSNumber, output: URL) throws {
    guard let path = ProcessInfo.processInfo.environment["VOICE_INPUT_TEST_MODEL"] else {
        throw SmokeFailure(description: "VOICE_INPUT_TEST_MODEL is required")
    }
    try setModelPath(root, "/no-such-voice-input-model.bin")
    try press(uiButton(root, ["Load local model"])!)
    try waitForUI("missing model error") { uiText(root).contains("モデルを読み込めません") }
    try interactionScreenshot(output, name: "model-error.png", windowID: windowID)
    try setModelPath(root, path)
    try press(uiButton(root, ["Load local model"])!)
    try waitForUI("loaded model and weight-derived model information", seconds: 90) {
        let text = uiText(root)
        return text.contains("モデル準備完了") && text.contains("モデル: base / 148 MB")
    }
    try interactionScreenshot(output, name: "model-ready.png", windowID: windowID)
}

func resultText(_ root: AXUIElement) -> String {
    guard let field = textField(root, label: "Transcription result") else { return "" }
    return axValue(field, kAXValueAttribute) as? String ?? ""
}

func fixtureRecognized(_ text: String) -> Bool {
    // This checks the real audio path, not CER. Kana is a valid spelling of 旅.
    text.contains("船") && (text.contains("旅") || text.contains("たび"))
}

func verifyTranscription(root: AXUIElement, windowID: NSNumber, output: URL) throws {
    try waitForUI("Japanese recognition result", seconds: 120) {
        let text = resultText(root)
        return fixtureRecognized(text) && uiButton(root, ["Copy transcript"]) != nil
    }
    let text = resultText(root)
    try text.write(to: output.appendingPathComponent("transcript.txt"), atomically: true, encoding: .utf8)
    try interactionScreenshot(output, name: "transcribed.png", windowID: windowID)
    NSPasteboard.general.clearContents()
    NSPasteboard.general.setString("voice-input-copy-sentinel", forType: .string)
    try press(uiButton(root, ["Copy transcript"])!)
    try waitForUI("real system clipboard") { NSPasteboard.general.string(forType: .string) == text }
    try writeJSON(["status": "passed", "clipboardMatches": true], to: output.appendingPathComponent("copy.json"))
    // Reuse the loaded model for a retry; then discard while native work runs.
    guard let retry = uiButton(root, ["Transcribe recording"]) else { throw SmokeFailure(description: "Retry control missing") }
    try press(retry)
    try waitForUI("recognition in progress") { uiText(root).contains("文字起こし中") }
    guard let clear = uiButton(root, ["Discard recording"]) else { throw SmokeFailure(description: "Discard control missing during inference") }
    try press(clear)
    try waitForUI("discarded in-flight result", seconds: 120) {
        // Read related status text in one AX snapshot; WebKit may briefly
        // invalidate an accessibility node while a live region is updated.
        let text = uiText(root)
        return resultText(root).isEmpty && text.contains("録音を開始できます") && text.contains("モデル準備完了")
            && uiButton(root, ["Start recording"]) != nil && uiButton(root, ["Copy transcript"]) == nil
    }
    try interactionScreenshot(output, name: "discarded.png", windowID: windowID)
    try writeJSON(["status": "passed", "scenario": "transcribe", "realPermissionDialog": true,
                   "missingModelError": true, "japaneseWords": ["船", text.contains("旅") ? "旅" : "たび"], "clipboardMatches": true,
                   "discardedDuringInference": true, "modelRetained": true],
                  to: output.appendingPathComponent("interaction.json"))
}

func key(_ code: CGKeyCode, flags: CGEventFlags = []) throws {
    let source = CGEventSource(stateID: .hidSystemState)
    for down in [true, false] {
        guard let event = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: down) else {
            throw SmokeFailure(description: "Could not construct native key event")
        }
        event.flags = flags
        event.post(tap: .cghidEventTap)
    }
}

func trayElements(_ root: AXUIElement) -> [AXUIElement] {
    if let extra = axValue(root, "AXExtrasMenuBar"), CFGetTypeID(extra) == AXUIElementGetTypeID() {
        return axElements(unsafeBitCast(extra, to: AXUIElement.self))
    }
    return axElements(root)
}

func trayItem(_ root: AXUIElement) -> AXUIElement? {
    trayElements(root).first { element in
        let role = axValue(element, kAXRoleAttribute) as? String ?? ""
        return ["AXMenuBarItem", "AXMenuBarExtra"].contains(role) && axText(element).contains("VI ")
    }
}

func trayAction(_ root: AXUIElement, title: String) throws {
    guard let tray = trayItem(root) else {
        throw SmokeFailure(description: "Voice Input menu bar item missing: \(trayElements(root).map(axText))")
    }
    try press(tray)
    var choice: AXUIElement?
    try waitForUI("tray menu \(title)") {
        choice = trayElements(root).first { axValue($0, kAXRoleAttribute) as? String == "AXMenuItem" && axText($0).contains(title) }
        return choice != nil
    }
    try press(choice!)
}

func desktopInteraction(pid: pid_t, windowID: NSNumber, output: URL) throws {
    let root = AXUIElementCreateApplication(pid)
    guard let app = NSRunningApplication(processIdentifier: pid), let appURL = app.bundleURL,
          let model = ProcessInfo.processInfo.environment["VOICE_INPUT_TEST_MODEL"],
          let audio = ProcessInfo.processInfo.environment["VOICE_INPUT_TEST_AUDIO"] else {
        throw SmokeFailure(description: "Missing app or desktop fixture configuration")
    }
    try configureModel(root: root, windowID: windowID, output: output)
    // Reject a bare key without replacing the working default shortcut.
    try typeField(root, label: "Recording shortcut", value: "Space")
    try press(uiButton(root, ["Save preferences"])!)
    try waitForUI("invalid shortcut error preserves default") {
        let text = uiText(root)
        return text.contains("いずれかを含めて") && text.contains("Command+Shift+Space（登録済み）")
    }
    try typeField(root, label: "Recording shortcut", value: "Control+Alt+KeyV")
    // Wait for the native popup to exist, then select its actual CPU menu item.
    guard let backend = axElements(root).first(where: {
        axValue($0, kAXRoleAttribute) as? String == "AXPopUpButton" && axText($0).contains("Inference backend")
    }) else { throw SmokeFailure(description: "Inference backend popup missing") }
    AXUIElementSetAttributeValue(backend, kAXFocusedAttribute as CFString, kCFBooleanTrue)
    try press(backend)
    var cpuOption: AXUIElement?
    try waitForUI("native CPU menu option") {
        cpuOption = (axElements(root) + axElements(backend)).first { element in
            axValue(element, kAXRoleAttribute) as? String == "AXMenuItem" &&
            [kAXTitleAttribute, kAXDescriptionAttribute, kAXValueAttribute].contains {
                axValue(element, $0) as? String == "CPU"
            }
        }
        return cpuOption != nil
    }
    try press(cpuOption!)
    try waitForUI("CPU selected") {
        axElements(root).contains { element in
            axValue(element, kAXRoleAttribute) as? String == "AXPopUpButton" &&
            axText(element).contains("Inference backend") &&
            (axValue(element, kAXValueAttribute) as? String ?? "").contains("CPU")
        }
    }
    try press(uiButton(root, ["Save preferences"])!)
    try waitForUI("custom shortcut saved and registered") {
        let text = uiText(root)
        return text.contains("Control+Alt+KeyV（登録済み）") && text.contains("設定を保存しました")
    }
    try interactionScreenshot(output, name: "preferences-saved.png", windowID: windowID)
    let settingsURL = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent("Library/Application Support/dev.toyb0x.voiceinput/settings.json")
    let stored = try JSONSerialization.jsonObject(with: Data(contentsOf: settingsURL)) as! [String: Any]
    guard Set(stored.keys) == Set(["version", "modelPath", "backend", "shortcut"]),
          stored["modelPath"] as? String == model, stored["backend"] as? String == "cpu",
          stored["shortcut"] as? String == "Control+Alt+KeyV" else {
        throw SmokeFailure(description: "Persisted preferences mismatch or unexpected private data")
    }
    // Close the actual native window, leaving only the resident process/tray.
    guard let window = (axValue(root, kAXWindowsAttribute) as? [AXUIElement])?.first,
          let closeRef = axValue(window, kAXCloseButtonAttribute) else {
        throw SmokeFailure(description: "Window close control missing")
    }
    try press(unsafeBitCast(closeRef, to: AXUIElement.self))
    try waitForUI("closed window keeps resident process") { visibleWindows(for: pid).isEmpty && !app.isTerminated }
    try trayElements(root).map(axText).joined(separator: "\n").write(to: output.appendingPathComponent("tray-before.txt"), atomically: true, encoding: .utf8)

    let textURL = output.appendingPathComponent("focus-target.txt")
    try "Voice Input shortcut test target\n".write(to: textURL, atomically: true, encoding: .utf8)
    let open = Process()
    open.executableURL = URL(fileURLWithPath: "/usr/bin/open")
    open.arguments = ["-a", "TextEdit", textURL.path]
    try open.run(); open.waitUntilExit()
    try waitForUI("TextEdit foreground") { NSWorkspace.shared.frontmostApplication?.bundleIdentifier == "com.apple.TextEdit" }
    NSPasteboard.general.clearContents()
    NSPasteboard.general.setString("voice-input-background-sentinel", forType: .string)
    try key(9, flags: [.maskControl, .maskAlternate]) // Control+Option+V
    try waitForUI("global shortcut starts hidden recording") { trayItem(root).map(axText)?.contains("録音中") == true }
    guard NSWorkspace.shared.frontmostApplication?.bundleIdentifier == "com.apple.TextEdit" else {
        throw SmokeFailure(description: "Background recording stole focus")
    }
    let playback = Process()
    playback.executableURL = URL(fileURLWithPath: "/usr/bin/afplay")
    playback.arguments = [audio]
    try playback.run()
    defer { if playback.isRunning { playback.terminate() } }
    try waitForUI("Japanese playback", seconds: 30) { !playback.isRunning }
    guard playback.terminationStatus == 0 else { throw SmokeFailure(description: "Playback failed") }
    try interactionScreenshot(output, name: "background-recording.png")
    try key(9, flags: [.maskControl, .maskAlternate])
    try waitForUI("tray shows recognition") { trayItem(root).map(axText)?.contains("処理中") == true }
    try interactionScreenshot(output, name: "background-transcribing.png")
    try waitForUI("tray returns to idle", seconds: 120) { trayItem(root).map(axText)?.contains("待機") == true }
    guard visibleWindows(for: pid).isEmpty,
          NSWorkspace.shared.frontmostApplication?.bundleIdentifier == "com.apple.TextEdit",
          NSPasteboard.general.string(forType: .string) == "voice-input-background-sentinel" else {
        throw SmokeFailure(description: "Background processing changed focus, clipboard or window visibility")
    }
    try trayAction(root, title: "設定・認識結果を表示")
    try waitForUI("tray opens recognized result") { fixtureRecognized(resultText(root)) }
    try interactionScreenshot(output, name: "tray-result.png")
    try trayAction(root, title: "Voice Inputを終了")
    try waitForUI("explicit tray quit") { app.isTerminated }

    let relaunch = Process()
    relaunch.executableURL = URL(fileURLWithPath: "/usr/bin/open")
    relaunch.arguments = ["-n", "-a", appURL.path]
    try relaunch.run(); relaunch.waitUntilExit()
    var restarted: NSRunningApplication?
    try waitForUI("application restart") {
        restarted = NSWorkspace.shared.runningApplications.first {
            $0.bundleIdentifier == "dev.toyb0x.voiceinput" && $0.processIdentifier != pid
        }
        return restarted != nil
    }
    defer { if let restarted, !restarted.isTerminated { restarted.forceTerminate() } }
    let restored = AXUIElementCreateApplication(restarted!.processIdentifier)
    try waitForUI("persisted preferences and automatic CPU model load", seconds: 120) {
        let text = uiText(restored)
        return text.contains("Control+Alt+KeyV（登録済み）") && text.contains("モデル準備完了（CPUを選択）")
            && textField(restored, label: "Model path").flatMap { axValue($0, kAXValueAttribute) as? String } == model
    }
    guard resultText(restored).isEmpty, uiButton(restored, ["Discard recording"]) == nil,
          uiButton(restored, ["Copy transcript"]) == nil else {
        throw SmokeFailure(description: "Audio or result unexpectedly survived restart")
    }
    try interactionScreenshot(output, name: "restored-settings.png")
    // Empty setting disables the actual global hotkey.
    try typeField(restored, label: "Recording shortcut", value: "")
    try press(uiButton(restored, ["Save preferences"])!)
    try waitForUI("shortcut disabled") { uiText(restored).contains("無効または未登録") }
    try key(9, flags: [.maskControl, .maskAlternate])
    RunLoop.current.run(until: Date().addingTimeInterval(1))
    guard !uiText(restored).contains("録音中…") else { throw SmokeFailure(description: "Disabled shortcut still records") }
    try trayAction(restored, title: "Voice Inputを終了")
    try waitForUI("restarted app explicit quit") { restarted!.isTerminated }
    try writeJSON(["status": "passed", "scenario": "desktop", "closedWindowRemainsResident": true,
                   "backgroundGlobalShortcut": true, "foregroundPreserved": true, "clipboardUnchanged": true,
                   "trayRecognitionState": true, "trayShowAndQuit": true, "preferencesRestored": true,
                   "restoredBackend": "cpu", "privateDataNotPersisted": true, "invalidShortcutRejected": true,
                   "shortcutDisabled": true], to: output.appendingPathComponent("interaction.json"))
}
