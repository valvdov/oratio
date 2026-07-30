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

    /// Theme id (cream/peach/ember) — same trio as desktop.
    static var theme: String {
        get { defaults.string(forKey: "theme") ?? "ember" }
        set { defaults.set(newValue, forKey: "theme") }
    }

    /// Polish style id ("" = neutral; formal/casual/prompt as on desktop).
    static var styleId: String {
        get { defaults.string(forKey: "style") ?? "" }
        set { defaults.set(newValue, forKey: "style") }
    }

    // Cloud STT (whisper-quality recognition over an OpenAI-compatible
    // /audio/transcriptions endpoint). Off by default — the on-device
    // recognizer is the free fallback.
    static var cloudSTTEnabled: Bool {
        get { defaults.bool(forKey: "stt_cloud_enabled") }
        set { defaults.set(newValue, forKey: "stt_cloud_enabled") }
    }

    static var sttBaseURL: String {
        get { defaults.string(forKey: "stt_base_url") ?? "https://api.groq.com/openai/v1" }
        set { defaults.set(newValue, forKey: "stt_base_url") }
    }

    static var sttApiKey: String {
        get { defaults.string(forKey: "stt_api_key") ?? "" }
        set { defaults.set(newValue, forKey: "stt_api_key") }
    }

    static var sttModel: String {
        get { defaults.string(forKey: "stt_model") ?? "whisper-large-v3" }
        set { defaults.set(newValue, forKey: "stt_model") }
    }

    // Round-trip: the app leaves freshly dictated text here; the keyboard
    // inserts it when it becomes visible again.
    static func setPendingText(_ text: String) {
        defaults.set(text, forKey: "pending_text")
        defaults.set(Date().timeIntervalSince1970, forKey: "pending_ts")
    }

    /// Returns and clears the pending text when it is fresh (< 3 minutes old).
    static func takePendingText() -> String? {
        guard let text = defaults.string(forKey: "pending_text"), !text.isEmpty else { return nil }
        let ts = defaults.double(forKey: "pending_ts")
        defaults.removeObject(forKey: "pending_text")
        defaults.removeObject(forKey: "pending_ts")
        guard Date().timeIntervalSince1970 - ts < 180 else { return nil }
        return text
    }
}
