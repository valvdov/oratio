import SwiftUI
import AVFoundation

/// In-app dictation with fully local whisper (oratio-core via FFI):
/// record → transcribe on-device → polish (if configured) → copy/share.
struct DictateView: View {
    @StateObject private var model = DictateModel()
    @State private var styleId = SharedSettings.styleId

    private var accent: Color { OratioTheme.current.accent }

    var body: some View {
        VStack(spacing: 24) {
            Spacer()

            ScrollView {
                Text(model.text.isEmpty ? model.statusHint : model.text)
                    .font(.body)
                    .foregroundStyle(model.text.isEmpty ? .secondary : .primary)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding()
                    .textSelection(.enabled)
            }
            .frame(maxHeight: 280)
            .background(.quaternary.opacity(0.3), in: RoundedRectangle(cornerRadius: 16))
            .padding(.horizontal)

            Picker("Style", selection: $styleId) {
                ForEach(PolishClient.styles, id: \.id) { style in
                    Text(style.label).tag(style.id)
                }
            }
            .pickerStyle(.segmented)
            .padding(.horizontal)
            .onChange(of: styleId) { SharedSettings.styleId = styleId }

            if model.modelReady {
                Button(action: { model.toggle() }) {
                    ZStack {
                        Circle()
                            .fill(model.isRecording ? Color.red.opacity(0.85) : accent)
                            .frame(width: 96, height: 96)
                        if model.isProcessing {
                            ProgressView().tint(.white)
                        } else {
                            Image(systemName: model.isRecording ? "stop.fill" : "mic.fill")
                                .font(.system(size: 38, weight: .semibold))
                                .foregroundStyle(.white)
                        }
                    }
                }
                .buttonStyle(.plain)
                .disabled(model.isProcessing)
            } else {
                VStack(spacing: 10) {
                    if model.downloadProgress > 0 && model.downloadProgress < 1 {
                        ProgressView(value: model.downloadProgress) {
                            Text("Downloading whisper model… \(Int(model.downloadProgress * 100))%")
                                .font(.footnote)
                        }
                        .padding(.horizontal, 32)
                    } else {
                        Button("Download whisper model (~180 MB)") {
                            model.downloadModel()
                        }
                        .buttonStyle(.borderedProminent)
                        .tint(accent)
                        Text("One-time download; recognition then runs fully on-device.")
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                    }
                }
            }

            HStack(spacing: 14) {
                Button {
                    UIPasteboard.general.string = model.text
                } label: {
                    Label("Copy", systemImage: "doc.on.doc")
                }
                .disabled(model.text.isEmpty)

                ShareLink(item: model.text) {
                    Label("Share", systemImage: "square.and.arrow.up")
                }
                .disabled(model.text.isEmpty)

                Button(role: .destructive) {
                    model.text = ""
                } label: {
                    Label("Clear", systemImage: "trash")
                }
                .disabled(model.text.isEmpty)
            }
            .padding(.bottom, 24)
        }
        .navigationTitle("Dictate")
        .onAppear { model.checkModel() }
        .onReceive(NotificationCenter.default.publisher(for: .oratioAutoDictate)) { _ in
            model.beginFromKeyboard()
        }
    }
}

@MainActor
final class DictateModel: ObservableObject {
    @Published var text = ""
    @Published var isRecording = false
    @Published var isProcessing = false
    @Published var modelReady = false
    @Published var downloadProgress: Double = 0
    @Published var statusHint = "Tap the microphone and speak. Everything runs on this device."

    private var recorder: AVAudioRecorder?
    private var fileURL: URL?
    private var downloadTask: URLSessionDownloadTask?
    private var progressObservation: NSKeyValueObservation?

