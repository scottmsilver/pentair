import Foundation
import OSLog

@MainActor
final class PoolViewModel: ObservableObject {
    private static let logger = Logger(subsystem: "com.ssilver.pentair.ios", category: "client")

    @Published var system: PoolSystem?
    @Published var connectionState: ConnectionState = .discovering
    @Published var manualAddress: String = ""
    @Published var discoveredAddress: String?
    @Published var activeAddress: String?
    @Published var statusMessage: String?
    @Published var bannerMessage: String?
    @Published var diagnostics: [DiagnosticEvent] = []
    @Published var isRefreshing = false
    @Published var isTestingAddress = false

    private let discovery = BonjourDiscovery()
    private let defaults = UserDefaults.standard
    private let addressDefaultsKey = "pentair.daemonAddress"

    private var activeBaseURL: URL?
    private var manualOverride = false
    private var hasStarted = false
    private var api: PoolAPI?
    private var webSocketTask: URLSessionWebSocketTask?
    private var webSocketURL: URL?
    private var wsReconnectDelay: TimeInterval = 2.0
    private var bannerDismissTask: Task<Void, Never>?
    private var discoveryHintTask: Task<Void, Never>?

    init() {
        if let savedAddress = defaults.string(forKey: addressDefaultsKey),
           let savedURL = PoolAPI.normalizeBaseURL(savedAddress) {
            manualAddress = savedURL.absoluteString
            activeBaseURL = savedURL
            activeAddress = savedURL.absoluteString
            api = PoolAPI(baseURL: savedURL)
            recordDiagnostic("startup", "Loaded saved daemon address \(savedURL.absoluteString)")
        }

        discovery.onResolvedURL = { [weak self] url in
            Task { @MainActor in
                await self?.handleDiscovered(url)
            }
        }
        discovery.onStatusMessage = { [weak self] message in
            Task { @MainActor in
                self?.statusMessage = message
                self?.recordDiagnostic("discovery", message)
            }
        }
        discovery.onEvent = { [weak self] message in
            Task { @MainActor in
                self?.recordDiagnostic("discovery", message)
            }
        }
    }

    func start() async {
        guard !hasStarted else {
            return
        }

        hasStarted = true
        recordDiagnostic("startup", "Starting iOS client")
        discovery.start()
        scheduleDiscoveryHint()

        if activeBaseURL != nil {
            await refresh()
        }
    }

    func refresh() async {
        await refresh(silent: false)
    }

    func applyManualAddress() async {
        guard let url = PoolAPI.normalizeBaseURL(manualAddress) else {
            let message = PoolAPIError.invalidAddress.errorDescription ?? "Invalid address."
            statusMessage = message
            connectionState = .disconnected(message)
            recordDiagnostic("probe", "Rejected invalid manual address '\(manualAddress)'")
            presentBanner(message)
            return
        }

        manualOverride = true
        recordDiagnostic("probe", "Applying manual daemon address \(url.absoluteString)")
        setActiveBaseURL(url)
        await refresh()
    }

    func testManualAddress() async {
        let candidate = manualAddress.isEmpty ? (activeAddress ?? "") : manualAddress
        guard let url = PoolAPI.normalizeBaseURL(candidate) else {
            let message = PoolAPIError.invalidAddress.errorDescription ?? "Invalid address."
            recordDiagnostic("probe", "Rejected invalid probe address '\(candidate)'")
            presentBanner(message)
            return
        }

        isTestingAddress = true
        defer { isTestingAddress = false }

        recordDiagnostic("probe", "Testing \(url.absoluteString)/api/pool")

        do {
            let pool = try await PoolAPI(baseURL: url).fetchPool()
            recordDiagnostic(
                "probe",
                "Success. Controller \(pool.system.controller), air \(pool.system.airTemperature)\(temperatureSymbol(for: pool.system))"
            )
            presentBanner("Connection OK: \(url.absoluteString)")
        } catch {
            recordDiagnostic("probe", "Failed: \(error.localizedDescription)")
            presentBanner(error.localizedDescription)
        }
    }

    func useDiscoveredAddress() async {
        guard let discoveredAddress,
              let url = PoolAPI.normalizeBaseURL(discoveredAddress) else {
            return
        }

        manualOverride = false
        manualAddress = url.absoluteString
        recordDiagnostic("probe", "Using discovered daemon address \(url.absoluteString)")
        setActiveBaseURL(url)
        await refresh()
    }

    func setPoolMode(_ mode: PoolBodyMode) {
        applyOptimistic { current in
            current.updating(pool: current.pool?.updating(on: mode == .on))
        }
        recordDiagnostic("command", "Pool \(mode.rawValue)")

        Task {
            switch mode {
            case .off:
                await sendCommand(path: "/api/pool/off")
            case .on:
                await sendCommand(path: "/api/pool/on")
            }
        }
    }

