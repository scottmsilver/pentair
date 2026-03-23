import SwiftUI

struct ContentView: View {
    @ObservedObject var viewModel: PoolViewModel
    @State private var showsSettings = false
    @State private var setpointTarget: SetpointTarget?

    var body: some View {
        NavigationStack {
            ZStack {
                LinearGradient(
                    colors: [
                        Color(red: 0.04, green: 0.11, blue: 0.19),
                        Color(red: 0.03, green: 0.20, blue: 0.30),
                        Color(red: 0.01, green: 0.33, blue: 0.42),
                    ],
                    startPoint: .topLeading,
                    endPoint: .bottomTrailing
                )
                .ignoresSafeArea()

                ScrollView {
                    VStack(spacing: 18) {
                        if let bannerMessage = viewModel.bannerMessage {
                            bannerCard(message: bannerMessage)
                        }

                        if let system = viewModel.system {
                            poolCard(pool: system.pool)
                            spaCard(spa: system.spa)
                            lightsCard(lights: system.lights)
                        } else {
                            loadingCard
                        }
                    }
                    .padding(20)
                }
                .refreshable {
                    await viewModel.refresh()
                }
            }
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        showsSettings = true
                    } label: {
                        Image(systemName: "gearshape")
                    }
                    .tint(.white)
                }

