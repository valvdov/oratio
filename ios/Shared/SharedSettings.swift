import Foundation

/// Settings shared between the container app and the keyboard extension via
/// the App Group. Falls back to standard defaults when the group container is
/// unavailable (e.g. unsigned simulator builds).
enum SharedSettings {
    static let appGroup = "group.dev.valvdov.oratio"

    static var defaults: UserDefaults {
        UserDefaults(suiteName: appGroup) ?? .standard
    }

    static var apiBaseURL: String {
        get { defaults.string(forKey: "polish_base_url") ?? "https://openrouter.ai/api/v1" }
        set { defaults.set(newValue, forKey: "polish_base_url") }
    }

    static var apiKey: String {
        get { defaults.string(forKey: "polish_api_key") ?? "" }
        set { defaults.set(newValue, forKey: "polish_api_key") }
    }

    static var model: String {
        get { defaults.string(forKey: "polish_model") ?? "meta-llama/llama-3.3-70b-instruct:free" }
        set { defaults.set(newValue, forKey: "polish_model") }
    }

    static var polishEnabled: Bool {
        get { defaults.object(forKey: "polish_enabled") as? Bool ?? true }
        set { defaults.set(newValue, forKey: "polish_enabled") }
    }

    static var dictionary: [String] {
        get { defaults.stringArray(forKey: "dictionary") ?? [] }
        set { defaults.set(newValue, forKey: "dictionary") }
    }

    /// Speech recognizer locale identifier.
    static var language: String {
        get { defaults.string(forKey: "language") ?? "ru-RU" }
        set { defaults.set(newValue, forKey: "language") }
    }
}