    func setSpaMode(_ mode: SpaMode) {
        let currentSpa = system?.spa
        recordDiagnostic("command", "Spa \(mode.rawValue)")

        applyOptimistic { current in
            let nextSpa: SpaState?
            switch mode {
            case .off:
                nextSpa = current.spa?.updating(
                    on: false,
                    accessories: current.spa?.accessories.mapValues { _ in false } ?? [:]
                )
            case .spa:
                nextSpa = current.spa?.updating(
                    on: true,
                    accessories: current.spa?.accessories.mapValues { _ in false } ?? [:]
                )
            case .jets:
                nextSpa = current.spa?.updating(
                    on: true,
                    accessories: (current.spa?.accessories ?? [:]).merging(["jets": true]) { _, new in new }
                )
            }

            return current.updating(spa: nextSpa)
        }

        Task {
            switch mode {
            case .off:
                await sendCommand(path: "/api/spa/off")
            case .spa:
                if currentSpa?.accessories["jets"] == true {
                    await sendCommand(path: "/api/spa/jets/off", refreshDelay: 300_000_000)
                }
                if currentSpa?.on != true {
                    await sendCommand(path: "/api/spa/on")
                } else {
                    await refresh(silent: true)
                }
            case .jets:
                if currentSpa?.on != true {
                    await sendCommand(path: "/api/spa/on", refreshDelay: 700_000_000)
                }
                await sendCommand(path: "/api/spa/jets/on")
            }
        }
    }

    func setLightMode(_ mode: String) {
        recordDiagnostic("command", "Lights \(mode)")
        applyOptimistic { current in
            current.updating(
                lights: current.lights?.updating(
                    on: mode != "off",
                    mode: mode == "off" ? .some(nil) : .some(mode)
                )
            )
        }

        Task {
            if mode == "off" {
                await sendCommand(path: "/api/lights/off")
            } else {
                await sendCommand(path: "/api/lights/mode", body: ["mode": mode])
            }
        }
    }

    func setSetpoint(body: String, temperature: Int) {
        recordDiagnostic("command", "\(body.capitalized) setpoint \(temperature)")
        applyOptimistic { current in
            switch body {
            case "spa":
                return current.updating(spa: current.spa?.updating(setpoint: temperature))
            default:
                return current.updating(pool: current.pool?.updating(setpoint: temperature))
            }
        }

        Task {
            let endpoint = body == "spa" ? "/api/spa/heat" : "/api/pool/heat"
            await sendCommand(path: endpoint, body: ["setpoint": temperature])
        }
    }

    func toggleAuxiliary(_ auxiliary: AuxiliaryState) {
        let targetState = auxiliary.on ? "off" : "on"
        recordDiagnostic("command", "Aux \(auxiliary.id) \(targetState)")
        applyOptimistic { current in
            current.updating(
                auxiliaries: current.auxiliaries.map { item in
                    item.id == auxiliary.id ? item.updating(on: !auxiliary.on) : item
                }
            )
        }
        Task {
            await sendCommand(path: "/api/auxiliary/\(auxiliary.id)/\(targetState)")
        }
    }

    func spaMode(for spa: SpaState?) -> SpaMode {
        guard let spa else {
            return .off
        }

        if !spa.on {
            return .off
        }

        return spa.accessories["jets"] == true ? .jets : .spa
    }

    private func handleDiscovered(_ url: URL) async {
        discoveredAddress = url.absoluteString
        discoveryHintTask?.cancel()
        recordDiagnostic("discovery", "Resolved daemon \(url.absoluteString)")

        if activeBaseURL == nil || !manualOverride {
            manualAddress = url.absoluteString
            setActiveBaseURL(url)
            await refresh(silent: system != nil)
        }
    }

    private func refresh(silent: Bool) async {
        guard let api else {
            connectionState = .discovering
            statusMessage = "Waiting for a daemon address."
            recordDiagnostic("http", "Refresh skipped because no daemon address is active.")
            return
        }

        if !silent {
            connectionState = .connecting
        }

        isRefreshing = true
        defer { isRefreshing = false }
        recordDiagnostic("http", "GET \(api.baseURL.absoluteString)/api/pool")

        do {
            system = try await api.fetchPool()
            connectionState = .connected
            statusMessage = nil
            discoveryHintTask?.cancel()
            recordDiagnostic("http", "GET /api/pool succeeded")
            connectWebSocketIfNeeded(using: api)
        } catch {
            connectionState = .disconnected(error.localizedDescription)
            statusMessage = error.localizedDescription
            recordDiagnostic("http", "GET /api/pool failed: \(error.localizedDescription)")
        }
    }

