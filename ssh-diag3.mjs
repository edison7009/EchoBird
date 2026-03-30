import { createRequire } from 'module';
const require = createRequire(import.meta.url);
const { Client } = require('ssh2');

const conn = new Client();
const run = (cmd) => new Promise((res) => {
  conn.exec(cmd, (err, stream) => {
    let o = '';
    stream.on('data', d => o += d);
    stream.stderr.on('data', d => o += '(err)' + d);
    stream.on('close', () => res(o.trim()));
    stream.resume();
  });
});

conn.on('ready', async () => {
  console.log('[SSH OK]');

  // List the user home
  const home = await run('ls ~');
  console.log('~ contents:', home);

  // Bitvise maps Windows paths — try the Windows AppData path directly
  const appdata = await run('ls "/c/Users/27200/AppData/Roaming"');
  console.log('AppData/Roaming:', appdata);

  conn.end();
}).on('error', e => console.error('SSH ERR:', e.message)
).connect({ host:'192.168.10.207', port:22, username:'27200', password:'669966' });
