import AppKit
import ApplicationServices
import Foundation

func comparisonInteraction(pid: pid_t, output: URL) throws {
    let root=AXUIElementCreateApplication(pid)
    try waitForUI("comparison opener") { uiButton(root,["比較画面を開く・閉じる"]) != nil }
    try press(uiButton(root,["比較画面を開く・閉じる"])!)
    try waitForUI("comparison controls") { uiButton(root,["すべてのパターンを選択"]) != nil }
    try press(uiButton(root,["Appleモデルの比較条件を選ぶ"])!)
    try waitForUI("guided Apple context comparison") { uiText(root).contains("6 件を直列に実行") }
    try press(uiButton(root,["設定中のQwenの比較条件を選ぶ"])!)
    try waitForUI("guided Qwen context comparison") { uiText(root).contains("6 件を直列に実行") }
    try press(uiButton(root,["すべてのパターンを選択"])!)
    try waitForUI("full Cartesian plan") { uiText(root).contains("1128 件を直列に実行") }
    try press(uiButton(root,["録音済み音声で直列比較を開始"])!)
    try waitForUI("empty audio rejected safely") { uiText(root).contains("先に比較する音声を録音してください") }
    try waitForUI("development updater reports missing signing configuration") {uiText(root).contains("更新署名キー未設定")}
    try interactionScreenshot(output,name:"comparison.png")
    try uiText(root).write(to:output.appendingPathComponent("comparison-state.txt"),atomically:true,encoding:.utf8)
    let selftestInferenceTested = try fixedCorrectionInteraction(root:root,output:output)
    try writeJSON(["status":"passed","fullMatrixCount":1128,"guidedPresetCount":6,"emptyAudioRejected":true,"unsignedUpdaterDisabled":true,"fixedSampleCount":5,"inferenceTested":selftestInferenceTested],to:output.appendingPathComponent("interaction.json"))
}

func comparisonOfRecordedAudio(root: AXUIElement, output: URL) throws {
    let original=resultText(root)
    try press(uiButton(root,["Save preferences"])!)
    try waitForUI("comparison current model saved") {uiText(root).contains("設定を保存しました")}
    try press(uiButton(root,["比較画面を開く・閉じる"])!)
    try waitForUI("single default comparison plan") {uiText(root).contains("1 件を直列に実行")}
    try press(uiButton(root,["録音済み音声で直列比較を開始"])!)
    try waitForUI("real recorded audio comparison",seconds:120) {uiText(root).contains("done: 1/1") && uiButton(root,["比較レポートJSONをコピー"]) != nil}
    guard resultText(root)==original else {throw SmokeFailure(description:"Comparison changed the ordinary transcript")}
    NSPasteboard.general.clearContents()
    try press(uiButton(root,["比較レポートJSONをコピー"])!)
    var report:[String:Any]=[:]
    try waitForUI("body-free comparison report") {
        guard let text=NSPasteboard.general.string(forType:.string),let data=text.data(using:.utf8),let parsed=try? JSONSerialization.jsonObject(with:data) as? [String:Any] else{return false}
        report=parsed;return parsed["schemaVersion"] as? Int == 1
    }
    guard report["includesText"] as? Bool == false,report["hasReference"] as? Bool == false,
          let rows=report["rows"] as? [[String:Any]],rows.count==1,rows[0]["status"] as? String == "uncorrected",
          rows[0]["raw"]==nil,rows[0]["candidate"]==nil,rows[0]["finalText"]==nil,
          let elapsed=rows[0]["asrMs"] as? Int,elapsed>0 else{throw SmokeFailure(description:"Invalid comparison result or leaked body in the default report")}
    try writeJSON(report,to:output.appendingPathComponent("comparison-report.json"))
    try interactionScreenshot(output,name:"recorded-comparison.png")
    try press(uiButton(root,["比較結果を破棄"])!)
    try waitForUI("comparison report discarded") {uiButton(root,["比較レポートJSONをコピー"]) == nil}
    try press(uiButton(root,["比較画面を開く・閉じる"])!)
    // Comparison released the ordinary model; restore it before the pre-existing cancellation check.
    try press(uiButton(root,["Load local model"])!)
    try waitForUI("ordinary model restored",seconds:90) {uiText(root).contains("モデル準備完了") && uiButton(root,["Transcribe recording"]) != nil}
}

// No microphone, screen-consent or Accessibility grant to the product is required.
func fixedCorrectionInteraction(root: AXUIElement, output: URL) throws -> Bool {
    try press(uiButton(root,["Appleモデルの固定サンプル診断"])!)
    try waitForUI("fixed correction without recording",seconds:810) {uiText(root).contains("固定サンプル補正 / done: 5/5")}
    NSPasteboard.general.clearContents()
    try press(uiButton(root,["比較レポートJSONをコピー"])!)
    var report:[String:Any]=[:]
    try waitForUI("fixed correction report") {
        guard let text=NSPasteboard.general.string(forType:.string),let data=text.data(using:.utf8),let parsed=try? JSONSerialization.jsonObject(with:data) as? [String:Any] else{return false}
        report=parsed;return parsed["mode"] as? String == "fixed-correction"
    }
    guard report["sampleSetVersion"] as? Int == 1,report["includesText"] as? Bool == false,
          report["audioMs"] as? Int == 0,report["axCharacters"] as? Int == 0,report["ocrCharacters"] as? Int == 0,
          let rows=report["rows"] as? [[String:Any]],rows.count==5,
          rows.allSatisfy({$0["sampleId"] is String && $0["asrMs"] is NSNull && $0["raw"]==nil && $0["candidate"]==nil && $0["finalText"]==nil})
        else {throw SmokeFailure(description:"Fixed correction used unexpected inputs or leaked report body")}
    try writeJSON(report,to:output.appendingPathComponent("fixed-correction-report.json"))
    try interactionScreenshot(output,name:"fixed-correction.png")
    return rows.contains { ["accepted","rejected"].contains($0["status"] as? String ?? "") }
}
