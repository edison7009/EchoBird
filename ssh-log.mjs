import { Client } from "ssh2";
const conn = new Client();
conn.on("ready", () => {
  console.log("SSH OK - Connected to remote Windows");
  conn.exec('powershell -c "Get-ChildItem $env:APPDATA -Recurse -Filter *.log -ErrorAction SilentlyContinue | Where-Object { $_.FullName -match \"echobird\" } | Select-Object -ExpandProperty FullName | Out-String"', (err, stream) => {
    let o = "";
    stream.on("data", d => o += d);
    stream.stderr.on("data", d => {});
    stream.on("close", () => {
      if (!o.trim()) {
        console.log("No echobird logs found in APPDATA, trying LOCALAPPDATA...");
        conn.exec('powershell -c "Get-ChildItem $env:LOCALAPPDATA -Recurse -Filter *.log -ErrorAction SilentlyContinue | Where-Object { $_.FullName -match \"echobird\" } | Select-Object -ExpandProperty FullName | Out-String"', (e2, s2) => {
          let o2 = "";
          s2.on("data", d => o2 += d);
          s2.on("close", () => {
            console.log("LOCALAPPDATA logs:\n" + (o2.trim() || "(none)"));
            // Also check what dirs exist
            conn.exec('powershell -c "Get-ChildItem $env:APPDATA | Where-Object Name -match echo | Select-Object FullName | Out-String"', (e3, s3) => {
              let o3 = "";
              s3.on("data", d => o3 += d);
              s3.on("close", () => {
                console.log("Echobird-related dirs in APPDATA:\n" + (o3.trim() || "(none)"));
                conn.end();
              });
              s3.resume();
            });
          });
          s2.resume();
        });
      } else {
        console.log("LOG FILES:\n" + o);
        conn.end();
      }
    });
    stream.resume();
  });
}).on("error", e => console.error("SSH ERR:", e.message)
).connect({ host:"192.168.10.207", port:22, username:"27200", password:"669966" });
