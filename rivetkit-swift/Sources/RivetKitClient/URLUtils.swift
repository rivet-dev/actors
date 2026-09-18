import Foundation

enum URLUtils {
    private static let encodeURIComponentAllowed = CharacterSet(charactersIn: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.!~*'()")

    static func combineUrlPath(
        endpoint: String,
        path: String,
        queryParams: [String: String?] = [:]
    ) throws -> URL {
        guard let baseUrl = URL(string: endpoint) else {
            throw ConfigurationError("invalid URL: \(endpoint)")
        }

        let pathParts = path.split(separator: "?", maxSplits: 1, omittingEmptySubsequences: false)
        let pathOnly = String(pathParts.first ?? "")
        let existingQuery = pathParts.count > 1 ? String(pathParts[1]) : ""

        var basePath = baseUrl.path
        if basePath.hasSuffix("/") {
            basePath.removeLast()
        }
        let cleanPath = pathOnly.hasPrefix("/") ? pathOnly : "/" + pathOnly
        let fullPath = (basePath + cleanPath).replacingOccurrences(of: "//", with: "/")

        var queryParts: [String] = []
        if !existingQuery.isEmpty {
            queryParts.append(existingQuery)
        }
        for (key, value) in queryParams {
            if let value {
                let encodedKey = key.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) ?? key
                let encodedValue = value.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) ?? value
                queryParts.append("\(encodedKey)=\(encodedValue)")
            }
        }

        var components = URLComponents(url: baseUrl, resolvingAgainstBaseURL: false)
        // fullPath is already percent-encoded (see buildActorGatewayUrl); .path would
        // re-encode `%` and break actor IDs containing `:` (e.g. Cloudflare Workers).
        components?.percentEncodedPath = fullPath
        components?.percentEncodedQuery = queryParts.isEmpty ? nil : queryParts.joined(separator: "&")
        components?.fragment = nil

        guard let finalUrl = components?.url else {
            throw ConfigurationError("invalid URL: \(endpoint)")
        }

        return finalUrl
    }

    static func buildActorGatewayUrl(
        endpoint: String,
        actorId: String,
        token: String?,
        path: String = ""
    ) throws -> URL {
        let tokenSegment = token.map { "@\($0.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? $0)" } ?? ""
        let gatewayPath = "/gateway/\(actorId.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? actorId)\(tokenSegment)\(path)"
        return try combineUrlPath(endpoint: endpoint, path: gatewayPath)
    }

    static func toWebSocketUrl(_ url: URL) throws -> URL {
        var components = URLComponents(url: url, resolvingAgainstBaseURL: false)
        switch components?.scheme?.lowercased() {
        case "http":
            components?.scheme = "ws"
        case "https":
            components?.scheme = "wss"
        case "ws", "wss":
            break
        default:
            throw ConfigurationError("unsupported URL scheme for websocket: \(url.absoluteString)")
        }
        guard let wsUrl = components?.url else {
            throw ConfigurationError("invalid websocket URL: \(url.absoluteString)")
        }
        return wsUrl
    }

    static func encodeURIComponent(_ value: String) -> String {
        return value.addingPercentEncoding(withAllowedCharacters: encodeURIComponentAllowed) ?? value
    }
}
