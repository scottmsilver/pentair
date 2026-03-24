import Foundation

struct PoolSystem: Decodable {
    let pool: BodyState?
    let spa: SpaState?
    let lights: LightState?
    let auxiliaries: [AuxiliaryState]
    let pump: PumpInfo?
    let system: SystemInfo
}

struct BodyState: Decodable {
    let on: Bool
    let active: Bool
    let temperature: Int
    let temperatureReliable: Bool
    let temperatureReason: String?
    let lastReliableTemperature: Int?
    let lastReliableTemperatureAtUnixMs: Int64?
    let setpoint: Int
    let heatMode: String
    let heating: String
    let heatEstimate: HeatEstimate?
    let temperatureDisplay: TemperatureDisplay
    let heatEstimateDisplay: HeatEstimateDisplay

    init(
        on: Bool,
        active: Bool,
        temperature: Int,
        temperatureReliable: Bool = true,
        temperatureReason: String? = nil,
        lastReliableTemperature: Int? = nil,
        lastReliableTemperatureAtUnixMs: Int64? = nil,
        setpoint: Int,
        heatMode: String,
        heating: String,
        heatEstimate: HeatEstimate? = nil,
        temperatureDisplay: TemperatureDisplay = .init(value: nil, isStale: false, staleReason: nil, lastReliableAtUnixMs: nil),
        heatEstimateDisplay: HeatEstimateDisplay = .init(state: "unavailable", reason: nil, availableInSeconds: nil, minutesRemaining: nil, targetTemperature: nil)
    ) {
        self.on = on
        self.active = active
        self.temperature = temperature
        self.temperatureReliable = temperatureReliable
        self.temperatureReason = temperatureReason
        self.lastReliableTemperature = lastReliableTemperature
        self.lastReliableTemperatureAtUnixMs = lastReliableTemperatureAtUnixMs
        self.setpoint = setpoint
        self.heatMode = heatMode
        self.heating = heating
        self.heatEstimate = heatEstimate
        self.temperatureDisplay = temperatureDisplay
        self.heatEstimateDisplay = heatEstimateDisplay
    }

    private enum CodingKeys: String, CodingKey {
        case on
        case active
        case temperature
        case temperatureReliable
        case temperatureReason
        case lastReliableTemperature
        case lastReliableTemperatureAtUnixMs
        case setpoint
        case heatMode
        case heating
        case heatEstimate
        case temperatureDisplay
        case heatEstimateDisplay
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        on = try container.decode(Bool.self, forKey: .on)
        active = try container.decode(Bool.self, forKey: .active)
        temperature = try container.decode(Int.self, forKey: .temperature)
        temperatureReliable = try container.decodeIfPresent(Bool.self, forKey: .temperatureReliable) ?? true
        temperatureReason = try container.decodeIfPresent(String.self, forKey: .temperatureReason)
        lastReliableTemperature = try container.decodeIfPresent(Int.self, forKey: .lastReliableTemperature)
        lastReliableTemperatureAtUnixMs =
            try container.decodeIfPresent(Int64.self, forKey: .lastReliableTemperatureAtUnixMs)
        setpoint = try container.decode(Int.self, forKey: .setpoint)
        heatMode = try container.decode(String.self, forKey: .heatMode)
        heating = try container.decode(String.self, forKey: .heating)
        heatEstimate = try container.decodeIfPresent(HeatEstimate.self, forKey: .heatEstimate)
        temperatureDisplay = try container.decodeIfPresent(TemperatureDisplay.self, forKey: .temperatureDisplay) ?? .init(value: nil, isStale: false, staleReason: nil, lastReliableAtUnixMs: nil)
        heatEstimateDisplay = try container.decodeIfPresent(HeatEstimateDisplay.self, forKey: .heatEstimateDisplay) ?? .init(state: "unavailable", reason: nil, availableInSeconds: nil, minutesRemaining: nil, targetTemperature: nil)
    }
}

