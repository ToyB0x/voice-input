import AppKit
import Darwin
import Foundation
import Vision

struct SmokeFailure: Error, CustomStringConvertible {
    let description: String
}

func writeJSON(_ value: Any, to url: URL) throws {
    let data = try JSONSerialization.data(withJSONObject: value, options: [.prettyPrinted, .sortedKeys])
    try data.write(to: url)
}

func visibleWindows(for pid: pid_t) -> [[String: Any]] {
    let windows = CGWindowListCopyWindowInfo(
        [.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID
    ) as? [[String: Any]] ?? []
    return windows.filter { window in
        guard let owner = window[kCGWindowOwnerPID as String] as? NSNumber,
              let layer = window[kCGWindowLayer as String] as? NSNumber,
              let bounds = window[kCGWindowBounds as String] as? [String: NSNumber]
        else { return false }
        return owner.int32Value == pid && layer.intValue == 0
            && (bounds["Width"]?.doubleValue ?? 0) >= 300
            && (bounds["Height"]?.doubleValue ?? 0) >= 300
    }
}

func smokeTest(appURL: URL, output: URL, scenario: String?) throws {
    guard let bundle = Bundle(url: appURL),
          bundle.executableURL != nil
    else { throw SmokeFailure(description: "Missing app bundle or executable: \(appURL.path)") }

    let appLog = output.appendingPathComponent("app.log")
    FileManager.default.createFile(atPath: appLog.path, contents: nil)
    // Launch Services gives the bundled app its own TCC identity instead of
    // inheriting the CI runner's permissions via direct executable launch.
    let previousPIDs = Set(NSWorkspace.shared.runningApplications.map(\.processIdentifier))
    let launcher = Process()
    launcher.executableURL = URL(fileURLWithPath: "/usr/bin/open")
    launcher.arguments = ["-n", "-a", appURL.path, "--stdout", output.appendingPathComponent("app.stdout.log").path, "--stderr", appLog.path]
    try launcher.run()
    launcher.waitUntilExit()
    guard launcher.terminationStatus == 0 else { throw SmokeFailure(description: "Launch Services could not open the app") }
    var launched: NSRunningApplication?
    let launchDeadline = Date().addingTimeInterval(15)
    while launched == nil && Date() < launchDeadline {
        launched = NSWorkspace.shared.runningApplications.first {
            $0.bundleIdentifier == bundle.bundleIdentifier && !previousPIDs.contains($0.processIdentifier)
        }
        RunLoop.current.run(until: Date().addingTimeInterval(0.1))
    }
    guard let app = launched else { throw SmokeFailure(description: "Launched app was not registered with the window server") }
    defer {
        if !app.isTerminated {
            app.terminate()
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
            if !app.isTerminated { app.forceTerminate() }
        }
    }

    let captureLog = output.appendingPathComponent("capture.log")
    FileManager.default.createFile(atPath: captureLog.path, contents: nil)
    let captureLogHandle = try FileHandle(forWritingTo: captureLog)
    defer { try? captureLogHandle.close() }

    let started = Date()
    let deadline = started.addingTimeInterval(60)
    let screenshot = output.appendingPathComponent("window.png")
    let expected = ["voiceinput", "rust", "tauri"]
    var lastFailure = "No visible application window found"

    while Date() < deadline {
        guard !app.isTerminated else {
            throw SmokeFailure(description: "App exited before readiness; see app.log")
        }

        let windows = visibleWindows(for: app.processIdentifier)
        try writeJSON(windows, to: output.appendingPathComponent("windows.json"))
        if let window = windows.first,
           let windowID = window[kCGWindowNumber as String] as? NSNumber {
            NSRunningApplication(processIdentifier: app.processIdentifier)?.activate(options: [.activateAllWindows])

            // Capture only this app's native window, including its rendered WKWebView.
            // A permission/capture failure is a test failure, not a skipped check.
            try? FileManager.default.removeItem(at: screenshot)
            let capture = Process()
            capture.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
            capture.arguments = ["-x", "-o", "-l", windowID.stringValue, screenshot.path]
            capture.standardOutput = captureLogHandle
            capture.standardError = captureLogHandle
            try capture.run()
            capture.waitUntilExit()

            if capture.terminationStatus == 0 && FileManager.default.fileExists(atPath: screenshot.path) {
                let request = VNRecognizeTextRequest()
                request.recognitionLevel = .accurate
                request.recognitionLanguages = ["en-US", "ja-JP"]
                request.usesLanguageCorrection = false
                try VNImageRequestHandler(url: screenshot, options: [:]).perform([request])
                let lines = (request.results ?? []).compactMap { $0.topCandidates(1).first?.string }
                try writeJSON(lines, to: output.appendingPathComponent("ocr.json"))
                let normalized = lines.joined().lowercased().filter { !$0.isWhitespace }
                // The heading alone could be the title bar. Rust/Tauri must also be
                // visible in the page body, so an empty WebView cannot pass.
                let missing = expected.filter { !normalized.contains($0) }
                if missing.isEmpty && !app.isTerminated {
                    if let scenario {
                        try recordingInteraction(pid: app.processIdentifier, windowID: windowID, scenario: scenario, output: output)
                    }
                    let result: [String: Any] = [
                        "status": "passed",
                        "pid": app.processIdentifier,
                        "windowID": windowID,
                        "expectedText": expected,
                        "recognizedText": lines,
                        "elapsedSeconds": Date().timeIntervalSince(started),
                        "screenshot": "window.png",
                    ]
                    try writeJSON(result, to: output.appendingPathComponent("result.json"))
                    print("PASS: bundled macOS app displayed the expected page; screenshot and OCR saved")
                    return
                }
                lastFailure = "Page text not yet visible; missing: \(missing.joined(separator: ", "))"
            } else {
                lastFailure = "Window capture failed (status \(capture.terminationStatus)); see capture.log"
            }
        }
        RunLoop.current.run(until: Date().addingTimeInterval(1))
    }
    throw SmokeFailure(description: "Timed out waiting for rendered app: \(lastFailure)")
}

guard CommandLine.arguments.count == 3
    || (CommandLine.arguments.count == 4 && ["models", "models-restore", "deny", "allow", "transcribe", "desktop", "insertion-denied", "insertion-grant", "insertion", "context-denied", "context", "context-restore", "ocr-denied", "ocr-grant", "ocr"].contains(CommandLine.arguments[3])) else {
    fputs("Usage: macos-window-smoke <app-bundle> <artifact-directory> [deny|allow|transcribe|desktop|insertion-denied|insertion-grant|insertion]\n", stderr)
    exit(2)
}

let appURL = URL(fileURLWithPath: CommandLine.arguments[1])
let output = URL(fileURLWithPath: CommandLine.arguments[2], isDirectory: true)
do {
    try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
    try smokeTest(appURL: appURL, output: output, scenario: CommandLine.arguments.count == 4 ? CommandLine.arguments[3] : nil)
} catch {
    try? writeJSON(["status": "failed", "error": String(describing: error)], to: output.appendingPathComponent("result.json"))
    fputs("FAIL: \(error)\n", stderr)
    exit(1)
}
