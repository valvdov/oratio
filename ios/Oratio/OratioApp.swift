import SwiftUI
import Speech

extension Notification.Name {
    static let oratioAutoDictate = Notification.Name("oratio.autodictate")
}

@main
struct OratioApp: App {
    @State private var tab = 0

    var body: some Scene {
        WindowGroup {
            TabView(selection: $tab) {
                NavigationStack { DictateView() }
                    .tabItem { Label("Dictate", systemImage: "mic.fill") }
                    .tag(0)
                SettingsView()
                    .tabItem { Label("Settings", systemImage: "gearshape") }
                    .tag(1)
            }
            .onOpenURL { url in
                // oratio://dictate — the keyboard asks for an in-app dictation.
                guard url.absoluteString.contains("dictate") else { return }
                tab = 0
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
                    NotificationCenter.default.post(name: .oratioAutoDictate, object: nil)
                }
            }
        }
    }
}

struct SettingsView: View {
    @State private var apiKey = SharedSettings.apiKey
    @State private var baseURL = SharedSettings.apiBaseURL
    @State private var model = SharedSettings.model
    @State private var polishEnabled = SharedSettings.polishEnabled
    @State private var language = SharedSettings.language
    @State private var dictionary = SharedSettings.dictionary
    @State private var newTerm = ""
    @State private var speechStatus = "unknown"
    @State private var cloudSTT = SharedSettings.cloudSTTEnabled
    @State private var sttBaseURL = SharedSettings.sttBaseURL
    @State private var sttApiKey = SharedSettings.sttApiKey
    @State private var sttModel = SharedSettings.sttModel

    private let accent = Color(red: 0.77, green: 0.42, blue: 0.24)

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Text(
                        """
                        1. Enable the keyboard: Settings → General → Keyboard → \
                        Keyboards → Add New Keyboard → Oratio, then Allow Full Access \
                        (needed for AI polish over the network).
                        2. In any app, switch to the Oratio keyboard with the globe key \
                        and tap the microphone.
                        """
                    )
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                } header: {
                    Text("Setup")
                }

                Section("Speech") {
                    Picker("Language", selection: $language) {
                        Text("Русский").tag("ru-RU")
                        Text("English").tag("en-US")
                    }
                    LabeledContent("Recognition permission", value: speechStatus)
                    Button("Request permissions") {
                        SFSpeechRecognizer.requestAuthorization { _ in updateSpeechStatus() }
                        AVAudioApplication.requestRecordPermission { _ in }
                    }
                }

                Section {
                    Toggle("Cloud recognition (whisper)", isOn: $cloudSTT)
                    if cloudSTT {
                        TextField("Base URL", text: $sttBaseURL)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                        TextField("Model", text: $sttModel)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                        SecureField("API key", text: $sttApiKey)
                    }
                } header: {
                    Text("Cloud STT")
                } footer: {
                    Text(
                        "Much better RU+EN accuracy than the on-device recognizer. "
                        + "Groq offers whisper-large-v3 with a free tier; one Groq key "
                        + "works for both recognition and polish."
                    )
                }

                Section("AI polish") {
                    Toggle("Polish with AI", isOn: $polishEnabled)
                    TextField("Base URL", text: $baseURL)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    TextField("Model", text: $model)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    SecureField("API key", text: $apiKey)
                }

                Section("Dictionary") {
                    ForEach(dictionary, id: \.self) { term in
                        Text(term)
                    }
                    .onDelete { dictionary.remove(atOffsets: $0) }
                    HStack {
                        TextField("Add term (e.g. Kubernetes)", text: $newTerm)
                            .autocorrectionDisabled()
                        Button("Add") {
                            let t = newTerm.trimmingCharacters(in: .whitespaces)
                            if !t.isEmpty, !dictionary.contains(t) {
                                dictionary.append(t)
                                newTerm = ""
                            }
                        }
                        .disabled(newTerm.trimmingCharacters(in: .whitespaces).isEmpty)
                    }
                }
            }
            .navigationTitle("Oratio")
            .tint(accent)
            .onAppear(perform: updateSpeechStatus)
            .onChange(of: apiKey) { SharedSettings.apiKey = apiKey }
            .onChange(of: baseURL) { SharedSettings.apiBaseURL = baseURL }
            .onChange(of: model) { SharedSettings.model = model }
            .onChange(of: polishEnabled) { SharedSettings.polishEnabled = polishEnabled }
            .onChange(of: language) { SharedSettings.language = language }
            .onChange(of: dictionary) { SharedSettings.dictionary = dictionary }
            .onChange(of: cloudSTT) { SharedSettings.cloudSTTEnabled = cloudSTT }
            .onChange(of: sttBaseURL) { SharedSettings.sttBaseURL = sttBaseURL }
            .onChange(of: sttApiKey) { SharedSettings.sttApiKey = sttApiKey }
            .onChange(of: sttModel) { SharedSettings.sttModel = sttModel }
        }
    }

    private func updateSpeechStatus() {
        switch SFSpeechRecognizer.authorizationStatus() {
        case .authorized: speechStatus = "granted"
        case .denied: speechStatus = "denied"
        case .restricted: speechStatus = "restricted"
        case .notDetermined: speechStatus = "not requested"
        @unknown default: speechStatus = "unknown"
        }
    }
}
