import Foundation

final class BonjourDiscovery: NSObject {
    var onResolvedURL: ((URL) -> Void)?
    var onStatusMessage: ((String) -> Void)?
    var onEvent: ((String) -> Void)?

    private let browser = NetServiceBrowser()
    private var services: [NetService] = []
    private var resolvedURLs = Set<String>()

    override init() {
        super.init()
        browser.delegate = self
    }

    func start() {
        stop()
        browser.searchForServices(ofType: "_pentair._tcp.", inDomain: "local.")
        onStatusMessage?("Browsing for Pentair daemons on your network.")
        onEvent?("Started Bonjour browse for _pentair._tcp.local.")
    }

    func stop() {
        browser.stop()
        services.forEach { $0.stop() }
        services.removeAll()
        resolvedURLs.removeAll()
    }
}

extension BonjourDiscovery: NetServiceBrowserDelegate {
    func netServiceBrowserWillSearch(_ browser: NetServiceBrowser) {
        onStatusMessage?("Searching nearby daemons.")
        onEvent?("Bonjour browser started.")
    }

    func netServiceBrowser(_ browser: NetServiceBrowser, didFind service: NetService, moreComing: Bool) {
        onEvent?("Found service \(service.name) type \(service.type) domain \(service.domain)")
        service.delegate = self
        services.append(service)
        service.resolve(withTimeout: 5)
    }

    func netServiceBrowser(_ browser: NetServiceBrowser, didNotSearch errorDict: [String: NSNumber]) {
        onStatusMessage?("Bonjour search failed.")
        onEvent?("Bonjour search failed: \(errorDict)")
    }
}

extension BonjourDiscovery: NetServiceDelegate {
    func netServiceDidResolveAddress(_ sender: NetService) {
        guard
            let host = sender.hostName?.trimmingCharacters(in: CharacterSet(charactersIn: ".")),
            sender.port > 0,
            let url = URL(string: "http://\(host):\(sender.port)")
        else {
            onEvent?("Resolved service \(sender.name) but host or port was missing.")
            return
        }

        let normalized = url.absoluteString
        guard resolvedURLs.insert(normalized).inserted else {
            onEvent?("Ignoring duplicate resolved URL \(normalized)")
            return
        }

        onEvent?("Resolved \(sender.name) to \(normalized)")
        onResolvedURL?(url)
    }

    func netService(_ sender: NetService, didNotResolve errorDict: [String: NSNumber]) {
        onStatusMessage?("Found a daemon but could not resolve its address.")
        onEvent?("Failed resolving \(sender.name): \(errorDict)")
    }
}
