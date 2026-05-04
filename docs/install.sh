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
echo "  ${CYAN}EchoBird 百灵鸟 Installer${RESET}"
echo "  ${GRAY}─────────────────────────${RESET}"

OS=$(uname -s)
ARCH=$(uname -m)

# Detect the asset matcher pattern for our platform.
# CI emits these names (see .github/workflows/release.yml rename step):
#   EchoBird_<ver>_Windows_x64-setup.exe
#   EchoBird_<ver>_macOS_Universal.dmg     ← single Universal binary covers Intel + ARM
#   EchoBird_<ver>_Linux_x64.deb
#   EchoBird_<ver>_Linux_arm64.deb
ASSET_GREP=""
PLATFORM=""
if [ "$OS" = "Darwin" ]; then
  PLATFORM="macos"
  ASSET_GREP="macOS_Universal\.dmg"
elif [ "$OS" = "Linux" ]; then
  case "$ARCH" in
    x86_64|amd64)
      PLATFORM="linux-x64"
      ASSET_GREP="(Linux_x64|amd64)\.deb"
      ;;
    aarch64|arm64)
      PLATFORM="linux-arm64"
      ASSET_GREP="(Linux_arm64|arm64)\.deb"
      ;;
    *)
      echo "  ${RED}Unsupported Linux architecture: $ARCH${RESET}"
      echo "  ${YELLOW}EchoBird ships amd64 and arm64 .deb only.${RESET}"
      echo "  ${YELLOW}Open an issue: https://github.com/edison7009/EchoBird/issues${RESET}"
      exit 1
      ;;
  esac
  if ! command -v dpkg > /dev/null 2>&1; then
    echo "  ${RED}This Linux distro doesn't have dpkg.${RESET}"
    echo "  ${YELLOW}EchoBird only ships .deb packages right now.${RESET}"
    echo "  ${YELLOW}Manual download: https://github.com/edison7009/EchoBird/releases/latest${RESET}"
    exit 1
  fi
else
  echo "  ${RED}Unsupported OS: $OS${RESET}"
  exit 1
fi

# Pull the latest release JSON. Anonymous quota is 60/h per IP, plenty for one install.
echo "  ${GRAY}Fetching latest version...${RESET}"
GH_JSON=$(curl -fsSL -H "User-Agent: EchoBird-Install" "$GITHUB_API" 2>/dev/null || true)
if [ -z "$GH_JSON" ]; then
  echo ""
  echo "  ${RED}Could not reach api.github.com.${RESET}"
  echo "  ${YELLOW}Manual download: https://github.com/edison7009/EchoBird/releases/latest${RESET}"
  echo ""
  exit 1
fi

LATEST_VER=$(echo "$GH_JSON" | grep -oE '"tag_name"[[:space:]]*:[[:space:]]*"[^"]*"' | head -1 | sed -E 's/.*"([^"]*)"$/\1/' | sed 's/^v//')
DOWNLOAD_URL=$(echo "$GH_JSON" | grep -oE "\"browser_download_url\"[[:space:]]*:[[:space:]]*\"[^\"]*${ASSET_GREP}\"" | head -1 | sed -E 's/.*"(https[^"]*)"$/\1/')

# Empty download URL = our platform's asset isn't out yet (mid-CI for a
# just-tagged release). Show a friendly "come back later" instead of
# advertising a version we can't deliver.
if [ -z "$LATEST_VER" ] || [ -z "$DOWNLOAD_URL" ]; then
  echo ""
  echo "  ${YELLOW}A new version of EchoBird was just released.${RESET}"
  echo "  ${YELLOW}The ${PLATFORM} installer is still uploading to GitHub.${RESET}"
  echo "  ${YELLOW}Please try again in about 10 minutes.${RESET}"
  echo ""
  if [ -r /dev/tty ]; then
    printf "  ${GRAY}Press Enter to close...${RESET}"
    read _ < /dev/tty
    echo ""
  fi
  exit 0
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

  INSTALLED_VER=""
  if command -v dpkg > /dev/null 2>&1; then
    INSTALLED_VER=$(dpkg -s echobird 2>/dev/null | grep '^Version:' | sed 's/Version: //' || true)
  fi

  if [ -n "$INSTALLED_VER" ]; then
    echo "  ${GRAY}Installed : v$INSTALLED_VER${RESET}"
    if [ "$INSTALLED_VER" = "$LATEST_VER" ]; then
      echo ""
      echo "  ${GREEN}EchoBird is already up to date (v$INSTALLED_VER).${RESET}"
      echo ""
      exit 0
    fi
    echo "  ${YELLOW}Upgrading v$INSTALLED_VER  →  v$LATEST_VER ...${RESET}"
  else
    echo "  ${GRAY}Not installed — performing fresh install...${RESET}"
  fi

  TMP="/tmp/echobird-v${LATEST_VER}.deb"
  rm -f "$TMP"
  echo "  ${GRAY}Downloading .deb package...${RESET}"
  if ! curl -fL --progress-bar --retry 5 --retry-all-errors --retry-delay 2 "$DOWNLOAD_URL" -o "$TMP"; then
    echo ""
    echo "  ${RED}Download failed.${RESET}"
    echo "  ${YELLOW}Retry in ~5 min, or download manually:${RESET}"
    echo "  ${YELLOW}https://github.com/edison7009/EchoBird/releases/latest${RESET}"
    echo ""
    exit 1
  fi
  echo "  ${GRAY}Installing (requires sudo)...${RESET}"
  sudo dpkg -i "$TMP" || sudo apt-get install -f -y
  rm "$TMP"
  echo ""
  echo "  ${GREEN}Done! EchoBird v$LATEST_VER installed.${RESET}"

fi

echo ""
