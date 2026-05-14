// Unit tests for pid-file lifecycle helpers
// Uses ECHOBIRD_RELAY_DIR to redirect the PID file to a per-test temp
// directory so we never touch the user's real ~/.echobird/.

const fs = require("fs");
const path = require("path");
const os = require("os");

describe("pid-file", () => {
    let tmpDir;
    let writePidFile;
    let updatePidFile;
    let readPidFile;
    let deletePidFile;
    let PID_FILE;

    beforeEach(() => {
        // Each test gets its own dir so we don't fight over filesystem state.
        tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "echobird-pid-test-"));
        process.env.ECHOBIRD_RELAY_DIR = tmpDir;
        // Re-require to pick up the new env var, since the module captures
        // RELAY_DIR at load time.
        delete require.cache[require.resolve("../pid-file.cjs")];
        ({ writePidFile, updatePidFile, readPidFile, deletePidFile, PID_FILE } = require("../pid-file.cjs"));
    });

    afterEach(() => {
        delete process.env.ECHOBIRD_RELAY_DIR;
        try { fs.rmSync(tmpDir, { recursive: true, force: true }); } catch { /* ignore */ }
    });

    test("writes a well-formed pid file", () => {
        const ok = writePidFile(12345, "4.6.2");
        expect(ok).toBe(true);

        const raw = fs.readFileSync(PID_FILE, "utf-8");
        const parsed = JSON.parse(raw);
        expect(parsed.pid).toBe(12345);
        expect(parsed.version).toBe("4.6.2");
        expect(typeof parsed.startedAt).toBe("string");
    });

    test("reads back the pid value", () => {
        writePidFile(54321, "test");

        const obj = readPidFile();
        expect(obj).not.toBeNull();
        expect(obj.pid).toBe(54321);
    });

    test("returns null when the pid file does not exist", () => {
        const obj = readPidFile();
        expect(obj).toBeNull();
    });

    test("returns null when the pid file is malformed JSON", () => {
        fs.mkdirSync(tmpDir, { recursive: true });
        fs.writeFileSync(PID_FILE, "{not json", "utf-8");

        const obj = readPidFile();
        expect(obj).toBeNull();
    });

    test("returns null when pid is missing from the JSON", () => {
        fs.mkdirSync(tmpDir, { recursive: true });
        fs.writeFileSync(PID_FILE, JSON.stringify({ version: "x" }), "utf-8");

        const obj = readPidFile();
        expect(obj).toBeNull();
    });

    test("returns null when pid is not a positive number", () => {
        fs.mkdirSync(tmpDir, { recursive: true });
        fs.writeFileSync(PID_FILE, JSON.stringify({ pid: -1 }), "utf-8");

        expect(readPidFile()).toBeNull();
    });

    test("deletePidFile is idempotent (no error when file is missing)", () => {
        expect(() => deletePidFile()).not.toThrow();
        writePidFile(99, "x");
        expect(() => deletePidFile()).not.toThrow();
        expect(readPidFile()).toBeNull();
    });

    test("writePidFile overwrites an existing file", () => {
        writePidFile(111, "old");
        writePidFile(222, "new");

        const obj = readPidFile();
        expect(obj.pid).toBe(222);
        expect(obj.version).toBe("new");
    });

    describe("updatePidFile", () => {
        test("merges codexPid into existing record without losing other fields", () => {
            writePidFile(1234, "4.6.3");

            const ok = updatePidFile({ codexPid: 5678 });
            expect(ok).toBe(true);

            const obj = readPidFile();
            expect(obj.pid).toBe(1234);
            expect(obj.codexPid).toBe(5678);
            expect(obj.version).toBe("4.6.3");
            expect(typeof obj.startedAt).toBe("string");
        });

        test("returns false (no-op) when no PID file exists yet", () => {
            // Don't writePidFile first — file is missing.
            const ok = updatePidFile({ codexPid: 99 });
            expect(ok).toBe(false);
            expect(readPidFile()).toBeNull();
        });

        test("returns false when existing file is malformed", () => {
            fs.mkdirSync(tmpDir, { recursive: true });
            fs.writeFileSync(PID_FILE, "{not json", "utf-8");

            const ok = updatePidFile({ codexPid: 1 });
            expect(ok).toBe(false);
        });

        test("can update arbitrary fields, not just codexPid", () => {
            writePidFile(100, "x");
            updatePidFile({ codexPid: 200, custom: "value" });

            const obj = readPidFile();
            expect(obj.pid).toBe(100);
            expect(obj.codexPid).toBe(200);
            expect(obj.custom).toBe("value");
        });

        test("regression: codexPid round-trips through write→update→read", () => {
            // Mirrors the launcher's actual sequence: writePidFile on
            // proxy bind, then updatePidFile after Codex is spawned, then
            // Tauri reads both pids back.
            writePidFile(process.pid, "4.6.3");
            updatePidFile({ codexPid: 99999 });

            const obj = readPidFile();
            expect(obj.pid).toBe(process.pid);
            expect(obj.codexPid).toBe(99999);
        });
    });
});
