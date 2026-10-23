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

func smokeTest(appURL: URL, output: URL) throws {
    guard let bundle = Bundle(url: appURL),
          let executable = bundle.executableURL
    else { throw SmokeFailure(description: "Missing app bundle or executable: \(appURL.path)") }

    let appLog = output.appendingPathComponent("app.log")
    FileManager.default.createFile(atPath: appLog.path, contents: nil)
    let appLogHandle = try FileHandle(forWritingTo: appLog)
    defer { try? appLogHandle.close() }

    // Launch the bundled executable, never a browser or the Vite development server.
    let app = Process()
    app.executableURL = executable
    app.standardOutput = appLogHandle
    app.standardError = appLogHandle
    try app.run()
    defer {
        if app.isRunning {
            app.terminate()
            Thread.sleep(forTimeInterval: 0.2)
            if app.isRunning { kill(app.processIdentifier, SIGKILL) }
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
        guard app.isRunning else {
            throw SmokeFailure(description: "App exited before readiness (status \(app.terminationStatus)); see app.log")
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
                if missing.isEmpty && app.isRunning {
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
        Thread.sleep(forTimeInterval: 1)
    }
    throw SmokeFailure(description: "Timed out waiting for rendered app: \(lastFailure)")
}

guard CommandLine.arguments.count == 3 else {
    fputs("Usage: macos-window-smoke <app-bundle> <artifact-directory>\n", stderr)
    exit(2)
}

let appURL = URL(fileURLWithPath: CommandLine.arguments[1])
let output = URL(fileURLWithPath: CommandLine.arguments[2], isDirectory: true)
do {
    try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
    try smokeTest(appURL: appURL, output: output)
} catch {
    try? writeJSON(["status": "failed", "error": String(describing: error)], to: output.appendingPathComponent("result.json"))
    fputs("FAIL: \(error)\n", stderr)
    exit(1)
}
