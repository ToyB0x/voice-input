// Hardware metadata only; no model, audio, image or user text is read.
import Foundation
import Metal
var report:[String:Any]=["os":ProcessInfo.processInfo.operatingSystemVersionString,"inferenceTested":false]
if let device=MTLCreateSystemDefaultDevice() {
    let apple7=device.supportsFamily(.apple7)
    let metal3=device.supportsFamily(.metal3)
    report.merge(["device":device.name,"apple7":apple7,"metal3":metal3,"quantizedMatmulCapability":apple7 || metal3,"maxBufferLength":device.maxBufferLength]) {_,new in new}
} else {
    report["quantizedMatmulCapability"]=false
    report["device"]="unavailable"
}
let data=try JSONSerialization.data(withJSONObject:report,options:[.prettyPrinted,.sortedKeys])
FileHandle.standardOutput.write(data)
FileHandle.standardOutput.write(Data("\n".utf8))
