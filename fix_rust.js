const fs = require('fs');
const path = 'd:/Echobird/src-tauri/src/services/agent_loop.rs';
let c = fs.readFileSync(path, 'utf8');
const lines = c.split('\n');
// Line 816 (index 815): fix the trailing \" to just "
// Current: ...official site.\\n\\n\"
// Should be: ...official site.\\n\\n"
const lineIdx = 815;
const oldLine = lines[lineIdx];
console.log('Before:', JSON.stringify(oldLine));
// The problematic ending is \\n\\n\" (backslash-quote) — remove the backslash before the closing quote
lines[lineIdx] = oldLine.replace(/\\\\n\\\\n\\\\"/, '\\\\n\\\\n"');
console.log('After:', JSON.stringify(lines[lineIdx]));
if (lines[lineIdx] === oldLine) {
    console.log('ERROR: pattern not found');
    process.exit(1);
}
fs.writeFileSync(path, lines.join('\n'), 'utf8');
console.log('Written OK');
