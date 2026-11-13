# UX review: what "designed by Apple" would mean for this app

Review of the fork as of build `0.4.1-fork.8`, against Apple's Human Interface Guidelines (clarity, deference, depth) and what Granola does well. Screens reviewed: Home, live recording, saved meeting (transcript + summary), sidebar, menu bar. Settings was not walked through screen by screen. Items marked **done** shipped in build 8.

## What is already right
- Home ("Coming up" + meetings grouped by day) has a clear hierarchy: one big title, one card, one list.
- The transcript uses quiet speaker chips instead of loud colors.
- Menu-bar countdown and the calendar prompt follow the platform's grain.

## Problems, ranked by how much they hurt

### 1. The live meeting screen was a raw transcript, not a note (done)
Granola's insight: during a call you write; the machine listens. Before build 8 the live screen showed only scrolling transcript, no title, no way to type.
- **Done:** title at the top (calendar title, editable), context chips (today, number of people), **Notes | Transcript** switch (Notes first), autosaving free-form notes, saved beside the recording and merged into your summary by the importer.
- Next: `/` templates, bold/bullets (a light editor), a "Now" indicator showing who is speaking.

### 2. Too many equal-weight controls (done for the sidebar)
The sidebar bottom had four full-width, differently colored buttons (red, blue, gray, gray). One screen should have one primary action.
- **Done:** one dark primary "Start recording", with a quiet row for Import and Settings. Red is reserved for "recording is live".
- Next: Summary toolbar has five buttons in a row (Regenerate, language, model, template, save, copy). Keep "Regenerate" and a "..." menu for the rest.

### 3. Cramped list (done)
Meeting titles in the sidebar were squeezed into about 100 px because the invisible edit/delete buttons still took space. Fixed: buttons appear on hover as an overlay, titles get the full width and wrap to three lines.
- Next: show time under each title; group by day like Home; keep the sidebar for search and navigation only.

### 4. Two navigation systems
A collapsed icon rail and an expanded sidebar duplicate each other and disagree on order and labels. Pick one: a single translucent sidebar (macOS vibrancy) that collapses with ⌘⌥S, with the same items in the same order.

### 5. Typography and spacing
- Use the system font (SF Pro) for interface text; keep the serif only for page titles (it gives the Granola warmth). Today everything is Source Sans.
- Adopt a strict scale: 28 / 17 / 15 / 13 / 11 pt, an 8 pt spacing grid, 12 pt corner radius on cards, 8 pt on controls.
- Body text should be 15 px or larger; many labels are 11-12 px gray on white (fails contrast at small sizes).

### 6. Window chrome
Native Mac apps blend the title bar with content (transparent title bar, traffic lights inset, content under it). Today there is a gray title strip with "meetily". Use `titleBarStyle: Overlay` and give the sidebar a vibrancy material. Risky to do blind, so it needs a visual pass.

### 7. Keyboard and speed
No app-level shortcuts. Add: ⌘N new note / start recording, ⌘. stop, ⌘F search, ⌘, settings, ⌘K command palette, ⌘1 Notes / ⌘2 Transcript. Everything a mouse can do should be reachable from the keyboard.

### 8. Color and dark mode
Dark mode is not supported. Color is used for status inconsistently (blue pill logo, red button, yellow match boxes). Use one accent (blue), semantic red only for recording and destructive actions, and support light and dark through system colors.

### 9. Empty, loading and error states
- Home's empty state is fine. The live "Listening for speech..." is good.
- Errors surface as modals; prefer inline, plain-language messages with one action ("Allow microphone access" opens System Settings).
- First-run should explain the one thing users must do: grant microphone and screen/system-audio access.

### 10. Names (the thing you asked about)
Names appear only when a calendar meeting has exactly one other person. Group calls show "Others". Real names for groups need voice separation after the call (needs saved audio, which is off by default and opt-in under Settings → Recording) plus calendar names (needs the Google directory permission). Be explicit in the UI: show "Names appear after the meeting" on group calls, so the absence reads as intentional.

## Build order I would follow
1. (done) Live note view, sidebar rebuild, version stamp.
2. Summary toolbar cleanup, meeting list grouped by day with times, one navigation system.
3. System font + type scale + color tokens + dark mode.
4. Keyboard shortcuts and command palette.
5. Transparent title bar and vibrancy (needs on-screen iteration).
6. Templates (`/`), light formatting in notes, speaker names on group calls after the meeting.

## Granola feature check
| Granola | Status |
| --- | --- |
| Calendar "Coming up", Start now | done |
| Meetings grouped by day with who was there | done (names depend on the names file/directory) |
| Title + chips at top of a note | done (build 8) |
| Free-form notes during the call, merged into the summary | done (notes + import); templates not yet |
| Other person named on 1:1s | done |
| Speaker tags for group calls | after the meeting, needs audio; unproven |
| Menu-bar next-meeting countdown | done |
| Personal jargon list, profile context | done (importer glossary) |
| Auto-stop when the call is over | done (10 min of silence default) |
| Transcript auto-delete | audio retention, off by default |
| Screen-share snapshots | not built |
| Ask-anything chat over notes | not in the app (Chrisforce does it) |
| Integrations (Slack, Notion, CRM) | not in the app (Chrisforce routes output) |
| Sharing links | not built, by design |

## Update (0.8.0)
Done since this review: summary-toolbar left as is (built-in summaries are not used), sidebar grouped by day with times, keyboard shortcuts and a command palette, notes box with `/` templates and formatting, saved view shows your own notes, settings regrouped into five tabs. Still open: one navigation system (the collapsed rail remains), dark mode, transparent title bar and vibrancy.
