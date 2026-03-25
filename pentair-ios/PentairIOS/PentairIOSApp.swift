import SwiftUI

@main
struct PentairIOSApp: App {
    @UIApplicationDelegateAdaptor(PoolAppDelegate.self) private var appDelegate
    @StateObject private var viewModel = PoolViewModel()

    var body: some Scene {
        WindowGroup {
            ContentView(viewModel: viewModel)
        }
    }
}
