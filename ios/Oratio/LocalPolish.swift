import Foundation

/// On-device LLM polish (llama.cpp via oratio-core FFI). App-only: keyboard
/// extensions have nowhere near enough memory for an LLM.
enum LocalPolish {
    struct ModelSpec: Identifiable {
        let id: String // file name
        let label: String
        let url: URL
        let sizeMB: Int
    }

    static let catalog: [ModelSpec] = [
        ModelSpec(
            id: "Qwen3-0.6B-Q4_K_M.gguf",
            label: "Qwen3 0.6B — light, ~0.4 GB",
            url: URL(string: "https://huggingface.co/unsloth/Qwen3-0.6B-GGUF/resolve/main/Qwen3-0.6B-Q4_K_M.gguf")!,
            sizeMB: 400),
        ModelSpec(
            id: "Qwen3-1.7B-Q4_K_M.gguf",
            label: "Qwen3 1.7B — fast, ~1.1 GB",
            url: URL(string: "https://huggingface.co/unsloth/Qwen3-1.7B-GGUF/resolve/main/Qwen3-1.7B-Q4_K_M.gguf")!,
            sizeMB: 1100),
        ModelSpec(
            id: "Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
            label: "Qwen3 4B — best quality, ~2.5 GB",
            url: URL(string: "https://huggingface.co/unsloth/Qwen3-4B-Instruct-2507-GGUF/resolve/main/Qwen3-4B-Instruct-2507-Q4_K_M.gguf")!,
            sizeMB: 2500),
        ModelSpec(
            id: "gemma-3-1b-it-Q4_K_M.gguf",
            label: "Gemma 3 1B — ~0.8 GB",
            url: URL(string: "https://huggingface.co/unsloth/gemma-3-1b-it-GGUF/resolve/main/gemma-3-1b-it-Q4_K_M.gguf")!,
            sizeMB: 800),
    ]

    static var modelsDir: URL {
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("models", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    static func path(for id: String) -> URL {
        modelsDir.appendingPathComponent(id)
    }

    static func isDownloaded(_ spec: ModelSpec) -> Bool {
        let size = (try? FileManager.default.attributesOfItem(
            atPath: path(for: spec.id).path)[.size] as? Int64) ?? 0
        return size > Int64(spec.sizeMB) * 1_000_000 / 2
    }

    /// Selected local polish model file name ("" = local polish off).
    static var activeModel: String {
        get { SharedSettings.defaults.string(forKey: "polish_local_model") ?? "" }
        set { SharedSettings.defaults.set(newValue, forKey: "polish_local_model") }
    }

    /// Running inside an app extension? (keyboard must not load LLMs)
    static var isExtension: Bool {
        Bundle.main.bundlePath.hasSuffix(".appex")
    }

    static var isReady: Bool {
        guard !isExtension, !activeModel.isEmpty else { return false }
        let p = path(for: activeModel).path
        return FileManager.default.fileExists(atPath: p)
    }

    /// Why the last polish() returned nil — shown in the UI instead of
    /// silently inserting raw text.
    nonisolated(unsafe) static var lastError: String?

    static func polish(_ raw: String) async -> String? {
        guard isReady else { return nil }
        lastError = nil
        // Qwen3 hybrid models: suppress thinking blocks.
        let prompt = PolishClient.composedSystemPrompt() + " /no_think"

        #if targetEnvironment(simulator)
        let useGpuFirst = false
        #else
        let useGpuFirst = true
        #endif

        let modelPath = path(for: activeModel).path
        return await Task.detached(priority: .userInitiated) { () -> String? in
            func run(_ gpu: Bool) throws -> String {
                try polishLocal(
                    modelPath: modelPath, systemPrompt: prompt, text: raw, useGpu: gpu)
            }
            do {
                var cleaned: String
                do {
                    cleaned = try run(useGpuFirst)
                } catch where useGpuFirst {
                    // Metal can fail on some device/OS combos — drop the broken
                    // engine and retry on CPU before giving up.
                    unloadEngines()
                    cleaned = try run(false)
                }
                let trimmed = cleaned.trimmingCharacters(in: .whitespacesAndNewlines)
                guard !trimmed.isEmpty else {
                    LocalPolish.lastError = "model returned empty output"
                    return nil
                }
                guard trimmed.count <= raw.count * 2 + 40 else {
                    LocalPolish.lastError = "model output looked like an answer, not a cleanup"
                    return nil
                }
                return trimmed
            } catch {
                LocalPolish.lastError = error.localizedDescription
                return nil
            }
        }.value
    }
}
