#!/bin/sh
# EchoBird (百灵鸟) — macOS / Linux Installer / Updater
# Usage:   curl -fsSL https://echobird.ai/install.sh | sh
# License: MIT (https://github.com/edison7009/EchoBird/blob/main/LICENSE)

set -e

GITHUB_API="https://api.github.com/repos/edison7009/EchoBird/releases/latest"

# Resolve ANSI codes via printf at assignment time so the variables hold real
# ESC bytes — plain string literals only render when echo interprets backslash
# escapes (sh/dash do, bash does not unless -e). Users piping through `| bash`
# would otherwise see literal "\033[0;36m" output.
CYAN=$(printf '\033[0;36m')
GREEN=$(printf '\033[0;32m')
GRAY=$(printf '\033[0;90m')
YELLOW=$(printf '\033[0;33m')
RED=$(printf '\033[0;31m')
RESET=$(printf '\033[0m')

echo ""
echo "  ${CYAN}EchoBird Installer${RESET}"
echo "  ${GRAY}──────────────────${RESET}"

OS=$(uname -s)
ARCH=$(uname -m)

# Each ASSET_GREP must match BOTH naming schemes the CI produces:
#   1. Default Tauri names (visible mid-build before the rename-assets job runs)
#      e.g. EchoBird_3.8.0_amd64.deb, EchoBird_3.8.0_universal.dmg
#   2. Renamed final names (post rename-assets job — see .github/workflows/release.yml)
#      e.g. EchoBird_3.8.0_Linux_x64.deb, EchoBird_3.8.0_macOS_Universal.dmg
# Matching both ensures `curl | sh` never reports "no asset" during the brief
# window where rename-assets hasn't run yet.
PLATFORM=""
DOWNLOAD_PLATFORM=""
ASSET_GREP=""

if [ "$OS" = "Darwin" ]; then
  # macOS: native build per architecture. As of v4.7.3 we again ship Intel
  # alongside Apple Silicon — Intel Macs (2018-2020 MBP, iMac Pro, etc.)
  # still have a non-trivial install base on Sonoma/Sequoia.
  case "$ARCH" in
    arm64)
      PLATFORM="macos"
      DOWNLOAD_PLATFORM="macos-arm"
      ASSET_GREP='(macOS_arm64|aarch64)\.dmg'
      ;;
    x86_64)
      PLATFORM="macos-intel"
      DOWNLOAD_PLATFORM="macos-intel"
      # Pattern matches both renamed (macOS_x64) and pre-rename (_x64) names.
      # The trailing .dmg keeps this from grabbing Windows_x64-*.exe assets.
      ASSET_GREP='(macOS_x64|_x64)\.dmg'
      ;;
    *)
      echo "  ${RED}Unsupported macOS architecture: $ARCH${RESET}"
      exit 1
      ;;
  esac

elif [ "$OS" = "Linux" ]; then
  case "$ARCH" in
    x86_64|amd64)  LINUX_ARCH="amd64" ;;
    aarch64|arm64) LINUX_ARCH="arm64" ;;
    *)
      echo "  ${RED}Unsupported Linux architecture: $ARCH${RESET}"
      echo "  ${YELLOW}EchoBird currently ships amd64 and arm64 Linux builds only.${RESET}"
      echo "  ${YELLOW}Open an issue: https://github.com/edison7009/EchoBird/issues${RESET}"
      exit 1
      ;;
  esac

  # Prefer dpkg (.deb on Debian/Ubuntu) → rpm (.rpm on Fedora/RHEL/openSUSE).
  # We dropped AppImage in v4 — it was 80MB per arch (bundled webkit/gtk),
  # vs 11MB for distro-native packages that share system webkit2gtk.
  # Arch / NixOS / Alpine users without dpkg or rpm will need to extract
  # the .deb manually or build from source.
  if command -v dpkg > /dev/null 2>&1; then
    if [ "$LINUX_ARCH" = "arm64" ]; then
      PLATFORM="linux-arm64-deb"
      DOWNLOAD_PLATFORM="linux-arm64-deb"
      ASSET_GREP='(Linux_arm64|arm64)\.deb'
    else
      PLATFORM="linux-x64-deb"
      DOWNLOAD_PLATFORM="linux-deb"
      ASSET_GREP='(Linux_x64|amd64)\.deb'
    fi
  elif command -v rpm > /dev/null 2>&1; then
    if [ "$LINUX_ARCH" = "arm64" ]; then
      PLATFORM="linux-arm64-rpm"
      DOWNLOAD_PLATFORM="linux-arm64-rpm"
      ASSET_GREP='(Linux_arm64\.rpm|aarch64\.rpm)'
    else
      PLATFORM="linux-x64-rpm"
      DOWNLOAD_PLATFORM="linux-rpm"
      ASSET_GREP='(Linux_x64\.rpm|x86_64\.rpm)'
    fi
  else
    echo "  ${RED}No supported package manager found (dpkg or rpm).${RESET}"
    echo "  ${YELLOW}EchoBird ships .deb (Debian/Ubuntu) and .rpm (Fedora/RHEL/openSUSE) only.${RESET}"
    echo "  ${YELLOW}For Arch / NixOS / Alpine, extract a .deb manually from:${RESET}"
    echo "  ${YELLOW}  https://github.com/edison7009/EchoBird/releases/latest${RESET}"
    exit 1
  fi
