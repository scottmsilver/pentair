import ActivityKit
import Foundation

struct SpaHeatAttributes: ActivityAttributes {
    struct ContentState: Codable, Hashable {
        var currentTempF: Int
        var targetTempF: Int
        var startTempF: Int
        var progressPct: Int
        var minutesRemaining: Int?
        var phase: String      // "started", "tracking", "reached"
        var milestone: String? // "heating_started", "halfway", "almost_ready", "at_temp"
    }

    var spaName: String
}
