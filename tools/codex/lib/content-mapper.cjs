// Content translation (Responses parts ↔ Chat parts)
//
// Responses API content is either a string OR an array of typed parts:
//   { type: "input_text",  text: "..." }       — user input text
//   { type: "text",        text: "..." }       — generic text
//   { type: "output_text", text: "..." }       — assistant history replay
//   { type: "input_image", image_url: "data:..." }  — image as URL/data URI
//   { type: "image_url",   image_url: "..." | {url:"..."} } — already chat shape
// Chat Completions accepts content as string OR an array of:
//   { type: "text",      text: "..." }
//   { type: "image_url", image_url: {url:"..."} }
// We collapse all-text parts to a plain string (less verbose, more providers
// accept it), otherwise emit the multimodal array form.

function mapContentPart(part) {
    const kind = part?.type;
    switch (kind) {
        case "input_text":
        case "text":
        case "output_text":
            return { type: "text", text: part.text || "" };
        case "input_image": {
            // Responses API: image_url is a plain string (often a data: URL).
            // Chat Completions wants it wrapped: { image_url: { url: "..." } }.
            const url = typeof part.image_url === "string" ? part.image_url : "";
            return { type: "image_url", image_url: { url } };
        }
        case "image_url": {
            // Either already chat-shape ({url:...} object) or flat string.
            const raw = part.image_url;
            const inner = raw && typeof raw === "object"
                ? raw
                : { url: typeof raw === "string" ? raw : "" };
            return { type: "image_url", image_url: inner };
        }
        default:
            // Unknown / future part type: pass through verbatim so providers
            // that accept it can use it, and we don't crash on schemas the
            // launcher hasn't been updated to know about.
            return part;
    }
}

function valueToChatContent(content) {
    if (content == null) return null;
    if (typeof content === "string") return content;
    if (!Array.isArray(content)) {
        // Object / number / etc — stringify defensively rather than drop it.
        try { return JSON.stringify(content); } catch { return String(content); }
    }

    // Pure text array → collapse to a single string (lower-friction shape
    // for providers that don't fully support multimodal content arrays).
    // output_text is treated like text because that's what Codex replays
    // for assistant history items.
    const hasNonText = content.some(p => {
        const k = p?.type;
        return k && k !== "input_text" && k !== "text" && k !== "output_text";
    });
    if (!hasNonText) {
        return content
            .map(p => (p && typeof p.text === "string") ? p.text : "")
            .join("");
    }
    return content.map(mapContentPart);
}

module.exports = { mapContentPart, valueToChatContent };