struct SpaState: Decodable {
    let on: Bool
    let active: Bool
    let temperature: Int
    let temperatureReliable: Bool
    let temperatureReason: String?
    let lastReliableTemperature: Int?
    let lastReliableTemperatureAtUnixMs: Int64?
    let setpoint: Int
    let heatMode: String
    let heating: String
    let heatEstimate: HeatEstimate?
    let temperatureDisplay: TemperatureDisplay
    let heatEstimateDisplay: HeatEstimateDisplay
    let accessories: [String: Bool]

    init(
        on: Bool,
        active: Bool,
        temperature: Int,
        temperatureReliable: Bool = true,
        temperatureReason: String? = nil,
        lastReliableTemperature: Int? = nil,
        lastReliableTemperatureAtUnixMs: Int64? = nil,
        setpoint: Int,
        heatMode: String,
        heating: String,
        heatEstimate: HeatEstimate? = nil,
        temperatureDisplay: TemperatureDisplay = .init(value: nil, isStale: false, staleReason: nil, lastReliableAtUnixMs: nil),
        heatEstimateDisplay: HeatEstimateDisplay = .init(state: "unavailable", reason: nil, availableInSeconds: nil, minutesRemaining: nil, targetTemperature: nil),
        accessories: [String: Bool]
    ) {
        self.on = on
        self.active = active
        self.temperature = temperature
        self.temperatureReliable = temperatureReliable
        self.temperatureReason = temperatureReason
        self.lastReliableTemperature = lastReliableTemperature
        self.lastReliableTemperatureAtUnixMs = lastReliableTemperatureAtUnixMs
        self.setpoint = setpoint
        self.heatMode = heatMode
        self.heating = heating
        self.heatEstimate = heatEstimate
        self.temperatureDisplay = temperatureDisplay
        self.heatEstimateDisplay = heatEstimateDisplay
        self.accessories = accessories
    }

    private enum CodingKeys: String, CodingKey {
        case on
        case active
        case temperature
        case temperatureReliable
        case temperatureReason
        case lastReliableTemperature
        case lastReliableTemperatureAtUnixMs
        case setpoint
        case heatMode
        case heating
        case heatEstimate
        case temperatureDisplay
        case heatEstimateDisplay
        case accessories
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        on = try container.decode(Bool.self, forKey: .on)
        active = try container.decode(Bool.self, forKey: .active)
        temperature = try container.decode(Int.self, forKey: .temperature)
        temperatureReliable = try container.decodeIfPresent(Bool.self, forKey: .temperatureReliable) ?? true
        temperatureReason = try container.decodeIfPresent(String.self, forKey: .temperatureReason)
        lastReliableTemperature = try container.decodeIfPresent(Int.self, forKey: .lastReliableTemperature)
        lastReliableTemperatureAtUnixMs =
            try container.decodeIfPresent(Int64.self, forKey: .lastReliableTemperatureAtUnixMs)
        setpoint = try container.decode(Int.self, forKey: .setpoint)
        heatMode = try container.decode(String.self, forKey: .heatMode)
        heating = try container.decode(String.self, forKey: .heating)
        heatEstimate = try container.decodeIfPresent(HeatEstimate.self, forKey: .heatEstimate)
        temperatureDisplay = try container.decodeIfPresent(TemperatureDisplay.self, forKey: .temperatureDisplay) ?? .init(value: nil, isStale: false, staleReason: nil, lastReliableAtUnixMs: nil)
        heatEstimateDisplay = try container.decodeIfPresent(HeatEstimateDisplay.self, forKey: .heatEstimateDisplay) ?? .init(state: "unavailable", reason: nil, availableInSeconds: nil, minutesRemaining: nil, targetTemperature: nil)
        accessories = try container.decode([String: Bool].self, forKey: .accessories)
    }
}

struct HeatEstimate: Decodable {
    let available: Bool
    let minutesRemaining: Int?
    let currentTemperature: Int
    let targetTemperature: Int
    let confidence: String
    let source: String
    let reason: String
    let observedRatePerHour: Double?
    let learnedRatePerHour: Double?
    let configuredRatePerHour: Double?
    let baselineRatePerHour: Double?
    let updatedAtUnixMs: Int64
}

struct TemperatureDisplay: Decodable {
    let value: Int?
    let isStale: Bool
    let staleReason: String?
    let lastReliableAtUnixMs: Int64?
}

