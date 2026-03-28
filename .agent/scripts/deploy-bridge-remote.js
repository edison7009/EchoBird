import { Client } from 'ssh2';
import path from 'path';
import { fileURLToPath } from 'url';
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const host = process.argv[2] || '192.168.10.39';
const port = parseInt(process.argv[3]) || 34567;
const username = process.argv[4] || 'eben';
const password = process.argv[5] || '669966';

const localBinary = path.resolve(__dirname, '../../bridge/bridge-linux-x86_64');
const remotePath = `/home/${username}/echobird/echobird-bridge`;

console.log(`\n🚀 Deploying bridge binary to ${username}@${host}:${port}...\n`);

const conn = new Client();

conn.on('ready', () => {
  console.log('✅ SSH Connected.');
  conn.sftp((err, sftp) => {
    if (err) throw err;
    
    // Ensure the directory exists first
    conn.exec(`mkdir -p /home/${username}/echobird && pkill -f echobird-bridge || true`, (err2, stream) => {
      stream.on('close', () => {
        console.log('📤 Uploading fresh bridge binary...');
        sftp.fastPut(localBinary, remotePath, {}, (uploadErr) => {
          if (uploadErr) throw uploadErr;
          console.log('✅ Upload complete. Setting permissions...');
          
          conn.exec(`chmod +x ${remotePath} && echo "✅ Done. Bridge is ready at ${remotePath}"`, (err3, stream3) => {
            stream3.on('data', (d) => process.stdout.write(d.toString()));
            stream3.stderr.on('data', (d) => process.stderr.write(d.toString()));
            stream3.on('close', () => {
              console.log('\n🎉 Deploy complete! The new bridge binary is live on the remote server.');
              console.log('   Next time the app connects to this server, it will use the latest bridge.');
              conn.end();
            });
          });
        });
      });
      stream.resume();
    });
  });
}).on('error', (err) => {
  console.error('\n❌ SSH Connection Error:', err.message);
}).connect({ host, port, username, password });
