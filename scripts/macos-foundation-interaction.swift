import AppKit
import ApplicationServices
import Foundation

func foundationInteraction(pid: pid_t, output: URL) throws {
    let root = AXUIElementCreateApplication(pid)
    let original = resultText(root)
    try chooseModelOption(root, label: "Correction engine", title: "Apple Foundation Models（実験的）")
    try press(uiButton(root, ["Save preferences"])!)
    try waitForUI("Apple engine saved") { uiText(root).contains("設定を保存しました") }
    guard let disclosure = axElements(root).first(where: {
        axValue($0, kAXRoleAttribute) as? String == "AXDisclosureTriangle" && axText($0).contains("Appleモデルの診断")
    }) else { throw SmokeFailure(description: "Missing Apple model diagnostics disclosure") }
    try press(disclosure)
    try waitForUI("Apple diagnostics expanded") { uiButton(root, ["Check Apple model availability"]) != nil }
    try press(uiButton(root, ["Check Apple model availability"])!)
    try waitForUI("real Apple helper status", seconds: 20) {
        uiText(root).contains("osTooOld") && uiText(root).contains("framework_imported")
    }
    try press(uiButton(root, ["Copy Apple model diagnostics"])!)
    var diagnostics: [String: Any] = [:]
    try waitForUI("diagnostics copied") {
        guard let value = NSPasteboard.general.string(forType: .string),
              let data = value.data(using: .utf8), let parsed = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return false }
        diagnostics = parsed
        return parsed["selected_engine"] as? String == "apple-foundation"
    }
    guard let availability = diagnostics["availability"] as? [String: Any],
          availability["availability"] as? String == "osTooOld",
          availability["framework_imported"] as? Bool == true,
          Set(diagnostics.keys) == Set(["app_version", "diagnostic_version", "selected_engine", "availability", "working", "checking_availability", "last_elapsed_ms", "last_error"]),
          resultText(root) == original else { throw SmokeFailure(description: "Unsafe or incomplete Apple model diagnostics") }
    try writeJSON(diagnostics, to: output.appendingPathComponent("diagnostics.json"))
    try interactionScreenshot(output, name: "diagnostics.png")
    // Subsequent full audio/OCR scenarios retain their original Qwen configuration.
    try chooseModelOption(root, label: "Correction engine", title: "Qwen3（ローカルGGUF）")
    try press(uiButton(root, ["Save preferences"])!)
    try waitForUI("Qwen engine restored") { uiText(root).contains("設定を保存しました") }
    try writeJSON(["status":"passed", "realHelper":true, "frameworkCompiled":true,
                   "inferenceTested":false, "engineSelectionSaved":true,
                   "diagnosticsCopiedWithoutBody":true, "originalUnchanged":true],
                  to: output.appendingPathComponent("interaction.json"))
}
