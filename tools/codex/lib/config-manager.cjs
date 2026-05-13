const fs = require("fs");
const path = require("path");
const os = require("os");

// Paths are derived from $HOME by default. ECHOBIRD_CODEX_CONFIG_DIR override
// exists for smoke tests so we don't touch the user's real Codex config.
const CODEX_DIR = process.env.ECHOBIRD_CODEX_CONFIG_DIR || path.join(os.homedir(), ".codex");
const RELAY_DIR = process.env.ECHOBIRD_RELAY_DIR || path.join(os.homedir(), ".echobird");
const CODEX_CONFIG = path.join(CODEX_DIR, "config.toml");
const ECHOBIRD_CONFIG = path.join(RELAY_DIR, "codex.json");

function loadEchobirdConfig() {
    try { return JSON.parse(fs.readFileSync(ECHOBIRD_CONFIG, "utf-8")); }
    catch { return null; }
}

function isOpenAI(url) { return !!url && url.includes("api.openai.com"); }

// config.toml base_url rewrite
//
// apply_codex writes the third-party URL inside a
// [model_providers.<provider_id>] section. We try three tiers from
// precise to blunt so a config that doesn't look exactly the way
// apply_codex wrote it still gets rewritten correctly:
//
//   1. Section-scoped: find [model_providers.<provider_id>] and
//      rewrite its base_url. Most precise — the case apply_codex
//      produces.
//   2. Host-scoped: find any base_url whose value matches the host
//      we know is the third-party endpoint (from relay JSON).
//      Survives user-edited TOML where section names don't match.
//   3. First-occurrence: replace the first base_url in the file
//      (the v4.0.2 approach). Last-resort for unusual layouts.
//
// Tier 1's regex uses [\s\S]*? rather than [^[]*? so it handles
// inline arrays / tables inside the section body. Bounded by the
// next `\n[` header to avoid leaking into the next section.

function escapeRegex(s) {
    return s.replace(/[.[\]\\^$*+?()|{}]/g, "\\$&");
}

function rewriteBaseUrl(providerId, currentBaseUrlHint, newUrl, logger) {
    const log = logger?.log || (() => {});
    const warn = logger?.warn || (() => {});
    const err = logger?.err || (() => {});

    let toml;
    try {
        toml = fs.readFileSync(CODEX_CONFIG, "utf-8");
    } catch (e) {
        err(`Cannot read config.toml: ${e.message}`);
        return { ok: false, tier: null };
    }

    const apply = (regex, label) => {
        const replaced = toml.replace(regex, (_m, prefix) => `${prefix}"${newUrl}"`);
        if (replaced === toml) return null;
        try {
            fs.writeFileSync(CODEX_CONFIG, replaced, "utf-8");
            log(`base_url rewritten via ${label} → ${newUrl}`);
            return label;
        } catch (e) {
            err(`config.toml write failed: ${e.message}`);
            return null;
        }
    };

    // Tier 1: section-scoped with the full TOML section name.
    if (providerId) {
        const fullSection = `model_providers.${providerId}`;
        const escaped = escapeRegex(fullSection);
        // Match [section] then non-greedy body until next [header (or EOF)
        // — capture the base_url = " prefix so we can replace just the value.
        const re = new RegExp(
            `(\\[${escaped}\\][\\s\\S]*?\\bbase_url\\s*=\\s*)"[^"]*"`,
            "m"
        );
        const hit = apply(re, `[${fullSection}]`);
        if (hit) return { ok: true, tier: hit };
    }

    // Tier 2: match by host. If we know the third-party endpoint host,
    // rewrite any base_url whose URL contains that host.
    if (currentBaseUrlHint) {
        try {
            const hintHost = new URL(currentBaseUrlHint).hostname;
            if (hintHost) {
                const escapedHost = escapeRegex(hintHost);
                const re = new RegExp(
                    `(\\bbase_url\\s*=\\s*)"https?://${escapedHost}[^"]*"`,
                    "m"
                );
                const hit = apply(re, `host-match ${hintHost}`);
                if (hit) return { ok: true, tier: hit };
            }
        } catch { /* malformed hint URL — skip tier */ }
    }

    // Tier 3: replace the first base_url in the file. Matches v4.0.2's
    // approach. Blunt but reliable when there's only one provider.
    const re = /(\bbase_url\s*=\s*)"[^"]*"/m;
    const hit = apply(re, "first-base_url (fallback)");
    if (hit) return { ok: true, tier: hit };

    warn("No base_url assignment found in config.toml — rewrite skipped");
    return { ok: false, tier: null };
}

module.exports = {
    loadEchobirdConfig,
    isOpenAI,
    rewriteBaseUrl,
    escapeRegex,
    CODEX_CONFIG,
    ECHOBIRD_CONFIG,
    CODEX_DIR,
    RELAY_DIR,
};