                ToolbarItem(placement: .principal) {
                    statusBadge
                }
            }
            .navigationBarTitleDisplayMode(.inline)
        }
        .task {
            await viewModel.start()
        }
        .sheet(isPresented: $showsSettings) {
            settingsSheet
        }
        .sheet(item: $setpointTarget) { target in
            SetpointSheet(target: target) { newValue in
                Task {
                    await viewModel.setSetpoint(body: target.body, temperature: newValue)
                }
            }
        }
    }

    private var settingsSheet: some View {
        NavigationStack {
            ZStack {
                LinearGradient(
                    colors: [
                        Color(red: 0.04, green: 0.11, blue: 0.19),
                        Color(red: 0.03, green: 0.20, blue: 0.30),
                        Color(red: 0.01, green: 0.33, blue: 0.42),
                    ],
                    startPoint: .topLeading,
                    endPoint: .bottomTrailing
                )
                .ignoresSafeArea()

                ScrollView {
                    VStack(spacing: 18) {
                        addressCard
                        diagnosticsCard

                        if let system = viewModel.system {
                            overviewCard(system: system)
                            auxiliariesCard(auxiliaries: system.auxiliaries)
                            pumpCard(system: system.system, pump: system.pump)
                        }
                    }
                    .padding(20)
                }
            }
            .navigationTitle("Settings")
            .navigationBarTitleDisplayMode(.inline)
        }
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
    }

    private var addressCard: some View {
        PanelCard {
            VStack(alignment: .leading, spacing: 12) {
                Text("Daemon Address")
                    .font(.headline)
                    .foregroundStyle(.white)

                HStack(spacing: 10) {
                    TextField("http://pool-daemon.local:8080", text: $viewModel.manualAddress)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .keyboardType(.URL)
                        .disabled(isBusy)
                        .padding(.horizontal, 14)
                        .padding(.vertical, 12)
                        .background(Color.white.opacity(0.10), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                        .foregroundStyle(.white)

                    Button("Test") {
                        Task { await viewModel.testManualAddress() }
                    }
                    .buttonStyle(PoolButtonStyle(fill: Color.white.opacity(0.16)))
                    .disabled(viewModel.manualAddress.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || isBusy)

                    Button("Apply") {
                        Task { await viewModel.applyManualAddress() }
                    }
                    .buttonStyle(PoolButtonStyle(fill: Color(red: 0.13, green: 0.76, blue: 0.79)))
                    .disabled(viewModel.manualAddress.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || isBusy)
                }

                Text("Default daemon port is usually 8080. Use Test to verify the simulator can reach the daemon before switching over.")
                    .font(.footnote)
                    .foregroundStyle(.white.opacity(0.62))

                Text("If your Mac SSH bridge forwards the daemon port with `-L 8080:localhost:8080`, use `http://127.0.0.1:8080` here.")
                    .font(.footnote)
                    .foregroundStyle(.white.opacity(0.62))

                if let discoveredAddress = viewModel.discoveredAddress, discoveredAddress != viewModel.activeAddress {
                    HStack {
                        Text("Discovered \(discoveredAddress)")
                            .font(.footnote)
                            .foregroundStyle(.white.opacity(0.70))

                        Spacer()

                        Button("Use") {
                            Task { await viewModel.useDiscoveredAddress() }
                        }
                        .buttonStyle(PoolButtonStyle(fill: Color.white.opacity(0.16)))
                        .disabled(isBusy)
                    }
                }

                Text("The first launch will ask for Local Network access so iOS can find and talk to your daemon.")
                    .font(.footnote)
                    .foregroundStyle(.white.opacity(0.62))
            }
        }
    }

    private func overviewCard(system: PoolSystem) -> some View {
        PanelCard {
            VStack(alignment: .leading, spacing: 14) {
                Text("System")
                    .font(.headline)
                    .foregroundStyle(.white)

                HStack(spacing: 12) {
                    MetricPill(title: "Air", value: "\(system.system.airTemperature)°")
                    MetricPill(title: "Controller", value: system.system.controller)
                    MetricPill(title: "Freeze", value: system.system.freezeProtection ? "On" : "Off")
                }

                if let firmware = system.system.firmware, !firmware.isEmpty {
                    Text("Firmware \(firmware)")
                        .font(.footnote)
                        .foregroundStyle(.white.opacity(0.66))
                }
            }
        }
    }

    private func bannerCard(message: String) -> some View {
        PanelCard {
            HStack(alignment: .top, spacing: 12) {
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundStyle(Color(red: 1.0, green: 0.82, blue: 0.45))

                Text(message)
                    .font(.footnote.weight(.medium))
                    .foregroundStyle(.white)
            }
        }
    }

    private var diagnosticsCard: some View {
        PanelCard {
            VStack(alignment: .leading, spacing: 12) {
                HStack {
                    Text("Diagnostics")
                        .font(.headline)
                        .foregroundStyle(.white)

                    Spacer()

                    if viewModel.isTestingAddress {
                        Text("Testing…")
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(Color(red: 0.98, green: 0.88, blue: 0.55))
                    }
                }

                diagnosticRow(title: "Active", value: viewModel.activeAddress ?? "none")
                diagnosticRow(title: "Discovered", value: viewModel.discoveredAddress ?? "none")
                diagnosticRow(title: "State", value: viewModel.connectionState.title)

                ForEach(Array(viewModel.diagnostics.suffix(8).reversed())) { event in
                    HStack(alignment: .top, spacing: 10) {
                        Text(event.timestamp.formatted(date: .omitted, time: .standard))
                            .font(.caption2.monospacedDigit())
                            .foregroundStyle(.white.opacity(0.55))
                            .frame(width: 62, alignment: .leading)

                        Text(event.category.uppercased())
                            .font(.caption2.weight(.bold))
                            .foregroundStyle(Color(red: 0.63, green: 0.95, blue: 0.87))
                            .frame(width: 68, alignment: .leading)

                        Text(event.message)
                            .font(.footnote)
                            .foregroundStyle(.white.opacity(0.82))
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
            }
        }
    }

    private func diagnosticRow(title: String, value: String) -> some View {
        HStack(alignment: .top, spacing: 8) {
            Text(title.uppercased())
                .font(.caption2.weight(.bold))
                .foregroundStyle(.white.opacity(0.55))
                .frame(width: 72, alignment: .leading)

            Text(value)
                .font(.footnote.monospaced())
                .foregroundStyle(.white.opacity(0.82))
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func poolCard(pool: BodyState?) -> some View {
        PanelCard {
            VStack(alignment: .leading, spacing: 16) {
                HStack {
                    Text("Pool")
                        .font(.headline)
                        .foregroundStyle(.white)
                    Spacer()
                    if let pool {
                        HStack(alignment: .firstTextBaseline, spacing: 6) {
                            Text(heatingStatusTitle(pool.heating))
                                .font(.caption.weight(.medium))
                                .foregroundStyle(heatingStatusColor(pool.heating))

                            Text(poolStatusTitle(pool))
                                .font(.subheadline.weight(.semibold))
                                .foregroundStyle(poolStatusColor(pool))
                        }
                    }
                }

                if let pool {
                    bodySummary(
                        temperature: pool.temperature
                    )

                    setpointButton(
                        title: "Setpoint",
                        value: pool.setpoint,
                        unitSymbol: temperatureUnitSymbol
                    ) {
                        setpointTarget = SetpointTarget(
                            id: "pool",
                            body: "pool",
                            title: "Pool Temperature",
                            currentValue: pool.setpoint,
                            range: poolSetpointRange,
                            unitSymbol: temperatureUnitSymbol
                        )
                    }
                } else {
                    unavailableText
                }
            }
        }
    }

    private func spaCard(spa: SpaState?) -> some View {
        PanelCard {
            VStack(alignment: .leading, spacing: 16) {
                HStack {
                    Text("Spa")
                        .font(.headline)
                        .foregroundStyle(.white)
                    Spacer()
                    if let spa {
                        HStack(alignment: .firstTextBaseline, spacing: 6) {
                            Text(heatingStatusTitle(spa.heating))
                                .font(.caption.weight(.medium))
                                .foregroundStyle(heatingStatusColor(spa.heating))

                            Text(spaStatusTitle(spa))
                                .font(.subheadline.weight(.semibold))
                                .foregroundStyle(spaStatusColor(spa))
                        }
                    }
                }

                if let spa {
                    bodySummary(
                        temperature: spa.temperature
                    )

                    let currentMode = viewModel.spaMode(for: spa)
                    HStack(spacing: 10) {
                        ForEach(SpaMode.allCases) { mode in
                            modeButton(title: mode.title, selected: currentMode == mode) {
                                Task { await viewModel.setSpaMode(mode) }
                            }
                        }
                    }

                    setpointButton(
                        title: "Setpoint",
                        value: spa.setpoint,
                        unitSymbol: temperatureUnitSymbol
                    ) {
                        setpointTarget = SetpointTarget(
                            id: "spa",
                            body: "spa",
                            title: "Spa Temperature",
                            currentValue: spa.setpoint,
                            range: spaSetpointRange,
                            unitSymbol: temperatureUnitSymbol
                        )
                    }
                } else {
                    unavailableText
                }
            }
        }
    }

    private func lightsCard(lights: LightState?) -> some View {
        PanelCard {
            VStack(alignment: .leading, spacing: 16) {
                HStack {
                    Text("Lights")
                        .font(.headline)
                        .foregroundStyle(.white)

                    Spacer()

                    if let lights {
                        Text(lightsStatusTitle(lights))
                            .font(.subheadline.weight(.semibold))
                            .foregroundStyle(lights.on ? .white : .white.opacity(0.68))
                    }
                }

                if let lights {
                    ScrollView(.horizontal, showsIndicators: false) {
                        LazyHGrid(rows: lightPickerRows, spacing: 12) {
                            LightSwatchButton(
                                title: "Off",
                                fill: nil,
                                selected: !lights.on
                            ) {
                                Task { await viewModel.setLightMode("off") }
                            }

                            ForEach(selectableLightModes(from: lights), id: \.self) { mode in
                                LightSwatchButton(
                                    title: lightModeLabel(mode),
                                    fill: lightModeFill(for: mode),
                                    selected: lights.on && lights.mode == mode
                                ) {
                                    Task { await viewModel.setLightMode(mode) }
                                }
                            }
                        }
                        .padding(.vertical, 2)
                    }
                    .frame(height: 108)
                } else {
                    unavailableText
                }
            }
        }
    }

    private func auxiliariesCard(auxiliaries: [AuxiliaryState]) -> some View {
        PanelCard {
            VStack(alignment: .leading, spacing: 16) {
                Text("Auxiliaries")
                    .font(.headline)
                    .foregroundStyle(.white)

                if auxiliaries.isEmpty {
                    Text("No auxiliary circuits are exposed by the daemon.")
                        .font(.footnote)
                        .foregroundStyle(.white.opacity(0.66))
                } else {
                    ForEach(auxiliaries) { auxiliary in
                        Button {
                            Task { await viewModel.toggleAuxiliary(auxiliary) }
                        } label: {
                            HStack {
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(auxiliary.name)
                                        .font(.body.weight(.semibold))
                                    Text(auxiliary.id)
                                        .font(.caption.monospaced())
                                        .foregroundStyle(.white.opacity(0.60))
                                }

                                Spacer()

                                Image(systemName: auxiliary.on ? "power.circle.fill" : "power.circle")
                                    .font(.title3)
                                    .foregroundStyle(auxiliary.on ? Color(red: 0.59, green: 0.98, blue: 0.86) : .white.opacity(0.65))
                            }
                            .padding(.horizontal, 14)
                            .padding(.vertical, 12)
                            .background(Color.white.opacity(0.08), in: RoundedRectangle(cornerRadius: 16, style: .continuous))
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
        }
    }

    private func pumpCard(system: SystemInfo, pump: PumpInfo?) -> some View {
        PanelCard {
            VStack(alignment: .leading, spacing: 14) {
                Text("Pump")
                    .font(.headline)
                    .foregroundStyle(.white)

                if let pump {
                    HStack(spacing: 12) {
                        MetricPill(title: "RPM", value: "\(pump.rpm)")
                        MetricPill(title: "Watts", value: "\(pump.watts)")
                        MetricPill(title: "Flow", value: "\(pump.gpm) gpm")
                    }

                    Text("\(pump.pumpType) • \(pump.running ? "Running" : "Stopped")")
                        .font(.footnote)
                        .foregroundStyle(.white.opacity(0.68))
                } else {
                    unavailableText
                }

                Text(system.tempUnit == "c" ? "Temperatures in Celsius" : "Temperatures in Fahrenheit")
                    .font(.footnote)
                    .foregroundStyle(.white.opacity(0.62))
            }
        }
    }

    private var loadingCard: some View {
        PanelCard {
            VStack(alignment: .leading, spacing: 16) {
                HStack {
                    ProgressView()
                        .tint(.white)
                    Text("Waiting for pool data")
                        .font(.headline)
                        .foregroundStyle(.white)
                }

                Text("If discovery doesn’t find the daemon, open Settings and enter the HTTP address there, for example `http://pool-daemon.local:8080`.")
                    .font(.footnote)
                    .foregroundStyle(.white.opacity(0.70))
            }
        }
    }

    private var statusBadge: some View {
        HStack(spacing: 8) {
            Circle()
                .fill(statusColor)
                .frame(width: 10, height: 10)
            Text(viewModel.connectionState.title)
                .font(.caption.weight(.semibold))
                .foregroundStyle(.white)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(Color.white.opacity(0.10), in: Capsule())
    }

    private var statusColor: Color {
        switch viewModel.connectionState {
        case .connected:
            return Color(red: 0.59, green: 0.98, blue: 0.86)
        case .connecting:
            return Color(red: 0.98, green: 0.88, blue: 0.55)
        case .discovering:
            return Color(red: 0.57, green: 0.78, blue: 0.99)
        case .disconnected:
            return Color(red: 1.0, green: 0.54, blue: 0.54)
        }
    }

    private var isBusy: Bool {
        viewModel.isRefreshing || viewModel.isTestingAddress
    }

    private var temperatureUnitSymbol: String {
        viewModel.system?.system.tempUnit == "c" ? "C" : "F"
    }

    private var poolSetpointRange: ClosedRange<Int> {
        temperatureUnitSymbol == "C" ? 7 ... 40 : 45 ... 104
    }

    private var spaSetpointRange: ClosedRange<Int> {
        temperatureUnitSymbol == "C" ? 16 ... 43 : 60 ... 110
    }

    private var unavailableText: some View {
        Text("Waiting for this part of the system to report in.")
            .font(.footnote)
            .foregroundStyle(.white.opacity(0.66))
    }

    private var lightPickerRows: [GridItem] {
        [
            GridItem(.fixed(46), spacing: 12),
            GridItem(.fixed(46), spacing: 12),
        ]
    }

    private func bodySummary(temperature: Int) -> some View {
        HStack(alignment: .firstTextBaseline) {
            Text("\(temperature)°")
                .font(.system(size: 44, weight: .bold, design: .rounded))
                .foregroundStyle(.white)
            Spacer()
        }
    }

    private func setpointButton(title: String, value: Int, unitSymbol: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack {
                Text(title)
                    .font(.subheadline.weight(.medium))
                    .foregroundStyle(.white)

                Spacer()

                Text("\(value)°\(unitSymbol)")
                    .font(.body.monospacedDigit().weight(.semibold))
                    .foregroundStyle(.white)

                Image(systemName: "chevron.right")
                    .font(.caption.weight(.bold))
                    .foregroundStyle(.white.opacity(0.55))
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 14)
            .background(Color.white.opacity(0.08), in: RoundedRectangle(cornerRadius: 18, style: .continuous))
        }
        .buttonStyle(.plain)
    }

    private func modeButton(title: String, selected: Bool, action: @escaping () -> Void) -> some View {
        Button(title, action: action)
            .buttonStyle(PoolButtonStyle(fill: selected ? Color(red: 0.13, green: 0.76, blue: 0.79) : Color.white.opacity(0.10)))
    }

    private func modeLabel(_ rawValue: String) -> String {
        rawValue
            .replacingOccurrences(of: "-", with: " ")
            .replacingOccurrences(of: "_", with: " ")
            .capitalized
    }

    private func lightsStatusTitle(_ lights: LightState) -> String {
        guard lights.on else {
            return "Off"
        }

        return lightModeLabel(effectiveLightMode(lights))
    }

    private func effectiveLightMode(_ lights: LightState) -> String {
        guard lights.on else {
            return "off"
        }

        return lights.mode ?? "on"
    }

    private func lightModeLabel(_ rawValue: String) -> String {
        switch rawValue {
        case "set":
            return "Color Set"
        case "sync":
            return "Sync"
        case "swim":
            return "Color Swim"
        default:
            return modeLabel(rawValue)
        }
    }

    private func selectableLightModes(from lights: LightState) -> [String] {
        let preferredOrder = [
            "swim",
            "party",
            "romantic",
            "caribbean",
            "american",
            "sunset",
            "royal",
            "blue",
            "green",
            "red",
            "white",
            "purple",
        ]

        let available = Set(lights.availableModes.filter { $0 != "off" })
        return preferredOrder.filter { available.contains($0) }
    }

    private func lightModeFill(for mode: String) -> AnyShapeStyle {
        switch mode {
        case "on":
            return AnyShapeStyle(
                RadialGradient(
                    colors: [Color.white, Color(red: 0.80, green: 0.85, blue: 0.90)],
                    center: .center,
                    startRadius: 2,
                    endRadius: 28
                )
            )
        case "set":
            return AnyShapeStyle(
                LinearGradient(
                    colors: [Color(red: 0.07, green: 0.71, blue: 0.83), Color(red: 0.15, green: 0.83, blue: 0.75)],
                    startPoint: .topLeading,
                    endPoint: .bottomTrailing
                )
            )
        case "sync":
            return AnyShapeStyle(
                AngularGradient(
                    colors: [
                        Color(red: 0.94, green: 0.27, blue: 0.27),
                        Color(red: 0.92, green: 0.67, blue: 0.03),
                        Color(red: 0.13, green: 0.77, blue: 0.37),
                        Color(red: 0.23, green: 0.51, blue: 0.96),
                        Color(red: 0.66, green: 0.33, blue: 0.97),
                        Color(red: 0.94, green: 0.27, blue: 0.27),
                    ],
                    center: .center
                )
            )
        case "swim":
            return AnyShapeStyle(
                AngularGradient(
                    colors: [
                        Color(red: 0.05, green: 0.65, blue: 0.91),
                        Color(red: 0.94, green: 0.98, blue: 1.0),
                        Color(red: 0.09, green: 0.28, blue: 0.88),
                        Color(red: 0.08, green: 0.72, blue: 0.65),
                        Color(red: 0.05, green: 0.65, blue: 0.91),
                    ],
                    center: .center,
                    angle: .degrees(270)
                )
            )
        case "party":
            return AnyShapeStyle(
                AngularGradient(
                    colors: [
                        Color(red: 0.94, green: 0.27, blue: 0.27),
                        Color(red: 0.92, green: 0.67, blue: 0.03),
                        Color(red: 0.13, green: 0.77, blue: 0.37),
                        Color(red: 0.23, green: 0.51, blue: 0.96),
                        Color(red: 0.66, green: 0.33, blue: 0.97),
                        Color(red: 0.94, green: 0.27, blue: 0.27),
                    ],
                    center: .center
                )
            )
        case "romantic":
            return AnyShapeStyle(
                LinearGradient(
                    colors: [Color(red: 0.93, green: 0.28, blue: 0.60), Color(red: 0.96, green: 0.62, blue: 0.04)],
                    startPoint: .topLeading,
                    endPoint: .bottomTrailing
                )
            )
        case "caribbean":
            return AnyShapeStyle(
                LinearGradient(
                    colors: [Color(red: 0.02, green: 0.71, blue: 0.83), Color(red: 0.18, green: 0.83, blue: 0.75)],
                    startPoint: .topLeading,
                    endPoint: .bottomTrailing
                )
            )
        case "american":
            return AnyShapeStyle(
                LinearGradient(
                    colors: [Color(red: 0.94, green: 0.27, blue: 0.27), Color(red: 0.94, green: 0.97, blue: 1.0), Color(red: 0.23, green: 0.51, blue: 0.96)],
                    startPoint: .leading,
                    endPoint: .trailing
                )
            )
        case "sunset":
            return AnyShapeStyle(
                LinearGradient(
                    colors: [Color(red: 0.98, green: 0.45, blue: 0.09), Color(red: 0.86, green: 0.15, blue: 0.15)],
                    startPoint: .topLeading,
                    endPoint: .bottomTrailing
                )
            )
        case "royal":
            return AnyShapeStyle(
                LinearGradient(
                    colors: [Color(red: 0.49, green: 0.23, blue: 0.92), Color(red: 0.23, green: 0.51, blue: 0.96)],
                    startPoint: .topLeading,
                    endPoint: .bottomTrailing
                )
            )
        case "blue":
            return AnyShapeStyle(Color(red: 0.23, green: 0.51, blue: 0.96))
        case "green":
            return AnyShapeStyle(Color(red: 0.13, green: 0.77, blue: 0.37))
        case "red":
            return AnyShapeStyle(Color(red: 0.94, green: 0.27, blue: 0.27))
        case "white":
            return AnyShapeStyle(
                RadialGradient(
                    colors: [Color.white, Color(red: 0.80, green: 0.85, blue: 0.90)],
                    center: .center,
                    startRadius: 2,
                    endRadius: 28
                )
            )
        case "purple":
            return AnyShapeStyle(Color(red: 0.66, green: 0.33, blue: 0.97))
        default:
            return AnyShapeStyle(
                LinearGradient(
                    colors: [Color(red: 0.08, green: 0.70, blue: 0.79), Color(red: 0.18, green: 0.82, blue: 0.73)],
                    startPoint: .topLeading,
                    endPoint: .bottomTrailing
                )
            )
        }
    }

    private func heatingStatusTitle(_ heating: String) -> String {
        heating == "off" ? "Not heating" : "Heating"
    }

    private func heatingStatusColor(_ heating: String) -> Color {
        heating == "off" ? .white.opacity(0.68) : Color(red: 1.0, green: 0.83, blue: 0.61)
    }

    private func poolStatusTitle(_ pool: BodyState) -> String {
        pool.on ? "Running" : "Off"
    }

    private func poolStatusColor(_ pool: BodyState) -> Color {
        return pool.on ? Color(red: 0.59, green: 0.98, blue: 0.86) : .white.opacity(0.68)
    }

    private func spaStatusTitle(_ spa: SpaState) -> String {
        viewModel.spaMode(for: spa).title
    }

    private func spaStatusColor(_ spa: SpaState) -> Color {
        return spa.on ? Color(red: 1.0, green: 0.83, blue: 0.61) : .white.opacity(0.68)
    }
}

private struct PanelCard<Content: View>: View {
    let content: Content

    init(@ViewBuilder content: () -> Content) {
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            content
        }
        .padding(18)
        .background(
            RoundedRectangle(cornerRadius: 26, style: .continuous)
                .fill(Color.black.opacity(0.20))
                .overlay(
                    RoundedRectangle(cornerRadius: 26, style: .continuous)
                        .stroke(Color.white.opacity(0.10), lineWidth: 1)
                )
        )
    }
}

private struct MetricPill: View {
    let title: String
    let value: String

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title.uppercased())
                .font(.caption2.weight(.bold))
                .foregroundStyle(.white.opacity(0.55))
            Text(value)
                .font(.headline)
                .foregroundStyle(.white)
                .lineLimit(1)
                .minimumScaleFactor(0.7)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 14)
        .padding(.vertical, 12)
        .background(Color.white.opacity(0.08), in: RoundedRectangle(cornerRadius: 18, style: .continuous))
    }
}

private struct LightSwatchButton: View {
    let title: String
    let fill: AnyShapeStyle?
    let selected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            ZStack {
                Circle()
                    .fill(fill ?? AnyShapeStyle(Color(red: 0.04, green: 0.04, blue: 0.04)))
                    .overlay(
                        Circle()
                            .stroke(Color.white.opacity(selected ? 1.0 : 0.14), lineWidth: selected ? 2.5 : 1)
                    )
                    .frame(width: 40, height: 40)

                if fill == nil {
                    Image(systemName: "lightbulb")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.white.opacity(0.34))
                }
            }
            .frame(width: 46, height: 46)
        }
        .buttonStyle(.plain)
        .accessibilityLabel(Text(accessibilityTitle))
    }

    private var accessibilityTitle: String {
        title
    }
}

