import SwiftUI

@main
struct PentairIOSApp: App {
    @StateObject private var viewModel = PoolViewModel()

    var body: some Scene {
        WindowGroup {
            ContentView(viewModel: viewModel)
        }
    }
}