else
  echo "  ${RED}Unsupported OS: $OS${RESET}"
  exit 1
fi

# Resolve version + asset URL with a two-tier lookup. Tier 1 is the GitHub
# Releases API (matches old behavior; works for most users). Tier 2 is our
# own version manifest at echobird.ai/api/version/index.json — no rate
# limit, no anonymous quota. The api.github.com 60-req/hour bucket runs
# out fast when a user retries within a minute of release publication,
# and Github also occasionally serves region-specific 403s. The manifest
# is updated by sync-version-manifest.yml on the same `release: published`
# event that makes binaries downloadable, so any version it reports is
# already-downloadable.
echo "  ${GRAY}Fetching latest version...${RESET}"
LATEST_VER=""
DOWNLOAD_URL=""

# Tier 1: GitHub API.
GH_JSON=$(curl -fsSL -H "User-Agent: EchoBird-Install" "$GITHUB_API" 2>/dev/null || true)
if [ -n "$GH_JSON" ]; then
  LATEST_VER=$(echo "$GH_JSON" | grep -oE '"tag_name"[[:space:]]*:[[:space:]]*"[^"]*"' | head -1 | sed -E 's/.*"([^"]*)"$/\1/' | sed 's/^v//')
  DOWNLOAD_URL=$(echo "$GH_JSON" | grep -oE "\"browser_download_url\"[[:space:]]*:[[:space:]]*\"[^\"]*${ASSET_GREP}\"" | head -1 | sed -E 's/.*"(https[^"]*)"$/\1/')
fi

# Tier 2: echobird.ai manifest fallback. Triggers when GitHub API is
# unreachable (rate-limited / 403'd / region-blocked) OR when the API
# response didn't carry our platform's asset (rare; only during the
# narrow window between rename-assets job finishing and `release:
# published` firing — once published, manifest reflects the new version
# and assets are renamed).
if [ -z "$LATEST_VER" ] || [ -z "$DOWNLOAD_URL" ]; then
  MANIFEST_VER=$(curl -fsSL -H "User-Agent: EchoBird-Install" "https://echobird.ai/api/version/index.json" 2>/dev/null \
    | grep -oE '"version"[[:space:]]*:[[:space:]]*"[^"]*"' | head -1 \
    | sed -E 's/.*"([^"]*)"$/\1/')
  if [ -n "$MANIFEST_VER" ]; then
    LATEST_VER="$MANIFEST_VER"
    # Construct the renamed-asset URL per .github/workflows/release.yml
    # rename-assets job. Keep ASSET_NAME in sync with that workflow.
    case "$PLATFORM" in
      macos)            ASSET_NAME="EchoBird_${LATEST_VER}_macOS_arm64.dmg" ;;
      macos-intel)      ASSET_NAME="EchoBird_${LATEST_VER}_macOS_x64.dmg" ;;
      linux-x64-deb)    ASSET_NAME="EchoBird_${LATEST_VER}_Linux_x64.deb" ;;
      linux-arm64-deb)  ASSET_NAME="EchoBird_${LATEST_VER}_Linux_arm64.deb" ;;
      linux-x64-rpm)    ASSET_NAME="EchoBird_${LATEST_VER}_Linux_x64.rpm" ;;
      linux-arm64-rpm)  ASSET_NAME="EchoBird_${LATEST_VER}_Linux_arm64.rpm" ;;
      *)                ASSET_NAME="" ;;
    esac
    if [ -n "$ASSET_NAME" ]; then
      DOWNLOAD_URL="https://echobird.ai/download/${DOWNLOAD_PLATFORM}"
    fi
  fi
fi

if [ -z "$LATEST_VER" ]; then
  echo ""
  echo "  ${RED}Could not reach api.github.com or echobird.ai.${RESET}"
  echo "  ${YELLOW}Manual download: https://github.com/edison7009/EchoBird/releases/latest${RESET}"
  echo ""
  exit 1
fi

