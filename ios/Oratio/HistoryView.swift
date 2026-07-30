import SwiftUI

/// Thin wrapper over the FFI history store (same SQLite/FTS5 schema and
/// RU-friendly trigram search as the desktop app).
enum HistoryStore {
    static var dbPath: String {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("history.db").path
    }

    static func add(raw: String, polished: String?, durationMs: Int64) {
        _ = try? historyAdd(dbPath: dbPath, raw: raw, polished: polished, durationMs: durationMs)
    }

    static func search(_ query: String) -> [HistoryItem] {
        (try? historySearch(dbPath: dbPath, query: query, limit: 200)) ?? []
    }

    static func delete(_ id: Int64) {
        try? historyDelete(dbPath: dbPath, id: id)
    }

    /// created_at is SQLite UTC "YYYY-MM-DD HH:MM:SS"; show it in local time.
    static func displayDate(_ raw: String) -> String {
        let parser = DateFormatter()
        // Fixed-format parsing must not depend on the device locale.
        parser.locale = Locale(identifier: "en_US_POSIX")
        parser.dateFormat = "yyyy-MM-dd HH:mm:ss"
        parser.timeZone = TimeZone(identifier: "UTC")
        guard let date = parser.date(from: raw) else { return raw }
        return date.formatted(date: .abbreviated, time: .shortened)
    }
}

struct HistoryView: View {
    @State private var query = ""
    @State private var items: [HistoryItem] = []

    var body: some View {
        NavigationStack {
            List {
                ForEach(items, id: \.id) { item in
                    NavigationLink {
                        HistoryDetailView(item: item) {
                            HistoryStore.delete(item.id)
                            reload()
                        }
                    } label: {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(item.polishedText ?? item.rawText)
                                .lineLimit(3)
                                .foregroundStyle(.primary)
                            Text(HistoryStore.displayDate(item.createdAt))
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
                .onDelete { offsets in
                    for index in offsets {
                        HistoryStore.delete(items[index].id)
                    }
                    items.remove(atOffsets: offsets)
                }
            }
            .searchable(text: $query, prompt: "Search dictations")
            .onChange(of: query) { reload() }
            .onAppear(perform: reload)
            .navigationTitle("History")
            .overlay {
                if items.isEmpty {
                    ContentUnavailableView(
                        query.isEmpty ? "No dictations yet" : "Nothing found",
                        systemImage: "clock",
                        description: Text(
                            query.isEmpty
                                ? "Dictations you make will appear here."
                                : "Try a different query."))
                }
            }
        }
    }

    private func reload() {
        items = HistoryStore.search(query)
    }
}

/// Full dictation: polished and raw side by side, copy either, delete.
struct HistoryDetailView: View {
    let item: HistoryItem
    let onDelete: () -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var copied: String?

    var body: some View {
        List {
            if let polished = item.polishedText {
                section("Polished", text: polished)
            }
            section(item.polishedText == nil ? "Transcript" : "Raw transcript",
                    text: item.rawText)

            Section {
                Button("Delete dictation", role: .destructive) {
                    onDelete()
                    dismiss()
                }
            }
        }
        .navigationTitle(HistoryStore.displayDate(item.createdAt))
        .navigationBarTitleDisplayMode(.inline)
    }

    @ViewBuilder
    private func section(_ title: String, text: String) -> some View {
        Section {
            Text(text)
                .textSelection(.enabled)
            Button {
                UIPasteboard.general.string = text
                copied = title
                DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) {
                    if copied == title { copied = nil }
                }
            } label: {
                Label(copied == title ? "Copied" : "Copy",
                      systemImage: copied == title ? "checkmark" : "doc.on.doc")
            }
        } header: {
            Text(title)
        }
    }
}
