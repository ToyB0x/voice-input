import AppKit
import ApplicationServices
import Foundation

func foundationInteraction(pid: pid_t, output: URL) throws {
    // Compare the UI with the real helper probe from this host, rather than
    // assuming every CI runner is on macOS 14.
    guard let path = ProcessInfo.processInfo.environment["VOICE_INPUT_FOUNDATION_VERIFICATION"],
          let probe = try JSONSerialization.jsonObject(with: Data(contentsOf: URL(fileURLWithPath: path))) as? [String: Any],
          let expected = probe["availability"] as? [String: Any],
          let expectedReason = expected["availability"] as? String,
          expected["framework_imported"] as? Bool == true
    else { throw SmokeFailure(description: "Missing verified Apple helper status for this host") }
    let root = AXUIElementCreateApplication(pid)
    let original = resultText(root)
    try chooseModelOption(root, label: "Correction engine", title: "Apple Foundation Models（実験的）")
    try press(uiButton(root, ["Save preferences"])!)
    try waitForUI("Apple engine saved") { uiText(root).contains("設定を保存しました") }
    try waitForUI("Apple diagnostics expanded") { uiButton(root, ["Check Apple model availability"]) != nil }
    try press(uiButton(root, ["Check Apple model availability"])!)
    try waitForUI("real Apple helper status", seconds: 20) {
        uiText(root).contains(expectedReason) && uiText(root).contains("framework_imported")
    }
    try press(uiButton(root, ["Copy processing diagnostics"])!)
    var diagnostics: [String: Any] = [:]
    try waitForUI("diagnostics copied") {
        guard let value = NSPasteboard.general.string(forType: .string),
              let data = value.data(using: .utf8), let parsed = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return false }
        diagnostics = parsed
        return parsed["selected_engine"] as? String == "apple-foundation"
    }
    guard let availability = diagnostics["availability"] as? [String: Any],
          NSDictionary(dictionary: availability).isEqual(NSDictionary(dictionary: expected)),
          availability["framework_imported"] as? Bool == true,
          Set(diagnostics.keys) == Set(["app_version", "diagnostic_version", "selected_engine", "availability", "working", "checking_availability", "last_elapsed_ms", "last_error", "timings", "reuse_model_enabled", "recognition_ms"]),
          diagnostics["diagnostic_version"] as? Int == 2,
          diagnostics["reuse_model_enabled"] as? Bool == false,
          resultText(root) == original else { throw SmokeFailure(description: "Unsafe or incomplete Apple model diagnostics") }
    try writeJSON(diagnostics, to: output.appendingPathComponent("diagnostics.json"))
    try interactionScreenshot(output, name: "diagnostics.png")
    // Subsequent full audio/OCR scenarios retain their original Qwen configuration.
    try chooseModelOption(root, label: "Correction engine", title: "Qwen3（ローカルGGUF）")
    try press(uiButton(root, ["Save preferences"])!)
    try waitForUI("Qwen engine restored") { uiText(root).contains("設定を保存しました") }
    try waitForUI("diagnostic controls ready after restore") { uiButton(root, ["Copy processing diagnostics"]) != nil }
    try press(uiButton(root, ["Copy processing diagnostics"])!)
    try waitForUI("Qwen preference actually restored") {
        guard let value = NSPasteboard.general.string(forType: .string), let data = value.data(using: .utf8),
              let current = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return false }
        return current["selected_engine"] as? String == "qwen3"
    }
    try writeJSON(["status":"passed", "realHelper":true, "frameworkCompiled":true,
                   "inferenceTested":false, "availability":expectedReason, "engineSelectionSaved":true,
                   "diagnosticsCopiedWithoutBody":true, "originalUnchanged":true],
                  to: output.appendingPathComponent("interaction.json"))
}