# Got a version but no matching asset URL. The old copy here said "still
# uploading, try in 10 min" — but release.yml uploads every asset BEFORE
# the draft release exists, so by the time the API reports a tag, all
# assets are already in place. A missing asset means either the release
# genuinely doesn't have one for this platform (CI matrix dropped it)
# or our grep pattern fell out of sync with the rename-assets job.
if [ -z "$DOWNLOAD_URL" ]; then
  echo ""
  echo "  ${RED}No ${PLATFORM} installer found in v${LATEST_VER}.${RESET}"
  echo "  ${YELLOW}Browse all assets: https://github.com/edison7009/EchoBird/releases/tag/v${LATEST_VER}${RESET}"
  echo ""
  if [ -r /dev/tty ]; then
    printf "  ${GRAY}Press Enter to close...${RESET}"
    read _ < /dev/tty
    echo ""
  fi
  exit 1
fi

echo "  ${GREEN}Latest    : v$LATEST_VER${RESET}"

# ── macOS ──────────────────────────────────────────────────────────────────────
if [ "$OS" = "Darwin" ]; then

  INSTALLED_VER=""
  APP_PATH="/Applications/EchoBird.app"
  if [ -d "$APP_PATH" ]; then
    INSTALLED_VER=$(defaults read "$APP_PATH/Contents/Info" CFBundleShortVersionString 2>/dev/null || true)
  fi

  if [ -n "$INSTALLED_VER" ]; then
    echo "  ${GRAY}Installed : v$INSTALLED_VER${RESET}"
    if [ "$INSTALLED_VER" = "$LATEST_VER" ]; then
      echo ""
      echo "  ${GREEN}EchoBird is already up to date (v$INSTALLED_VER).${RESET}"
      echo ""
      # Pause when there's a TTY so users running this via a .command
      # double-click (which closes the terminal on script exit) actually
      # see the message instead of a window that pops and vanishes.
      # In a normal interactive shell this just waits for Enter.
      if [ -r /dev/tty ]; then
        printf "  ${GRAY}Press Enter to continue...${RESET}"
        read _ < /dev/tty
        echo ""
      fi
      exit 0
    fi
    echo "  ${YELLOW}Upgrading v$INSTALLED_VER  →  v$LATEST_VER ...${RESET}"
  else
    echo "  ${GRAY}Not installed — performing fresh install...${RESET}"
  fi

  TMP="/tmp/echobird-v${LATEST_VER}.dmg"
  # Always wipe leftovers before downloading. `curl -C -` (resume) is
  # specifically REMOVED here: when curl receives a 502/connection reset
  # mid-response (error 56), the partial 5xx HTML body is already in $TMP.
  # On retry, `-C -` would send `Range: bytes=N-` and the server's 206
  # response would splice valid DMG bytes onto the 5xx-page prefix —
  # final file hits Content-Length but hdiutil rejects it as corrupt.
  # Re-downloading from scratch on each retry is the only correct option.
  rm -f "$TMP"

  echo "  ${GRAY}Downloading...${RESET}"
  if ! curl -fL --progress-bar --retry 5 --retry-all-errors --retry-delay 2 "$DOWNLOAD_URL" -o "$TMP"; then
    echo ""
    echo "  ${RED}Download failed.${RESET}"
    echo "  ${YELLOW}Retry in ~5 min, or download manually:${RESET}"
    echo "  ${YELLOW}https://github.com/edison7009/EchoBird/releases/latest${RESET}"
    echo ""
    exit 1
  fi

  echo "  ${GRAY}Mounting DMG...${RESET}"
  # NOTE: do NOT pass -quiet to hdiutil attach — it suppresses the very
  # stdout we grep for the mountpoint, leaving MOUNT empty and falsely
  # reporting "Failed to mount DMG" even on a successful attach.
  MOUNT=$(hdiutil attach "$TMP" -nobrowse | grep -oE '/Volumes/[^	]+' | tail -1)
  if [ -z "$MOUNT" ] || [ ! -d "$MOUNT" ]; then
    echo "  ${RED}Failed to mount DMG.${RESET}"
    rm -f "$TMP"
    exit 1
  fi
  APP=$(find "$MOUNT" -maxdepth 1 -name "*.app" | head -1)
  if [ -z "$APP" ]; then
    echo "  ${RED}No .app bundle found inside DMG.${RESET}"
    hdiutil detach "$MOUNT" -quiet || true
    rm -f "$TMP"
    exit 1
  fi

  echo "  ${GRAY}Installing to /Applications...${RESET}"
  rm -rf "/Applications/EchoBird.app"
  cp -R "$APP" /Applications/
  hdiutil detach "$MOUNT" -quiet
  rm "$TMP"

  # Strip com.apple.quarantine that curl-downloaded files inherit.
  # On Apple Silicon macOS 14+, Gatekeeper silently refuses to launch
  # adhoc-signed apps that still carry quarantine — clicking the dock
  # icon does nothing, no error dialog. Removing the xattr tells
  # LaunchServices the user has explicitly opted to trust this binary.
  xattr -dr com.apple.quarantine "/Applications/EchoBird.app" 2>/dev/null || true

  echo ""
  echo "  ${GREEN}Done! EchoBird v$LATEST_VER installed.${RESET}"
  echo "  ${GRAY}Launch it from /Applications or Spotlight.${RESET}"

