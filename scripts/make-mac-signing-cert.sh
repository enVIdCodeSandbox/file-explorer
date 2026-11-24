#!/usr/bin/env bash
# Makes a free, self-signed code-signing certificate for the macOS build, ONCE.
#
# Why: a Mac app built without a developer signature gets a different identity on every build, so macOS forgets
# Full Disk Access (and other permissions) at every update, and an entry that looks switched on in System Settings
# belongs to the earlier build and does nothing. Signing every build with the same certificate keeps one identity,
# so permissions given once stay given. (A paid Apple Developer certificate does the same and also removes the
# "unidentified developer" warning; this one only gives the app a stable identity.)
#
# Run it on any Mac or Linux with openssl, then add the three values it prints as GitHub secrets
# (Settings > Secrets and variables > Actions): APPLE_CERTIFICATE, APPLE_CERTIFICATE_PASSWORD, APPLE_SIGNING_IDENTITY.
# Keep the .p12 file: a new certificate is a new identity, and permissions would be forgotten once more.
set -euo pipefail

NAME="${1:-FAST File Explorer Signing}"
DIR="$(mktemp -d)"
PASS="$(openssl rand -hex 16)"
OUT="${2:-fast-explorer-signing.p12}"

cat > "$DIR/openssl.cnf" <<CNF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $NAME
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
CNF

openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -keyout "$DIR/key.pem" -out "$DIR/cert.pem" -config "$DIR/openssl.cnf" 2>/dev/null

# The old encryption names are what macOS's `security import` reads on every version.
if openssl pkcs12 -export -legacy -inkey "$DIR/key.pem" -in "$DIR/cert.pem" -out "$OUT" -passout "pass:$PASS" 2>/dev/null; then :; else
  openssl pkcs12 -export -inkey "$DIR/key.pem" -in "$DIR/cert.pem" -out "$OUT" -passout "pass:$PASS" -keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES -macalg sha1
fi
rm -rf "$DIR"

echo
echo "Saved $OUT (keep it somewhere safe)."
echo
echo "Add these three GitHub secrets:"
echo "  APPLE_SIGNING_IDENTITY      = $NAME"
echo "  APPLE_CERTIFICATE_PASSWORD  = $PASS"
echo "  APPLE_CERTIFICATE           = (the one line below)"
echo
base64 < "$OUT" | tr -d '\n'
echo
