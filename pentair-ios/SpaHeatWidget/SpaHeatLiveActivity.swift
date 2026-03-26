import ActivityKit
import SwiftUI
import WidgetKit

// MARK: - Color helpers

private extension Color {
    static let deepOrange = Color(red: 1.0, green: 0.427, blue: 0.0)       // #FF6D00
    static let orange = Color(red: 1.0, green: 0.569, blue: 0.0)           // #FF9100
    static let amber = Color(red: 1.0, green: 0.757, blue: 0.027)          // #FFC107
    static let tempGreen = Color(red: 0.298, green: 0.686, blue: 0.314)    // #4CAF50
}

private func progressColor(for pct: Int) -> Color {
    switch pct {
    case 0..<30:
        return .deepOrange
    case 30..<70:
        return .orange
    case 70..<90:
        return .amber
    default:
        return .tempGreen
    }
}

// MARK: - Shared subviews

private struct HeatProgressBar: View {
    let progressPct: Int
    let phase: String

    var body: some View {
        GeometryReader { geometry in
            ZStack(alignment: .leading) {
                RoundedRectangle(cornerRadius: 4)
                    .fill(Color.white.opacity(0.15))
                    .frame(height: 8)

                if phase == "reached" {
                    RoundedRectangle(cornerRadius: 4)
                        .fill(Color.tempGreen)
                        .frame(height: 8)
                } else if phase == "started" && progressPct == 0 {
                    // Indeterminate: show a static partial fill for reduced motion
                    RoundedRectangle(cornerRadius: 4)
                        .fill(Color.deepOrange.opacity(0.6))
                        .frame(width: geometry.size.width * 0.3, height: 8)
                } else {
                    RoundedRectangle(cornerRadius: 4)
                        .fill(progressColor(for: progressPct))
                        .frame(width: geometry.size.width * CGFloat(min(max(progressPct, 0), 100)) / 100.0, height: 8)
                }
            }
        }
        .frame(height: 8)
    }
}

private struct MiniProgressBar: View {
    let progressPct: Int
    let phase: String

    var body: some View {
        GeometryReader { geometry in
            ZStack(alignment: .leading) {
                RoundedRectangle(cornerRadius: 2)
                    .fill(Color.white.opacity(0.15))
                    .frame(height: 4)

                if phase == "reached" {
                    RoundedRectangle(cornerRadius: 2)
                        .fill(Color.tempGreen)
                        .frame(height: 4)
                } else {
                    RoundedRectangle(cornerRadius: 2)
                        .fill(progressColor(for: progressPct))
                        .frame(width: geometry.size.width * CGFloat(min(max(progressPct, 0), 100)) / 100.0, height: 4)
                }
            }
        }
        .frame(height: 4)
    }
}

private func etaText(minutesRemaining: Int?) -> String? {
    guard let minutes = minutesRemaining else {
        return nil
    }
    if minutes < 60 {
        return "~\(minutes) min"
    }
    let hours = minutes / 60
    let remaining = minutes % 60
    if remaining == 0 {
        return "~\(hours) hr"
    }
    return "~\(hours) hr \(remaining) min"
}

// MARK: - Lock Screen view

private struct LockScreenView: View {
    let context: ActivityViewContext<SpaHeatAttributes>

    var body: some View {
        let state = context.state

        VStack(alignment: .leading, spacing: 8) {
            // Title row: icon + "Spa Heating" + ETA
            HStack {
                Label("Spa Heating", systemImage: "flame.fill")
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(.white)

                Spacer()

                if state.phase == "reached" {
                    Text("Ready!")
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(Color.tempGreen)
                } else if let eta = etaText(minutesRemaining: state.minutesRemaining) {
                    Text(eta)
                        .font(.system(size: 15, weight: .medium))
                        .foregroundStyle(Color.orange)
                } else {
                    Text("Heating...")
                        .font(.system(size: 15, weight: .medium))
                        .foregroundStyle(Color.orange)
                }
            }

            // Temperature row
            HStack {
                if state.phase == "reached" {
                    Image(systemName: "checkmark.circle.fill")
                        .font(.system(size: 24))
                        .foregroundStyle(Color.tempGreen)
                    Text("Spa is ready!")
                        .font(.system(size: 28, weight: .bold))
                        .foregroundStyle(.white)
                    Spacer()
                    Text("\(state.targetTempF)\u{00B0}F")
                        .font(.system(size: 28, weight: .bold))
                        .foregroundStyle(Color.tempGreen)
                } else {
                    Text("\(state.currentTempF)\u{00B0}F")
                        .font(.system(size: 28, weight: .bold))
                        .foregroundStyle(.white)
                    Spacer()
                    Image(systemName: "arrow.right")
                        .font(.system(size: 14))
                        .foregroundStyle(.secondary)
                    Spacer()
                    Text("\(state.targetTempF)\u{00B0}F")
                        .font(.system(size: 28, weight: .bold))
                        .foregroundStyle(.secondary)
                }
            }

            // Progress bar
            HeatProgressBar(progressPct: state.progressPct, phase: state.phase)

        }
        .padding()
        .accessibilityElement(children: .combine)
        .accessibilityLabel(accessibilityDescription(for: state))
    }