struct HeatEstimateDisplay: Decodable {
    let state: String
    let reason: String?
    let availableInSeconds: Int?
    let minutesRemaining: Int?
    let targetTemperature: Int?
}

protocol TemperaturePresentationSource {
    var temperature: Int { get }
    var temperatureReliable: Bool { get }
    var temperatureReason: String? { get }
    var lastReliableTemperature: Int? { get }
    var lastReliableTemperatureAtUnixMs: Int64? { get }
    var heatEstimate: HeatEstimate? { get }
}

struct BodyTemperaturePresentation {
    let temperatureText: String
    let staleText: String?
    let detailText: String?
    let isStale: Bool
}

private let emptyTemperatureDisplay = TemperatureDisplay(
    value: nil,
    isStale: false,
    staleReason: nil,
    lastReliableAtUnixMs: nil
)

private let emptyHeatEstimateDisplay = HeatEstimateDisplay(
    state: "unavailable",
    reason: nil,
    availableInSeconds: nil,
    minutesRemaining: nil,
    targetTemperature: nil
)

struct LightState: Decodable {
    let on: Bool
    let mode: String?
    let availableModes: [String]
}

struct AuxiliaryState: Decodable, Identifiable {
    let id: String
    let name: String
    let on: Bool
}

struct PumpInfo: Decodable {
    let pumpType: String
    let running: Bool
    let watts: Int
    let rpm: Int
    let gpm: Int
}

struct SystemInfo: Decodable {
    let controller: String
    let firmware: String?
    let tempUnit: String
    let airTemperature: Int
    let freezeProtection: Bool
    let poolSpaSharedPump: Bool
}

struct DiagnosticEvent: Identifiable {
    let id = UUID()
    let timestamp = Date()
    let category: String
    let message: String
}

struct PendingPoolMutation {
    let id: UUID
    let description: String
    let createdAt: Date
    let mutate: (PoolSystem) -> PoolSystem
    let verify: (PoolSystem) -> Bool

    init(
        id: UUID = UUID(),
        description: String,
        createdAt: Date = Date(),
        mutate: @escaping (PoolSystem) -> PoolSystem,
        verify: @escaping (PoolSystem) -> Bool
    ) {
        self.id = id
        self.description = description
        self.createdAt = createdAt
        self.mutate = mutate
        self.verify = verify
    }
}

struct ReconciledPoolState {
    let system: PoolSystem
    let remainingMutations: [PendingPoolMutation]
}

func reconcileServerSnapshot(
    _ serverState: PoolSystem,
    pendingMutations: [PendingPoolMutation],
    now: Date = Date(),
    gracePeriod: TimeInterval = 15
) -> ReconciledPoolState {
    var merged = serverState
    var remainingMutations: [PendingPoolMutation] = []

    for mutation in pendingMutations {
        if mutation.verify(serverState) {
            continue
        }

        if now.timeIntervalSince(mutation.createdAt) > gracePeriod {
            continue
        }

        remainingMutations.append(mutation)
        merged = mutation.mutate(merged)
    }

    return ReconciledPoolState(system: merged, remainingMutations: remainingMutations)
}

extension PoolSystem {
    func updating(
        pool: BodyState? = nil,
        spa: SpaState? = nil,
        lights: LightState? = nil,
        auxiliaries: [AuxiliaryState]? = nil,
        pump: PumpInfo? = nil,
        system: SystemInfo? = nil
    ) -> PoolSystem {
        PoolSystem(
            pool: pool ?? self.pool,
            spa: spa ?? self.spa,
            lights: lights ?? self.lights,
            auxiliaries: auxiliaries ?? self.auxiliaries,
            pump: pump ?? self.pump,
            system: system ?? self.system
        )
    }
}

