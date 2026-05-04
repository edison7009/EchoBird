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
ASSET_GREP=""

if [ "$OS" = "Darwin" ]; then
  PLATFORM="macos"
  # macOS Universal binary covers both Intel and Apple Silicon.
  ASSET_GREP='(macOS_Universal|universal)\.dmg'

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

  # Prefer dpkg (.deb on Debian/Ubuntu) → rpm (.rpm on Fedora/RHEL/openSUSE)
  # → AppImage (everything else, incl. Arch / NixOS / minimal containers).
  # Distro-native packages register the binary system-wide and integrate with
  # the package manager; AppImage is portable but requires the user to put
  # ~/.local/bin on PATH themselves.
  if command -v dpkg > /dev/null 2>&1; then
    if [ "$LINUX_ARCH" = "arm64" ]; then
      PLATFORM="linux-arm64-deb"
      ASSET_GREP='(Linux_arm64|arm64)\.deb'
    else
      PLATFORM="linux-x64-deb"
      ASSET_GREP='(Linux_x64|amd64)\.deb'
    fi
  elif command -v rpm > /dev/null 2>&1; then
    # Tauri default rpm name is "EchoBird-3.8.0-1.x86_64.rpm" (note the
    # build counter "-1" before .arch.rpm). The renamed name is
    # "EchoBird_3.8.0_Linux_x64.rpm".
    if [ "$LINUX_ARCH" = "arm64" ]; then
      PLATFORM="linux-arm64-rpm"
      ASSET_GREP='(Linux_arm64\.rpm|aarch64\.rpm)'
    else
      PLATFORM="linux-x64-rpm"
      ASSET_GREP='(Linux_x64\.rpm|x86_64\.rpm)'
    fi
  else
    if [ "$LINUX_ARCH" = "arm64" ]; then
      PLATFORM="linux-arm64-appimage"
      ASSET_GREP='(Linux_arm64|aarch64)\.AppImage'
    else
      PLATFORM="linux-x64-appimage"
      ASSET_GREP='(Linux_x64|amd64)\.AppImage'
    fi
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
# just-tagged release: Linux runner usually finishes first, mac/Win take
# longer, then rename-assets runs). Show "come back later" instead of
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

  # ── AppImage branch (Arch / NixOS / Alpine / minimal containers) ──
  DEST="$HOME/.local/bin/echobird"
  mkdir -p "$HOME/.local/bin"
  # Download to a versioned temp first, then move into place. Writing
  # straight to $DEST would clobber a working install if the download
  # failed partway.
  TMP="/tmp/echobird-v${LATEST_VER}.AppImage"
  rm -f "$TMP"
  echo "  ${GRAY}Downloading AppImage...${RESET}"
  if ! curl -fL --progress-bar --retry 5 --retry-all-errors --retry-delay 2 "$DOWNLOAD_URL" -o "$TMP"; then
    echo ""
    echo "  ${RED}Download failed.${RESET}"
    echo "  ${YELLOW}Retry in ~5 min, or manual: https://github.com/edison7009/EchoBird/releases/latest${RESET}"
    echo ""
    exit 1
  fi
  mv -f "$TMP" "$DEST"
  chmod +x "$DEST"
  echo ""
  echo "  ${GREEN}Done! EchoBird v$LATEST_VER installed to $DEST${RESET}"
  echo "  ${GRAY}Make sure ~/.local/bin is in your PATH.${RESET}"

fi

echo ""
