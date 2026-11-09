// ccez-keys-translate: Apple's on-device translation for the Mac app.
// Reads one JSON request per line on stdin: {"id":1,"from":"en","to":"fr","text":"…"}
// and answers one line each: {"id":1,"text":"…"} or {"id":1,"error":"…"}.
// Only languages already installed on the Mac (System Settings > General >
// Language & Region > Translation Languages); it never downloads.
import Foundation
import Translation

struct Ask: Decodable { let id: Int; let from: String; let to: String; let text: String }
struct Answer: Encodable { let id: Int; let text: String?; let error: String? }

actor Sessions {
    var open: [String: TranslationSession] = [:]
    func session(_ a: Ask) -> TranslationSession {
        let key = "\(a.from)>\(a.to)"
        if let have = open[key] { return have }
        let s = TranslationSession(installedSource: Locale.Language(identifier: a.from), target: Locale.Language(identifier: a.to))
        open[key] = s
        return s
    }
    func drop(_ a: Ask) { open["\(a.from)>\(a.to)"] = nil }
    // Each language's session translates on its own (an actor method would line them up).
    nonisolated func translate(_ a: Ask) async -> Answer {
        let s = await session(a)
        do {
            let r = try await s.translate(a.text)
            return Answer(id: a.id, text: r.targetText, error: nil)
        } catch {
            await drop(a)
            return Answer(id: a.id, text: nil, error: "\(error)")
        }
    }
}

// Requests run at the same time (each language's model loads apart, about
// 0.3 s); answers go out as they finish, matched by id. Reading stdin has
// its own thread: a blocking read would hold up the tasks.
let sessions = Sessions()
let out = DispatchQueue(label: "out")
Thread.detachNewThread {
    while let line = readLine() {
        guard let data = line.data(using: .utf8), let ask = try? JSONDecoder().decode(Ask.self, from: data) else { continue }
        Task {
            let answer = await sessions.translate(ask)
            if let data = try? JSONEncoder().encode(answer), let s = String(data: data, encoding: .utf8) {
                out.sync { print(s); fflush(stdout) }
            }
        }
    }
    exit(0)
}
while true { try? await Task.sleep(nanoseconds: 3_600_000_000_000) }
