#!/usr/bin/env bash

set -euo pipefail

CERT_NAME="${CERT_NAME:-Flick Self-Signed Code Signing}"
CERT_DIR="${CERT_DIR:-$HOME/.config/flick-signing}"
KEYCHAIN_PATH="${KEYCHAIN_PATH:-$HOME/Library/Keychains/login.keychain-db}"

mkdir -p "$CERT_DIR"
chmod 700 "$CERT_DIR"

KEY_PATH="$CERT_DIR/${CERT_NAME}.key.pem"
CERT_PATH="$CERT_DIR/${CERT_NAME}.cert.pem"
P12_PATH="$CERT_DIR/${CERT_NAME}.p12"
OPENSSL_CONFIG_PATH="$CERT_DIR/${CERT_NAME}.openssl.cnf"
PASSWORD_PATH="$CERT_DIR/${CERT_NAME}.password"

identity_exists() {
  security find-identity -v -p codesigning | grep -F "\"$CERT_NAME\"" >/dev/null 2>&1
}

if identity_exists; then
  echo "Codesigning identity already available: $CERT_NAME"
  exit 0
fi

if security find-certificate -c "$CERT_NAME" "$KEYCHAIN_PATH" >/dev/null 2>&1 && [[ -f "$CERT_PATH" ]]; then
  security add-trusted-cert -d -r trustRoot -k "$KEYCHAIN_PATH" "$CERT_PATH" >/dev/null 2>&1 || true
  if identity_exists; then
    echo "Trusted existing certificate: $CERT_NAME"
    exit 0
  fi
fi

if [[ ! -f "$PASSWORD_PATH" ]]; then
  openssl rand -hex 16 >"$PASSWORD_PATH"
  chmod 600 "$PASSWORD_PATH"
fi

cat >"$OPENSSL_CONFIG_PATH" <<EOF
[ req ]
distinguished_name = req_distinguished_name
x509_extensions = v3_codesign
prompt = no

[ req_distinguished_name ]
CN = ${CERT_NAME}
O = Flick Development
OU = Local Build Signing

[ v3_codesign ]
basicConstraints = critical,CA:FALSE
keyUsage = critical,digitalSignature
extendedKeyUsage = codeSigning
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid,issuer
EOF

openssl req \
  -newkey rsa:2048 \
  -nodes \
  -keyout "$KEY_PATH" \
  -x509 \
  -days 3650 \
  -out "$CERT_PATH" \
  -config "$OPENSSL_CONFIG_PATH" >/dev/null 2>&1

openssl pkcs12 -export \
  -inkey "$KEY_PATH" \
  -in "$CERT_PATH" \
  -out "$P12_PATH" \
  -name "$CERT_NAME" \
  -passout "pass:$(cat "$PASSWORD_PATH")" >/dev/null 2>&1

security import "$P12_PATH" \
  -k "$KEYCHAIN_PATH" \
  -f pkcs12 \
  -P "$(cat "$PASSWORD_PATH")" \
  -T /usr/bin/codesign \
  -T /usr/bin/security >/dev/null

security add-trusted-cert -d -r trustRoot -k "$KEYCHAIN_PATH" "$CERT_PATH" >/dev/null 2>&1 || true

echo "Created and imported certificate: $CERT_NAME"
echo "Files stored under: $CERT_DIR"
