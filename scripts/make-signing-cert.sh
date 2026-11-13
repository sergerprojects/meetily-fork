#!/usr/bin/env bash
# One-time: create a self-signed "Graphite Local" code-signing identity in your
# login keychain. Signing every build with the same identity lets macOS keep
# the Accessibility / Screen Recording permissions you grant to Graphite.
# Local to this Mac, never leaves the keychain.
set -euo pipefail

NAME="Graphite Local"
if security find-identity -p codesigning 2>/dev/null | grep -q "$NAME"; then
  echo "'$NAME' already exists."
  exit 0
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
cat > "$TMP/o.cnf" <<CNF
[req]
distinguished_name=dn
x509_extensions=ext
prompt=no
[dn]
CN=$NAME
[ext]
keyUsage=critical,digitalSignature
extendedKeyUsage=critical,codeSigning
basicConstraints=critical,CA:false
CNF
PASS="$(openssl rand -hex 8)"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$TMP/k.pem" -out "$TMP/c.pem" -days 3650 -config "$TMP/o.cnf" 2>/dev/null
openssl pkcs12 -export -inkey "$TMP/k.pem" -in "$TMP/c.pem" -out "$TMP/g.p12" -passout "pass:$PASS" -name "$NAME"
security import "$TMP/g.p12" -k "$HOME/Library/Keychains/login.keychain-db" -P "$PASS" -T /usr/bin/codesign
echo "Created '$NAME'. Run scripts/install-mac.sh to build and sign with it."