extension BodyState {
    func updating(
        on: Bool? = nil,
        active: Bool? = nil,
        temperature: Int? = nil,
        temperatureReliable: Bool? = nil,
        temperatureReason: String?? = nil,
        lastReliableTemperature: Int?? = nil,
        lastReliableTemperatureAtUnixMs: Int64?? = nil,
        setpoint: Int? = nil,
        heatMode: String? = nil,
        heating: String? = nil,
        heatEstimate: HeatEstimate?? = nil
    ) -> BodyState {
        BodyState(
            on: on ?? self.on,
            active: active ?? self.active,
            temperature: temperature ?? self.temperature,
            temperatureReliable: temperatureReliable ?? self.temperatureReliable,
            temperatureReason: temperatureReason ?? self.temperatureReason,
            lastReliableTemperature: lastReliableTemperature ?? self.lastReliableTemperature,
            lastReliableTemperatureAtUnixMs: lastReliableTemperatureAtUnixMs ?? self.lastReliableTemperatureAtUnixMs,
            setpoint: setpoint ?? self.setpoint,
            heatMode: heatMode ?? self.heatMode,
            heating: heating ?? self.heating,
            heatEstimate: heatEstimate ?? self.heatEstimate
        )
    }

    func optimisticCommand(
        on: Bool,
        sharedPump: Bool,
        now: Date = Date()
    ) -> BodyState {
        let snapshot = snapshotLastReliable(now: now)
        let nextReliable = !sharedPump
        let nextReason: String?
        if !sharedPump {
            nextReason = nil
        } else if !on {
            nextReason = "inactive-shared-body"
        } else {
            nextReason = "waiting-for-flow"
        }

        return BodyState(
            on: on,
            active: false,
            temperature: temperature,
            temperatureReliable: nextReliable,
            temperatureReason: nextReason,
            lastReliableTemperature: snapshot.temperature,
            lastReliableTemperatureAtUnixMs: snapshot.atUnixMs,
            setpoint: setpoint,
            heatMode: heatMode,
            heating: heating,
            heatEstimate: nil,
            temperatureDisplay: emptyTemperatureDisplay,
            heatEstimateDisplay: emptyHeatEstimateDisplay
        )
    }

    func optimisticSetpointChange(_ setpoint: Int) -> BodyState {
        BodyState(
            on: on,
            active: active,
            temperature: temperature,
            temperatureReliable: temperatureReliable,
            temperatureReason: temperatureReason,
            lastReliableTemperature: lastReliableTemperature,
            lastReliableTemperatureAtUnixMs: lastReliableTemperatureAtUnixMs,
            setpoint: setpoint,
            heatMode: heatMode,
            heating: heating,
            heatEstimate: nil,
            temperatureDisplay: temperatureDisplay,
            heatEstimateDisplay: emptyHeatEstimateDisplay
        )
    }
}

extension BodyState: TemperaturePresentationSource {}

extension SpaState {
    func updating(
        on: Bool? = nil,
        active: Bool? = nil,
        temperature: Int? = nil,
        temperatureReliable: Bool? = nil,
        temperatureReason: String?? = nil,
        lastReliableTemperature: Int?? = nil,
        lastReliableTemperatureAtUnixMs: Int64?? = nil,
        setpoint: Int? = nil,
        heatMode: String? = nil,
        heating: String? = nil,
        heatEstimate: HeatEstimate?? = nil,
        accessories: [String: Bool]? = nil
    ) -> SpaState {
        SpaState(
            on: on ?? self.on,
            active: active ?? self.active,
            temperature: temperature ?? self.temperature,
            temperatureReliable: temperatureReliable ?? self.temperatureReliable,
            temperatureReason: temperatureReason ?? self.temperatureReason,
            lastReliableTemperature: lastReliableTemperature ?? self.lastReliableTemperature,
            lastReliableTemperatureAtUnixMs: lastReliableTemperatureAtUnixMs ?? self.lastReliableTemperatureAtUnixMs,
            setpoint: setpoint ?? self.setpoint,
            heatMode: heatMode ?? self.heatMode,
            heating: heating ?? self.heating,
            heatEstimate: heatEstimate ?? self.heatEstimate,
            accessories: accessories ?? self.accessories
        )
    }

