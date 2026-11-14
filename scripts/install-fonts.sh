#!/usr/bin/env bash
# Optional: use your own UI/display fonts without committing them.
#   GRAPHITE_FONT_DIR=/path/to/fonts bash scripts/install-fonts.sh
# Looks in the folder for  *Sans*Light* / *Sans*Regular* / *Sans*Bold*  (ttf/otf) as the UI
# font ("Brand Sans") and one *Demi*|*Display* font as the title font ("Brand Display").
# Copies them to frontend/public/fonts/brand/ (git-ignored) and writes local.css there.
# Without GRAPHITE_FONT_DIR nothing happens and the app uses the system font.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$ROOT/frontend/public/fonts/brand"
SRC="${GRAPHITE_FONT_DIR:-}"
[ -n "$SRC" ] && [ -d "$SRC" ] || { echo "GRAPHITE_FONT_DIR not set; using system fonts"; exit 0; }
rm -rf "$DEST"; mkdir -p "$DEST"
css="$DEST/local.css"; : > "$css"
face() { # family file weight
  cp "$2" "$DEST/"; f="$(basename "$2")"
  fmt="truetype"; case "$f" in *.otf) fmt="opentype";; esac
  printf '@font-face { font-family: "%s"; src: url("./%s") format("%s"); font-weight: %s; font-style: normal; font-display: swap; }\n' "$1" "$f" "$fmt" "$3" >> "$css"
}
pick() { find "$SRC" -maxdepth 1 -type f \( -iname "$1" \) | head -1; }
n=0
for spec in "*Sans*Light*.[ot]tf:300" "*Sans*Regular*.[ot]tf:400" "*Sans*Bold*.[ot]tf:600 700"; do
  f="$(pick "${spec%%:*}")"; [ -n "$f" ] && { face "Brand Sans" "$f" "${spec#*:}"; n=$((n+1)); }
done
d="$(pick '*Demi*.[ot]tf')"; [ -n "$d" ] || d="$(pick '*Display*.[ot]tf')"
[ -n "$d" ] && { face "Brand Display" "$d" "600 700"; n=$((n+1)); }
echo "installed $n font file(s) from $SRC into $DEST"
