import UIKit
import SwiftUI
import Speech
import AVFoundation

/// The Oratio keyboard: one big dictation button plus the essentials
/// (globe, backspace, return). Speech is recognized on-device via
/// SFSpeechRecognizer; the transcript is AI-polished (if configured) and
/// inserted into the host app.
final class KeyboardViewController: UIInputViewController {
    private let state = KeyboardState()

    override func viewDidLoad() {
        super.viewDidLoad()

        state.needsGlobe = needsInputModeSwitchKey
        state.onGlobe = { [weak self] in self?.advanceToNextInputMode() }
        state.onInsert = { [weak self] text in self?.textDocumentProxy.insertText(text) }
        state.onBackspace = { [weak self] in self?.textDocumentProxy.deleteBackward() }
        state.onReturn = { [weak self] in self?.textDocumentProxy.insertText("\n") }

        let host = UIHostingController(rootView: KeyboardView(state: state))
        host.view.backgroundColor = .clear
        addChild(host)
        view.addSubview(host.view)
        host.view.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            host.view.topAnchor.constraint(equalTo: view.topAnchor),
            host.view.bottomAnchor.constraint(equalTo: view.bottomAnchor),
            host.view.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            host.view.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            view.heightAnchor.constraint(equalToConstant: 220),
        ])
    }

    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        // Round-trip: if the app just dictated something for us, insert it.
        if let pending = SharedSettings.takePendingText() {
            textDocumentProxy.insertText(pending)
        }
    }

    override func viewWillDisappear(_ animated: Bool) {
        super.viewWillDisappear(animated)
        state.stopRecording(discard: true)
    }

}

@MainActor
final class KeyboardState: ObservableObject {
    enum Phase: Equatable {
        case idle
        case recording
        case processing
        case error(String)
    }

    @Published var phase: Phase = .idle
    @Published var partial: String = ""
    var needsGlobe = true
    var onGlobe: () -> Void = {}
    var onInsert: (String) -> Void = { _ in }
    var onBackspace: () -> Void = {}
    var onReturn: () -> Void = {}

    private let audioEngine = AVAudioEngine()
    private var recognizer: SFSpeechRecognizer?
    private var request: SFSpeechAudioBufferRecognitionRequest?
    private var task: SFSpeechRecognitionTask?
    private var finalText = ""
    private var audioFile: AVAudioFile?
    private var audioFileURL: URL?

    func toggleRecording() {
        switch phase {
        case .recording: stopRecording(discard: false)
        case .processing: break
        default:
            if CloudSTT.isConfigured {
                startCloudRecording()
            } else {
                startRecording()
            }
        }
    }

    // MARK: Cloud STT: record to a wav file, upload on stop.

    private func startCloudRecording() {
        do {
            let session = AVAudioSession.sharedInstance()
            try session.setCategory(.record, mode: .measurement, options: .duckOthers)
            try session.setActive(true, options: .notifyOthersOnDeactivation)

            let url = FileManager.default.temporaryDirectory
                .appendingPathComponent("dictation-\(UUID().uuidString).wav")
            let input = audioEngine.inputNode
            let format = input.outputFormat(forBus: 0)
            let file = try AVAudioFile(forWriting: url, settings: format.settings)
            audioFile = file
            audioFileURL = url

            input.installTap(onBus: 0, bufferSize: 1024, format: format) { buffer, _ in
                try? file.write(from: buffer)
            }
            audioEngine.prepare()
            try audioEngine.start()
            partial = ""
            phase = .recording
        } catch {
            phase = .error(Self.describeAudioError(error))
        }
    }

    /// iOS refuses to start audio input inside keyboard extensions on recent
    /// versions (Apple bug FB16791704) — explain instead of dumping OSStatus.
    static func describeAudioError(_ error: Error) -> String {
        let code = (error as NSError).code
        if [2003329396, 561145187, 561015905].contains(code) {
            return "iOS blocks the mic in keyboards — use the big button to dictate in the app."
        }
        return "Audio: \(error.localizedDescription)"
    }

    private func finishCloud() async {
        guard let url = audioFileURL else {
            phase = .idle
            return
        }
        audioFile = nil
        audioFileURL = nil
        do {
            let raw = try await CloudSTT.transcribe(fileURL: url)
            try? FileManager.default.removeItem(at: url)
            guard !raw.isEmpty else {
                phase = .idle
                partial = ""
                return
            }
            let polished = await PolishClient.polish(raw)
            onInsert(polished)
            phase = .idle
            partial = ""
        } catch {
            try? FileManager.default.removeItem(at: url)
            phase = .error("STT: \(error.localizedDescription)")
        }
    }

    // MARK: On-device STT via SFSpeechRecognizer.