    func optimisticCommand(
        on: Bool,
        accessories: [String: Bool],
        sharedPump: Bool,
        now: Date = Date()
    ) -> SpaState {
        let snapshot = snapshotLastReliable(now: now)
        let nextReliable = !sharedPump
        let nextReason: String?
        if !sharedPump {
            nextReason = nil
        } else if !on {
            nextReason = "inactive-shared-body"
        } else {
            nextReason = "waiting-for-flow"
        }

        return SpaState(
            on: on,
            active: false,
            temperature: temperature,
            temperatureReliable: nextReliable,
            temperatureReason: nextReason,
            lastReliableTemperature: snapshot.temperature,
            lastReliableTemperatureAtUnixMs: snapshot.atUnixMs,
            setpoint: setpoint,
            heatMode: heatMode,
            heating: heating,
            heatEstimate: nil,
            temperatureDisplay: emptyTemperatureDisplay,
            heatEstimateDisplay: emptyHeatEstimateDisplay,
            accessories: accessories
        )
    }

    func optimisticSetpointChange(_ setpoint: Int) -> SpaState {
        SpaState(
            on: on,
            active: active,
            temperature: temperature,
            temperatureReliable: temperatureReliable,
            temperatureReason: temperatureReason,
            lastReliableTemperature: lastReliableTemperature,
            lastReliableTemperatureAtUnixMs: lastReliableTemperatureAtUnixMs,
            setpoint: setpoint,
            heatMode: heatMode,
            heating: heating,
            heatEstimate: nil,
            temperatureDisplay: temperatureDisplay,
            heatEstimateDisplay: emptyHeatEstimateDisplay,
            accessories: accessories
        )
    }
}

extension SpaState: TemperaturePresentationSource {}

extension LightState {
    func updating(on: Bool? = nil, mode: String?? = nil, availableModes: [String]? = nil) -> LightState {
        LightState(
            on: on ?? self.on,
            mode: mode ?? self.mode,
            availableModes: availableModes ?? self.availableModes
        )
    }
}

extension AuxiliaryState {
    func updating(on: Bool? = nil) -> AuxiliaryState {
        AuxiliaryState(
            id: id,
            name: name,
            on: on ?? self.on
        )
    }
}

extension TemperaturePresentationSource {
    func temperaturePresentation(now: Date = Date()) -> BodyTemperaturePresentation {
        return BodyTemperaturePresentation(
            temperatureText: displayTemperatureText(),
            staleText: staleDisplayText(now: now),
            detailText: estimateDisplayText(),
            isStale: temperatureDisplay.isStale
        )
    }

    private var temperatureDisplay: TemperatureDisplay {
        switch self {
        case let body as BodyState:
            if body.temperatureDisplay.value != nil
                || body.temperatureDisplay.isStale
                || body.temperatureDisplay.staleReason != nil
                || body.temperatureDisplay.lastReliableAtUnixMs != nil {
                return body.temperatureDisplay
            }
            return TemperatureDisplay(
                value: body.temperatureReliable ? body.temperature : body.lastReliableTemperature,
                isStale: !body.temperatureReliable,
                staleReason: body.temperatureReason,
                lastReliableAtUnixMs: body.lastReliableTemperatureAtUnixMs
            )
        case let spa as SpaState:
            if spa.temperatureDisplay.value != nil
                || spa.temperatureDisplay.isStale
                || spa.temperatureDisplay.staleReason != nil
                || spa.temperatureDisplay.lastReliableAtUnixMs != nil {
                return spa.temperatureDisplay
            }
            return TemperatureDisplay(
                value: spa.temperatureReliable ? spa.temperature : spa.lastReliableTemperature,
                isStale: !spa.temperatureReliable,
                staleReason: spa.temperatureReason,
                lastReliableAtUnixMs: spa.lastReliableTemperatureAtUnixMs
            )
        default:
            return .init(value: nil, isStale: false, staleReason: nil, lastReliableAtUnixMs: nil)
        }
    }

