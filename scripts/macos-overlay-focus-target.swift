import AppKit
import Foundation
let app=NSApplication.shared
app.setActivationPolicy(.regular)
let window=NSWindow(contentRect:NSRect(x:120,y:200,width:440,height:200),styleMask:[.titled,.closable],backing:.buffered,defer:false)
window.title="Overlay focus fixture"
let field=NSTextView(frame:NSRect(x:20,y:20,width:400,height:140))
field.string="Synthetic focus target. No microphone or screen access."
window.contentView?.addSubview(field)
window.makeKeyAndOrderFront(nil)
window.makeFirstResponder(field)
app.activate(ignoringOtherApps:true)
if CommandLine.arguments.count>1 {try String(ProcessInfo.processInfo.processIdentifier).write(toFile:CommandLine.arguments[1],atomically:true,encoding:.utf8)}
app.run()
