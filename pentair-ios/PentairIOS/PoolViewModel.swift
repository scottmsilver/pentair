import ActivityKit
import Foundation
import OSLog

@MainActor
final class PoolViewModel: ObservableObject {
    private static let logger = Logger(subsystem: "com.ssilver.pentair.ios", category: "client")
    private let decoder = JSONDecoder()

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
    private var pendingMutations: [PendingPoolMutation] = []
    private var currentSpaHeatActivity: Activity<SpaHeatAttributes>?
    private var pushTokenObservationTask: Task<Void, Never>?
    private var wasProgressActive = false

    init() {
        if let savedAddress = defaults.string(forKey: addressDefaultsKey),
           let savedURL = PoolAPI.normalizeBaseURL(savedAddress) {
            manualAddress = savedURL.absoluteString
            setActiveBaseURL(savedURL)
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
        let turnOn = mode == .on
        let pendingMutationID = applyOptimistic(
            description: "Pool \(mode.rawValue)",
            verify: { current in
                current.pool?.on == turnOn
            }
        ) { current in
            current.updating(
                pool: current.pool?.optimisticCommand(
                    on: turnOn,
                    sharedPump: current.system.poolSpaSharedPump
                )
            )
        }
        recordDiagnostic("command", "Pool \(mode.rawValue)")

        Task {
            switch mode {
            case .off:
                await sendCommand(path: "/api/pool/off", pendingMutationID: pendingMutationID)
            case .on:
                await sendCommand(path: "/api/pool/on", pendingMutationID: pendingMutationID)
            }
        }
    }

    func setSpaMode(_ mode: SpaMode) {
        let currentSpa = system?.spa
        recordDiagnostic("command", "Spa \(mode.rawValue)")

        let pendingMutationID = applyOptimistic(
            description: "Spa \(mode.rawValue)",
            verify: { current in
                guard let spa = current.spa else {
                    return false
                }

                switch mode {
                case .off:
                    return spa.on == false
                case .spa:
                    return spa.on == true && spa.accessories["jets"] != true
                case .jets:
                    return spa.on == true && spa.accessories["jets"] == true
                }
            }
        ) { current in
            let nextSpa: SpaState?
            switch mode {
            case .off:
                nextSpa = current.spa?.optimisticCommand(
                    on: false,
                    accessories: current.spa?.accessories.mapValues { _ in false } ?? [:],
                    sharedPump: current.system.poolSpaSharedPump
                )
            case .spa:
                nextSpa = current.spa?.optimisticCommand(
                    on: true,
                    accessories: current.spa?.accessories.mapValues { _ in false } ?? [:],
                    sharedPump: current.system.poolSpaSharedPump
                )
            case .jets:
                nextSpa = current.spa?.optimisticCommand(
                    on: true,
                    accessories: (current.spa?.accessories ?? [:]).merging(["jets": true]) { _, new in new },
                    sharedPump: current.system.poolSpaSharedPump
                )
            }

            return current.updating(spa: nextSpa)
        }

        Task {
            switch mode {
            case .off:
                await sendCommand(path: "/api/spa/off", pendingMutationID: pendingMutationID)
            case .spa:
                if currentSpa?.accessories["jets"] == true {
                    await sendCommand(path: "/api/spa/jets/off", refreshDelay: 300_000_000, pendingMutationID: pendingMutationID)
                }
                if currentSpa?.on != true {
                    await sendCommand(path: "/api/spa/on", pendingMutationID: pendingMutationID)
                } else {
                    await refresh(silent: true)
                }
            case .jets:
                if currentSpa?.on != true {
                    await sendCommand(path: "/api/spa/on", refreshDelay: 700_000_000, pendingMutationID: pendingMutationID)
                }
                await sendCommand(path: "/api/spa/jets/on", pendingMutationID: pendingMutationID)
            }
        }
    }

    func setLightMode(_ mode: String) {
        recordDiagnostic("command", "Lights \(mode)")
        let pendingMutationID = applyOptimistic(
            description: "Lights \(mode)",
            verify: { current in
                if mode == "off" {
                    return current.lights?.on == false
                }
                return current.lights?.on == true && current.lights?.mode == mode
            }
        ) { current in
            current.updating(
                lights: current.lights?.updating(
                    on: mode != "off",
                    mode: mode == "off" ? .some(nil) : .some(mode)
                )
            )
        }

        Task {
            if mode == "off" {
                await sendCommand(path: "/api/lights/off", pendingMutationID: pendingMutationID)
            } else {
                await sendCommand(path: "/api/lights/mode", body: ["mode": mode], pendingMutationID: pendingMutationID)
            }
        }
    }

    func setSetpoint(body: String, temperature: Int) {
        recordDiagnostic("command", "\(body.capitalized) setpoint \(temperature)")
        let pendingMutationID = applyOptimistic(
            description: "\(body.capitalized) setpoint \(temperature)",
            verify: { current in
                switch body {
                case "spa":
                    return current.spa?.setpoint == temperature
                default:
                    return current.pool?.setpoint == temperature
                }
            }
        ) { current in
            switch body {
            case "spa":
                return current.updating(spa: current.spa?.optimisticSetpointChange(temperature))
            default:
                return current.updating(pool: current.pool?.optimisticSetpointChange(temperature))
            }
        }

        Task {
            let endpoint = body == "spa" ? "/api/spa/heat" : "/api/pool/heat"
            await sendCommand(path: endpoint, body: ["setpoint": temperature], pendingMutationID: pendingMutationID)
        }
    }

    func toggleAuxiliary(_ auxiliary: AuxiliaryState) {
        let targetState = auxiliary.on ? "off" : "on"
        recordDiagnostic("command", "Aux \(auxiliary.id) \(targetState)")
        let nextState = !auxiliary.on
        let pendingMutationID = applyOptimistic(
            description: "Aux \(auxiliary.id) \(targetState)",
            verify: { current in
                current.auxiliaries.first(where: { $0.id == auxiliary.id })?.on == nextState
            }
        ) { current in
            current.updating(
                auxiliaries: current.auxiliaries.map { item in
                    item.id == auxiliary.id ? item.updating(on: !auxiliary.on) : item
                }
            )
        }
        Task {
            await sendCommand(path: "/api/auxiliary/\(auxiliary.id)/\(targetState)", pendingMutationID: pendingMutationID)
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
            applyServerState(try await api.fetchPool())
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

    private func sendCommand(
        path: String,
        body: [String: Any]? = nil,
        refreshDelay: UInt64 = 800_000_000,
        pendingMutationID: UUID? = nil
    ) async {
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
            removePendingMutation(id: pendingMutationID)
            presentBanner(error.localizedDescription)
            await refresh(silent: true)
        }
    }

    @discardableResult
    private func applyOptimistic(
        description: String,
        verify: @escaping (PoolSystem) -> Bool,
        mutate: @escaping (PoolSystem) -> PoolSystem
    ) -> UUID? {
        guard let system else {
            return nil
        }

        let pendingMutation = PendingPoolMutation(
            description: description,
            mutate: mutate,
            verify: verify
        )
        pendingMutations.append(pendingMutation)
        self.system = mutate(system)
        return pendingMutation.id
    }

    private func setActiveBaseURL(_ url: URL) {
        activeBaseURL = url
        activeAddress = url.absoluteString
        api = PoolAPI(baseURL: url)
        defaults.set(url.absoluteString, forKey: addressDefaultsKey)
        recordDiagnostic("startup", "Active daemon address set to \(url.absoluteString)")
        Task {
            await NotificationTokenManager.shared.ensureRegistered(activeBaseURL: url)
        }

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
                case .success(let message):
                    self.wsReconnectDelay = 2.0
                    switch message {
                    case .string(let text):
                        if let data = text.data(using: .utf8),
                           let serverState = try? self.decoder.decode(PoolSystem.self, from: data) {
                            self.applyServerState(serverState)
                            self.connectionState = .connected
                            self.statusMessage = nil
                            self.recordDiagnostic("websocket", "Applied daemon state snapshot")
                        } else {
                            self.recordDiagnostic("websocket", "Failed to decode daemon state snapshot")
                            await self.refresh(silent: true)
                        }
                    case .data(let data):
                        if let serverState = try? self.decoder.decode(PoolSystem.self, from: data) {
                            self.applyServerState(serverState)
                            self.connectionState = .connected
                            self.statusMessage = nil
                            self.recordDiagnostic("websocket", "Applied daemon state snapshot")
                        } else {
                            self.recordDiagnostic("websocket", "Failed to decode binary daemon state snapshot")
                            await self.refresh(silent: true)
                        }
                    @unknown default:
                        self.recordDiagnostic("websocket", "Received unsupported daemon websocket payload")
                        await self.refresh(silent: true)
                    }
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

    // MARK: - Live Activity lifecycle

    private func evaluateSpaHeatActivity(for system: PoolSystem) {
        guard let spa = system.spa else {
            endSpaHeatActivityIfNeeded(reason: "no spa")
            wasProgressActive = false
            return
        }

        let progress = spa.spaHeatProgress

        if progress.active && !wasProgressActive {
            startSpaHeatActivity(progress: progress)
        } else if progress.active && wasProgressActive {
            updateSpaHeatActivity(progress: progress)
        } else if !progress.active && wasProgressActive {
            endSpaHeatActivityIfNeeded(reason: "cancelled")
        }

        wasProgressActive = progress.active
    }

    private func startSpaHeatActivity(progress: SpaHeatProgress) {
        guard ActivityAuthorizationInfo().areActivitiesEnabled else {
            recordDiagnostic("live-activity", "Live Activities not enabled, skipping")
            return
        }

        // Don't start a duplicate
        if currentSpaHeatActivity != nil {
            updateSpaHeatActivity(progress: progress)
            return
        }

        let attributes = SpaHeatAttributes(spaName: "Spa")
        let contentState = buildContentState(from: progress)
        let content = ActivityContent(state: contentState, staleDate: Date().addingTimeInterval(120))

        do {
            let activity = try Activity.request(
                attributes: attributes,
                content: content,
                pushType: .token
            )
            currentSpaHeatActivity = activity
            recordDiagnostic("live-activity", "Started spa heat Live Activity (id: \(activity.id))")
            observePushTokenUpdates(for: activity)
        } catch {
            recordDiagnostic("live-activity", "Failed to start Live Activity: \(error.localizedDescription)")
        }
    }

    private func updateSpaHeatActivity(progress: SpaHeatProgress) {
        guard let activity = currentSpaHeatActivity else {
            return
        }

        // Preserve the original start temperature from when the activity began
        let preservedStartTemp = activity.content.state.startTempF
        let contentState = buildContentState(from: progress, startTempOverride: preservedStartTemp)
        let content = ActivityContent(state: contentState, staleDate: Date().addingTimeInterval(120))

        Task {
            await activity.update(content)
            recordDiagnostic("live-activity", "Updated Live Activity: \(progress.currentTempF)\u{00B0}F, phase=\(progress.phase)")
        }

        // If reached, end after a short display period
        if progress.phase == "reached" {
            endSpaHeatActivityIfNeeded(reason: "reached")
        }
    }

    private func endSpaHeatActivityIfNeeded(reason: String) {
        guard let activity = currentSpaHeatActivity else {
            return
        }

        pushTokenObservationTask?.cancel()
        pushTokenObservationTask = nil

        let finalState: SpaHeatAttributes.ContentState
        let dismissalPolicy: ActivityUIDismissalPolicy

        if reason == "reached" {
            finalState = SpaHeatAttributes.ContentState(
                currentTempF: activity.content.state.targetTempF,
                targetTempF: activity.content.state.targetTempF,
                startTempF: activity.content.state.startTempF,
                progressPct: 100,
                minutesRemaining: 0,
                phase: "reached",
                milestone: "at_temp"
            )
            dismissalPolicy = .after(Date().addingTimeInterval(30))
        } else {
            finalState = activity.content.state
            dismissalPolicy = .after(Date().addingTimeInterval(5))
        }

        let content = ActivityContent(state: finalState, staleDate: nil)

        Task {
            await activity.end(content, dismissalPolicy: dismissalPolicy)
            recordDiagnostic("live-activity", "Ended Live Activity (reason: \(reason))")
        }

        currentSpaHeatActivity = nil
    }

    private func observePushTokenUpdates(for activity: Activity<SpaHeatAttributes>) {
        pushTokenObservationTask?.cancel()
        pushTokenObservationTask = Task { [weak self] in
            for await tokenData in activity.pushTokenUpdates {
                guard let self else { return }
                let tokenString = tokenData.map { String(format: "%02x", $0) }.joined()
                await MainActor.run {
                    self.recordDiagnostic("live-activity", "Received Live Activity push token (\(tokenString.prefix(12))...)")
                }
                await self.registerLiveActivityToken(tokenString)
            }
        }
    }

    private func registerLiveActivityToken(_ token: String) async {
        guard let baseURL = await MainActor.run(body: { self.activeBaseURL }) else {
            return
        }

        do {
            // The daemon requires "token" (FCM token). Send the existing FCM token
            // alongside the live_activity_token so the daemon can match devices.
            let fcmToken = await NotificationTokenManager.shared.currentToken ?? ""
            try await PoolAPI(baseURL: baseURL).post("/api/devices/register", body: [
                "token": fcmToken,
                "platform": "ios",
                "live_activity_token": token
            ])
            await MainActor.run {
                self.recordDiagnostic("live-activity", "Registered Live Activity push token with daemon")
            }
        } catch {
            await MainActor.run {
                self.recordDiagnostic("live-activity", "Failed to register Live Activity token: \(error.localizedDescription)")
            }
        }
    }

    private func buildContentState(from progress: SpaHeatProgress, startTempOverride: Int? = nil) -> SpaHeatAttributes.ContentState {
        let startTemp = startTempOverride ?? progress.startTempF ?? progress.currentTempF
        let pct = progress.phase == "reached" ? 100 : progress.progressPct

        return SpaHeatAttributes.ContentState(
            currentTempF: progress.currentTempF,
            targetTempF: progress.targetTempF,
            startTempF: startTemp,
            progressPct: pct,
            minutesRemaining: progress.minutesRemaining,
            phase: progress.phase,
            milestone: progress.milestone
        )
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

    private func applyServerState(_ serverState: PoolSystem) {
        let reconciled = reconcileServerSnapshot(
            serverState,
            pendingMutations: pendingMutations
        )
        pendingMutations = reconciled.remainingMutations
        system = reconciled.system
        evaluateSpaHeatActivity(for: reconciled.system)
    }

    private func removePendingMutation(id: UUID?) {
        guard let id else {
            return
        }
        pendingMutations.removeAll { $0.id == id }
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
