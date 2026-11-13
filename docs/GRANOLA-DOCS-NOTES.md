# What Granola's public docs say, and what Graphite should take

Source: docs.granola.ai help center and Granola's Snapshots blog post, read through search summaries (the docs host did not resolve from this machine's fetch tool, so pages were not read word for word). Read on 2026-11-13.

## Findings that change the plan

1. **Speaker names come from the meeting app, not from voices.** Granola reads the participant names and the "who is speaking" highlight from Zoom, Teams and Meet through macOS Accessibility (Meet can also use a Chrome extension). It does not use Accessibility to record the screen or audio. Where unavailable the transcript shows "Me" / "Them". Limits: shared room devices, people talking over each other, and people with odd display names. This is more reliable than voice clustering for real names, and it needs no audio saved.
2. **Snapshots** need Screen Recording plus Accessibility, macOS 14+. Only someone else's share is captured, and only the shared window or content. Capture pauses while the meeting window is not visible. Images collect in an "Images" stack at the bottom of the note, above chat. Click to expand, click an image for full size, hover and press x to remove. A per-meeting "Turn off for this meeting" switch sits on the live indicator. The blog says the first version only worked through a Chrome extension for Meet, and moving to Accessibility is what made Zoom and Teams work.
3. **Audio is never stored.** Only the transcript is kept (this matches Graphite's default).

## Other features worth knowing

| Granola feature | Detail | Graphite today |
| --- | --- | --- |
| Call detection | Mic in use triggers a prompt: "Huddle detected" (Slack), "Call detected" (FaceTime/WhatsApp), "Meeting detected" (others). A call within 15 min of a calendar event is linked to it. Calendar prompts only for events with 2+ attendees. | Mic detector and calendar prompt exist |
| Auto-stop | Call ended, 15 min with no audio, computer sleep, or scheduled end for calendar meetings | Silence auto-stop (10 min). Add sleep and scheduled-end triggers |
| Transcript tools | Delete a chunk, copy a chunk, Stop/Resume without ending the meeting. Editing the transcript is not supported (their most requested gap) | Not built |
| Retention | Auto-delete transcripts after 1 day to 1 year; notes kept | Audio retention only |
| Templates | `/` in notes picks a template; the Enhanced tab can switch template and regenerate | Chrisforce does summaries; no in-app templates |
| Recipes | Saved chat prompts; `/` in chat; single-meeting and multi-meeting; shareable; no variables | Chat exists, no saved prompts |
| People and Companies | Searchable list built from calendar attendees; chat on one person | Chrisforce people files cover this |
| Pre-meeting briefs | A docs page exists (not read) | Not built |
| Live indicator | Floating chip on the right while recording, draggable, can be hidden | Tray only |
| Transparency | Auto chat message when transcription starts (pinned on Meet, only once another person has joined) and a watermark badge on your video | Not built. Relevant to recording consent |
| Voice typing | Separate product (dictation with screen context) | Out of scope |
| Folders, sharing, workspaces, API | Team features | Out of scope (private app) |

## Suggested build order

1. Speaker names from Accessibility (names plus active speaker) matched to the system-audio timeline. Replaces most of the voice-clustering guesswork on group calls.
2. Snapshots (Accessibility to detect the share, ScreenCaptureKit to capture that one window, Images stack and viewer, per-meeting off switch, OCR text for chat). Shares the same permission as step 1.
3. Small ones: delete transcript chunk, Stop/Resume, sleep and scheduled-end auto-stop, floating live indicator, saved chat prompts.
