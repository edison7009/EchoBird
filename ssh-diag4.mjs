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
  // Try basic commands to figure out what shell / filesystem layout this is
  const pwd    = await run('pwd');
  const uname  = await run('uname -a');
  const lsroot = await run('ls /');
  const env    = await run('env | grep -i appdata');
  console.log('pwd:', pwd);
  console.log('uname:', uname);
  console.log('ls /:', lsroot);
  console.log('env APPDATA:', env);
  conn.end();
}).on('error', e => console.error('SSH ERR:', e.message)
).connect({ host:'192.168.10.207', port:22, username:'27200', password:'669966' });
