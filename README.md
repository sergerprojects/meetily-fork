# Graphite

Private-first meeting notes for macOS. Graphite records your web meetings (Zoom, Teams, Google Meet, Slack huddles) from system audio, transcribes them on this Mac, puts real names on the transcript, keeps snapshots of what other people share on screen, and lets you ask Claude about the meeting while it happens. No bot joins your calls, and no audio leaves your computer.

Graphite is a personal, internal-use fork of [Meetily](https://github.com/Zackriya-Solutions/meetily) by [Zackriya Solutions](https://www.zackriya.com), used under the MIT License. All credit for the original app, the on-device transcription pipeline and the Tauri foundation goes to them. The original copyright notice is kept in [LICENSE.md](LICENSE.md); changes here are released under the same MIT License, with no warranty. Based on upstream commit `07c4dfd` (v0.4.1).

**What Graphite adds**
- "You / Others" speaker labels on transcripts (microphone vs. system audio)
- Calendar awareness: meeting titles and attendees on recordings, a menu-bar list of upcoming meetings, a prompt (or optional auto-record) when you join a call. Sources: a private iCal link or Google credentials from `gcloud`
- A Home screen: today's and tomorrow's calendar ("Coming up", with Start now) and your meetings grouped by day with who was in them
- The other person's name instead of "Others" on calendar 1:1s, live and when saved
- Menu-bar countdown to your next meeting, and a list of apps whose microphone use never counts as a meeting
- Optional live voice separation for group calls (Speaker 1, 2, ... as people talk, audio analysed in memory and never saved; name them from the invite in the live view): `scripts/setup-speakers.sh`
- Optional after-the-fact voice separation of saved audio: `tools/diarize_meeting.py`
- Speaker names from the call app (Google Meet in Chrome): who was speaking is read from the page through macOS Accessibility and put on the transcript (needs the Accessibility permission; nothing is recorded or sent)
- Snapshots of shared screens (Google Meet in Chrome, opt-in): pictures of what other people share, saved with the meeting on this Mac, with the text read off them on-device; an "Images" strip under the notes with a full-size viewer
- Notes box with `/` templates, list continuation and bold/italics; chat quick-questions you can edit; a command palette (⌘K) and shortcuts
- A small floating recording chip (time, Open, Stop) while Graphite is in the background
- Auto-stop when the Mac sleeps, when a calendar slot is over and the call is quiet, or after silence
- Edit saved transcripts: delete a line, rename a speaker
- Settings reorganised into Recording, Calendar, Capture, Transcription and General; built-in summaries, analytics and the update check are no longer in the interface
- Launch at login (macOS)
- Audio is not kept by default: only transcripts are saved unless you turn on "Keep audio files"
- Upstream auto-update disabled (it would overwrite this fork)
- Upstream usage analytics permanently disabled: this fork contains no telemetry key and sends nothing


## Install

macOS on Apple Silicon is the only tested platform. Point your AI agent at [SETUP-FOR-AGENTS.md](SETUP-FOR-AGENTS.md), or run:

```bash
bash scripts/make-signing-cert.sh   # once: a local signing identity so macOS keeps your permissions
bash scripts/install-mac.sh
```

## Privacy

- Audio is not kept unless you switch on "Keep audio files".
- Transcription runs on this Mac (Parakeet).
- Snapshots are opt-in and stay on this Mac. Recording other people can need their consent: tell them.
- Upstream analytics and the auto-updater are removed. This fork sends nothing anywhere on its own. Chat uses your own Claude Code login, only when you ask a question.

See [docs/GRANOLA-DOCS-NOTES.md](docs/GRANOLA-DOCS-NOTES.md) for the features this was modelled on and [docs/UX-REVIEW.md](docs/UX-REVIEW.md) for the design notes.
