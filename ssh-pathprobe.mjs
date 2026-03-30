import { createRequire } from 'module';
const require = createRequire(import.meta.url);
const { Client } = require('ssh2');

const conn = new Client();
conn.on('ready', () => {
  conn.sftp((err, sftp) => {
    if (err) { console.error('SFTP err:', err.message); conn.end(); return; }
    
    // Try different Windows path formats in Bitvise SFTP
    const paths = [
      '/C:/Users/27200/AppData/Roaming',
      'C:\\Users\\27200\\AppData\\Roaming',
      '/Users/27200',
      '/home/27200',
    ];
    
    let i = 0;
    const tryNext = () => {
      if (i >= paths.length) { conn.end(); return; }
      const p = paths[i++];
      sftp.readdir(p, (e, list) => {
        if (e) console.log('FAIL', p, '->', e.message);
        else console.log('OK', p, '->', list.slice(0,5).map(f=>f.filename).join(', '));
        tryNext();
      });
    };
    tryNext();
  });
}).on('error', e => console.error('SSH ERR:', e.message)
).connect({ host:'192.168.10.207', port:22, username:'27200', password:'669966' });
