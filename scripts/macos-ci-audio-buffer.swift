// Disposable CI only. Increase the virtual device buffer, never ignore dropouts.
import CoreAudio
import Foundation

func check(_ status: OSStatus, _ operation: String) throws {
    if status != noErr {throw NSError(domain:operation,code:Int(status))}
}
func address(_ selector: AudioObjectPropertySelector) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress(mSelector:selector,mScope:kAudioObjectPropertyScopeGlobal,mElement:kAudioObjectPropertyElementMain)
}
var device=AudioDeviceID(0)
var property=address(kAudioHardwarePropertyDefaultInputDevice)
var size=UInt32(MemoryLayout<AudioDeviceID>.size)
try check(AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject),&property,0,nil,&size,&device),"default input")
var name:Unmanaged<CFString>?
property=address(kAudioObjectPropertyName)
size=UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
try check(AudioObjectGetPropertyData(device,&property,0,nil,&size,&name),"device name")
guard let value=name?.takeUnretainedValue(), value as String == "BlackHole 2ch" else {throw NSError(domain:"Expected synthetic BlackHole input",code:1)}
var range=AudioValueRange()
property=address(kAudioDevicePropertyBufferFrameSizeRange)
size=UInt32(MemoryLayout<AudioValueRange>.size)
try check(AudioObjectGetPropertyData(device,&property,0,nil,&size,&range),"buffer range")
property=address(kAudioDevicePropertyBufferFrameSize)
var settable=DarwinBoolean(false)
try check(AudioObjectIsPropertySettable(device,&property,&settable),"buffer mutability")
var frames=UInt32(0)
size=UInt32(MemoryLayout<UInt32>.size)
try check(AudioObjectGetPropertyData(device,&property,0,nil,&size,&frames),"current buffer")
let previous=frames
if settable.boolValue {
    frames=UInt32(min(range.mMaximum,max(range.mMinimum,2048)))
    try check(AudioObjectSetPropertyData(device,&property,0,nil,size,&frames),"CI virtual buffer size")
}
try check(AudioObjectGetPropertyData(device,&property,0,nil,&size,&frames),"applied buffer")
let report:[String:Any]=["device":"BlackHole 2ch","previousFrames":previous,"frames":frames,"settable":settable.boolValue]
let data=try JSONSerialization.data(withJSONObject:report,options:[.sortedKeys])
FileHandle.standardOutput.write(data)
FileHandle.standardOutput.write(Data("\n".utf8))
