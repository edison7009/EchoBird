#!/usr/bin/env bash
set -euo pipefail

target=${1:?Rust target required}
tag=${2:?Release tag required}
output=${3:?Output directory required}
[[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]
version=${tag#v}
case "$target" in
  aarch64-apple-darwin) arch=arm64; dmg_arch=aarch64; suffix=arm64 ;;
  x86_64-apple-darwin) arch=x86_64; dmg_arch=x64; suffix=x64 ;;
  *) echo "Unsupported macOS target: $target" >&2; exit 1 ;;
esac
name="EchoBird_${version}_macOS_${suffix}"
[[ "$(node -p "require('./src-tauri/tauri.conf.json').version")" = "$version" ]]

scratch=$(mktemp -d)
mount="$scratch/mount"
mounted=false
cleanup() {
  if [[ "$mounted" = true ]]; then
    hdiutil detach "$mount" -quiet || return
  fi
  rm -rf "$scratch"
}
trap cleanup EXIT

# Normalize the entitlement plist for codesign's stricter XML parser.
entitlements=$(node -p "require('./src-tauri/tauri.macos.conf.json').bundle.macOS.entitlements")
export ECHOBIRD_BUILD_ENTITLEMENTS="$scratch/entitlements.plist"
plutil -convert xml1 -o "$ECHOBIRD_BUILD_ENTITLEMENTS" "src-tauri/$entitlements"
plutil -lint "$ECHOBIRD_BUILD_ENTITLEMENTS"

signed=${ECHOBIRD_MACOS_SIGNED:-false}
args=(--target "$target" --bundles app)
if [[ "$signed" = true ]]; then
  : "${APPLE_CERTIFICATE:?Missing Developer ID certificate}"
  : "${APPLE_CERTIFICATE_PASSWORD:?Missing certificate password}"
  : "${APPLE_SIGNING_IDENTITY:?Missing Developer ID signing identity}"
  : "${APPLE_API_ISSUER:?Missing Apple API issuer ID}"
  : "${APPLE_API_KEY:?Missing Apple API key ID}"
  : "${APPLE_API_PRIVATE_KEY:?Missing Apple API private key contents}"
  : "${TAURI_SIGNING_PRIVATE_KEY:?Missing updater signing key}"
  : "${ECHOBIRD_UPDATER_PUBLIC_KEY:?Missing updater public key}"
  [[ "$APPLE_SIGNING_IDENTITY" = "Developer ID Application: "* ]]
  export APPLE_API_KEY_PATH="$scratch/auth.p8"
  (umask 077; printf '%s\n' "$APPLE_API_PRIVATE_KEY" > "$APPLE_API_KEY_PATH")
  unset APPLE_API_PRIVATE_KEY

  node -e 'process.stdout.write(JSON.stringify({
    bundle: { createUpdaterArtifacts: true, macOS: { entitlements: process.env.ECHOBIRD_BUILD_ENTITLEMENTS } },
    plugins: { updater: {
      pubkey: process.env.ECHOBIRD_UPDATER_PUBLIC_KEY.trim(),
      requireSignedVersion: true
    } }
  }))' > "$scratch/updater.json"
  args=(--target "$target" --bundles app --config "$scratch/updater.json")
elif [[ "$signed" = false ]]; then
  # Unset empty GitHub secret variables so Tauri does not try importing a
  # certificate or submitting a notarization request for an ad-hoc build.
  unset APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_API_ISSUER APPLE_API_KEY
  unset APPLE_API_KEY_PATH APPLE_API_PRIVATE_KEY APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID
  export APPLE_SIGNING_IDENTITY=-
  # Tauri signs ad-hoc with hardened runtime unless told otherwise. Ad-hoc +
  # hardened runtime + no notarization makes Gatekeeper kill the GUI launch
  # silently (the icon bounces, nothing opens), so unsigned builds opt out.
  node -e 'process.stdout.write(JSON.stringify({
    bundle: { macOS: { hardenedRuntime: false, entitlements: process.env.ECHOBIRD_BUILD_ENTITLEMENTS } }
  }))' > "$scratch/adhoc.json"
  args+=(--config "$scratch/adhoc.json")
else
  echo 'ECHOBIRD_MACOS_SIGNED must be true or false' >&2
  exit 1
