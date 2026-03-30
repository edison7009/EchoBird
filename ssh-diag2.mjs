import { execSync } from 'child_process';
import { createRequire } from 'module';
const require = createRequire(import.meta.url);
const { Client } = require('ssh2');

const conn = new Client();
const run = (cmd) => new Promise((res) => {
  conn.exec(cmd, (err, stream) => {
    let o = '';
    stream.on('data', d => o += d);
    stream.stderr.on('data', d => o += d);
    stream.on('close', () => res(o.trim()));
    stream.resume();
  });
});

conn.on('ready', async () => {
  console.log('[SSH OK]');

  // 1. Find what shell we have
  const whoami = await run('whoami');
  console.log('whoami:', whoami);

  // 2. List APPDATA
  const appdata = await run('echo %APPDATA%');
  console.log('APPDATA:', appdata);

  // 3. Find echobird dirs
  const dirs = await run('dir /b /ad "' + appdata.trim() + '"');
  console.log('APPDATA dirs:\n', dirs);

  conn.end();
}).on('error', e => console.error('SSH ERR:', e.message)
).connect({ host:'192.168.10.207', port:22, username:'27200', password:'669966' });