    private var heatEstimateDisplay: HeatEstimateDisplay {
        switch self {
        case let body as BodyState:
            if body.heatEstimateDisplay.state != "unavailable" || body.heatEstimateDisplay.reason != nil || body.heatEstimateDisplay.availableInSeconds != nil || body.heatEstimateDisplay.minutesRemaining != nil {
                return body.heatEstimateDisplay
            }
            return body.heatEstimate?.toDisplay() ?? .init(state: "unavailable", reason: nil, availableInSeconds: nil, minutesRemaining: nil, targetTemperature: nil)
        case let spa as SpaState:
            if spa.heatEstimateDisplay.state != "unavailable" || spa.heatEstimateDisplay.reason != nil || spa.heatEstimateDisplay.availableInSeconds != nil || spa.heatEstimateDisplay.minutesRemaining != nil {
                return spa.heatEstimateDisplay
            }
            return spa.heatEstimate?.toDisplay() ?? .init(state: "unavailable", reason: nil, availableInSeconds: nil, minutesRemaining: nil, targetTemperature: nil)
        default:
            return .init(state: "unavailable", reason: nil, availableInSeconds: nil, minutesRemaining: nil, targetTemperature: nil)
        }
    }

    private func estimateDisplayText() -> String? {
        switch heatEstimateDisplay.state {
        case "ready":
            guard let minutesRemaining = heatEstimateDisplay.minutesRemaining,
                  let targetTemperature = heatEstimateDisplay.targetTemperature else {
                return nil
            }
            return "About \(formatEta(minutesRemaining)) to \(targetTemperature)°"
        case "pending":
            if let availableInSeconds = heatEstimateDisplay.availableInSeconds {
                return availableInSeconds < 60
                    ? "Estimate in under 1 min"
                    : "Estimate in about \(Int(ceil(Double(availableInSeconds) / 60.0))) min"
            }
            if heatEstimateDisplay.reason == "insufficient-data" {
                return "Learning estimate"
            }
            return "Generating estimate"
        default:
            return nil
        }
    }

    private func displayTemperatureText() -> String {
        let shownTemperature = temperatureDisplay.value
        guard let shownTemperature else {
            return "--°"
        }

        return "\(shownTemperature)°"
    }

    private func staleDisplayText(now: Date) -> String? {
        guard let lastReliableTemperatureAtUnixMs = temperatureDisplay.lastReliableAtUnixMs else {
            return temperatureDisplay.isStale ? "Waiting for a live water temperature" : nil
        }

        return relativeTimeText(from: lastReliableTemperatureAtUnixMs, now: now)
    }

    private func relativeTimeText(from unixMs: Int64, now: Date) -> String {
        let ageSeconds = max(0, Int(now.timeIntervalSince1970 * 1000) - Int(unixMs)) / 1000
        if ageSeconds < 60 {
            return "just now"
        }

        let minutes = ageSeconds / 60
        if minutes < 60 {
            return "\(minutes) min ago"
        }

        let hours = (minutes + 30) / 60
        if hours < 24 {
            return hours == 1 ? "1h ago" : "\(hours)h ago"
        }

        if hours < 48 {
            return "yesterday"
        }

        let days = (hours + 12) / 24
        return "\(days)d ago"
    }

    private func formatEta(_ minutes: Int) -> String {
        if minutes < 60 {
            return "\(minutes) min"
        }

        let hours = minutes / 60
        let remainingMinutes = minutes % 60
        if remainingMinutes == 0 {
            return hours == 1 ? "1 hr" : "\(hours) hr"
        }

        let hourText = hours == 1 ? "1 hr" : "\(hours) hr"
        return "\(hourText) \(remainingMinutes) min"
    }
}

private extension HeatEstimate {
    func toDisplay() -> HeatEstimateDisplay {
        if available {
            return HeatEstimateDisplay(
                state: "ready",
                reason: nil,
                availableInSeconds: nil,
                minutesRemaining: minutesRemaining,
                targetTemperature: targetTemperature
            )
        }

        if reason == "sensor-warmup" || reason == "insufficient-data" {
            return HeatEstimateDisplay(
                state: "pending",
                reason: reason,
                availableInSeconds: nil,
                minutesRemaining: nil,
                targetTemperature: targetTemperature
            )
        }

        return HeatEstimateDisplay(
            state: "unavailable",
            reason: reason,
            availableInSeconds: nil,
            minutesRemaining: nil,
            targetTemperature: targetTemperature
        )
    }
}

