// Public synthetic data only. This executable is never part of the product.
import AppKit
import Foundation
var valueReads=0
var rangeReads=0
final class OCRInput: NSTextView {
    override func accessibilityValue() -> String? { valueReads += 1; return super.accessibilityValue() }
    // Make AX text extraction unavailable: the product must use real OCR.
    override func accessibilityString(for range:NSRange) -> String? {rangeReads += 1; return nil}
}
var secureReads=0
final class ProtectedOCRField:NSSecureTextField {
    override func accessibilityValue()->String? {secureReads += 1;return super.accessibilityValue()}
}
let app=NSApplication.shared
app.setActivationPolicy(.regular)
let window=NSWindow(contentRect:NSRect(x:100,y:130,width:620,height:430),styleMask:[.titled,.closable],backing:.buffered,defer:false)
window.title="Local OCR Fixture"
let canary=NSTextField(labelWithString:"OUTSIDE CROP CANARY / DO NOT READ")
canary.font=NSFont.systemFont(ofSize:26)
canary.frame=NSRect(x:20,y:350,width:580,height:45)
window.contentView!.addSubview(canary)
let original="月日、百代、過客、旅人。"
let text=OCRInput(frame:NSRect(x:20,y:230,width:560,height:90))
text.string=original;text.font=NSFont.systemFont(ofSize:26);text.isRichText=false
text.setAccessibilityLabel("OCR input")
window.contentView!.addSubview(text)
let readonly=OCRInput(frame:NSRect(x:20,y:120,width:560,height:75))
readonly.string="READONLY SYNTHETIC CANARY";readonly.isEditable=false
readonly.setAccessibilityLabel("Read only OCR input")
window.contentView!.addSubview(readonly)
let secure=ProtectedOCRField(frame:NSRect(x:20,y:50,width:560,height:35))
secure.stringValue="SYNTHETIC-PASSWORD";secure.setAccessibilityLabel("Protected OCR input")
window.contentView!.addSubview(secure)
let state=URL(fileURLWithPath:CommandLine.arguments[1])
let timer=Timer.scheduledTimer(withTimeInterval:0.05,repeats:true) {_ in
    let values:[String:Any]=["valueReads":valueReads,"rangeReads":rangeReads,"secureReads":secureReads,"unchanged":text.string==original,"secureUnchanged":secure.stringValue=="SYNTHETIC-PASSWORD"]
    if let data=try? JSONSerialization.data(withJSONObject:values,options:[.sortedKeys]) {try? data.write(to:state,options:.atomic)}
}
window.makeKeyAndOrderFront(nil)
app.activate(ignoringOtherApps:true)
app.run()