    private static let modelFileName = "ggml-small-q5_1.bin"
    private static let modelURL = URL(
        string:
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_1.bin")!

    static var modelPath: URL {
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("models", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir.appendingPathComponent(modelFileName)
    }

    func checkModel() {
        let size = (try? FileManager.default.attributesOfItem(
            atPath: Self.modelPath.path)[.size] as? Int64) ?? 0
        modelReady = size > 100_000_000
    }

    func downloadModel() {
        downloadProgress = 0.001
        let task = URLSession.shared.downloadTask(with: Self.modelURL) { [weak self] tmp, _, error in
            Task { @MainActor in
                guard let self else { return }
                defer { self.downloadTask = nil }
                guard let tmp, error == nil else {
                    self.statusHint = "Download failed: \(error?.localizedDescription ?? "?")"
                    self.downloadProgress = 0
                    return
                }
                try? FileManager.default.removeItem(at: Self.modelPath)
                do {
                    try FileManager.default.moveItem(at: tmp, to: Self.modelPath)
                    self.downloadProgress = 1
                    self.checkModel()
                } catch {
                    self.statusHint = "Save failed: \(error.localizedDescription)"
                    self.downloadProgress = 0
                }
            }
        }
        progressObservation = task.progress.observe(\.fractionCompleted) { [weak self] progress, _ in
            Task { @MainActor in self?.downloadProgress = max(0.001, progress.fractionCompleted * 0.99) }
        }
        downloadTask = task
        task.resume()
    }

    /// True when this dictation was requested by the keyboard: the result is
    /// left in the App Group for the keyboard to insert on return.
    private var fromKeyboard = false

    func toggle() {
        isRecording ? stop() : start()
    }

    func beginFromKeyboard() {
        checkModel()
        guard modelReady, !isRecording, !isProcessing else { return }
        fromKeyboard = true
        text = ""
        start()
    }

    private func start() {
        AVAudioApplication.requestRecordPermission { [weak self] granted in
            Task { @MainActor in
                guard let self else { return }
                guard granted else {
                    self.statusHint = "Microphone access denied — enable it in Settings."
                    return
                }
                self.beginRecording()
            }
        }
    }

    private func beginRecording() {
        do {
            let session = AVAudioSession.sharedInstance()
            try session.setCategory(.record, mode: .default)
            try session.setActive(true)

            let url = FileManager.default.temporaryDirectory
                .appendingPathComponent("dictate-\(UUID().uuidString).wav")
            // AVAudioRecorder writes whisper's native format directly and
            // finalizes the wav header synchronously on stop() — the previous
            // AVAudioEngine tap left the header at 0 frames on real devices.
            let settings: [String: Any] = [
                AVFormatIDKey: Int(kAudioFormatLinearPCM),
                AVSampleRateKey: 16_000,
                AVNumberOfChannelsKey: 1,
                AVLinearPCMBitDepthKey: 16,
                AVLinearPCMIsFloatKey: false,
                AVLinearPCMIsBigEndianKey: false,
            ]
            let recorder = try AVAudioRecorder(url: url, settings: settings)
            guard recorder.record() else {
                statusHint = "Audio error: recording could not start."
                return
            }
            self.recorder = recorder
            self.fileURL = url
            isRecording = true
        } catch {
            statusHint = "Audio error: \(error.localizedDescription)"
        }
    }

    private func stop() {
        let duration = recorder?.currentTime ?? 0
        recorder?.stop()
        recorder = nil
        try? AVAudioSession.sharedInstance()
            .setActive(false, options: .notifyOthersOnDeactivation)
        isRecording = false
        guard let url = fileURL else { return }
        guard duration > 0.6 else {
            try? FileManager.default.removeItem(at: url)
            fileURL = nil
            statusHint = "Recording too short (\(String(format: "%.1f", duration)) s) — hold the mic and speak."
            return
        }
        fileURL = nil
        isProcessing = true

        let modelPath = Self.modelPath.path
        let language = String(SharedSettings.language.prefix(2))
        let dict = SharedSettings.dictionary.joined(separator: ", ")
        // Same trick as the desktop core: a code-switched seed sentence primes
        // whisper to keep English tech terms in Latin script within RU speech.
        let seed = language == "ru"
            ? "Запушь коммит в репозиторий на GitHub, задеплой на прод и открой pull request."
            : ""
        var prompt = seed
        if !dict.isEmpty {
            prompt += (prompt.isEmpty ? "" : " ") + "Термины: \(dict)."
        }

        // Metal in the iOS simulator cannot allocate whisper's buffers.
        #if targetEnvironment(simulator)
        let useGpu = false
        #else
        let useGpu = true
        #endif

        let recordedMs = Int64(duration * 1000)
        Task.detached {
            var result = ""
            var failure: String?
            do {
                let stats = try wavStats(wavPath: url.path)
                if stats.peak < 0.001 {
                    failure = "Microphone recorded silence "
                        + "(\(String(format: "%.1f", Double(stats.durationMs) / 1000)) s). "
                        + "Check Settings → Privacy & Security → Microphone → Oratio."
                } else {
                    do {
                        result = try transcribeWav(
                            modelPath: modelPath, wavPath: url.path,
                            language: language, initialPrompt: prompt, useGpu: useGpu)
                    } catch where useGpu {
                        // Metal misbehaves on some device/OS combos — retry on CPU.
                        unloadEngines()
                        result = try transcribeWav(
                            modelPath: modelPath, wavPath: url.path,
                            language: language, initialPrompt: prompt, useGpu: false)
                    }
                }
            } catch {
                failure = "Recognition: \(error.localizedDescription)"
            }
            if let failure {
                Task { @MainActor [weak self] in self?.statusHint = failure }
            }
            try? FileManager.default.removeItem(at: url)
            let polished = result.isEmpty ? "" : await PolishClient.polish(result)
            if !result.isEmpty {
                HistoryStore.add(
                    raw: result,
                    polished: polished == result ? nil : polished,
                    durationMs: recordedMs)
            }
            Task { @MainActor [weak self] in
                guard let self else { return }
                if !polished.isEmpty {
                    self.text = self.text.isEmpty ? polished : self.text + " " + polished
                    // Round-trip: hand the text to the keyboard (and clipboard
                    // as a fallback). One tap on "← back" and it auto-inserts.
                    if self.fromKeyboard {
                        SharedSettings.setPendingText(self.text)
                        UIPasteboard.general.string = self.text
                        self.statusHint = "Готово — вернитесь в приложение стрелкой ← вверху, текст вставится сам."
                    }
                }
                self.fromKeyboard = false
                self.isProcessing = false
            }
        }
    }
}
