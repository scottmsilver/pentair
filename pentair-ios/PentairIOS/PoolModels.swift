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
    let setpoint: Int
    let heatMode: String
    let heating: String
}

struct SpaState: Decodable {
    let on: Bool
    let active: Bool
    let temperature: Int
    let setpoint: Int
    let heatMode: String
    let heating: String
    let accessories: [String: Bool]
}

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
        setpoint: Int? = nil,
        heatMode: String? = nil,
        heating: String? = nil
    ) -> BodyState {
        BodyState(
            on: on ?? self.on,
            active: active ?? self.active,
            temperature: temperature ?? self.temperature,
            setpoint: setpoint ?? self.setpoint,
            heatMode: heatMode ?? self.heatMode,
            heating: heating ?? self.heating
        )
    }
}

extension SpaState {
    func updating(
        on: Bool? = nil,
        active: Bool? = nil,
        temperature: Int? = nil,
        setpoint: Int? = nil,
        heatMode: String? = nil,
        heating: String? = nil,
        accessories: [String: Bool]? = nil
    ) -> SpaState {
        SpaState(
            on: on ?? self.on,
            active: active ?? self.active,
            temperature: temperature ?? self.temperature,
            setpoint: setpoint ?? self.setpoint,
            heatMode: heatMode ?? self.heatMode,
            heating: heating ?? self.heating,
            accessories: accessories ?? self.accessories
        )
    }
}

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
