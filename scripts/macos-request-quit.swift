import AppKit
import Foundation

guard CommandLine.arguments.count == 3,
      let pid = Int32(CommandLine.arguments[1]) else { exit(2) }
let deadline = Date().addingTimeInterval(10)
var application: NSRunningApplication?
repeat {
    application = NSRunningApplication(processIdentifier: pid)
    if application?.isFinishedLaunching == true { break }
    RunLoop.current.run(until: Date().addingTimeInterval(0.1))
} while Date() < deadline
guard let app = application, app.isFinishedLaunching, !app.isTerminated else { exit(3) }
if CommandLine.arguments[2] == "quit" {
    // The app can defer native termination while releasing its resources.
    // The parent probe requires eventual exit code 0 and completed cleanup.
    _ = app.terminate()
} else if CommandLine.arguments[2] != "ready" { exit(2) }
