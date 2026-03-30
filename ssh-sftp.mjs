import { createRequire } from 'module';
const require = createRequire(import.meta.url);
const { Client } = require('ssh2');

const conn = new Client();
conn.on('ready', () => {
  console.log('[SSH OK] - testing SFTP access...');
  conn.sftp((err, sftp) => {
    if (err) { console.error('SFTP err:', err.message); conn.end(); return; }
    
    // Try to list the root via SFTP
    sftp.readdir('/', (err2, list) => {
      if (err2) console.log('SFTP list /:', err2.message);
      else console.log('SFTP / contents:', list.map(f => f.filename).join(', '));
      
      // Try typical Tauri log path on Windows
      const logPath = 'C:/Users/27200/AppData/Roaming/com.echobird.app/logs';
      sftp.readdir(logPath, (e3, files) => {
        if (e3) console.log('Log dir error:', e3.message);
        else console.log('Log files:', files.map(f => f.filename).join('\n'));
        conn.end();
      });
    });
  });
}).on('error', e => console.error('SSH ERR:', e.message)
).connect({ host:'192.168.10.207', port:22, username:'27200', password:'669966' });
