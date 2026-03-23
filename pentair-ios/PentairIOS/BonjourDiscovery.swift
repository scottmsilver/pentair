import Foundation
import Network

@MainActor
final class BonjourDiscovery {
    var onResolvedURL: ((URL) -> Void)?
    var onStatusMessage: ((String) -> Void)?
    var onEvent: ((String) -> Void)?

    private var browser: NWBrowser?
    private var resolvedURLs = Set<String>()
    private var activeConnections: [NWConnection] = []

    func start() {
        stop()

        let params = NWParameters.tcp
        let browser = NWBrowser(for: .bonjour(type: "_pentair._tcp", domain: "local."), using: params)

        browser.stateUpdateHandler = { [weak self] state in
            Task { @MainActor [weak self] in
                guard let self else { return }
                switch state {
                case .ready:
                    self.onStatusMessage?("Searching nearby daemons.")
                    self.onEvent?("Bonjour browser started.")
                case .failed(let error):
                    self.onStatusMessage?("Bonjour search failed.")
                    self.onEvent?("Bonjour search failed: \(error)")
                default:
                    break
                }
            }
        }

        browser.browseResultsChangedHandler = { [weak self] results, changes in
            Task { @MainActor [weak self] in
                guard let self else { return }
                for change in changes {
                    if case .added(let result) = change {
                        self.onEvent?("Found service: \(result.endpoint)")
                        self.resolve(endpoint: result.endpoint)
                    }
                }
            }
        }

        self.browser = browser
        browser.start(queue: .main)
        onStatusMessage?("Browsing for Pentair daemons on your network.")
        onEvent?("Started Bonjour browse for _pentair._tcp.local.")
    }

    func stop() {
        browser?.cancel()
        browser = nil
        for connection in activeConnections {
            connection.cancel()
        }
        activeConnections.removeAll()
        resolvedURLs.removeAll()
    }

    private func resolve(endpoint: NWEndpoint) {
        let connection = NWConnection(to: endpoint, using: .tcp)
        activeConnections.append(connection)

        connection.stateUpdateHandler = { [weak self] state in
            Task { @MainActor [weak self] in
                guard let self else { return }
                switch state {
                case .ready:
                    if let remoteEndpoint = connection.currentPath?.remoteEndpoint,
                       case .hostPort(let host, let port) = remoteEndpoint {
                        let hostString = "\(host)"
                            .replacingOccurrences(of: "%.*", with: "", options: .regularExpression)
                        let urlHost = hostString.contains(":") ? "[\(hostString)]" : hostString
                        let portInt = port.rawValue
                        let urlString = "http://\(urlHost):\(portInt)"

                        guard self.resolvedURLs.insert(urlString).inserted else {
                            self.onEvent?("Ignoring duplicate resolved URL \(urlString)")
                            connection.cancel()
                            self.removeConnection(connection)
                            return
                        }

                        if let url = URL(string: urlString) {
                            self.onEvent?("Resolved \(endpoint) to \(urlString)")
                            self.onResolvedURL?(url)
                        } else {
                            self.onEvent?("Resolved endpoint but could not form URL from \(urlString)")
                        }
                    } else {
                        self.onEvent?("Connection ready but no remote endpoint available.")
                    }
                    connection.cancel()
                    self.removeConnection(connection)

                case .failed(let error):
                    self.onStatusMessage?("Found a daemon but could not resolve its address.")
                    self.onEvent?("Failed resolving \(endpoint): \(error)")
                    connection.cancel()
                    self.removeConnection(connection)

                case .cancelled:
                    self.removeConnection(connection)

                default:
                    break
                }
            }
        }

        connection.start(queue: .main)
    }

    private func removeConnection(_ connection: NWConnection) {
        activeConnections.removeAll { $0 === connection }
    }
}
