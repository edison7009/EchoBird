import { readFileSync, writeFileSync } from 'fs';
const path = 'd:/Echobird/src-tauri/src/services/agent_loop.rs';
let c = readFileSync(path, 'utf8');
const lines = c.split('\n');
const lineIdx = 815;
const oldLine = lines[lineIdx];
// Last 4 chars reported as: \n\"  i.e. backslash,n,backslash,quote
// We want to strip the trailing backslash-quote, keeping just the closing double-quote
// The line ends with: ...site.\\n\\n\"  (inside the file these are literal \\n\\n\")
// Strip the backslash before the trailing " 
const trimmed = oldLine.trimEnd();
const lastTwo = trimmed.slice(-2);
console.log('Last 2 chars:', [...lastTwo].map(c => c.charCodeAt(0)));
// Should be: 92 (backslash), 34 (double-quote)
if (lastTwo.charCodeAt(0) === 92 && lastTwo.charCodeAt(1) === 34) {
    lines[lineIdx] = trimmed.slice(0, -2) + '"';
    const suffix = oldLine.slice(trimmed.length); // preserve trailing whitespace/\r
    lines[lineIdx] += suffix;
    console.log('Fixed line:', JSON.stringify(lines[lineIdx]));
    writeFileSync(path, lines.join('\n'), 'utf8');
    console.log('Written OK');
} else {
    console.error('Unexpected ending:', JSON.stringify(lastTwo));
    process.exit(1);
}