    private func sendCommand(path: String, body: [String: Any]? = nil, refreshDelay: UInt64 = 800_000_000) async {
        guard let api else {
            return
        }

        recordDiagnostic("http", "POST \(path)")

        do {
            try await api.post(path, body: body)
            recordDiagnostic("http", "POST \(path) succeeded")
            try? await Task.sleep(nanoseconds: refreshDelay)
            await refresh(silent: true)
        } catch {
            recordDiagnostic("http", "POST \(path) failed: \(error.localizedDescription)")
            presentBanner(error.localizedDescription)
            await refresh(silent: true)
        }
    }

    private func applyOptimistic(_ mutate: (PoolSystem) -> PoolSystem) {
        guard let system else {
            return
        }

        self.system = mutate(system)
    }

    private func setActiveBaseURL(_ url: URL) {
        activeBaseURL = url
        activeAddress = url.absoluteString
        api = PoolAPI(baseURL: url)
        defaults.set(url.absoluteString, forKey: addressDefaultsKey)
        recordDiagnostic("startup", "Active daemon address set to \(url.absoluteString)")

        if webSocketURL?.absoluteString != activeAddress {
            webSocketTask?.cancel(with: .goingAway, reason: nil)
            webSocketTask = nil
            webSocketURL = nil
        }
    }

    private func connectWebSocketIfNeeded(using api: PoolAPI) {
        guard let url = api.webSocketURL() else {
            recordDiagnostic("websocket", "Could not form websocket URL from \(api.baseURL.absoluteString)")
            return
        }

        if webSocketURL == url, webSocketTask != nil {
            return
        }

        webSocketTask?.cancel(with: .goingAway, reason: nil)
        webSocketURL = url

        let task = URLSession.shared.webSocketTask(with: url)
        webSocketTask = task
        wsReconnectDelay = 2.0
        recordDiagnostic("websocket", "Connecting \(url.absoluteString)")
        task.resume()
        receiveNextMessage(from: task)
    }

    private func receiveNextMessage(from task: URLSessionWebSocketTask) {
        task.receive { [weak self] result in
            Task { @MainActor in
                guard let self else {
                    return
                }

                guard task === self.webSocketTask else {
                    return
                }

                switch result {
                case .success(_):
                    self.wsReconnectDelay = 2.0
                    self.recordDiagnostic("websocket", "Received daemon event")
                    await self.refresh(silent: true)
                    self.receiveNextMessage(from: task)
                case .failure(let error):
                    self.handleWebSocketFailure(error, task: task)
                }
            }
        }
    }

    private func handleWebSocketFailure(_ error: Error, task: URLSessionWebSocketTask) {
        guard task === webSocketTask else {
            return
        }

        webSocketTask = nil
        webSocketURL = nil

        if case .connected = connectionState {
            connectionState = .disconnected("Lost live updates. Reconnecting…")
        }

        let delay = wsReconnectDelay
        wsReconnectDelay = min(wsReconnectDelay * 2, 60.0)

        Task { @MainActor in
            try? await Task.sleep(nanoseconds: UInt64(delay * 1_000_000_000))
            await refresh(silent: true)
        }

        statusMessage = error.localizedDescription
        recordDiagnostic("websocket", "Websocket failed: \(error.localizedDescription). Reconnecting in \(Int(delay))s.")
    }

    private func presentBanner(_ message: String) {
        bannerDismissTask?.cancel()
        bannerMessage = message
        recordDiagnostic("ui", "Banner: \(message)")

        bannerDismissTask = Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: 4_000_000_000)
            self?.bannerMessage = nil
        }
    }

    private func scheduleDiscoveryHint() {
        discoveryHintTask?.cancel()
        discoveryHintTask = Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: 6_000_000_000)
            guard let self, self.discoveredAddress == nil, self.system == nil else {
                return
            }

            let hint = "No daemon discovered yet. Try a direct address like http://<host>:8080."
            self.statusMessage = hint
            self.recordDiagnostic("discovery", hint)
        }
    }

    private func recordDiagnostic(_ category: String, _ message: String) {
        diagnostics.append(DiagnosticEvent(category: category, message: message))
        if diagnostics.count > 40 {
            diagnostics.removeFirst(diagnostics.count - 40)
        }

        PoolViewModel.logger.info("[\(category, privacy: .public)] \(message, privacy: .public)")
    }

    private func temperatureSymbol(for system: SystemInfo) -> String {
        system.tempUnit == "c" ? "C" : "F"
    }
}
