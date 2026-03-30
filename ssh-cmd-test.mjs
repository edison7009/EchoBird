import { createRequire } from 'module';
const require = createRequire(import.meta.url);
const { Client } = require('ssh2');

const conn = new Client();
const run = (cmd) => new Promise((res) => {
  conn.exec(cmd, (err, stream) => {
    if (err) { res('[exec err] ' + err.message); return; }
    let o = '';
    stream.on('data', d => o += d);
    stream.stderr.on('data', d => o += '[stderr] ' + d);
    stream.on('close', (code) => res('[exit:' + code + '] ' + o.trim()));
    stream.resume();
  });
});

conn.on('ready', async () => {
  console.log('[SSH OK]');
  
  // Test basic cmd commands
  console.log('whoami:', await run('whoami'));
  console.log('echo test:', await run('echo hello_world'));
  console.log('mkdir test:', await run('mkdir C:\\SftpRoot\\echobird_test 2>&1'));
  console.log('dir APPDATA:', await run('dir "%APPDATA%" /b /ad 2>&1'));
  
  conn.end();
}).on('error', e => console.error('SSH ERR:', e.message)
).connect({ host:'192.168.10.207', port:22, username:'27200', password:'669966' });
