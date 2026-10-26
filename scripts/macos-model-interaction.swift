import AppKit
import ApplicationServices
import Foundation

func chooseModelOption(_ root: AXUIElement, label:String, title:String) throws {
    revealProductControl(root, label)
    guard let popup=axElements(root).first(where:{axValue($0,kAXRoleAttribute) as? String == "AXPopUpButton" && axText($0).contains(label)}) else {throw SmokeFailure(description:"Missing model popup: "+label)}
    try press(popup)
    let normalizedTitle = title.precomposedStringWithCompatibilityMapping
    var option: AXUIElement?
    let deadline = Date().addingTimeInterval(3)
    repeat {
        // Recent WebKit menus can be detached from the application's AX tree.
        // Include the focused menu and its parents, while still matching the
        // exact option title before pressing it.
        var roots = [root, popup]
        var focused = axValue(AXUIElementCreateSystemWide(), kAXFocusedUIElementAttribute)
        for _ in 0..<4 {
            guard let value = focused, CFGetTypeID(value) == AXUIElementGetTypeID() else { break }
            let element = value as! AXUIElement
            roots.append(element)
            focused = axValue(element, kAXParentAttribute)
        }
        option = roots.flatMap(axElements).first { element in
            axValue(element,kAXRoleAttribute) as? String == "AXMenuItem" &&
            [kAXTitleAttribute,kAXValueAttribute,kAXDescriptionAttribute].contains {
                (axValue(element,$0) as? String)?.precomposedStringWithCompatibilityMapping == normalizedTitle
            }
        }
        if option != nil { break }
        RunLoop.current.run(until: Date().addingTimeInterval(0.1))
    } while Date() < deadline
    if let option {
        try press(option)
    } else {
        // Exercise native type-to-select when WebKit does not expose its menu
        // items. The selected popup value below remains a required assertion.
        let source = CGEventSource(stateID: .hidSystemState)
        let text = Array(title.utf16)
        for down in [true, false] {
            guard let event = CGEvent(keyboardEventSource: source, virtualKey: 0, keyDown: down) else {
                throw SmokeFailure(description: "Could not construct menu selection event")
            }
            text.withUnsafeBufferPointer {
                event.keyboardSetUnicodeString(stringLength: $0.count, unicodeString: $0.baseAddress!)
            }
            event.post(tap: .cghidEventTap)
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        try key(36) // Return commits the native menu selection.
    }
    try waitForUI("model option selected: " + title) {
        // Reacquire the popup from the live WebView tree. A cached AXValue
        // can already match while the native menu is still closing, especially
        // when selecting the option that was selected before opening the menu.
        guard let current = axElements(root).first(where: {
            axValue($0, kAXRoleAttribute) as? String == "AXPopUpButton" && axText($0).contains(label)
        }) else { return false }
        return axText(current).precomposedStringWithCompatibilityMapping.contains(normalizedTitle)
            && uiButton(root, ["Save preferences"]) != nil
    }
}

func modelTransportRunning(_ pid:pid_t)->Bool {
    let task=Process();task.executableURL=URL(fileURLWithPath:"/usr/bin/pgrep");task.arguments=["-P",String(pid),"-x","curl"]
    task.standardOutput=FileHandle.nullDevice;task.standardError=FileHandle.nullDevice
    do {try task.run();task.waitUntilExit();return task.terminationStatus==0} catch {return false}
}
func modelInteraction(pid:pid_t,scenario:String,output:URL) throws {
    let root=AXUIElementCreateApplication(pid)
    revealProductControl(root, "Model preset")
    let storage=FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support/dev.toyb0x.voiceinput/models")
    func noPartial()->Bool {((try? FileManager.default.contentsOfDirectory(atPath:storage.path)) ?? []).allSatisfy{!$0.hasSuffix(".part")}}
    func value(_ label:String)->String {textField(root,label:label).flatMap{axValue($0,kAXValueAttribute) as? String} ?? ""}
    func unchangedContext()->Bool {
        ["Local recognition context","Local OCR context","Use context in correction"].allSatisfy {label in
            guard let box=checkBox(root,label) else {return false}
            return (axValue(box,kAXValueAttribute) as? NSNumber)?.intValue==0
        }
    }
    let clipboard=NSPasteboard.general.string(forType:.string)
    try waitForUI("model catalog loaded") {axElements(root).contains(where:{axValue($0,kAXRoleAttribute) as? String == "AXPopUpButton" && axText($0).contains("Model preset") && (axValue($0,kAXEnabledAttribute) as? Bool)==true})}
    if scenario=="models" {
        let initialPath=value("Model path")
        for (title,expected) in [("認識重視（medium）","Whisper medium"),("大きい音声モデル（turbo）","Whisper large-v3-turbo"),("整形比較（medium + 4B）","Whisper medium")] {
            try chooseModelOption(root,label:"Model preset",title:title)
            try waitForUI("preset model selection") {axElements(root).contains(where:{axValue($0,kAXRoleAttribute) as? String == "AXPopUpButton" && axText($0).contains("Managed speech model") && axText($0).contains(expected)})}
            guard unchangedContext(),value("Model path")==initialPath else {throw SmokeFailure(description:"Preset changed saved paths or screen context")}
        }
        try chooseModelOption(root,label:"Model preset",title:"認識重視（medium）")
        try press(uiButton(root,["Download selected models"])!)
        try waitForUI("actual model download child",seconds:60) {modelTransportRunning(pid) && uiText(root).contains("公式配布元からモデルを取得")}
        try press(uiButton(root,["Cancel model preparation"])!)
        try waitForUI("model download cancelled and cleaned",seconds:20) {uiText(root).contains("取り消") && !modelTransportRunning(pid) && noPartial()}
        guard value("Model path")==initialPath else {throw SmokeFailure(description:"Cancelled preparation applied a model")}
        try interactionScreenshot(output,name:"cancelled.png")
        try chooseModelOption(root,label:"Model preset",title:"軽量（base）")
        try press(uiButton(root,["Download selected models"])!)
        try waitForUI("pinned model downloaded and verified",seconds:600) {uiButton(root,["Apply prepared models"]) != nil && uiText(root).contains("検証完了")}
        try press(uiButton(root,["Apply prepared models"])!)
        let path=value("Model path")
        guard path.hasPrefix(storage.path+"/"),path.hasSuffix("ggml-base.bin"),FileManager.default.fileExists(atPath:path),value("Correction model path").isEmpty,noPartial(),unchangedContext() else {throw SmokeFailure(description:"Unverified or unexpected model applied")}
        try chooseModelOption(root,label:"Inference backend",title:"CPU")
        try press(uiButton(root,["Save preferences"])!)
        try press(uiButton(root,["Load local model"])!)
        try waitForUI("prepared real Whisper model loads",seconds:120) {uiText(root).contains("モデル準備完了") && uiText(root).contains("モデル: base")}
        try interactionScreenshot(output,name:"prepared.png")
        try writeJSON(["status":"passed","downloadedAndVerified":true,"actualTransportCancelled":true,"partialFilesRemoved":true,"realModelLoaded":true,"contextUnchanged":true],to:output.appendingPathComponent("interaction.json"))
    } else {
        try chooseModelOption(root,label:"Model preset",title:"軽量（base）")
        try waitForUI("managed model preferences restored",seconds:120) {value("Model path").hasPrefix(storage.path+"/") && uiText(root).contains("モデル準備完了")}
        guard !modelTransportRunning(pid) else {throw SmokeFailure(description:"Restore downloaded without an explicit request")}
        try press(uiButton(root,["Verify cached models"])!)
        try waitForUI("saved model verified without transport",seconds:60) {uiButton(root,["Apply prepared models"]) != nil}
        guard !modelTransportRunning(pid),unchangedContext(),noPartial() else {throw SmokeFailure(description:"Cached verification changed transport or context")}
        try interactionScreenshot(output,name:"restored.png")
        // Restore an empty model preference for the subsequent first-use
        // microphone tests, through the real product settings UI.
        try typeField(root,label:"Model path",value:"")
        try press(uiButton(root,["Save preferences"])!)
        try writeJSON(["status":"passed","modelRestored":true,"cachedVerification":true,"noDownloadOnRestore":true,"contextUnchanged":true],to:output.appendingPathComponent("interaction.json"))
    }
    guard NSPasteboard.general.string(forType:.string)==clipboard else {throw SmokeFailure(description:"Model setup changed clipboard")}
}
