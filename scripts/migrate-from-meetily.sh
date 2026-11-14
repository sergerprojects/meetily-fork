#!/usr/bin/env bash
# One-time move of an existing Meetily-era install to Graphite's names.
#
#   ~/Movies/meetily-recordings          -> ~/Movies/graphite-recordings
#   ~/.meetily                           -> ~/.graphite
#   ~/Library/Application Support/com.meetily.ai  -> .../com.graphite.app
#   ~/Library/WebKit, ~/Library/Caches (com.meetily.ai) -> com.graphite.app
#   ~/Library/Application Support/meetily (notification prefs) -> .../graphite
#   meetings.folder_path inside the database is rewritten to the new recordings path
#
# Quit Graphite first. A copy of the database is saved to ~/graphite-backup/.
# Nothing is deleted: folders are renamed, so undoing = renaming them back.
set -euo pipefail

H="$HOME"
APPSUP="$H/Library/Application Support"
DB_NEW="$APPSUP/com.graphite.app/meeting_minutes.sqlite"

if pgrep -x graphite >/dev/null || pgrep -x meetily >/dev/null; then
  echo "Quit Graphite first." >&2; exit 1
fi

move() { # src dst
  if [ -e "$1" ] && [ ! -e "$2" ]; then mv "$1" "$2" && echo "moved $1 -> $2"
  elif [ -e "$1" ] && [ -e "$2" ]; then echo "SKIP: both exist: $1 and $2" >&2
  fi
}

mkdir -p "$H/graphite-backup"
if [ -f "$APPSUP/com.meetily.ai/meeting_minutes.sqlite" ]; then
  sqlite3 "$APPSUP/com.meetily.ai/meeting_minutes.sqlite" ".backup $H/graphite-backup/db-before-rename-$(date +%Y%m%d-%H%M).sqlite"
fi

move "$H/Movies/meetily-recordings" "$H/Movies/graphite-recordings"
move "$H/.meetily" "$H/.graphite"
move "$APPSUP/com.meetily.ai" "$APPSUP/com.graphite.app"
move "$APPSUP/meetily" "$APPSUP/graphite"
move "$H/Library/WebKit/com.meetily.ai" "$H/Library/WebKit/com.graphite.app"
move "$H/Library/Caches/com.meetily.ai" "$H/Library/Caches/com.graphite.app"

if [ -f "$DB_NEW" ]; then
  sqlite3 "$DB_NEW" "UPDATE meetings SET folder_path = replace(folder_path, '/Movies/meetily-recordings', '/Movies/graphite-recordings') WHERE folder_path LIKE '%/Movies/meetily-recordings%';"
  echo "database paths updated: $(sqlite3 "$DB_NEW" "SELECT count(*) FROM meetings WHERE folder_path LIKE '%graphite-recordings%';") meetings"
fi

# Old login item points at the old binary name; the app rewrites it on first run.
rm -f "$H/Library/LaunchAgents/meetily.plist"
echo "Done."
