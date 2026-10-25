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
