import AppKit
import ApplicationServices
import Foundation

func comparisonInteraction(pid: pid_t, output: URL) throws {
    let root=AXUIElementCreateApplication(pid)
    try waitForUI("comparison opener") { uiButton(root,["比較画面を開く・閉じる"]) != nil }
    try press(uiButton(root,["比較画面を開く・閉じる"])!)
    try waitForUI("comparison controls") { uiButton(root,["すべてのパターンを選択"]) != nil }
    try press(uiButton(root,["すべてのパターンを選択"])!)
    try waitForUI("full Cartesian plan") { uiText(root).contains("1128 件を直列に実行") }
    try press(uiButton(root,["録音済み音声で直列比較を開始"])!)
    try waitForUI("empty audio rejected safely") { uiText(root).contains("先に比較する音声を録音してください") }
    guard uiText(root).contains("更新署名キー未設定") else {throw SmokeFailure(description:"Development updater should explicitly report missing signing configuration")}
    try interactionScreenshot(output,name:"comparison.png")
    try uiText(root).write(to:output.appendingPathComponent("comparison-state.txt"),atomically:true,encoding:.utf8)
    try writeJSON(["status":"passed","fullMatrixCount":1128,"emptyAudioRejected":true,"unsignedUpdaterDisabled":true,"inferenceTested":false],to:output.appendingPathComponent("interaction.json"))
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
