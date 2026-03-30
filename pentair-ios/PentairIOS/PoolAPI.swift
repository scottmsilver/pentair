import Foundation

private struct APIErrorPayload: Decodable {
    let error: String
}

private struct APIResponse: Decodable {
    let ok: Bool
    let error: String?
}

enum PoolAPIError: LocalizedError {
    case invalidAddress
    case invalidResponse
    case httpStatus(Int)
    case server(String)

    var errorDescription: String? {
        switch self {
        case .invalidAddress:
            return "Enter a valid daemon address."
        case .invalidResponse:
            return "The daemon returned an unexpected response."
        case .httpStatus(let statusCode):
            return "The daemon returned HTTP \(statusCode)."
        case .server(let message):
            return message
        }
    }
}

struct PoolAPI {
    let baseURL: URL
    private let session: URLSession
    private let decoder: JSONDecoder

    init(baseURL: URL, session: URLSession = .shared) {
        self.baseURL = baseURL
        self.session = session

        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        self.decoder = decoder
    }

    static func normalizeBaseURL(_ rawValue: String) -> URL? {
        let trimmed = rawValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            return nil
        }

        let candidate = trimmed.contains("://") ? trimmed : "http://\(trimmed)"
        guard var components = URLComponents(string: candidate) else {
            return nil
        }

        guard components.host != nil else {
            return nil
        }

        components.scheme = components.scheme ?? "http"
        components.host = components.host?.trimmingCharacters(in: CharacterSet(charactersIn: "."))
        if components.percentEncodedPath == "/" {
            components.percentEncodedPath = ""
        } else if components.percentEncodedPath.hasSuffix("/") {
            components.percentEncodedPath.removeLast()
        }

        return components.url
    }

    func fetchPool() async throws -> PoolSystem {
        let url = try endpointURL(for: "/api/pool")
        var request = URLRequest(url: url)
        request.timeoutInterval = 10

        let (data, response) = try await session.data(for: request)
        try validate(response: response, data: data)

        do {
            return try decoder.decode(PoolSystem.self, from: data)
        } catch {
            if let payload = try? decoder.decode(APIErrorPayload.self, from: data) {
                throw PoolAPIError.server(payload.error)
            }
            throw error
        }
    }

    func post(_ path: String, body: [String: Any]? = nil) async throws {
        let url = try endpointURL(for: path)
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.timeoutInterval = 10

        if let body {
            request.httpBody = try JSONSerialization.data(withJSONObject: body)
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        }

        let (data, response) = try await session.data(for: request)
        try validate(response: response, data: data)

        guard !data.isEmpty else {
            return
        }

        if let payload = try? decoder.decode(APIResponse.self, from: data), payload.ok == false {
            throw PoolAPIError.server(payload.error ?? "The daemon rejected the request.")
        }
    }

    func goodnight() async throws {
        try await post("/api/goodnight")
    }

    func webSocketURL() -> URL? {
        guard var components = URLComponents(url: baseURL, resolvingAgainstBaseURL: false) else {
            return nil
        }

        components.scheme = components.scheme == "https" ? "wss" : "ws"
        let currentPath = components.percentEncodedPath.isEmpty ? "" : components.percentEncodedPath
        components.percentEncodedPath = currentPath + "/api/ws"
        return components.url
    }

    private func endpointURL(for path: String) throws -> URL {
        guard let url = URL(string: path, relativeTo: baseURL)?.absoluteURL else {
            throw PoolAPIError.invalidAddress
        }
        return url
    }

    private func validate(response: URLResponse, data: Data) throws {
        guard let httpResponse = response as? HTTPURLResponse else {
            throw PoolAPIError.invalidResponse
        }

        guard (200 ..< 300).contains(httpResponse.statusCode) else {
            if let payload = try? decoder.decode(APIErrorPayload.self, from: data) {
                throw PoolAPIError.server(payload.error)
            }
            throw PoolAPIError.httpStatus(httpResponse.statusCode)
        }
    }
}
