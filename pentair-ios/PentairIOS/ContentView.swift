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

                if let system = viewModel.system {
                    ScrollView {
                        VStack(spacing: 18) {
                            if let bannerMessage = viewModel.bannerMessage {
                                bannerCard(message: bannerMessage)
                            }

                            if let pool = system.pool {
                                poolCard(system: system, pool: pool)
                            }

                            if let spa = system.spa {
                                spaCard(system: system, spa: spa)
                            }

                            if let lights = system.lights {
                                lightsCard(lights: lights)
                            }
                        }
                        .padding(20)
                    }
                    .refreshable {
                        await viewModel.refresh()
                    }
                } else {
                    VStack {
                        if let bannerMessage = viewModel.bannerMessage {
                            bannerCard(message: bannerMessage)
                                .padding(.horizontal, 20)
                                .padding(.top, 20)
                        }

                        Spacer()

                        ProgressView()
                            .controlSize(.large)
                            .tint(.white)

                        Spacer()
                    }
                }
            }
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    HStack(spacing: 16) {
                        if viewModel.system?.goodnightAvailable == true {
                            Button {
                                viewModel.goodnight()
                            } label: {
                                Image(systemName: "moon.fill")
                            }
                            .tint(.white)
                        }

                        Button {
                            showsSettings = true
                        } label: {
                            Image(systemName: "gearshape")
                        }
                        .tint(.white)
                    }
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
                viewModel.setSetpoint(body: target.body, temperature: newValue)
            }
        }
    }

    private var settingsSheet: some View {
        NavigationStack {
            Form {
                daemonSection
                diagnosticsSection

                if let system = viewModel.system {
                    systemSection(system.system)
                    advancedSection(pool: system.pool)
                    auxiliariesSection(auxiliaries: system.auxiliaries)
                    pumpSection(system: system.system, pump: system.pump)
                }
            }
            .navigationTitle("Settings")
            .navigationBarTitleDisplayMode(.inline)
        }
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
    }

    private var daemonSection: some View {
        Section {
            TextField("http://pool-daemon.local:8080", text: $viewModel.manualAddress)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .keyboardType(.URL)
                .disabled(isBusy)

            Button("Test Connection") {
                Task { await viewModel.testManualAddress() }
            }
            .disabled(!hasManualAddress || isBusy)

            Button("Use This Address") {
                Task { await viewModel.applyManualAddress() }
            }
            .disabled(!hasManualAddress || isBusy)

            if let discoveredAddress = viewModel.discoveredAddress,
               discoveredAddress != viewModel.activeAddress {
                Button("Use Discovered Address") {
                    Task { await viewModel.useDiscoveredAddress() }
                }
                .disabled(isBusy)

                LabeledContent("Discovered") {
                    Text(discoveredAddress)
                        .font(.footnote.monospaced())
                        .foregroundStyle(.secondary)
                }
            }
        } header: {
            Text("Daemon")
        } footer: {
            Text("The first launch asks for Local Network access so iOS can find the daemon. Default port is usually 8080.")
        }
    }

    private var diagnosticsSection: some View {
        Section("Diagnostics") {
            LabeledContent("State", value: viewModel.connectionState.title)
            LabeledContent("Active", value: viewModel.activeAddress ?? "None")
            LabeledContent("Discovered", value: viewModel.discoveredAddress ?? "None")

            if viewModel.isTestingAddress {
                LabeledContent("Probe") {
                    Text("Testing")
                        .foregroundStyle(.secondary)
                }
            }

            ForEach(Array(viewModel.diagnostics.suffix(8).reversed())) { event in
                VStack(alignment: .leading, spacing: 4) {
                    Text(event.message)
                        .font(.footnote)

                    HStack {
                        Text(event.category.uppercased())
                        Spacer()
                        Text(event.timestamp.formatted(date: .omitted, time: .standard))
                    }
                    .font(.caption)
                    .foregroundStyle(.secondary)
                }
                .padding(.vertical, 2)
            }
        }
    }

    private func systemSection(_ system: SystemInfo) -> some View {
        Section("System") {
            LabeledContent("Air") {
                Text("\(system.airTemperature)°")
                    .monospacedDigit()
            }
            LabeledContent("Controller", value: system.controller)
            LabeledContent("Freeze Protection", value: system.freezeProtection ? "On" : "Off")

            if let firmware = system.firmware, !firmware.isEmpty {
                LabeledContent("Firmware", value: firmware)
            }
        }
    }

    private func advancedSection(pool: BodyState?) -> some View {
        Section {
            if let pool {
                Toggle("Pool Circuit", isOn: poolCircuitBinding(for: pool))
            }
        } header: {
            Text("Advanced")
        } footer: {
            Text("Most people should leave the pool circuit alone. Normal control is setpoint, spa mode, and lights.")
        }
    }

    private func auxiliariesSection(auxiliaries: [AuxiliaryState]) -> some View {
        Section("Auxiliaries") {
            if auxiliaries.isEmpty {
                Text("No auxiliary circuits are exposed by the daemon.")
                    .foregroundStyle(.secondary)
            } else {
                ForEach(auxiliaries) { auxiliary in
                    Toggle(isOn: auxiliaryBinding(for: auxiliary)) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(auxiliary.name)
                            Text(auxiliary.id)
                                .font(.caption.monospaced())
                                .foregroundStyle(.secondary)
                        }
                    }
                }
            }
        }
    }

    private func pumpSection(system: SystemInfo, pump: PumpInfo?) -> some View {
        Section("Pump") {
            if let pump {
                LabeledContent("Type", value: pump.pumpType)
                LabeledContent("Status", value: pump.running ? "Running" : "Stopped")
                LabeledContent("RPM") {
                    Text("\(pump.rpm)")
                        .monospacedDigit()
                }
                LabeledContent("Watts") {
                    Text("\(pump.watts)")
                        .monospacedDigit()
                }
                LabeledContent("Flow") {
                    Text("\(pump.gpm) gpm")
                        .monospacedDigit()
                }
            } else {
                Text("Waiting for pump telemetry.")
                    .foregroundStyle(.secondary)
            }

            LabeledContent("Temperature Units", value: system.tempUnit == "c" ? "Celsius" : "Fahrenheit")
        }
    }

    private func poolCard(system: PoolSystem, pool: BodyState) -> some View {
        bodyCard(
            title: "Pool",
            presentation: pool.temperaturePresentation(),
            setpoint: pool.setpoint,
            setpointAction: {
                setpointTarget = makeSetpointTarget(
                    id: "pool",
                    body: "pool",
                    title: "Pool Temperature",
                    currentValue: pool.setpoint,
                    range: poolSetpointRange
                )
            }
        ) {
            if let status = system.heatingStatus(for: .pool) {
                heatingStatusText(status)
            }
        } controls: {
            EmptyView()
        }
    }

    private func spaCard(system: PoolSystem, spa: SpaState) -> some View {
        let currentMode = viewModel.spaMode(for: spa)

        return bodyCard(
            title: "Spa",
            presentation: spa.temperaturePresentation(),
            setpoint: spa.setpoint,
            setpointAction: {
                setpointTarget = makeSetpointTarget(
                    id: "spa",
                    body: "spa",
                    title: "Spa Temperature",
                    currentValue: spa.setpoint,
                    range: spaSetpointRange
                )
            }
        ) {
            if let status = system.heatingStatus(for: .spa) {
                heatingStatusText(status)
            }
        } controls: {
            HStack(spacing: 10) {
                ForEach(SpaMode.allCases) { mode in
                    modeButton(title: mode.title, selected: currentMode == mode) {
                        viewModel.setSpaMode(mode)
                    }
                    .accessibilityLabel("\(mode.title), \(currentMode == mode ? "selected" : "not selected")")
                }
            }
        }
    }

    private func lightsCard(lights: LightState) -> some View {
        PanelCard {
            VStack(alignment: .leading, spacing: 16) {
                HStack {
                    Text("Lights")
                        .font(.headline)
                        .foregroundStyle(.white)

                    Spacer()

                    Text(lightsStatusTitle(lights))
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(lights.on ? .white : .white.opacity(0.68))
                }

                LazyVGrid(columns: lightGridColumns, alignment: .leading, spacing: lightGridSpacing) {
                    LightSwatchButton(
                        title: "Off",
                        fill: nil,
                        selected: !lights.on
                    ) {
                        viewModel.setLightMode("off")
                    }

                    ForEach(selectableLightModes(from: lights), id: \.self) { mode in
                        LightSwatchButton(
                            title: lightModeLabel(mode),
                            fill: lightModeFill(for: mode),
                            selected: lights.on && lights.mode == mode
                        ) {
                            viewModel.setLightMode(mode)
                        }
                    }
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

    private var hasManualAddress: Bool {
        !viewModel.manualAddress.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
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

    private var lightGridSpacing: CGFloat { 12 }

    private var lightGridColumns: [GridItem] {
        [
            GridItem(
                .adaptive(minimum: 46, maximum: 46),
                spacing: lightGridSpacing,
                alignment: .leading
            ),
        ]
    }

    private func bodySummary(
        presentation: BodyTemperaturePresentation
    ) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                temperatureLine(
                    presentation: presentation
                )
                Spacer()
            }

            if let detailText = presentation.detailText {
                Text(detailText)
                    .font(.footnote.weight(.medium))
                    .foregroundStyle(bodyDetailColor(isStale: presentation.isStale))
            }
        }
    }

    private func temperatureLine(
        presentation: BodyTemperaturePresentation
    ) -> Text {
        let base = Text(presentation.temperatureText)
        .font(.system(size: 44, weight: .bold, design: .rounded))
        .foregroundStyle(presentation.isStale ? .white.opacity(0.72) : .white)

        guard let staleText = presentation.staleText else {
            return base
        }

        return base + Text(" \(staleText)")
            .font(.footnote.weight(.medium))
            .foregroundStyle(bodyDetailColor(isStale: presentation.isStale))
    }

    private func bodyCard<StatusContent: View, ControlsContent: View>(
        title: String,
        presentation: BodyTemperaturePresentation,
        setpoint: Int,
        setpointAction: @escaping () -> Void,
        @ViewBuilder status: () -> StatusContent,
        @ViewBuilder controls: () -> ControlsContent
    ) -> some View {
        PanelCard {
            VStack(alignment: .leading, spacing: 16) {
                HStack {
                    Text(title)
                        .font(.headline)
                        .foregroundStyle(.white)
                    Spacer()
                    status()
                }

                bodySummary(
                    presentation: presentation
                )
                controls()

                setpointButton(
                    title: "Setpoint",
                    value: setpoint,
                    unitSymbol: temperatureUnitSymbol,
                    action: setpointAction
                )
            }
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

    private func heatingStatusText(_ status: HeatingStatusSummary) -> some View {
        Text(status.text)
            .font(.caption.weight(.medium))
            .foregroundStyle(heatingStatusColor(status))
    }

    private func bodyDetailColor(isStale: Bool) -> Color {
        isStale ? .white.opacity(0.62) : .white.opacity(0.76)
    }

    private func auxiliaryBinding(for auxiliary: AuxiliaryState) -> Binding<Bool> {
        Binding(
            get: { auxiliary.on },
            set: { _ in
                viewModel.toggleAuxiliary(auxiliary)
            }
        )
    }

    private func poolCircuitBinding(for pool: BodyState) -> Binding<Bool> {
        Binding(
            get: { pool.on },
            set: { isOn in
                viewModel.setPoolMode(isOn ? .on : .off)
            }
        )
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
        let ordered = preferredOrder.filter { available.contains($0) }
        let remaining = lights.availableModes.filter { $0 != "off" && !preferredOrder.contains($0) }
        return ordered + remaining
    }

    private func makeSetpointTarget(
        id: String,
        body: String,
        title: String,
        currentValue: Int,
        range: ClosedRange<Int>
    ) -> SetpointTarget {
        SetpointTarget(
            id: id,
            body: body,
            title: title,
            currentValue: currentValue,
            range: range,
            unitSymbol: temperatureUnitSymbol
        )
    }

    private var whiteLightFill: AnyShapeStyle {
        AnyShapeStyle(
            RadialGradient(
                colors: [Color.white, Color(red: 0.80, green: 0.85, blue: 0.90)],
                center: .center,
                startRadius: 2,
                endRadius: 28
            )
        )
    }

    private var rainbowLightFill: AnyShapeStyle {
        AnyShapeStyle(
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
    }

    private func lightModeFill(for mode: String) -> AnyShapeStyle {
        switch mode {
        case "on":
            return whiteLightFill
        case "set":
            return AnyShapeStyle(
                LinearGradient(
                    colors: [Color(red: 0.07, green: 0.71, blue: 0.83), Color(red: 0.15, green: 0.83, blue: 0.75)],
                    startPoint: .topLeading,
                    endPoint: .bottomTrailing
                )
            )
        case "sync":
            return rainbowLightFill
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
            return rainbowLightFill
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
            return whiteLightFill
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

    private func heatingStatusColor(_ status: HeatingStatusSummary) -> Color {
        switch status.tone {
        case .heating:
            return Color(red: 1.0, green: 0.83, blue: 0.61)
        case .neutral:
            return .white.opacity(0.68)
        case .warning:
            return Color(red: 0.98, green: 0.88, blue: 0.55)
        case .error:
            return Color(red: 1.0, green: 0.54, blue: 0.54)
        }
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
        .accessibilityLabel(Text(title))
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
            Form {
                Section {
                    LabeledContent("Setpoint") {
                        Text("\(tempValue)°\(target.unitSymbol)")
                            .monospacedDigit()
                    }

                    Stepper(value: $tempValue, in: target.range) {
                        Text("Adjust")
                    }
                } footer: {
                    Text("Range \(target.range.lowerBound)°\(target.unitSymbol) to \(target.range.upperBound)°\(target.unitSymbol)")
                }
            }
            .navigationTitle(target.title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        dismiss()
                    }
                }

                ToolbarItem(placement: .confirmationAction) {
                    Button("Set") {
                        onSet(tempValue)
                        dismiss()
                    }
                }
            }
        }
        .presentationDetents([.medium])
        .presentationDragIndicator(.visible)
    }
}