private struct PoolButtonStyle: ButtonStyle {
    @Environment(\.isEnabled) private var isEnabled

    let fill: Color

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.subheadline.weight(.semibold))
            .foregroundStyle(.white)
            .padding(.horizontal, 14)
            .padding(.vertical, 11)
            .background(
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .fill(fill.opacity(configuration.isPressed ? 0.72 : 1.0))
            )
            .opacity(isEnabled ? 1 : 0.45)
            .scaleEffect(configuration.isPressed ? 0.98 : 1.0)
            .animation(.easeOut(duration: 0.12), value: configuration.isPressed)
    }
}

private struct SetpointTarget: Identifiable {
    let id: String
    let body: String
    let title: String
    let currentValue: Int
    let range: ClosedRange<Int>
    let unitSymbol: String
}

private struct SetpointSheet: View {
    @Environment(\.dismiss) private var dismiss

    let target: SetpointTarget
    let onSet: (Int) -> Void

    @State private var tempValue: Int

    init(target: SetpointTarget, onSet: @escaping (Int) -> Void) {
        self.target = target
        self.onSet = onSet
        _tempValue = State(initialValue: target.currentValue)
    }

    var body: some View {
        NavigationStack {
            ZStack {
                LinearGradient(
                    colors: [
                        Color(red: 0.04, green: 0.11, blue: 0.19),
                        Color(red: 0.03, green: 0.20, blue: 0.30),
                        Color(red: 0.01, green: 0.33, blue: 0.42),
                    ],
                    startPoint: .topLeading,
                    endPoint: .bottomTrailing
                )
                .ignoresSafeArea()

                VStack(spacing: 28) {
                    Spacer(minLength: 8)

                    Text(target.title)
                        .font(.headline)
                        .foregroundStyle(.white)

                    HStack(alignment: .firstTextBaseline, spacing: 2) {
                        Text("\(tempValue)")
                            .font(.system(size: 72, weight: .bold, design: .rounded))
                            .foregroundStyle(.white)
                            .monospacedDigit()

                        Text("°\(target.unitSymbol)")
                            .font(.title3.weight(.medium))
                            .foregroundStyle(.white.opacity(0.7))
                    }

                    HStack(spacing: 24) {
                        CircleActionButton(symbol: "minus", disabled: tempValue <= target.range.lowerBound) {
                            tempValue = max(target.range.lowerBound, tempValue - 1)
                        }

                        CircleActionButton(symbol: "plus", disabled: tempValue >= target.range.upperBound) {
                            tempValue = min(target.range.upperBound, tempValue + 1)
                        }
                    }

                    Text("Range \(target.range.lowerBound)°\(target.unitSymbol) to \(target.range.upperBound)°\(target.unitSymbol)")
                        .font(.footnote)
                        .foregroundStyle(.white.opacity(0.65))

                    Spacer()

                    HStack(spacing: 12) {
                        Button("Cancel") {
                            dismiss()
                        }
                        .buttonStyle(PoolButtonStyle(fill: Color.white.opacity(0.10)))

                        Button {
                            onSet(tempValue)
                            dismiss()
                        } label: {
                            Text("Set")
                                .frame(maxWidth: .infinity)
                        }
                        .buttonStyle(PoolButtonStyle(fill: Color(red: 0.13, green: 0.76, blue: 0.79)))
                    }
                }
                .padding(24)
            }
            .navigationBarTitleDisplayMode(.inline)
        }
        .presentationDetents([.fraction(0.46)])
        .presentationDragIndicator(.visible)
    }
}

private struct CircleActionButton: View {
    let symbol: String
    let disabled: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: symbol)
                .font(.title2.weight(.semibold))
                .foregroundStyle(.white)
                .frame(width: 64, height: 64)
                .background(Color.white.opacity(0.10), in: Circle())
                .overlay(Circle().stroke(Color.white.opacity(0.12), lineWidth: 1))
        }
        .buttonStyle(.plain)
        .disabled(disabled)
        .opacity(disabled ? 0.4 : 1)
    }
}
