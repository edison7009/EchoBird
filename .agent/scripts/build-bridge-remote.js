import fs from 'fs';
import path from 'path';
import { execSync } from 'child_process';
import { createRequire } from 'module';
import { fileURLToPath } from 'url';
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const require = createRequire(import.meta.url);

// Ensure ssh2 and ssh2-sftp-client are installed globally or in a temp folder
try {
  require.resolve('ssh2');
} catch (e) {
  console.log('Installing required dependencies (ssh2, ssh2-sftp-client)...');
  execSync('npm install --no-save ssh2 ssh2-sftp-client', { stdio: 'inherit' });
}

const { Client } = require('ssh2');

const host = process.argv[2] || '192.168.10.39';
const port = parseInt(process.argv[3]) || 34567;
const username = process.argv[4] || 'eben';
const password = process.argv[5] || '669966';

console.log(`\n🚀 Starting Remote Bridge Build on ${username}@${host}:${port}...\n`);

const sftpConfig = { host, port, username, password };
const sourceDir = path.resolve(__dirname, '../../bridge-src');
const zipPath = path.join(process.env.TEMP || process.env.TMP || '/tmp', 'bridge-src.zip');

console.log('📦 Archiving local bridge-src directory (skipping target/)...');
if (fs.existsSync(zipPath)) fs.unlinkSync(zipPath);
// Use tar (built into Windows 10+)
execSync(`tar -a -c -f "${zipPath}" --exclude=bridge-src/target -C "${path.dirname(sourceDir)}" bridge-src`, { stdio: 'inherit' });

const conn = new Client();

conn.on('ready', () => {
  console.log('✅ SSH Client Connected.');
  
  conn.sftp((err, sftp) => {
    if (err) throw err;
    console.log(`📤 Uploading source archive to /home/${username}/bridge-src.zip...`);
    
    sftp.fastPut(zipPath, `/home/${username}/bridge-src.zip`, {}, (uploadErr) => {
      if (uploadErr) throw uploadErr;
      console.log('✅ Upload Complete.\n');
      
      const compileCmd = `
        echo '🔧 Preparing build environment...'
        echo '${password}' | sudo -S apt-get update > /dev/null 2>&1
        echo '${password}' | sudo -S apt-get install -y curl build-essential pkg-config libssl-dev unzip > /dev/null 2>&1
        
        mkdir -p /home/${username}/bridge-build
        cd /home/${username}/bridge-build
        
        echo '📦 Extracting source code...'
        unzip -q -o /home/${username}/bridge-src.zip
        
        if ! command -v cargo &> /dev/null; then 
          echo '🦀 Rust not found. Installing rustup...'
          curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y > /dev/null 2>&1
        fi
        
        source $HOME/.cargo/env
        cd bridge-src
        
        echo '⚡ Building Release Native Binary (this may take 1-3 minutes)...'
        cargo build --release
        
        if [ $? -eq 0 ]; then
            echo '✅ Build Success!'
            echo 'ARCH:'$(uname -m)
        else
            echo '❌ Build Failed!'
            exit 1
        fi
      `;
      
      conn.exec(compileCmd, (execErr, stream) => {
        if (execErr) throw execErr;
        
        let outputBuffer = '';
        stream.on('data', (data) => {
          const text = data.toString();
          outputBuffer += text;
          process.stdout.write(text);
        }).stderr.on('data', (data) => {
          process.stderr.write(data.toString());
        });
        
        stream.on('close', (code) => {
          if (code === 0) {
            // Parse architecture
            const archMatch = outputBuffer.match(/ARCH:(aarch64|x86_64)/);
            const suffix = archMatch ? archMatch[1] : 'x86_64';
            
            const destFile = path.resolve(__dirname, '../../bridge', `bridge-linux-${suffix}`);
            const remotePath = `/home/${username}/bridge-build/bridge-src/target/release/echobird-bridge`;
            
            console.log(`\n📥 Downloading compiled binary to: ${destFile}`);
            
            sftp.fastGet(remotePath, destFile, {}, (downloadErr) => {
              if (downloadErr) throw downloadErr;
              console.log('🎉 Remote Compilation & Retrieval Complete! Ready for Tauri bundling.');
              
              // Cleanup remotely
              conn.exec(`rm -rf /home/${username}/bridge-build /home/${username}/bridge-src.zip`, () => {
                conn.end();
                // Cleanup locally
                if (fs.existsSync(zipPath)) fs.unlinkSync(zipPath);
              });
            });
          } else {
            console.error('\n❌ Compilation process failed.');
            conn.end();
          }
        });
      });
    });
  });
}).on('error', (err) => {
  console.error('\n❌ SSH Connection Error:', err.message);
}).connect(sftpConfig);
