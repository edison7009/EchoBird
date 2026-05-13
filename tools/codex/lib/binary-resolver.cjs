const fs = require("fs");
const path = require("path");
const os = require("os");
const { execFileSync } = require("child_process");

// Binary resolution
//
// CLI mode: Codex v0.107+ ships as a Rust binary inside
// @openai/codex-<platform>. Direct-spawning preserves the TTY chain;
// going through codex.cmd → node codex.js → codex.exe with shell:true
// drops TTY-ness inside the cmd /d /s /c wrapper and the Rust TUI aborts
// with "stdin is not a terminal".
//
// Desktop mode: looks for the standalone Codex app (.exe on Windows,
// .app on macOS). The desktop installer is independent of npm, so we
// search the well-known install locations from tools/codexdesktop/paths.json.

function resolveDesktopBinary() {
    const platform = process.platform;
    const candidates = [];
    if (platform === "win32") {
        const localAppData = process.env.LOCALAPPDATA;
        if (localAppData) {
            // 1. Standalone installer (default location).
            candidates.push(path.join(localAppData, "Programs", "Codex", "Codex.exe"));
            // 2. Microsoft Store install — Windows 10+ exposes an
            //    executable alias here that resolves to the Store
            //    package, so we can spawn it like a normal exe.
            candidates.push(path.join(localAppData, "Microsoft", "WindowsApps", "Codex.exe"));
        }
        // 3. PATH lookup as a last resort.
        try {
            // Silence stderr: `where` writes localized "not found" messages
            // to stderr in the system's ANSI codepage (e.g. GBK on
            // zh-CN), which then bleeds into our launcher console as
            // mojibake. We only care about stdout for the resolved path.
            const found = execFileSync("where", ["Codex.exe"], {
                encoding: "utf-8", timeout: 3000,
                stdio: ["ignore", "pipe", "ignore"],
            }).trim().split(/\r?\n/)[0].trim();
            if (found) candidates.push(found);
        } catch { /* not in PATH */ }
    } else if (platform === "darwin") {
        candidates.push("/Applications/Codex.app/Contents/MacOS/Codex");
        candidates.push(path.join(os.homedir(), "Applications", "Codex.app", "Contents", "MacOS", "Codex"));
    }
    // Codex Desktop has no Linux build as of 2026-05.
    for (const c of candidates) {
        if (fs.existsSync(c)) return c;
    }
    return null;
}

// Read tools/codexdesktop/paths.json to get the Windows shell:AppsFolder
// URI. We use this only as a fallback when resolveDesktopBinary fails —
// the URI launch is fire-and-forget (no child process to track), so we
// have to poll for the Codex process to know when to tear down.
function resolveDesktopLaunchUri(launcherDir) {
    if (process.platform !== "win32") return null;
    try {
        const desktopPathsJson = path.join(launcherDir, "..", "codexdesktop", "paths.json");
        if (!fs.existsSync(desktopPathsJson)) return null;
        const cfg = JSON.parse(fs.readFileSync(desktopPathsJson, "utf-8"));
        return typeof cfg.launchUri === "string" ? cfg.launchUri : null;
    } catch { return null; }
}

function resolveCodexBinary() {
    const platform = process.platform;
    const arch = process.arch;
    let platPkg, triple, exeName;
    if (platform === "win32") {
        if (arch === "arm64") { platPkg = "@openai/codex-win32-arm64"; triple = "aarch64-pc-windows-msvc"; }
        else                  { platPkg = "@openai/codex-win32-x64";   triple = "x86_64-pc-windows-msvc"; }
        exeName = "codex.exe";
    } else if (platform === "darwin") {
        if (arch === "arm64") { platPkg = "@openai/codex-darwin-arm64"; triple = "aarch64-apple-darwin"; }
        else                  { platPkg = "@openai/codex-darwin-x64";   triple = "x86_64-apple-darwin"; }
        exeName = "codex";
    } else if (platform === "linux") {
        if (arch === "arm64") { platPkg = "@openai/codex-linux-arm64"; triple = "aarch64-unknown-linux-musl"; }
        else                  { platPkg = "@openai/codex-linux-x64";   triple = "x86_64-unknown-linux-musl"; }
        exeName = "codex";
    } else return null;

    const codexPkgRoots = [];
    try {
        const findCmd = platform === "win32" ? "where" : "which";
        const findArg = platform === "win32" ? "codex.cmd" : "codex";
        const stub = execFileSync(findCmd, [findArg], {
            encoding: "utf8", timeout: 3000,
            stdio: ["ignore", "pipe", "ignore"],
        }).trim().split(/\r?\n/)[0].trim();
        if (stub) {
            const npmDir = path.dirname(stub);
            codexPkgRoots.push(path.join(npmDir, "node_modules", "@openai", "codex"));
            codexPkgRoots.push(path.join(path.dirname(npmDir), "lib", "node_modules", "@openai", "codex"));
        }
    } catch { /* fall through */ }

    if (platform === "win32") {
        const appdata = process.env.APPDATA || process.env.LOCALAPPDATA;
        if (appdata && appdata.length > 2) {
            codexPkgRoots.push(path.join(appdata, "npm", "node_modules", "@openai", "codex"));
        }
    } else {
        codexPkgRoots.push("/usr/local/lib/node_modules/@openai/codex");
        codexPkgRoots.push("/usr/lib/node_modules/@openai/codex");
        codexPkgRoots.push(path.join(os.homedir(), ".npm-global", "lib", "node_modules", "@openai", "codex"));
    }

    for (const pkgRoot of codexPkgRoots) {
        const candidate = path.join(pkgRoot, "node_modules", platPkg, "vendor", triple, "codex", exeName);
        if (fs.existsSync(candidate)) return candidate;
    }
    return null;
}

module.exports = {
    resolveDesktopBinary,
    resolveDesktopLaunchUri,
    resolveCodexBinary,
};
