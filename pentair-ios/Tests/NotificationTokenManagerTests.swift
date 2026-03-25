import Foundation

private final class InMemoryDefaults: NotificationTokenDefaults {
    private var storage: [String: String] = [:]

    func string(forKey defaultName: String) -> String? {
        storage[defaultName]
    }

    func setString(_ value: String?, forKey defaultName: String) {
        storage[defaultName] = value
    }
}

private func expect(_ condition: @autoclosure () -> Bool, _ message: String) {
    if !condition() {
        fputs("FAIL: \(message)\n", stderr)
        exit(1)
    }
}

private func testSavingTokenRegistersWhenAddressIsKnown() async {
    let defaults = InMemoryDefaults()
    var registrations: [(String, String)] = []
    let manager = NotificationTokenManager(
        defaults: defaults,
        registerer: { baseURL, token in
            registrations.append((baseURL.absoluteString, token))
        }
    )

    await manager.saveToken("token-1", activeBaseURL: URL(string: "http://daemon:8080")!)

    expect(registrations.count == 1, "saving a token should register it immediately when daemon address is known")
    expect(registrations[0].0 == "http://daemon:8080", "registration should use the active daemon address")
    expect(registrations[0].1 == "token-1", "registration should use the saved token")
}

private func testEnsureRegisteredUsesCachedTokenLater() async {
    let defaults = InMemoryDefaults()
    var registrations: [(String, String)] = []
    let manager = NotificationTokenManager(
        defaults: defaults,
        registerer: { baseURL, token in
            registrations.append((baseURL.absoluteString, token))
        }
    )

    await manager.saveToken("token-2", activeBaseURL: nil)
    await manager.ensureRegistered(activeBaseURL: URL(string: "http://daemon:8080")!)

    expect(registrations.count == 1, "ensureRegistered should use the cached token once the daemon address is known")
    expect(registrations[0].1 == "token-2", "ensureRegistered should send the cached token")
}

private func testDuplicateRegistrationIsSkippedForSameTokenAndAddress() async {
    let defaults = InMemoryDefaults()
    var registrations: [(String, String)] = []
    let manager = NotificationTokenManager(
        defaults: defaults,
        registerer: { baseURL, token in
            registrations.append((baseURL.absoluteString, token))
        }
    )

    let daemon = URL(string: "http://daemon:8080")!
    await manager.saveToken("token-3", activeBaseURL: daemon)
    await manager.ensureRegistered(activeBaseURL: daemon)

    expect(registrations.count == 1, "same token and daemon address should not register twice")
}

@main
struct NotificationTokenManagerTestRunner {
    static func main() async {
        await testSavingTokenRegistersWhenAddressIsKnown()
        await testEnsureRegisteredUsesCachedTokenLater()
        await testDuplicateRegistrationIsSkippedForSameTokenAndAddress()
        print("NotificationTokenManagerTests passed")
    }
}
