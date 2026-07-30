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
}

struct HistoryView: View {
    @State private var query = ""
    @State private var items: [HistoryItem] = []
    @State private var copiedId: Int64?

    var body: some View {
        NavigationStack {
            List {
                ForEach(items, id: \.id) { item in
                    Button {
                        UIPasteboard.general.string = item.polishedText ?? item.rawText
                        copiedId = item.id
                        DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) {
                            if copiedId == item.id { copiedId = nil }
                        }
                    } label: {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(item.polishedText ?? item.rawText)
                                .lineLimit(4)
                                .foregroundStyle(.primary)
                            HStack {
                                Text(Self.displayDate(item.createdAt))
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                if copiedId == item.id {
                                    Label("copied", systemImage: "checkmark")
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                }
                            }
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
                                ? "Dictations you make will appear here. Tap one to copy it."
                                : "Try a different query."))
                }
            }
        }
    }

    private func reload() {
        items = HistoryStore.search(query)
    }

    /// created_at is SQLite UTC "YYYY-MM-DD HH:MM:SS"; show it in local time.
    private static func displayDate(_ raw: String) -> String {
        let parser = DateFormatter()
        parser.dateFormat = "yyyy-MM-dd HH:mm:ss"
        parser.timeZone = TimeZone(identifier: "UTC")
        guard let date = parser.date(from: raw) else { return raw }
        return date.formatted(date: .abbreviated, time: .shortened)
    }
}
