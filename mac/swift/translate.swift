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
    func translate(_ a: Ask) async -> Answer {
        let key = "\(a.from)>\(a.to)"
        do {
            let s: TranslationSession
            if let have = open[key] { s = have } else {
                s = TranslationSession(installedSource: Locale.Language(identifier: a.from), target: Locale.Language(identifier: a.to))
                open[key] = s
            }
            let r = try await s.translate(a.text)
            return Answer(id: a.id, text: r.targetText, error: nil)
        } catch {
            open[key] = nil
            return Answer(id: a.id, text: nil, error: "\(error)")
        }
    }
}

// Top-level code with await: the framework answers (a semaphore-held main thread never got them).
let sessions = Sessions()
let enc = JSONEncoder()
while let line = readLine() {
    guard let data = line.data(using: .utf8), let ask = try? JSONDecoder().decode(Ask.self, from: data) else { continue }
    let answer = await sessions.translate(ask)
    if let out = try? enc.encode(answer), let s = String(data: out, encoding: .utf8) {
        print(s)
        fflush(stdout)
    }
}