fi

if [[ "$signed" = true ]]; then
  # A .p12 carries only our certificate; codesign also needs Apple's
  # Developer ID G2 intermediate to chain to Apple's root.
  if curl -fsSL https://www.apple.com/certificateauthority/DeveloperIDG2CA.cer -o "$scratch/DeveloperIDG2CA.cer"; then
    security import "$scratch/DeveloperIDG2CA.cer" -k "$HOME/Library/Keychains/login.keychain-db" >/dev/null 2>&1 || true
  fi
fi

# Build output stays in a file: this runs in the public repository, whose
# logs must not show the private desktop source.
build_log="${RUNNER_TEMP:-$scratch}/echobird-build.log"
if ! npm run tauri -- build "${args[@]}" > "$build_log" 2>&1; then
  if [[ -n "${ECHOBIRD_RELEASE_DIAGNOSTICS_KEY:-}" ]]; then
    node --input-type=module - "$build_log" <<'NODE'
import { constants, createCipheriv, publicEncrypt, randomBytes } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { gzipSync } from 'node:zlib';
const publicKey = Buffer.from(process.env.ECHOBIRD_RELEASE_DIAGNOSTICS_KEY, 'base64');
const key = randomBytes(32);
const nonce = randomBytes(12);
const cipher = createCipheriv('aes-256-gcm', key, nonce);
const ciphertext = Buffer.concat([cipher.update(gzipSync(readFileSync(process.argv[2]))), cipher.final()]);
const wrapped = publicEncrypt({ key: publicKey, padding: constants.RSA_PKCS1_OAEP_PADDING, oaepHash: 'sha256' }, key);
const record = Buffer.concat([wrapped, nonce, cipher.getAuthTag(), ciphertext]).toString('base64');
for (const line of record.match(/.{1,160}/g)) console.log(`ECHOBIRD_ENCRYPTED_DIAGNOSTICS ${line}`);
NODE
  fi
  echo "::error::Desktop build failed. Check the exact source commit in the private repository." >&2
  exit 1
fi

target_dir=$(cargo metadata --locked --no-deps --format-version 1 --manifest-path src-tauri/Cargo.toml |
  python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')
bundle="$target_dir/$target/release/bundle"
dmg="$bundle/dmg/EchoBird_${version}_${dmg_arch}.dmg"

# Finder window layout written straight into .DS_Store by dmgbuild. Tauri's
# own layout drives Finder through AppleScript, which needs Automation
# permission and a logged-in Finder; on runners it times out (-1712).
mkdir -p "$(dirname "$dmg")"
rm -f "$dmg"
python3 -m venv "$scratch/dmgbuild"
"$scratch/dmgbuild/bin/pip" install --quiet --disable-pip-version-check "dmgbuild==1.6.5"
"$scratch/dmgbuild/bin/dmgbuild" -s src-tauri/dmg/dmg-settings.py \
  -D app="$bundle/macos/EchoBird.app" -D root="$PWD" EchoBird "$dmg"
test -s "$dmg"

verify_app() {
  local app=$1 sdk
  test "$(lipo -archs "$app/Contents/MacOS/echobird")" = "$arch"
  test "$(plutil -extract CFBundleShortVersionString raw "$app/Contents/Info.plist")" = "$version"
  sdk=$(xcrun vtool -show-build "$app/Contents/MacOS/echobird" | awk '$1 == "sdk" { print $2; exit }')
  test -n "$sdk"
  test "${sdk%%.*}" -ge 26
}

# Apple's notary and CloudKit (stapler) services drop requests now and then.
# Upload once (retrying a failed upload), then wait on that one submission: a
# slow queue — a new team's first submissions can take hours — is waited out,
# never re-uploaded. Rejected credentials or a rejected submission stop at once.
notarize() {
  local file=$1 attempt out id=""
  local auth=(--key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY" --issuer "$APPLE_API_ISSUER")
  for attempt in 1 2 3; do
    out=$(xcrun notarytool submit "$file" "${auth[@]}" --output-format json 2>&1) || true
    id=$(plutil -extract id raw - <<<"$out" 2>/dev/null || true)
    [[ -n "$id" ]] && break
    echo "$out" >&2
    if grep -qiE 'HTTP status code: (401|403)|agreement|unauthorized' <<<"$out"; then
      echo "Apple rejected the notarization credentials; if it mentions an agreement, the Account Holder must accept it at developer.apple.com." >&2
      return 1
    fi
    [[ "$attempt" -lt 3 ]] && sleep $((attempt * 60))
  done
  [[ -n "$id" ]] || return 1
  for attempt in 1 2 3 4 5 6; do
    xcrun notarytool wait "$id" "${auth[@]}" --timeout 30m --output-format json > "$scratch/notary.json" 2>&1 || true
    cat "$scratch/notary.json"
    case "$(plutil -extract status raw "$scratch/notary.json" 2>/dev/null || true)" in
      Accepted) return ;;
      Invalid|Rejected) xcrun notarytool log "$id" "${auth[@]}" || true; return 1 ;;
    esac
  done
  return 1
}

