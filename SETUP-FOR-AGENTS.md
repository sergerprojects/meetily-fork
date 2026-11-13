# Setup guide for an AI agent (this fork of Meetily)

You are an AI agent helping a person set up a local meeting recorder on their own computer. This is a shared fork, not a managed product: nobody maintains it for you. Adapt freely, and tell the person what you changed.

Everything stays on the machine: audio, on-device transcription (Parakeet), calendar access. No server.

## What this fork adds over upstream Meetily

- "You / Others" speaker labels (mic vs system audio energy)
- Calendar awareness: meeting titles and attendees on recordings, a menu-bar list of today's/tomorrow's meetings, a prompt (or auto-record) when you join a Zoom / Teams / Meet / Slack call
- Launch at login (starts hidden in the menu bar)
- Upstream updater turned off (it would overwrite this fork)

Recording and transcription work with no calendar at all. The calendar is a convenience; if you cannot connect it, the app still records and the person can name meetings by hand.

## 1. Install

**macOS (Apple Silicon, tested):**

```bash
bash scripts/install-mac.sh
```

It checks prerequisites (Xcode command line tools, Rust, Node, cmake), builds, backs up any existing install to `~/meetily-backup/`, installs to `/Applications/meetily.app`, clears quarantine (unsigned build) and launches. First build: 10-20 minutes. The final "A public key has been found, but no private key" message is expected and harmless.

**Windows / Intel Mac / Linux (untested):** upstream Meetily supports Windows, and most of this fork's code is not platform-specific, but nobody has built or run it there. Read `docs/BUILDING.md` and `frontend/build-gpu.*`, build with the Tauri CLI, and fix what breaks. Known Mac-only pieces in this fork:
- `frontend/src-tauri/src/calendar/mic_activity.rs`: "you joined a call" detection reads CoreAudio. On other OSes `sample()` returns `None`, so join prompts are off (calendar-time prompts still work). Implementing it needs per-process audio-session enumeration (WASAPI on Windows).
- `scripts/install-mac.sh`, the `llama-helper` sidecar naming (`llama-helper-<target-triple>`), and Mac login-item handling.

**Permissions (macOS):** allow Microphone, Screen & System Audio Recording, and Notifications when asked. System audio is how the other side of a call is captured. The first recording downloads the speech model (a few hundred MB).

## 2. Connect the person's calendar (optional, per person)

The app picks a calendar source in this order: a pasted private iCal link, otherwise Google credentials from gcloud. Both feed the same code (`frontend/src-tauri/src/calendar/`). Settings → Recording shows which one is active and the next meetings.

### How it was bound for the original author (gcloud, worked)

The author's setup used gcloud's own sign-in client with a personal Google Cloud project as the quota project. Some organizations provision such a project per employee, so check `gcloud projects list`. If the person has one, no shared project is needed; otherwise they need a project with the Calendar API enabled. These are the standard commands for that setup (the exact original commands were not recorded):

```bash
gcloud auth application-default login \
  --scopes=https://www.googleapis.com/auth/calendar.readonly,openid,https://www.googleapis.com/auth/userinfo.email
gcloud auth application-default set-quota-project <their-project-id>
gcloud services enable calendar-json.googleapis.com --project <their-project-id>   # if not already on
```

That writes `application_default_credentials.json` (macOS/Linux `~/.config/gcloud/`, Windows `%APPDATA%\gcloud\`, or `$CLOUDSDK_CONFIG`). Restart Meetily; Settings → Recording should say "Connected via Google sign-in (gcloud)". Read-only calendar scope only.

Things that can go wrong: no personal project or no permission to enable the Calendar API; the company blocks the calendar scope for gcloud's client; `gcloud` not installed. If so, try a different source below.

### Other sources you can try or add

- **Private iCal link**: Google Calendar → Settings → your calendar → Integrate calendar → "Secret address in iCal format", pasted into Settings → Recording. It is built and tested against fixtures. It did **not** work for the author: their organization's Google admin hides that address, as many do. Never use the "Public address" instead: it requires making the whole calendar public.
- **macOS Calendar app (EventKit)**: if the person's Google account is already in Calendar.app this could read it with no Google sign-in at all. Not built; untested whether Meet links come through. A command-line probe cannot trigger the permission prompt, so it must run inside the app bundle with `NSCalendarsFullAccessUsageDescription` in its Info.plist.
- **Anything else** (Outlook/Graph, CalDAV, a file your own scheduled job writes): add a variant to `Source` in `calendar/mod.rs`. A source only has to return events as Google-Calendar-API-shaped JSON from `list_events(min, max)`; `parse_event` does the rest. `calendar/ical.rs` is a worked example.

### Rules

- Treat any calendar link or credential as a password. Never put it in a repo, log, chat summary or commit.
- Ask the person before changing their account, enabling APIs or running `gcloud auth`. Their browser sign-in is theirs to do.
- Organization admin settings may block a path. Report the block; do not work around it.
- **Recording consent:** recording a call can require consent from everyone on it, depending on where they are and on your employer's policy. Make sure the person knows this before enabling automatic recording.

## 3. Verify

- Menu-bar icon lists today's and tomorrow's meetings (if a calendar is connected).
- Settings → Recording shows the connected source, and "Launch at login" is on.
- "When I join a meeting" defaults to **Ask me**. Switch to **Record automatically** only if the person wants it and has the right to record their calls (company policy, other participants' consent).

## 4. Speaker names (optional)

The app labels lines "You" (microphone) and "Others" (everyone else). `tools/diarize_meeting.py` goes further on-device: it separates the "Others" audio into Speaker 1, Speaker 2, ... by voice. It needs the meeting's `audio.mp4`, so Settings -> Recording -> save audio must be on.

```bash
python3 -m venv ~/.meetily/venv && ~/.meetily/venv/bin/pip install sherpa-onnx numpy
~/.meetily/venv/bin/python tools/diarize_meeting.py --download-models   # ~32 MB from the k2-fsa/sherpa-onnx releases, once
~/.meetily/venv/bin/python tools/diarize_meeting.py ~/Movies/meetily-recordings/<meeting folder>   # writes speakers.json
```

Turning "Speaker 2" into a person's name is left to you: match against the calendar attendee list (`calendar_event` in the meeting's `metadata.json`) using what is said in the meeting ("thanks, Sam", self-introductions). Only apply a name when the dialogue says so; otherwise keep "Speaker N". Measured so far: separates you from one other voice well; with several similar voices it can merge or split people. Treat labels as best guesses.

## Notes

- Recordings: `~/Movies/meetily-recordings/`. Database: `~/Library/Application Support/com.meetily.ai/meeting_minutes.sqlite` (SQLite WAL: copy with `sqlite3 ... ".backup"`, never `cp`).
- Leave Meetily's built-in summary and its API-key providers alone unless the person's organization has approved sending transcripts to that provider.
- This app shares a bundle id with upstream Meetily, so only one can be installed.
- Code map: calendar in `frontend/src-tauri/src/calendar/` (`google.rs`, `ical.rs`, `poller.rs`, `autorecord.rs`, `mic_activity.rs`); speaker labels in `frontend/src-tauri/src/audio/pipeline.rs`; launch at login in `lib.rs` (`tauri-plugin-autostart`).
