#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${1:-"$ROOT_DIR/out/macos-release-certificate"}"
IDENTITY_NAME="${SMU_MACOS_RELEASE_SIGN_IDENTITY:-SMU macOS Release}"
DAYS="${SMU_MACOS_RELEASE_CERT_DAYS:-3650}"
P12_PASSWORD="${SMU_MACOS_RELEASE_CERTIFICATE_PASSWORD:-}"
IMPORT_CERT="${SMU_IMPORT_MACOS_RELEASE_CERT:-OFF}"

if [[ -z "$P12_PASSWORD" ]]; then
  echo "Set SMU_MACOS_RELEASE_CERTIFICATE_PASSWORD to protect the exported .p12." >&2
  exit 1
fi

if ! command -v openssl >/dev/null 2>&1; then
  echo "openssl was not found." >&2
  exit 1
fi

mkdir -p "$OUT_DIR"
chmod 700 "$OUT_DIR"

CONFIG_PATH="$OUT_DIR/smu-macos-release-cert.cnf"
KEY_PATH="$OUT_DIR/smu-macos-release.key.pem"
CERT_PATH="$OUT_DIR/smu-macos-release.cert.pem"
P12_PATH="$OUT_DIR/smu-macos-release.p12"

cat > "$CONFIG_PATH" <<EOF_CERT_CONFIG
[req]
default_bits = 3072
distinguished_name = dn
x509_extensions = v3_codesign
prompt = no

[dn]
CN = $IDENTITY_NAME
O = Spencer Macro Utilities

[v3_codesign]
basicConstraints = critical,CA:true,pathlen:0
keyUsage = critical,digitalSignature,keyCertSign,cRLSign
extendedKeyUsage = critical,codeSigning
subjectKeyIdentifier = hash
EOF_CERT_CONFIG

openssl req \
  -new \
  -newkey rsa:3072 \
  -nodes \
  -x509 \
  -days "$DAYS" \
  -config "$CONFIG_PATH" \
  -keyout "$KEY_PATH" \
  -out "$CERT_PATH"

openssl pkcs12 \
  -export \
  -inkey "$KEY_PATH" \
  -in "$CERT_PATH" \
  -name "$IDENTITY_NAME" \
  -out "$P12_PATH" \
  -passout "pass:$P12_PASSWORD"

chmod 600 "$KEY_PATH" "$P12_PATH"

if [[ "$IMPORT_CERT" == "ON" ]]; then
  if ! command -v security >/dev/null 2>&1; then
    echo "security was not found; cannot import into the login keychain." >&2
    exit 1
  fi
  security import "$P12_PATH" -P "$P12_PASSWORD" -T /usr/bin/codesign
  security add-trusted-cert -d -r trustRoot -p codeSign -k "$HOME/Library/Keychains/login.keychain-db" "$CERT_PATH"
  security set-key-partition-list -S apple-tool:,apple: -s -k "" "$HOME/Library/Keychains/login.keychain-db" >/dev/null 2>&1 || true
fi

echo "Created macOS release certificate:"
echo "  Identity: $IDENTITY_NAME"
echo "  Certificate: $CERT_PATH"
echo "  Private key: $KEY_PATH"
echo "  GitHub Actions .p12: $P12_PATH"
echo
echo "Store these GitHub Actions secrets for stable macOS release signing:"
if [[ "${SMU_PRINT_MACOS_RELEASE_CERTIFICATE_SECRET:-OFF}" == "ON" ]]; then
  echo "  SMU_MACOS_RELEASE_CERTIFICATE_BASE64=$(base64 < "$P12_PATH" | tr -d '\n')"
else
  echo "  SMU_MACOS_RELEASE_CERTIFICATE_BASE64=<output of: base64 < \"$P12_PATH\" | tr -d '\\n'>"
fi
echo "  SMU_MACOS_RELEASE_CERTIFICATE_PASSWORD=<the password you set>"
echo "  SMU_MACOS_RELEASE_SIGN_IDENTITY=$IDENTITY_NAME"
