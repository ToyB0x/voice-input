import AppKit
import ApplicationServices
import Foundation

func contextCheckbox(_ root: AXUIElement) -> AXUIElement? {
    revealProductControl(root, "Local recognition context")
    return axElements(root).first { axValue($0, kAXRoleAttribute) as? String == "AXCheckBox" && axText($0).contains("Local recognition context") }
}
func pressContextButton(_ root: AXUIElement, _ label: String) throws {
    var button: AXUIElement?
    try waitForUI("enabled context control: " + label) {
        button = uiButton(root, [label])
        return button != nil
    }
    guard let button else { throw SmokeFailure(description: "Context control missing: " + label) }
    try press(button)
}
func configureContext(_ voice: AXUIElement, enabled: Bool) throws {
    try waitForUI("restored context model", seconds: 120) { uiText(voice).contains("モデル準備完了") }
    try typeField(voice, label: "Recording shortcut", value: "Control+Alt+KeyV")
    guard let checkbox = contextCheckbox(voice), let insertion = insertionCheckbox(voice) else { throw SmokeFailure(description: "Missing context preference") }
    if (axValue(insertion, kAXValueAttribute) as? NSNumber)?.intValue == 1 { try press(insertion) }
    if ((axValue(checkbox, kAXValueAttribute) as? NSNumber)?.intValue == 1) != enabled { try press(checkbox) }
    try pressContextButton(voice, "Save preferences")
    try waitForUI("context preference applied") {
        let text = uiText(voice)
        return (enabled ? text.contains("認識補助:") && !text.contains("認識補助: 無効") : text.contains("認識補助: 無効")) && text.contains("直接入力: 無効")
    }
}
func contextInteraction(pid: pid_t, scenario: String, output: URL) throws {
    let voice = AXUIElementCreateApplication(pid)
    if scenario == "context-restore" {
        try waitForUI("context preference restored", seconds: 120) {
            revealProductControl(voice, "Local recognition context")
            return uiText(voice).contains("認識補助: 有効。次の録音") && resultText(voice).isEmpty
                && (contextCheckbox(voice).flatMap { axValue($0, kAXValueAttribute) as? NSNumber })?.intValue == 1
        }
        try writeJSON(["status": "passed", "preferenceRestored": true, "noPendingHintOrTranscript": true], to: output.appendingPathComponent("interaction.json"))
        return
    }
    try configureContext(voice, enabled: true)
    try hideInsertionWindow(voice)
    if scenario == "context-denied" {
        let (_, field) = try openTextTarget(output, name: "context-denied", text: "月日、百代、旅人。")
        try setRange(field, 0, 0)
        try recordForInsertion(voice)
        try trayAction(voice, title: "設定・認識結果を表示")
        try waitForUI("context permission fallback") { uiText(voice).contains("認識補助: ヒントなしで続行") && fixtureRecognized(resultText(voice)) }
        try interactionScreenshot(output, name: "denied.png")
        try configureContext(voice, enabled: false)
        try writeJSON(["status": "passed", "untrustedFallback": true, "recognitionContinues": true], to: output.appendingPathComponent("interaction.json"))
        return
    }
    guard let fixturePath = ProcessInfo.processInfo.environment["VOICE_INPUT_CONTEXT_TARGET"] else { throw SmokeFailure(description: "Missing context fixture") }
    let stateURL = output.appendingPathComponent("fixture-state.json")
    let launch = Process()
    launch.executableURL = URL(fileURLWithPath: "/usr/bin/open")
    launch.arguments = ["-n", "-a", fixturePath, "--args", stateURL.path]
    try launch.run(); launch.waitUntilExit()
    var fixture: NSRunningApplication?
    try waitForUI("context fixture") {
        fixture = NSWorkspace.shared.runningApplications.first { $0.bundleIdentifier == "dev.toyb0x.contextfixture" }
        return fixture != nil && FileManager.default.fileExists(atPath: stateURL.path)
    }
    defer { fixture?.forceTerminate() }
    let target = AXUIElementCreateApplication(fixture!.processIdentifier)
    func state() throws -> [String: Any] {
        try JSONSerialization.jsonObject(with: Data(contentsOf: stateURL)) as! [String: Any]
    }
    func counts(_ state: [String: Any], _ key: String) -> Int {
        (state[key] as? [String: Int] ?? [:]).values.reduce(0, +)
    }
    func field(_ label: String) throws -> AXUIElement {
        guard let element = axElements(target).first(where: { axValue($0, kAXDescriptionAttribute) as? String == label }) else { throw SmokeFailure(description: "Missing labeled context field") }
        return element
    }
    var passed = [String]()
    for mode in ["bounded", "disabled", "secure", "readonly", "unsupported", "discard", "disable-during-recording"] {
        // A redundant tray Show can enqueue a late activation after we hide the
        // window, stealing focus back from the fixture in the next case.
        if visibleWindows(for: pid).isEmpty {
            try trayAction(voice, title: "設定・認識結果を表示")
            try waitForUI("context settings foreground") {
                !visibleWindows(for: pid).isEmpty && NSWorkspace.shared.frontmostApplication?.processIdentifier == pid
            }
        }
        try configureContext(voice, enabled: mode != "disabled")
        try hideInsertionWindow(voice)
        let label = mode == "secure" ? "Protected context" : mode == "readonly" ? "Read only context" : mode == "unsupported" ? "Unsupported context" : "Context input"
        let element = try field(label)
        try focusElement(target, element, pid: fixture!.processIdentifier)
        if mode != "secure" { try setRange(element, mode == "readonly" || mode == "unsupported" ? 0 : (try state()["caret"] as! Int), 0) }
        // Let pending OS focus notifications finish before measuring AX requests.
        RunLoop.current.run(until: Date().addingTimeInterval(0.3))
        let before = try state()
        try key(9, flags: [.maskControl, .maskAlternate])
        try waitForUI("context recording begins") { trayItem(voice).map(axText)?.contains("録音中") == true }
        RunLoop.current.run(until: Date().addingTimeInterval(0.3))
        let after = try state()
        let reads = counts(after, "rangeReads") - counts(before, "rangeReads")
        let values = counts(after, "valueReads") - counts(before, "valueReads")
        guard values == 0 else { throw SmokeFailure(description: "Context requested a full field value: \(mode)") }
        guard reads == (["disabled", "secure", "readonly"].contains(mode) ? 0 : 1), (after["maximumRange"] as! Int) <= 512 else { throw SmokeFailure(description: "Unexpected or unbounded range read: \(mode)") }
        if mode == "bounded" {
            guard after["lastRangeStart"] as? Int == (after["caret"] as! Int) - 256,
                  after["lastRangeLength"] as? Int == 512 else { throw SmokeFailure(description: "Context range did not surround caret") }
        }
        print("Context case \(mode): full value reads=\(values), range reads=\(reads)")
        if mode == "discard" || mode == "disable-during-recording" {
            try trayAction(voice, title: "設定・認識結果を表示")
            try waitForUI("recording context controls") {
                uiText(voice).contains("録音中") && uiButton(voice, ["Discard recording"]) != nil
            }
            try waitForUI("pending context captured") { uiText(voice).contains("認識補助: 取得済み") }
        }
        if mode == "discard" {
            try pressContextButton(voice, "Discard recording")
            try waitForUI("pending hint discarded") { uiText(voice).contains("認識補助: 次の録音") && resultText(voice).isEmpty }
        } else {
            if mode == "disable-during-recording" { try configureContext(voice, enabled: false) }
            // Keep normal cases in the real target app, using the same proven
            // global shortcut path for both recording and stopping.
            let playback = Process()
            playback.executableURL = URL(fileURLWithPath: "/usr/bin/afplay")
            guard let audio = ProcessInfo.processInfo.environment["VOICE_INPUT_TEST_AUDIO"] else { throw SmokeFailure(description: "Missing context audio") }
            playback.arguments = [audio]
            try playback.run()
            defer { if playback.isRunning { playback.terminate() } }
            try waitForUI("context audio completes", seconds: 30) { !playback.isRunning }
            try waitForCIVirtualAudioDrain()
            guard playback.terminationStatus == 0, trayItem(voice).map(axText)?.contains("録音中") == true else {
                throw SmokeFailure(description: "Context recording ended before stop: " + mode)
            }
            try key(9, flags: [.maskControl, .maskAlternate])
            try waitForUI("context inference starts") { trayItem(voice).map(axText)?.contains("処理中") == true }
            try waitForUI("context inference ends", seconds: 120) { trayItem(voice).map(axText)?.contains("待機") == true }
            if visibleWindows(for: pid).isEmpty { try trayAction(voice, title: "設定・認識結果を表示") }
            try waitForUI("context result completes", seconds: 120) { !resultText(voice).isEmpty && uiText(voice).contains("文字起こしが完了") }
            let expected = ["disabled", "disable-during-recording"].contains(mode) ? "認識補助: 無効" : ["secure", "readonly", "unsupported"].contains(mode) ? "認識補助: ヒントなしで続行" : "ローカルヒントを使用しました"
            try waitForUI("context delivery state") { uiText(voice).contains(expected) }
            if mode == "bounded" {
                try pressContextButton(voice, "Transcribe recording")
                try waitForUI("hintless retry begins") { uiText(voice).contains("文字起こし中") && trayItem(voice).map(axText)?.contains("処理中") == true }
                try waitForUI("hintless retry completes", seconds: 120) { uiText(voice).contains("文字起こしが完了") && fixtureRecognized(resultText(voice)) && trayItem(voice).map(axText)?.contains("待機") == true }
                let retried = try state()
                guard counts(retried, "rangeReads") == counts(after, "rangeReads") else { throw SmokeFailure(description: "Retry reread field") }
            }
        }
        guard (try state()["firstUnchanged"] as? Bool) == true, (try state()["secureUnchanged"] as? Bool) == true else { throw SmokeFailure(description: "Context feature modified native target") }
        try uiText(voice).write(to: output.appendingPathComponent(mode + ".txt"), atomically: true, encoding: .utf8)
        try interactionScreenshot(output, name: mode + ".png")
        passed.append(mode)
    }
    // The user starts recording in Voice Input itself: its settings never become context.
    try configureContext(voice, enabled: true)
    let beforeSelf = try state()
    try pressContextButton(voice, "Start recording")
    try waitForUI("own window excluded") { uiText(voice).contains("録音中") && uiText(voice).contains("認識補助: ヒントなしで続行") }
    try pressContextButton(voice, "Discard recording")
    guard counts(try state(), "rangeReads") == counts(beforeSelf, "rangeReads") else { throw SmokeFailure(description: "Self focus read another field") }
    passed.append("own-app-excluded")
    // Preference is restored on the next invocation; leave enabled for that check.
    try writeJSON(["status": "passed", "cases": passed, "maximumUTF16Range": 512, "fullValueReads": 0,
                   "directInsertionDisabled": true, "realAudioAndRecognition": true], to: output.appendingPathComponent("interaction.json"))
}
