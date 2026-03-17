// Deploy role images: rename, copy, and update JSON
// Run after ALL 156 images are generated
//
// 1. Finds role_N_*.png files in artifacts dir
// 2. Copies them to docs/roles/N.png
// 3. Updates both JSON files with correct image URLs

const fs = require('fs');
const path = require('path');

const ARTIFACTS_DIR = 'C:\\Users\\eben\\.gemini\\antigravity\\brain\\504967ac-c25d-479d-989a-e8be88002897';
const DOCS_ROLES_PRIVATE = 'd:\\Echobird\\docs\\roles';
const DOCS_ROLES_PUBLIC = 'd:\\Echobird-MotherAgent\\docs\\roles';
const CDN_BASE = 'https://echobird.ai/roles/';

// Step 1: Find and copy images
console.log('=== Step 1: Copy images ===');
const files = fs.readdirSync(ARTIFACTS_DIR);
let copied = 0;
const missing = [];

for (let i = 1; i <= 156; i++) {
  const match = files.find(f => f.startsWith(`role_${i}_`) && f.endsWith('.png'));
  if (match) {
    const src = path.join(ARTIFACTS_DIR, match);
    const dest1 = path.join(DOCS_ROLES_PRIVATE, `${i}.png`);
    const dest2 = path.join(DOCS_ROLES_PUBLIC, `${i}.png`);
    fs.copyFileSync(src, dest1);
    fs.copyFileSync(src, dest2);
    copied++;
  } else {
    missing.push(i);
  }
}
console.log(`Copied: ${copied}/156`);
if (missing.length > 0) {
  console.log(`Missing: ${missing.join(', ')}`);
}

// Step 2: Update JSON image URLs
console.log('\n=== Step 2: Update JSON ===');
['roles-en.json', 'roles-zh-Hans.json'].forEach(jsonFile => {
  const jsonPath = path.join('d:\\Echobird\\roles', jsonFile);
  const data = JSON.parse(fs.readFileSync(jsonPath, 'utf8'));

  data.roles.forEach((role, idx) => {
    const num = idx + 1;
    role.img = `${CDN_BASE}${num}.png`;
  });

  fs.writeFileSync(jsonPath, JSON.stringify(data, null, 2), 'utf8');
  console.log(`${jsonFile}: updated ${data.roles.length} image URLs`);
});

// Step 3: Clean up old .jpg files in docs/roles/
console.log('\n=== Step 3: Cleanup old files ===');
[DOCS_ROLES_PRIVATE, DOCS_ROLES_PUBLIC].forEach(dir => {
  fs.readdirSync(dir).forEach(f => {
    if (f.endsWith('.jpg') && /^\d+\.jpg$/.test(f)) {
      fs.unlinkSync(path.join(dir, f));
      console.log(`Deleted old: ${dir}/${f}`);
    }
  });
});

console.log('\n=== Done! ===');
console.log('Next: git add + commit + push');
