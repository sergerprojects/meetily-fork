# Setup guide for an AI agent (this fork of Meetily)

Goal: get this local meeting recorder running on the user's Mac and connected to **their own** Google Calendar. Everything stays on the machine: audio, on-device transcription (Parakeet), calendar link. No cloud project, no OAuth app, no server.

What this fork adds over upstream Meetily: "You / Others" speaker labels, Google Calendar titles and attendees, a menu-bar list of today's meetings, a prompt (or auto-record) when the user joins a Zoom / Teams / Meet / Slack call, launch at login, and the upstream updater turned off.

Requirements: Apple Silicon Mac, macOS 14+.

## 1. Install

```bash
bash scripts/install-mac.sh
```

The script checks prerequisites and tells you the exact install command for anything missing (Xcode command line tools, Rust via rustup, Node, cmake). Install those with the user's approval, then re-run. First build takes 10-20 minutes. It quits any running Meetily, backs it up to `~/meetily-backup/`, installs to `/Applications/meetily.app`, clears quarantine (the build is unsigned) and launches it.

If the build fails, read the error rather than retrying. Known traps are already handled in the script (`CARGO_PROFILE_RELEASE_STRIP=none`, `CMAKE_DISABLE_FIND_PACKAGE_OpenSSL=ON`, the `llama-helper` sidecar). The final "A public key has been found, but no private key" message is expected and harmless.

## 2. Permissions (the user must click these)

On first run macOS asks for **Microphone** and **Screen & System Audio Recording** access, and for **Notifications**. Tell the user to allow all three. System audio is how the other side of a call is captured. If a prompt was dismissed: System Settings → Privacy & Security.

First recording also downloads the speech model (a few hundred MB). Let it finish.

## 3. Connect the user's Google Calendar

Ask the user to do this themselves (it is their credential; do not read it from their browser or Google account):

1. Google Calendar (web) → Settings (gear) → click their calendar in the left list → **Integrate calendar**.
2. Copy **Secret address in iCal format**.
3. In Meetily: Settings → Recording → **Google Calendar** → paste into the field → **Connect**.

Meetily downloads the feed, and only saves the link if it parses. Success shows "Connected via private iCal link" and the next three meetings.

Rules for you, the agent:
- Treat the link as a password. Never write it to a file in a repo, a log, a chat summary or a commit. The app stores it locally on the user's Mac only.
- If Connect fails with HTTP 403/404 the link was reset or the company admin disabled secret iCal addresses. Tell the user; do not work around it.
- Google updates this feed slowly. A meeting added or moved in the last few hours may be missing. Meetings booked a day or more ahead are fine. The mic-based prompt still catches calls the feed missed.

## 4. Verify

- Menu-bar icon shows today's and tomorrow's meetings.
- Settings → Recording shows "Connected via private iCal link".
- Launch at login is on (Settings → Recording), file `~/Library/LaunchAgents/meetily.plist`.
- "When I join a meeting" defaults to **Ask me**. Switch to **Record automatically** only if the user wants it and has the right to record their calls (company policy, other participants' consent).

## Notes

- Recordings: `~/Movies/meetily-recordings/`. Database: `~/Library/Application Support/com.meetily.ai/meeting_minutes.sqlite` (SQLite WAL: copy with `sqlite3 ... ".backup"`, never `cp`).
- Summaries: leave Meetily's built-in summary and its API-key providers alone unless the user's company has approved sending transcripts to that provider.
- The app and the original Meetily share a bundle id, so only one can be installed.
- Code map for the fork's additions: `frontend/src-tauri/src/calendar/` (`ical.rs`, `google.rs`, `poller.rs`, `autorecord.rs`, `mic_activity.rs`), speaker labels in `frontend/src-tauri/src/audio/pipeline.rs`.
- Optional, advanced: if the user has `gcloud` application-default credentials with calendar scope and no iCal link is set, the app falls back to those.