staple() {
  local attempt
  for attempt in 1 2 3 4 5; do
    xcrun stapler staple "$1" && xcrun stapler validate "$1" && return
    [[ "$attempt" -lt 5 ]] && sleep $((attempt * 20))
  done
  return 1
}

verify_developer_id() {
  codesign --verify --deep --strict \
    -R='anchor apple generic and certificate leaf[field.1.2.840.113635.100.6.1.13] exists' "$1"
}

if [[ "$signed" = true ]]; then
  # The DMG needs its own Developer ID signature: notarized but unsigned, it
  # still fails Gatekeeper's open check once a browser quarantines it.
  codesign --force --sign "$APPLE_SIGNING_IDENTITY" --timestamp "$dmg"
  verify_developer_id "$dmg"
  notarize "$dmg"
  staple "$dmg"
  spctl --assess --type open --context context:primary-signature "$dmg"
fi

mkdir -p "$mount"
hdiutil attach "$dmg" -readonly -nobrowse -mountpoint "$mount"
mounted=true
test "$(readlink "$mount/Applications")" = /Applications
test -s "$mount/.DS_Store"
test -s "$mount/.background.tiff"  # dmgbuild merges background.png + @2x here
verify_app "$mount/EchoBird.app"
# Strict: no Finder-info detritus on the bundle, which would travel with it
# into /Applications and break the signature check.
codesign --verify --deep --strict "$mount/EchoBird.app"

mkdir -p "$output"
if [[ "$signed" = true ]]; then
  verify_developer_id "$mount/EchoBird.app"
  xcrun stapler validate "$mount/EchoBird.app"
  spctl --assess --type execute "$mount/EchoBird.app"

  archive="$bundle/macos/EchoBird.app.tar.gz"
  test -s "$archive"
  test -s "$archive.sig"
  mkdir "$scratch/archive"
  tar -xzf "$archive" -C "$scratch/archive"
  verify_app "$scratch/archive/EchoBird.app"
  verify_developer_id "$scratch/archive/EchoBird.app"
  spctl --assess --type execute "$scratch/archive/EchoBird.app"

  # Tauri warns on a key mismatch but still writes the signature. Check the
  # minisign key ID and signed version before exposing the feed to clients.
  python3 - "$archive.sig" "$output/$name.json" "$name" "$version" <<'PY'
import base64
import json
import os
import sys
from pathlib import Path

signature_path, output, name, version = sys.argv[1:]
signature = Path(signature_path).read_text().strip()
public_key = os.environ["ECHOBIRD_UPDATER_PUBLIC_KEY"].strip()
key_record = base64.b64decode(base64.b64decode(public_key).splitlines()[1])
signature_lines = base64.b64decode(signature).splitlines()
signature_record = base64.b64decode(signature_lines[1])
if key_record[2:10] != signature_record[2:10]:
    raise SystemExit("The updater public and private keys do not match")
if f"version:{version}".encode() not in signature_lines[2].split(b"\t"):
    raise SystemExit("The updater signature does not contain the release version")
repo = os.environ.get("GITHUB_REPOSITORY", "edison7009/EchoBird")
url = f"https://github.com/{repo}/releases/download/v{version}/{name}.app.tar.gz"
Path(output).write_text(json.dumps({"version": version, "url": url, "signature": signature}) + "\n")
PY
  cp "$archive" "$output/$name.app.tar.gz"
fi
cp "$dmg" "$output/$name.dmg"
