// meetily-eventkit: read calendar events from the Mac's Calendar database (EventKit).
// Works for any account added to the Calendar app (Google, Exchange, iCloud): no
// Google Cloud project, no OAuth client. Prints events as Google-Calendar-API-shaped
// JSON so the rest of Meetily treats them like any other source.
//
//   meetily-eventkit --status                        -> granted | undetermined | denied
//   meetily-eventkit --request                       -> granted | denied (shows the macOS prompt once)
//   meetily-eventkit --events <startISO> <endISO>    -> JSON array (exit 3 if access is not granted)
import EventKit
import Foundation

let store = EKEventStore()

func authState() -> String {
    let s = EKEventStore.authorizationStatus(for: .event)
    if #available(macOS 14.0, *) {
        if s == .fullAccess { return "granted" }
    } else if s == .authorized { return "granted" }
    return s == .notDetermined ? "undetermined" : "denied"
}

func requestAccess() -> Bool {
    let sem = DispatchSemaphore(value: 0)
    var ok = false
    if #available(macOS 14.0, *) {
        store.requestFullAccessToEvents { granted, _ in ok = granted; sem.signal() }
    } else {
        store.requestAccess(to: .event) { granted, _ in ok = granted; sem.signal() }
    }
    _ = sem.wait(timeout: .now() + 120)
    return ok
}

func fail(_ msg: String, _ code: Int32) -> Never {
    FileHandle.standardError.write((msg + "\n").data(using: .utf8)!)
    exit(code)
}

let args = CommandLine.arguments
guard args.count >= 2 else { fail("usage: meetily-eventkit --status | --request | --events START END", 2) }

switch args[1] {
case "--status":
    print(authState())
case "--request":
    print(authState() == "granted" || requestAccess() ? "granted" : "denied")
case "--events":
    guard args.count >= 4 else { fail("usage: --events START END", 2) }
    let iso = ISO8601DateFormatter()
    iso.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    let plain = ISO8601DateFormatter()
    func parse(_ s: String) -> Date? { iso.date(from: s) ?? plain.date(from: s) }
    guard let start = parse(args[2]), let end = parse(args[3]) else { fail("bad date", 2) }
    if authState() != "granted" && !requestAccess() { fail("Calendar access not granted", 3) }

    let out = ISO8601DateFormatter()
    let predicate = store.predicateForEvents(withStart: start, end: end, calendars: nil)
    let videoRe = try! NSRegularExpression(
        pattern: "https?://(?:[a-z0-9-]+\\.)*(?:zoom\\.us|zoomgov\\.com|teams\\.microsoft\\.com|teams\\.live\\.com|meet\\.google\\.com)[^\\s\"'<>]*",
        options: [.caseInsensitive])
    func firstVideoURL(_ texts: [String?]) -> String? {
        for t in texts {
            guard let t = t else { continue }
            let range = NSRange(t.startIndex..., in: t)
            if let m = videoRe.firstMatch(in: t, range: range), let r = Range(m.range, in: t) { return String(t[r]) }
        }
        return nil
    }
    func email(_ p: EKParticipant) -> String? {
        let u = p.url.absoluteString
        return u.lowercased().hasPrefix("mailto:") ? String(u.dropFirst(7)) : nil
    }

    var result: [[String: Any]] = []
    for e in store.events(matching: predicate) where !e.isAllDay && e.status != .canceled {
        var obj: [String: Any] = [
            "id": e.eventIdentifier + (e.hasRecurrenceRules ? "_" + String(Int(e.startDate.timeIntervalSince1970)) : ""),
            "status": "confirmed",
            "summary": e.title ?? "",
            "start": ["dateTime": out.string(from: e.startDate)],
            "end": ["dateTime": out.string(from: e.endDate)],
        ]
        if let l = e.location, !l.isEmpty { obj["location"] = l }
        if let n = e.notes, !n.isEmpty { obj["description"] = n }
        if let v = firstVideoURL([e.url?.absoluteString, e.location, e.notes]) { obj["hangoutLink"] = v }
        if let o = e.organizer, let mail = email(o) { obj["organizer"] = ["email": mail] }
        if let people = e.attendees {
            var list: [[String: Any]] = []
            for p in people {
                guard let mail = email(p) else { continue }
                var a: [String: Any] = ["email": mail, "self": p.isCurrentUser]
                if let n = p.name, !n.isEmpty, !n.contains("@") { a["displayName"] = n }
                switch p.participantStatus {
                case .accepted: a["responseStatus"] = "accepted"
                case .declined: a["responseStatus"] = "declined"
                case .tentative: a["responseStatus"] = "tentative"
                default: a["responseStatus"] = "needsAction"
                }
                if p.participantType == .resource { a["resource"] = true }
                list.append(a)
            }
            if !list.isEmpty { obj["attendees"] = list }
        }
        result.append(obj)
    }
    let data = try JSONSerialization.data(withJSONObject: result)
    FileHandle.standardOutput.write(data)
default:
    fail("unknown option \(args[1])", 2)
}
