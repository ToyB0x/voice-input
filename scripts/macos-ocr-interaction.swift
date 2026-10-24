import AppKit
import ApplicationServices
import Foundation

func checkBox(_ root:AXUIElement,_ label:String)->AXUIElement? {
    axElements(root).first {axValue($0,kAXRoleAttribute) as? String == "AXCheckBox" && axText($0).contains(label)}
}
func setCheckBox(_ root:AXUIElement,_ label:String,_ value:Bool) throws {
    guard let control=checkBox(root,label) else {throw SmokeFailure(description:"Missing preference: "+label)}
    if ((axValue(control,kAXValueAttribute) as? NSNumber)?.intValue==1) != value {try press(control)}
}
func configureOCR(_ root:AXUIElement, enabled:Bool, correction:Bool=false) throws {
    try waitForUI("OCR model ready",seconds:120) {uiText(root).contains("モデル準備完了")}
    try typeField(root,label:"Recording shortcut",value:"Control+Alt+KeyV")
    try setCheckBox(root,"Automatic text insertion",false)
    try setCheckBox(root,"Local recognition context",false)
    try setCheckBox(root,"Local OCR context",enabled)
    try setCheckBox(root,"Use context in Whisper",true)
    try setCheckBox(root,"Use context in correction",correction)
    try pressContextButton(root,"Save preferences")
    try waitForUI("OCR preference applied") {uiText(root).contains(enabled ? "OCR: 有効" : "OCR: 無効") && uiText(root).contains("直接入力: 無効")}
}
func productChildRunning(_ pid:pid_t)->Bool {
    let process=Process();process.executableURL=URL(fileURLWithPath:"/usr/bin/pgrep")
    process.arguments=["-P",String(pid),"-x","local-correction"]
    process.standardOutput=FileHandle.nullDevice;process.standardError=FileHandle.nullDevice
    do {try process.run();process.waitUntilExit();return process.terminationStatus==0} catch {return false}
}
func grantScreenPermission(_ voice:AXUIElement,output:URL) throws {
    try pressContextButton(voice,"Request screen permission")
    var settings:AXUIElement?
    try waitForUI("Screen Recording system settings",seconds:40) {
        for process in NSWorkspace.shared.runningApplications {
            let root=AXUIElementCreateApplication(process.processIdentifier)
            let text=uiText(root)
            if text.contains("Voice Input") && (text.lowercased().contains("screen") || text.contains("画面")),
               let open=uiButton(root,["Open System Settings","Open System Preferences","システム設定を開く"]) {try press(open)}
        }
        guard let app=NSWorkspace.shared.runningApplications.first(where:{$0.bundleIdentifier=="com.apple.systempreferences"}) else {return false}
        settings=AXUIElementCreateApplication(app.processIdentifier)
        let text=uiText(settings!).lowercased()
        return text.contains("screen recording") || text.contains("screen & system audio") || text.contains("画面収録")
    }
    var authorizationAttempted=false
    do {
        var toggle:AXUIElement?
        try waitForUI("product Screen Recording switch",seconds:40) {
            let nodes=axElements(settings!)
            toggle=nodes.first { ["AXCheckBox","AXSwitch"].contains(axValue($0,kAXRoleAttribute) as? String ?? "") && axText($0).contains("Voice Input") }
            if toggle != nil {return true}
            for container in nodes.reversed() {
                let children=axElements(container)
                if children.count<50 && children.map(axText).joined().contains("Voice Input"),
                   let control=children.first(where:{["AXCheckBox","AXSwitch"].contains(axValue($0,kAXRoleAttribute) as? String ?? "")}) {toggle=control;return true}
            }
            for app in NSWorkspace.shared.runningApplications {
                let root=AXUIElementCreateApplication(app.processIdentifier)
                let text=uiText(root)
                if text.contains("Voice Input") && (text.localizedCaseInsensitiveContains("screen") || text.contains("画面")),
                   let button=uiButton(root,["Open System Settings","Open System Preferences","システム設定を開く"]) {try press(button)}
            }
            return false
        }
        if (axValue(toggle!,kAXValueAttribute) as? NSNumber)?.intValue != 1 {try press(toggle!)}
        authorizationAttempted=true
        // Some macOS revisions ask for admin confirmation; others grant directly.
        var admin=false
        try waitForUI("Screen Recording consent transition",seconds:30) {
            for app in NSWorkspace.shared.runningApplications {
                let root=AXUIElementCreateApplication(app.processIdentifier)
                if uiText(root).contains("trying to modify your system settings"), uiButton(root,["Modify Settings"]) != nil {admin=true;return true}
            }
            return (axValue(toggle!,kAXValueAttribute) as? NSNumber)?.intValue==1
        }
        if admin {try authorizeCISettingsChange()}
        try waitForUI("Screen Recording switch on") {(axValue(toggle!,kAXValueAttribute) as? NSNumber)?.intValue==1}
        for app in NSWorkspace.shared.runningApplications {
            let root=AXUIElementCreateApplication(app.processIdentifier)
            if uiText(root).contains("Voice Input") {
                if let later=uiButton(root,["Later","Not Now","あとで"]) {try press(later)}
                else if let restart=uiButton(root,["Quit & Reopen","Quit and Reopen","終了して再度開く"]) {try press(restart)}
            }
        }
        try writeJSON(["status":"passed","method":"System Settings real Screen Recording consent","enabled":true],to:output.appendingPathComponent("permission.json"))
        try interactionScreenshot(output,name:"screen-permission.png")
    } catch {
        // Never capture an authentication form or ephemeral credential.
        if !authorizationAttempted {try? uiText(settings!).write(to:output.appendingPathComponent("settings-before-auth.txt"),atomically:true,encoding:.utf8)}
        throw error
    }
}
func ocrInteraction(pid:pid_t,scenario:String,output:URL) throws {
    let voice=AXUIElementCreateApplication(pid)
    if scenario=="ocr-grant" {try grantScreenPermission(voice,output:output);return}
    try configureOCR(voice,enabled:true,correction:scenario=="ocr")
    guard let fixturePath=ProcessInfo.processInfo.environment["VOICE_INPUT_OCR_TARGET"] else {throw SmokeFailure(description:"Missing OCR fixture")}
    let stateURL=output.appendingPathComponent("fixture-state.json")
    let launch=Process();launch.executableURL=URL(fileURLWithPath:"/usr/bin/open")
    launch.arguments=["-n","-a",fixturePath,"--args",stateURL.path]
    try launch.run();launch.waitUntilExit()
    var fixture:NSRunningApplication?
    try waitForUI("OCR fixture") {fixture=NSWorkspace.shared.runningApplications.first{$0.bundleIdentifier=="dev.toyb0x.ocrfixture"};return fixture != nil && FileManager.default.fileExists(atPath:stateURL.path)}
    defer {fixture?.forceTerminate()}
    let target=AXUIElementCreateApplication(fixture!.processIdentifier)
    func field(_ label:String)throws->AXUIElement {
        guard let value=axElements(target).first(where:{axValue($0,kAXDescriptionAttribute) as? String==label}) else {throw SmokeFailure(description:"Missing OCR field")}
        return value
    }
    func state()throws->[String:Any] {try JSONSerialization.jsonObject(with:Data(contentsOf:stateURL)) as! [String:Any]}
    func show()throws {
        if visibleWindows(for:pid).isEmpty {try trayAction(voice,title:"設定・認識結果を表示")}
        try waitForUI("OCR app shown") {!visibleWindows(for:pid).isEmpty}
    }
    try hideInsertionWindow(voice)
    try focusElement(target,field("OCR input"),pid:fixture!.processIdentifier)
    try setRange(field("OCR input"),0,0)
    try recordForInsertion(voice)
    try show()
    if scenario=="ocr-denied" {
        try waitForUI("OCR denied but ASR continues") {uiText(voice).contains("画面収録が未許可") && fixtureRecognized(resultText(voice))}
        try interactionScreenshot(output,name:"denied.png")
        try pressContextButton(voice,"Request screen permission")
        try writeJSON(["status":"passed","audioContinues":true,"permissionDenied":true],to:output.appendingPathComponent("interaction.json"))
        return
    }
    try waitForUI("real product OCR result") {uiText(voice).contains("OCR: 取得済み") && uiText(voice).contains("画面収録許可済み") && fixtureRecognized(resultText(voice))}
    let productState=uiText(voice)
    let expression=try NSRegularExpression(pattern:"OCR: 取得済み（([0-9]+)文字）")
    guard let match=expression.firstMatch(in:productState,range:NSRange(productState.startIndex...,in:productState)),
          let range=Range(match.range(at:1),in:productState), let count=Int(productState[range]), (8...22).contains(count) else {throw SmokeFailure(description:"OCR crop included unexpected surrounding text")}
    let capturedValues=try state()
    guard capturedValues["valueReads"] as? Int==0, capturedValues["rangeReads"] as? Int==0 else {throw SmokeFailure(description:"OCR-only capture read AX text")}
    try interactionScreenshot(output,name:"ocr-preview.png")
    let raw=resultText(voice)
    guard let model=ProcessInfo.processInfo.environment["VOICE_INPUT_CORRECTION_MODEL"] else {throw SmokeFailure(description:"Missing correction model")}
    try typeField(voice,label:"Correction model path",value:model)
    guard let backend=axElements(voice).first(where:{axValue($0,kAXRoleAttribute) as? String=="AXPopUpButton" && axText($0).contains("Correction backend")}) else {throw SmokeFailure(description:"Correction backend control missing")}
    AXUIElementSetAttributeValue(backend,kAXFocusedAttribute as CFString,kCFBooleanTrue)
    try press(backend)
    var cpu:AXUIElement?
    try waitForUI("correction CPU menu") {
        cpu=(axElements(voice)+axElements(backend)).first {element in
            axValue(element,kAXRoleAttribute) as? String=="AXMenuItem" && [kAXTitleAttribute,kAXDescriptionAttribute,kAXValueAttribute].contains {axValue(element,$0) as? String=="CPU"}
        }
        return cpu != nil
    }
    guard let cpu else {throw SmokeFailure(description:"Correction CPU option missing")}
    try press(cpu)
    try waitForUI("correction CPU selected") {axText(backend).contains("CPU")}
    try pressContextButton(voice,"Save preferences")
    try pressContextButton(voice,"Preview local correction")
    try waitForUI("native correction preview",seconds:120) {
        let fields=axElements(voice)
        return fields.contains{axValue($0,kAXRoleAttribute) as? String=="AXTextArea" && axText($0).contains("Corrected text") && !(axValue($0,kAXValueAttribute) as? String ?? "").isEmpty} && uiText(voice).contains("補正プレビュー:")
    }
    guard resultText(voice)==raw, NSPasteboard.general.string(forType:.string)=="insertion-clipboard-sentinel" else {throw SmokeFailure(description:"Correction changed raw text or clipboard without explicit copy")}
    let corrected=axElements(voice).first{axValue($0,kAXRoleAttribute) as? String=="AXTextArea" && axText($0).contains("Corrected text")}.flatMap{axValue($0,kAXValueAttribute) as? String} ?? ""
    guard !corrected.contains("OUTSIDE"), !corrected.contains("SYNTHETIC-PASSWORD") else {throw SmokeFailure(description:"Correction included data outside crop")}
    try pressContextButton(voice,"Copy correction")
    try waitForUI("explicit correction copy") {NSPasteboard.general.string(forType:.string)==corrected}
    try interactionScreenshot(output,name:"corrected.png")
    // Start a real native model process, then discard and require its exit.
    try pressContextButton(voice,"Preview local correction")
    try waitForUI("native correction child started") {productChildRunning(pid)}
    try pressContextButton(voice,"Discard recording")
    try waitForUI("correction child cancelled",seconds:15) {!productChildRunning(pid) && resultText(voice).isEmpty && !uiText(voice).contains("ローカル補正中")}
    let afterCorrection=try state()
    guard afterCorrection["unchanged"] as? Bool==true, afterCorrection["secureUnchanged"] as? Bool==true else {throw SmokeFailure(description:"OCR modified source")}
    var exclusions=[String]()
    for label in ["Protected OCR input","Read only OCR input"] {
        try hideInsertionWindow(voice)
        try focusElement(target,field(label),pid:fixture!.processIdentifier)
        try recordForInsertion(voice)
        try show()
        try waitForUI("OCR protected target excluded") {uiText(voice).contains("OCR: 安全な撮影対象がない") && fixtureRecognized(resultText(voice))}
        exclusions.append(label)
    }
    let excludedValues=try state()
    guard excludedValues["valueReads"] as? Int==0, excludedValues["rangeReads"] as? Int==0, excludedValues["secureReads"] as? Int==0 else {throw SmokeFailure(description:"Protected OCR case read AX text")}
    try configureOCR(voice,enabled:false)
    try writeJSON(["status":"passed","excluded":exclusions,"ocrCharacters":count,"axTextReads":0,"rawPreserved":true,"explicitCopy":true,"childCancellation":true],to:output.appendingPathComponent("interaction.json"))
}
