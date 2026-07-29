import Foundation

/// Whisper-quality transcription over an OpenAI-compatible
/// `/audio/transcriptions` endpoint (Groq, OpenAI, etc.).
enum CloudSTT {
    static var isConfigured: Bool {
        SharedSettings.cloudSTTEnabled && !SharedSettings.sttApiKey.isEmpty
    }

    static func transcribe(fileURL: URL) async throws -> String {
        let base = SharedSettings.sttBaseURL.hasSuffix("/")
            ? String(SharedSettings.sttBaseURL.dropLast())
            : SharedSettings.sttBaseURL
        guard let url = URL(string: base + "/audio/transcriptions") else {
            throw URLError(.badURL)
        }

        let boundary = "oratio-\(UUID().uuidString)"
        var request = URLRequest(url: url, timeoutInterval: 30)
        request.httpMethod = "POST"
        request.setValue("Bearer \(SharedSettings.sttApiKey)", forHTTPHeaderField: "Authorization")
        request.setValue(
            "multipart/form-data; boundary=\(boundary)", forHTTPHeaderField: "Content-Type")

        let audio = try Data(contentsOf: fileURL)
        // Two-letter language code from the locale ("ru-RU" -> "ru").
        let lang = String(SharedSettings.language.prefix(2))

        var body = Data()
        func field(_ name: String, _ value: String) {
            body.append("--\(boundary)\r\n".data(using: .utf8)!)
            body.append(
                "Content-Disposition: form-data; name=\"\(name)\"\r\n\r\n\(value)\r\n"
                    .data(using: .utf8)!)
        }
        field("model", SharedSettings.sttModel)
        field("language", lang)
        field("temperature", "0")
        body.append("--\(boundary)\r\n".data(using: .utf8)!)
        body.append(
            "Content-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\n"
                .data(using: .utf8)!)
        body.append("Content-Type: audio/wav\r\n\r\n".data(using: .utf8)!)
        body.append(audio)
        body.append("\r\n--\(boundary)--\r\n".data(using: .utf8)!)
        request.httpBody = body

        let (data, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
            let text = String(data: data, encoding: .utf8) ?? ""
            throw NSError(
                domain: "CloudSTT", code: 1,
                userInfo: [NSLocalizedDescriptionKey: "STT HTTP error: \(text.prefix(120))"])
        }
        guard let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let text = json["text"] as? String
        else {
            throw NSError(
                domain: "CloudSTT", code: 2,
                userInfo: [NSLocalizedDescriptionKey: "Unexpected STT response"])
        }
        return text.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
