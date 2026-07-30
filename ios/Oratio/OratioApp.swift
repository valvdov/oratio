import SwiftUI
import Speech

extension Notification.Name {
    static let oratioAutoDictate = Notification.Name("oratio.autodictate")
}

@main
struct OratioApp: App {
    @State private var tab = 0
    @AppStorage("theme", store: SharedSettings.defaults) private var themeRaw = "ember"

    init() {
        PolishClient.localPolisher = { raw in await LocalPolish.polish(raw) }
    }

    private var theme: OratioTheme { OratioTheme(rawValue: themeRaw) ?? .ember }

    var body: some Scene {
        WindowGroup {
            TabView(selection: $tab) {
                NavigationStack { DictateView() }
                    .tabItem { Label("Dictate", systemImage: "mic.fill") }
                    .tag(0)
                HistoryView()
                    .tabItem { Label("History", systemImage: "clock") }
                    .tag(1)
                SettingsView()
                    .tabItem { Label("Settings", systemImage: "gearshape") }
                    .tag(2)
            }
            .tint(theme.accent)
            .preferredColorScheme(theme.colorScheme)
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
    @State private var localModel = LocalPolish.activeModel
    @State private var llmProgress: [String: Double] = [:]
    @State private var llmObservations: [String: NSKeyValueObservation] = [:]
    @State private var llmRefresh = 0
    @AppStorage("theme", store: SharedSettings.defaults) private var themeRaw = "ember"
    @State private var styleId = SharedSettings.styleId

    private var accent: Color { OratioTheme.current.accent }

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

                Section {
                    HStack(spacing: 12) {
                        ForEach(OratioTheme.allCases) { theme in
                            Button {
                                themeRaw = theme.rawValue
                            } label: {
                                VStack(spacing: 6) {
                                    Circle()
                                        .fill(theme.accent)
                                        .frame(width: 28, height: 28)
                                        .overlay {
                                            Circle().strokeBorder(
                                                themeRaw == theme.rawValue
                                                    ? Color.primary : .clear,
                                                lineWidth: 2)
                                        }
                                    Text(theme.label).font(.caption)
                                }
                                .frame(maxWidth: .infinity)
                                .padding(.vertical, 6)
                                .background(
                                    themeRaw == theme.rawValue
                                        ? theme.accent.opacity(0.12) : .clear,
                                    in: RoundedRectangle(cornerRadius: 10))
                            }
                            .buttonStyle(.plain)
                        }
                    }
                } header: {
                    Text("Appearance")
                } footer: {
                    Text("Cream and Peach are light, Ember is dark — same as desktop.")
                }

                Section {
                    Picker("Style", selection: $styleId) {
                        ForEach(PolishClient.styles, id: \.id) { style in
                            Text(style.label).tag(style.id)
                        }
                    }
                    .pickerStyle(.segmented)
                } header: {
                    Text("Polish style")
                } footer: {
                    Text(
                        "Formal — full polite sentences; Casual — short and lively; "
                        + "Prompt — rewrites the dictation as a structured AI prompt.")
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

                Section {
                    let _ = llmRefresh
                    ForEach(LocalPolish.catalog) { spec in
                        HStack {
                            VStack(alignment: .leading) {
                                Text(spec.label).font(.subheadline)
                            }
                            Spacer()
                            if let progress = llmProgress[spec.id] {
                                ProgressView(value: progress).frame(width: 70)
                            } else if LocalPolish.isDownloaded(spec) {
                                if localModel == spec.id {
                                    Text("active").font(.caption).foregroundStyle(.secondary)
                                } else {
                                    Button("Use") {
                                        localModel = spec.id
                                        LocalPolish.activeModel = spec.id
                                    }
                                }
                            } else {
                                Button("Get") { downloadLLM(spec) }
                            }
                        }
                    }
                    if !localModel.isEmpty {
                        Button("Disable local polish", role: .destructive) {
                            localModel = ""
                            LocalPolish.activeModel = ""
                        }
                    }
                } header: {
                    Text("Local AI polish (on-device)")
                } footer: {
                    Text(
                        "Runs entirely on this device in the Oratio app. The keyboard "
                        + "cannot host an LLM (iOS memory limits) — it uses the cloud "
                        + "polish below when a key is set, or the in-app dictation flow."
                    )
                }

                Section("Cloud AI polish") {
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
            .onChange(of: styleId) { SharedSettings.styleId = styleId }
        }
    }

    private func downloadLLM(_ spec: LocalPolish.ModelSpec) {
        llmProgress[spec.id] = 0.001
        let task = URLSession.shared.downloadTask(with: spec.url) { tmp, _, error in
            Task { @MainActor in
                defer {
                    llmProgress.removeValue(forKey: spec.id)
                    llmObservations.removeValue(forKey: spec.id)
                    llmRefresh += 1
                }
                guard let tmp, error == nil else { return }
                let dest = LocalPolish.path(for: spec.id)
                try? FileManager.default.removeItem(at: dest)
                try? FileManager.default.moveItem(at: tmp, to: dest)
                if localModel.isEmpty {
                    localModel = spec.id
                    LocalPolish.activeModel = spec.id
                }
            }
        }
        llmObservations[spec.id] = task.progress.observe(\.fractionCompleted) { progress, _ in
            Task { @MainActor in
                llmProgress[spec.id] = max(0.001, progress.fractionCompleted)
            }
        }
        task.resume()
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
