import SwiftUI
import AVFoundation

/// In-app dictation with fully local whisper (oratio-core via FFI):
/// record → transcribe on-device → polish (if configured) → copy/share.
struct DictateView: View {
    @StateObject private var model = DictateModel()

    private let accent = Color(red: 0.77, green: 0.42, blue: 0.24)

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

    private let engine = AVAudioEngine()
    private var file: AVAudioFile?
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

    func toggle() {
        isRecording ? stop() : start()
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
            try session.setCategory(.record, mode: .measurement)
            try session.setActive(true)

            let url = FileManager.default.temporaryDirectory
                .appendingPathComponent("dictate-\(UUID().uuidString).wav")
            let input = engine.inputNode
            let format = input.outputFormat(forBus: 0)
            let file = try AVAudioFile(forWriting: url, settings: format.settings)
            self.file = file
            self.fileURL = url

            input.installTap(onBus: 0, bufferSize: 1024, format: format) { buffer, _ in
                try? file.write(from: buffer)
            }
            engine.prepare()
            try engine.start()
            isRecording = true
        } catch {
            statusHint = "Audio error: \(error.localizedDescription)"
        }
    }

    private func stop() {
        engine.stop()
        engine.inputNode.removeTap(onBus: 0)
        file = nil
        isRecording = false
        guard let url = fileURL else { return }
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

        Task.detached {
            var result: String
            do {
                result = try transcribeWav(
                    modelPath: modelPath, wavPath: url.path,
                    language: language, initialPrompt: prompt, useGpu: useGpu)
            } catch {
                result = ""
                Task { @MainActor [weak self] in
                    self?.statusHint = "Recognition: \(error.localizedDescription)"
                }
            }
            try? FileManager.default.removeItem(at: url)
            let polished = result.isEmpty ? "" : await PolishClient.polish(result)
            Task { @MainActor [weak self] in
                guard let self else { return }
                if !polished.isEmpty {
                    self.text = self.text.isEmpty ? polished : self.text + " " + polished
                }
                self.isProcessing = false
            }
        }
    }
}