    private func startRecording() {
        let status = SFSpeechRecognizer.authorizationStatus()
        guard status == .authorized || status == .notDetermined else {
            phase = .error("Enable speech recognition in the Oratio app")
            return
        }
        if status == .notDetermined {
            SFSpeechRecognizer.requestAuthorization { [weak self] result in
                Task { @MainActor in
                    if result == .authorized { self?.startRecording() }
                    else { self?.phase = .error("Speech recognition denied") }
                }
            }
            return
        }

        recognizer = SFSpeechRecognizer(locale: Locale(identifier: SharedSettings.language))
        guard let recognizer, recognizer.isAvailable else {
            phase = .error("Recognizer unavailable")
            return
        }

        do {
            let session = AVAudioSession.sharedInstance()
            try session.setCategory(.record, mode: .measurement, options: .duckOthers)
            try session.setActive(true, options: .notifyOthersOnDeactivation)

            let request = SFSpeechAudioBufferRecognitionRequest()
            request.shouldReportPartialResults = true
            if recognizer.supportsOnDeviceRecognition {
                request.requiresOnDeviceRecognition = true
            }
            self.request = request

            let input = audioEngine.inputNode
            let format = input.outputFormat(forBus: 0)
            input.installTap(onBus: 0, bufferSize: 1024, format: format) { buffer, _ in
                request.append(buffer)
            }
            audioEngine.prepare()
            try audioEngine.start()

            finalText = ""
            partial = ""
            phase = .recording
        } catch {
            phase = .error(Self.describeAudioError(error))
            return
        }
        guard let request = self.request else { return }
        task = recognizer.recognitionTask(with: request) { [weak self] result, error in
            Task { @MainActor in
                guard let self else { return }
                if let result {
                    self.partial = result.bestTranscription.formattedString
                    if result.isFinal {
                        self.finalText = result.bestTranscription.formattedString
                        await self.finish()
                    }
                }
                if error != nil, self.phase == .processing {
                    // Recognition ended without a final result — use the partial.
                    self.finalText = self.partial
                    await self.finish()
                }
            }
        }
    }

    func stopRecording(discard: Bool) {
        guard phase == .recording else { return }
        audioEngine.stop()
        audioEngine.inputNode.removeTap(onBus: 0)
        let wasCloud = audioFile != nil
        request?.endAudio()
        if discard {
            task?.cancel()
            audioFile = nil
            if let url = audioFileURL {
                try? FileManager.default.removeItem(at: url)
                audioFileURL = nil
            }
            phase = .idle
            partial = ""
        } else {
            phase = .processing
            if wasCloud {
                audioFile = nil
                Task { await finishCloud() }
            }
        }
    }

    private func finish() async {
        task = nil
        request = nil
        let raw = finalText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !raw.isEmpty else {
            phase = .idle
            partial = ""
            return
        }
        let polished = await PolishClient.polish(raw)
        onInsert(polished)
        phase = .idle
        partial = ""
    }
}

struct KeyboardView: View {
    @ObservedObject var state: KeyboardState

    private var accent: Color { OratioTheme.current.accent }

    var body: some View {
        VStack(spacing: 10) {
            Text(statusText)
                .font(.footnote)
                .foregroundStyle(.secondary)
                .lineLimit(2)
                .frame(maxWidth: .infinity)
                .padding(.horizontal, 12)
                .padding(.top, 10)

            // iOS blocks microphone input inside keyboard extensions (Apple bug
            // FB16791704), so the big button hands dictation to the app; a
            // SwiftUI Link is the only URL-opening that iOS 18+ still allows
            // from a keyboard. The result auto-inserts on return (App Group).
            Link(destination: URL(string: "oratio://dictate")!) {
                ZStack {
                    Circle()
                        .fill(state.phase == .recording ? Color.red.opacity(0.85) : accent)
                        .frame(width: 84, height: 84)
                    Image(systemName: iconName)
                        .font(.system(size: 34, weight: .semibold))
                        .foregroundStyle(.white)
                }
            }
            .buttonStyle(.plain)
            .disabled(state.phase == .processing)

            HStack {
                if state.needsGlobe {
                    Button(action: { state.onGlobe() }) {
                        Image(systemName: "globe").frame(width: 60, height: 40)
                    }
                }
                // In-keyboard recognition attempt (works if Apple ever unblocks
                // the mic for keyboards; today it shows a friendly error).
                Button(action: { state.toggleRecording() }) {
                    Image(systemName: "waveform").frame(width: 60, height: 40)
                }
                Spacer()
                Button(action: { state.onBackspace() }) {
                    Image(systemName: "delete.left").frame(width: 60, height: 40)
                }
                Button(action: { state.onReturn() }) {
                    Image(systemName: "return").frame(width: 60, height: 40)
                }
            }
            .font(.system(size: 20))
            .foregroundStyle(.primary)
            .padding(.horizontal, 16)
            .padding(.bottom, 8)
        }
    }

    private var iconName: String {
        switch state.phase {
        case .recording: return "stop.fill"
        case .processing: return "ellipsis"
        default: return "mic.fill"
        }
    }

    private var statusText: String {
        switch state.phase {
        case .idle: return "Tap the mic — Oratio dictates, the text inserts here when you return."
        case .recording: return state.partial.isEmpty ? "Listening…" : state.partial
        case .processing: return "Polishing…"
        case .error(let message): return message
        }
    }
}
