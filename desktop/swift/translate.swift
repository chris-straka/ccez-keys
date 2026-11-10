// ccez-keys-translate: Apple's on-device translation for the Mac app.
// Reads one JSON request per line on stdin: {"id":1,"from":"en","to":"fr","text":"…"}
// and answers one line each: {"id":1,"text":"…"} or {"id":1,"error":"…"}.
// Only languages already installed on the Mac (System Settings > General >
// Language & Region > Translation Languages); it never downloads.
import Foundation
import Translation
import FoundationModels

// Or, for a word's card: {"id":2,"meaning":{"lang":"fr","word":"d'accord","sentence":"D'accord, on y va."}}
// → {"id":2,"text":"okay, agreed"} from Apple's on-device model, or {"id":2,"error":"unavailable: …"}.
struct Meaning: Decodable { let lang: String; let word: String; let sentence: String }
struct Ask: Decodable { let id: Int; let from: String?; let to: String?; let text: String?; let readings: Bool?; let meaning: Meaning? }
struct Answer: Encodable { let id: Int; let text: String?; let error: String?; var ruby: [[String]]? = nil }

/// A Japanese word's reading shared out over its kanji runs, the kana in it
/// matched against the reading ("間に合わせ", "まにあわせ" → 間:ま に 合:あ わせ),
/// each part with the whole word. Nil when it's all kanji or doesn't line up.
func okurigana(_ word: String, _ reading: String, _ han: (String) -> Bool) -> [[String]]? {
    var runs: [(String, Bool)] = []
    for ch in word {
        let k = han(String(ch))
        if let last = runs.last, last.1 == k { runs[runs.count - 1].0 += String(ch) } else { runs.append((String(ch), k)) }
    }
    guard runs.count > 1, !reading.isEmpty else { return nil }
    let hira = { (s: String) in s.applyingTransform(.hiraganaToKatakana, reverse: true) ?? s }
    var pattern = "^"
    for (text, kanji) in runs { pattern += kanji ? "(.+?)" : NSRegularExpression.escapedPattern(for: hira(text)) }
    pattern += "$"
    guard let re = try? NSRegularExpression(pattern: pattern),
          let m = re.firstMatch(in: reading, range: NSRange(reading.startIndex..., in: reading)) else { return nil }
    var out: [[String]] = []
    var group = 1
    for (text, kanji) in runs {
        if kanji {
            let r = Range(m.range(at: group), in: reading).map { String(reading[$0]) } ?? ""
            out.append([text, r, word])
            group += 1
        } else {
            out.append([text, "", word])
        }
    }
    return out
}

/// Japanese and Chinese: each word with its reading (hiragana over kanji, pinyin
/// over hanzi), from macOS's own transcription; pieces without Han get none.
func ruby(_ text: String, _ lang: String) -> [[String]] {
    let ns = text as NSString
    let cf = text as CFString
    let tok = CFStringTokenizerCreate(nil, cf, CFRange(location: 0, length: ns.length), kCFStringTokenizerUnitWordBoundary, Locale(identifier: lang) as CFLocale)
    var out: [[String]] = []
    var at = 0
    let han = { (s: String) in s.unicodeScalars.contains { (0x4E00...0x9FFF).contains($0.value) || (0x3400...0x4DBF).contains($0.value) || $0.value == 0x3005 } }
    while CFStringTokenizerAdvanceToNextToken(tok).rawValue != 0 {
        let r = CFStringTokenizerGetCurrentTokenRange(tok)
        if r.location > at { out.append([ns.substring(with: NSRange(location: at, length: r.location - at)), ""]) }
        let piece = ns.substring(with: NSRange(location: r.location, length: r.length))
        if lang.hasPrefix("zh") && han(piece) {
            // Chinese: a syllable over each character, evenly spaced (owner, 2026-11-09:
            // pinyin over whole words left uneven gaps and read badly).
            // Each character carries the word it's part of, so a click opens the word.
            for ch in piece {
                let c = String(ch)
                out.append([c, han(c) ? (c.applyingTransform(.mandarinToLatin, reverse: false) ?? "") : "", piece])
            }
        } else {
            var reading = ""
            if han(piece), let latin = CFStringTokenizerCopyCurrentTokenAttribute(tok, kCFStringTokenizerAttributeLatinTranscription) as? String {
                reading = latin.applyingTransform(.latinToHiragana, reverse: false) ?? latin
            }
            // Furigana over the kanji only: 間に合わせ → 間(ま) に 合(あ) わせ.
            if let parts = okurigana(piece, reading, han) { out += parts } else { out.append([piece, reading]) }
        }
        at = r.location + r.length
    }
    if at < ns.length { out.append([ns.substring(from: at), ""]) }
    return out
}

/// What a word means in its sentence, in a few English words, from the
/// on-device model (free, offline). A fresh session each time: no history.
func meaning(_ id: Int, _ m: Meaning) async -> Answer {
    let model = SystemLanguageModel.default
    guard case .available = model.availability else {
        return Answer(id: id, text: nil, error: "unavailable: \(model.availability)")
    }
    let session = LanguageModelSession(instructions: "You help someone learning a language. Given a sentence and one word or phrase from it, say what that word means in that sentence, in English. Answer with only the meaning: at most six words, no quotes, no explanation.")
    do {
        let r = try await session.respond(to: "Language: \(m.lang)\nSentence: \(m.sentence)\nWord: \(m.word)")
        let text = r.content.trimmingCharacters(in: .whitespacesAndNewlines).trimmingCharacters(in: CharacterSet(charactersIn: "\"“”«»."))
        return Answer(id: id, text: text, error: nil)
    } catch {
        return Answer(id: id, text: nil, error: "\(error)")
    }
}

actor Sessions {
    var open: [String: TranslationSession] = [:]
    func session(_ from: String, _ to: String) -> TranslationSession {
        let key = "\(from)>\(to)"
        if let have = open[key] { return have }
        let s = TranslationSession(installedSource: Locale.Language(identifier: from), target: Locale.Language(identifier: to))
        open[key] = s
        return s
    }
    func drop(_ from: String, _ to: String) { open["\(from)>\(to)"] = nil }
    // Each language's session translates on its own (an actor method would line them up).
    nonisolated func translate(_ a: Ask) async -> Answer {
        if let m = a.meaning { return await meaning(a.id, m) }
        guard let from = a.from, let to = a.to, let text = a.text else { return Answer(id: a.id, text: nil, error: "bad request") }
        let s = await session(from, to)
        do {
            let r = try await s.translate(text)
            let wantRuby = (a.readings ?? false) && (to.hasPrefix("ja") || to.hasPrefix("zh"))
            return Answer(id: a.id, text: r.targetText, error: nil, ruby: wantRuby ? ruby(r.targetText, to) : nil)
        } catch {
            await drop(from, to)
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
