// Public synthetic input only. Never enables Apple Intelligence or signs in.
import Foundation
#if canImport(FoundationModels)
import FoundationModels
#endif

@main struct Probe {
    static func main() async {
        var report: [String: Any] = [
            "os": ProcessInfo.processInfo.operatingSystemVersionString,
            "architecture": "\(ProcessInfo.processInfo.machineArchitecture)",
            "physicalMemoryBytes": ProcessInfo.processInfo.physicalMemory,
            "inferenceTested": false
        ]
        #if canImport(FoundationModels)
        report["frameworkImported"] = true
        if #available(macOS 26.0, *) {
            switch SystemLanguageModel.default.availability {
            case .available:
                report["availability"] = "available"
                do {
                    let session = LanguageModelSession(instructions:
                        "日本語の文字起こしの句読点だけを整えてください。事実・数字・否定を変更せず、補正した本文だけを返してください。")
                    let started = ContinuousClock.now
                    let response = try await session.respond(
                        to: "明日の会議は10時です資料を準備してください",
                        options: GenerationOptions(temperature: 0))
                    report["inferenceTested"] = true
                    report["response"] = response.content
                    report["elapsed"] = String(describing: started.duration(to: .now))
                } catch {
                    report["generationError"] = String(describing: error)
                }
            case .unavailable(let reason):
                switch reason {
                case .deviceNotEligible: report["availability"] = "deviceNotEligible"
                case .appleIntelligenceNotEnabled: report["availability"] = "appleIntelligenceNotEnabled"
                case .modelNotReady: report["availability"] = "modelNotReady"
                @unknown default: report["availability"] = "unknownUnavailableReason"
                }
            @unknown default: report["availability"] = "unknownAvailability"
            }
        } else {
            report["availability"] = "osTooOld"
        }
        #else
        report["frameworkImported"] = false
        report["availability"] = "frameworkMissingFromSDK"
        #endif
        do {
            let data = try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
            FileHandle.standardOutput.write(data)
            FileHandle.standardOutput.write(Data("\n".utf8))
            // Unavailable is evidence, not successful model inference.
            if report["generationError"] != nil { exit(1) }
        } catch { exit(1) }
    }
}

extension ProcessInfo {
    var machineArchitecture: String {
        #if arch(arm64)
        return "arm64"
        #elseif arch(x86_64)
        return "x86_64"
        #else
        return "other"
        #endif
    }
}