private extension TemperaturePresentationSource {
    func snapshotLastReliable(now: Date) -> (temperature: Int?, atUnixMs: Int64?) {
        let nowUnixMs = Int64(now.timeIntervalSince1970 * 1000)
        let snapshotTemperature: Int?
        if temperatureReliable {
            snapshotTemperature = temperature
        } else {
            snapshotTemperature = temperatureDisplay.value ?? lastReliableTemperature
        }
        let snapshotTime: Int64?
        if temperatureReliable {
            snapshotTime = nowUnixMs
        } else {
            snapshotTime = temperatureDisplay.lastReliableAtUnixMs ?? lastReliableTemperatureAtUnixMs
        }
        return (snapshotTemperature, snapshotTime)
    }
}

enum ConnectionState: Equatable {
    case discovering
    case connecting
    case connected
    case disconnected(String)

    var title: String {
        switch self {
        case .discovering:
            return "Searching"
        case .connecting:
            return "Connecting"
        case .connected:
            return "Connected"
        case .disconnected:
            return "Disconnected"
        }
    }
}

enum PoolBodyMode: String, CaseIterable, Identifiable {
    case off
    case on

    var id: String { rawValue }

    var title: String {
        rawValue.capitalized
    }
}

enum SpaMode: String, CaseIterable, Identifiable {
    case off
    case spa
    case jets

    var id: String { rawValue }

    var title: String {
        rawValue.capitalized
    }
}

enum HeatingBody {
    case pool
    case spa
}

enum HeatingStatusTone {
    case heating
    case neutral
    case warning
    case error
}

struct HeatingStatusSummary {
    let text: String
    let tone: HeatingStatusTone
}

extension PoolSystem {
    func heatingStatus(for body: HeatingBody) -> HeatingStatusSummary? {
        switch body {
        case .pool:
            guard let pool else { return nil }
            return resolveHeatingStatus(
                on: pool.on,
                active: pool.active,
                temperature: pool.temperature,
                temperatureReliable: pool.temperatureReliable,
                temperatureReason: pool.temperatureReason,
                setpoint: pool.setpoint,
                heatMode: pool.heatMode,
                heating: pool.heating,
                other: spa.map {
                    OtherBodyStatus(
                        on: $0.on
                    )
                },
                sharedPump: system.poolSpaSharedPump
            )
        case .spa:
            guard let spa else { return nil }
            return resolveHeatingStatus(
                on: spa.on,
                active: spa.active,
                temperature: spa.temperature,
                temperatureReliable: spa.temperatureReliable,
                temperatureReason: spa.temperatureReason,
                setpoint: spa.setpoint,
                heatMode: spa.heatMode,
                heating: spa.heating,
                other: pool.map {
                    OtherBodyStatus(
                        on: $0.on
                    )
                },
                sharedPump: system.poolSpaSharedPump
            )
        }
    }
}

private struct OtherBodyStatus {
    let on: Bool
}

private func resolveHeatingStatus(
    on: Bool,
    active: Bool,
    temperature: Int,
    temperatureReliable: Bool,
    temperatureReason: String?,
    setpoint: Int,
    heatMode: String,
    heating: String,
    other: OtherBodyStatus?,
    sharedPump: Bool
) -> HeatingStatusSummary? {
    let normalizedHeating = heating.lowercased()
    let normalizedHeatMode = heatMode.lowercased()

    if normalizedHeating != "off", normalizedHeating != "unknown" {
        return HeatingStatusSummary(text: "Heating", tone: .heating)
    }

    if on {
        if normalizedHeatMode == "off" {
            return HeatingStatusSummary(text: "Heat off", tone: .neutral)
        }

        if !temperatureReliable, temperatureReason == "sensor-warmup" {
            return HeatingStatusSummary(text: "Heating", tone: .heating)
        }

        if temperature >= setpoint {
            return HeatingStatusSummary(text: "At temp", tone: .neutral)
        }

        if !active {
            return HeatingStatusSummary(text: "Waiting for flow", tone: .warning)
        }

        return HeatingStatusSummary(text: "Heat error", tone: .error)
    }

    if sharedPump, let other, other.on {
        return nil
    }

    return HeatingStatusSummary(text: "Off", tone: .neutral)
}
