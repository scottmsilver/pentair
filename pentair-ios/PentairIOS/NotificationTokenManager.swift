import Foundation
import OSLog

protocol NotificationTokenDefaults: AnyObject {
    func string(forKey defaultName: String) -> String?
    func setString(_ value: String?, forKey defaultName: String)
}

extension UserDefaults: NotificationTokenDefaults {
    func setString(_ value: String?, forKey defaultName: String) {
        set(value, forKey: defaultName)
    }
}

actor NotificationTokenManager {
    static let shared = NotificationTokenManager()

    private static let logger = Logger(subsystem: "com.ssilver.pentair.ios", category: "push")

    private let defaults: NotificationTokenDefaults
    private let registerer: (URL, String) async throws -> Void

    private let tokenKey = "pentair.notificationToken"
    private let lastRegisteredTokenKey = "pentair.notificationToken.lastRegistered"
    private let lastRegisteredAddressKey = "pentair.notificationToken.lastRegisteredAddress"

    init(
        defaults: NotificationTokenDefaults = UserDefaults.standard,
        registerer: @escaping (URL, String) async throws -> Void = { baseURL, token in
            try await PoolAPI(baseURL: baseURL).post("/api/devices/register", body: ["token": token])
        }
    ) {
        self.defaults = defaults
        self.registerer = registerer
    }

    var currentToken: String? {
        defaults.string(forKey: tokenKey)
    }

    func saveToken(_ token: String, activeBaseURL: URL?) async {
        defaults.setString(token, forKey: tokenKey)
        await ensureRegistered(activeBaseURL: activeBaseURL)
    }

    func ensureRegistered(activeBaseURL: URL?) async {
        guard
            let activeBaseURL,
            let token = defaults.string(forKey: tokenKey),
            shouldRegister(token: token, baseURL: activeBaseURL)
        else {
            return
        }

        do {
            try await registerer(activeBaseURL, token)
            defaults.setString(token, forKey: lastRegisteredTokenKey)
            defaults.setString(activeBaseURL.absoluteString, forKey: lastRegisteredAddressKey)
            Self.logger.info("Registered notification token with daemon \(activeBaseURL.absoluteString, privacy: .public)")
        } catch {
            Self.logger.error("Failed to register notification token: \(error.localizedDescription, privacy: .public)")
        }
    }

    private func shouldRegister(token: String, baseURL: URL) -> Bool {
        let lastToken = defaults.string(forKey: lastRegisteredTokenKey)
        let lastAddress = defaults.string(forKey: lastRegisteredAddressKey)
        return lastToken != token || lastAddress != baseURL.absoluteString
    }
}