    private func accessibilityDescription(for state: SpaHeatAttributes.ContentState) -> String {
        if state.phase == "reached" {
            return "Spa is ready at \(state.targetTempF) degrees"
        }

        var parts = ["Spa heating, currently \(state.currentTempF) degrees, target \(state.targetTempF) degrees"]
        if let minutes = state.minutesRemaining {
            parts.append("approximately \(minutes) minutes remaining")
        }
        parts.append("\(state.progressPct) percent progress")
        return parts.joined(separator: ", ")
    }
}

// MARK: - Dynamic Island expanded view

private struct ExpandedView: View {
    let context: ActivityViewContext<SpaHeatAttributes>

    var body: some View {
        let state = context.state

        VStack(alignment: .leading, spacing: 6) {
            // Title
            Label("Spa Heating", systemImage: "flame.fill")
                .font(.system(size: 14, weight: .semibold))
                .foregroundStyle(.white)

            // Temps
            HStack {
                if state.phase == "reached" {
                    Image(systemName: "checkmark.circle.fill")
                        .foregroundStyle(Color.tempGreen)
                    Text("Spa is ready!")
                        .font(.system(size: 20, weight: .bold))
                        .foregroundStyle(.white)
                    Spacer()
                    Text("\(state.targetTempF)\u{00B0}F")
                        .font(.system(size: 20, weight: .bold))
                        .foregroundStyle(Color.tempGreen)
                } else {
                    Text("\(state.currentTempF)\u{00B0}F")
                        .font(.system(size: 20, weight: .bold))
                        .foregroundStyle(.white)
                    Spacer()
                    Text("\(state.targetTempF)\u{00B0}F")
                        .font(.system(size: 20, weight: .bold))
                        .foregroundStyle(.secondary)
                }
            }

            // Progress bar
            HeatProgressBar(progressPct: state.progressPct, phase: state.phase)

            // Bottom row: ETA
            if state.phase != "reached", let eta = etaText(minutesRemaining: state.minutesRemaining) {
                HStack {
                    Spacer()
                    Text("\(eta) remaining")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(Color.orange)
                }
            }
        }
        .padding(.horizontal, 4)
        .accessibilityElement(children: .combine)
    }
}

// MARK: - Activity configuration

struct SpaHeatLiveActivity: Widget {
    var body: some WidgetConfiguration {
        ActivityConfiguration(for: SpaHeatAttributes.self) { context in
            // Lock Screen / StandBy presentation
            LockScreenView(context: context)
                .activityBackgroundTint(.black.opacity(0.7))
                .activitySystemActionForegroundColor(.white)
        } dynamicIsland: { context in
            DynamicIsland {
                // Expanded regions
                DynamicIslandExpandedRegion(.leading) {
                    EmptyView()
                }
                DynamicIslandExpandedRegion(.trailing) {
                    EmptyView()
                }
                DynamicIslandExpandedRegion(.center) {
                    ExpandedView(context: context)
                }
                DynamicIslandExpandedRegion(.bottom) {
                    EmptyView()
                }
            } compactLeading: {
                // Current temperature
                Text("\(context.state.currentTempF)\u{00B0}")
                    .font(.system(size: 17, weight: .bold))
                    .foregroundStyle(.white)
            } compactTrailing: {
                // ETA or status
                if context.state.phase == "reached" {
                    Image(systemName: "checkmark.circle.fill")
                        .font(.system(size: 17))
                        .foregroundStyle(Color.tempGreen)
                } else if let minutes = context.state.minutesRemaining {
                    Text("\(minutes)m")
                        .font(.system(size: 17, weight: .medium))
                        .foregroundStyle(Color.orange)
                } else {
                    Text("...")
                        .font(.system(size: 17))
                        .foregroundStyle(Color.orange)
                }
            } minimal: {
                // Minimal: current temp only
                Text("\(context.state.currentTempF)\u{00B0}")
                    .font(.system(size: 15, weight: .bold))
                    .foregroundStyle(.white)
            }
        }
    }
}