# ── Linux ──────────────────────────────────────────────────────────────────────
elif [ "$OS" = "Linux" ]; then

  # Detect installed version — try both package managers
  INSTALLED_VER=""
  if command -v dpkg > /dev/null 2>&1; then
    INSTALLED_VER=$(dpkg -s echobird 2>/dev/null | grep '^Version:' | sed 's/Version: //' || true)
  fi
  if [ -z "$INSTALLED_VER" ] && command -v rpm > /dev/null 2>&1; then
    INSTALLED_VER=$(rpm -q --queryformat '%{VERSION}' echobird 2>/dev/null || true)
  fi

  if [ -n "$INSTALLED_VER" ]; then
    echo "  ${GRAY}Installed : v$INSTALLED_VER${RESET}"
    if [ "$INSTALLED_VER" = "$LATEST_VER" ]; then
      echo ""
      echo "  ${GREEN}EchoBird is already up to date (v$INSTALLED_VER).${RESET}"
      echo ""
      # Pause when there's a TTY — see macOS branch comment above.
      if [ -r /dev/tty ]; then
        printf "  ${GRAY}Press Enter to continue...${RESET}"
        read _ < /dev/tty
        echo ""
      fi
      exit 0
    fi
    echo "  ${YELLOW}Upgrading v$INSTALLED_VER  →  v$LATEST_VER ...${RESET}"
  else
    echo "  ${GRAY}Not installed — performing fresh install...${RESET}"
  fi

  # ── .deb branch (Debian / Ubuntu / Mint / etc.) ──
  case "$PLATFORM" in *-deb)
    TMP="/tmp/echobird-v${LATEST_VER}.deb"
    rm -f "$TMP"
    echo "  ${GRAY}Downloading .deb package...${RESET}"
    if ! curl -fL --progress-bar --retry 5 --retry-all-errors --retry-delay 2 "$DOWNLOAD_URL" -o "$TMP"; then
      echo ""
      echo "  ${RED}Download failed.${RESET}"
      echo "  ${YELLOW}Retry in ~5 min, or manual: https://github.com/edison7009/EchoBird/releases/latest${RESET}"
      echo ""
      exit 1
    fi
    echo "  ${GRAY}Installing (requires sudo)...${RESET}"
    # `dpkg -i` then `apt-get install -f -y` is the idiomatic way to install a
    # local .deb that depends on packages the user doesn't have yet — apt fixes
    # the broken state by pulling deps from the configured repos.
    sudo dpkg -i "$TMP" || sudo apt-get install -f -y
    rm "$TMP"
    echo ""
    echo "  ${GREEN}Done! EchoBird v$LATEST_VER installed.${RESET}"
    exit 0
  esac

  # ── .rpm branch (Fedora / RHEL / openSUSE / CentOS) ──
  case "$PLATFORM" in *-rpm)
    TMP="/tmp/echobird-v${LATEST_VER}.rpm"
    rm -f "$TMP"
    echo "  ${GRAY}Downloading .rpm package...${RESET}"
    if ! curl -fL --progress-bar --retry 5 --retry-all-errors --retry-delay 2 "$DOWNLOAD_URL" -o "$TMP"; then
      echo ""
      echo "  ${RED}Download failed.${RESET}"
      echo "  ${YELLOW}Retry in ~5 min, or manual: https://github.com/edison7009/EchoBird/releases/latest${RESET}"
      echo ""
      exit 1
    fi
    echo "  ${GRAY}Installing (requires sudo)...${RESET}"
    # Prefer dnf/zypper (resolves dependencies) over raw `rpm -i`. Plain
    # `rpm -i` fails with "Failed dependencies: ..." on newer Fedora / RHEL
    # where webkit2gtk is split into many runtime sub-packages.
    if command -v dnf > /dev/null 2>&1; then
      sudo dnf install -y "$TMP"
    elif command -v zypper > /dev/null 2>&1; then
      sudo zypper --non-interactive install --allow-unsigned-rpm "$TMP"
    elif command -v yum > /dev/null 2>&1; then
      sudo yum install -y "$TMP"
    else
      sudo rpm -i --replacepkgs "$TMP"
    fi
    rm "$TMP"
    echo ""
    echo "  ${GREEN}Done! EchoBird v$LATEST_VER installed.${RESET}"
    exit 0
  esac

fi

echo ""
