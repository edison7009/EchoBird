import fs from 'fs';
import { execSync } from 'child_process';
import { createRequire } from 'module';
const require = createRequire(import.meta.url);
try { require.resolve('ssh2'); } catch(e) {
  console.log('Installing ssh2...');
  execSync('npm install --no-save ssh2', { stdio: 'inherit' });
}
const { Client } = require('ssh2');
const conn = new Client();
conn.on('ready', () => {
  console.log('[SSH OK] Connected to 192.168.10.207');
  const cmd = 'powershell -NonInteractive -Command "Get-ChildItem C:\Users\eben\AppData\Roaming -Depth 3 -Filter *.log -ErrorAction SilentlyContinue | Where-Object FullName -match echobird | Select-Object FullName | Format-List; Get-ChildItem C:\Users\eben\AppData\Roaming -Depth 1 | Where-Object Name -match echo | Select-Object FullName | Format-List"';
  conn.exec(cmd, (err, stream) => {
    let o = '';
    stream.on('data', d => { o += d; process.stdout.write(d.toString()); });
    stream.stderr.on('data', d => process.stderr.write(d.toString()));
    stream.on('close', () => {
      if (!o.trim()) console.log('[RESULT] No echobird log found in APPDATA');
      conn.end();
    });
    stream.resume();
  });
}).on('error', e => console.error('[SSH ERR]', e.message)
).connect({ host:'192.168.10.207', port:22, username:'27200', password:'669966' });
