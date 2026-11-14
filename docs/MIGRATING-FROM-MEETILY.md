# Moving an existing install to Graphite's names

Graphite was first built under Meetily's names. Builds from 0.9.0 use new ones:

| Was | Now |
| --- | --- |
| `~/Movies/meetily-recordings` | `~/Movies/graphite-recordings` |
| `~/.meetily` (voice models, calendar cache, `people.json`) | `~/.graphite` |
| bundle id `com.meetily.ai` and its data folder | `com.graphite.ai` |
| process / binary `meetily` | `graphite` |
| env vars `MEETILY_*` | `GRAPHITE_*` |

A fresh install needs nothing. If you already have meetings:

1. Quit Graphite.
2. Run `bash scripts/migrate-from-meetily.sh`. It renames the folders (nothing is deleted), rewrites the recording paths inside the database and keeps a database copy in `~/graphite-backup/`.
3. Build and install this version (`bash scripts/install-mac.sh`).
4. macOS treats the new bundle id as a new app: allow Microphone, Audio capture, Accessibility and (for snapshots) Screen Recording again.

Browser-stored settings (the chat quick-questions you edited, the confidence switch) move with the WebKit folder. The script also repoints the login item at the new binary name.
