import { describe, it, expect } from 'vitest';
import { expandPath } from '../electron/tools/utils';
import * as os from 'os';
import * as path from 'path';

describe('expandPath', () => {
    it('should expand ~ to home directory', () => {
        const result = expandPath('~/config');
        const expected = path.join(os.homedir(), 'config');
        expect(result).toBe(expected);
    });

    it('should expand ~ alone to home directory', () => {
        const result = expandPath('~');
        expect(result).toBe(os.homedir());
    });

    it('should normalize path separators', () => {
        const result = expandPath('~/a/b/c');
        expect(result).toBe(path.join(os.homedir(), 'a', 'b', 'c'));
    });

    it('should not expand ~ in the middle of a path', () => {
        const result = expandPath('/some/~/path');
        // ~ in the middle should not be expanded
        expect(result).not.toContain(os.homedir());
    });

    if (process.platform === 'win32') {
        it('should expand %APPDATA% on Windows', () => {
            const appData = process.env.APPDATA;
            if (appData) {
                const result = expandPath('%APPDATA%/npm');
                expect(result).toBe(path.normalize(`${appData}/npm`));
            }
        });

        it('should expand %LOCALAPPDATA% on Windows', () => {
            const localAppData = process.env.LOCALAPPDATA;
            if (localAppData) {
                const result = expandPath('%LOCALAPPDATA%/Programs');
                expect(result).toBe(path.normalize(`${localAppData}/Programs`));
            }
        });

        it('should expand %USERPROFILE% on Windows', () => {
            const userProfile = process.env.USERPROFILE;
            if (userProfile) {
                const result = expandPath('%USERPROFILE%/.config');
                expect(result).toBe(path.normalize(`${userProfile}/.config`));
            }
        });
    }

    it('should return normalized path for absolute paths', () => {
        const result = expandPath('/usr/local/bin');
        expect(result).toBe(path.normalize('/usr/local/bin'));
    });
});
