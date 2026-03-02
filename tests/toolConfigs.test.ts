import { describe, it, expect } from 'vitest';
import * as fs from 'fs';
import * as path from 'path';

const toolsDir = path.resolve(__dirname, '../electron/tools');

describe('Tool JSON configs validation', () => {
    const toolDirs = fs.readdirSync(toolsDir).filter(d => {
        const full = path.join(toolsDir, d);
        return fs.statSync(full).isDirectory();
    });

    it('should have at least one tool directory', () => {
        expect(toolDirs.length).toBeGreaterThan(0);
    });

    for (const dir of toolDirs) {
        describe(`Tool: ${dir}`, () => {
            const pathsFile = path.join(toolsDir, dir, 'paths.json');
            const configFile = path.join(toolsDir, dir, 'config.json');

            it('should have paths.json', () => {
                expect(fs.existsSync(pathsFile)).toBe(true);
            });

            it('should have config.json', () => {
                expect(fs.existsSync(configFile)).toBe(true);
            });

            it('paths.json should be valid JSON with required fields', () => {
                const content = fs.readFileSync(pathsFile, 'utf-8');
                const parsed = JSON.parse(content);
                expect(parsed.name).toBeTruthy();
                expect(parsed.category).toBeTruthy();
                expect(Array.isArray(parsed.apiProtocol)).toBe(true);
            });

            it('config.json should be valid JSON with format field', () => {
                const content = fs.readFileSync(configFile, 'utf-8');
                const parsed = JSON.parse(content);
                expect(parsed.format).toBeTruthy();
            });

            it('paths.json category should be a known value', () => {
                const parsed = JSON.parse(fs.readFileSync(pathsFile, 'utf-8'));
                const knownCategories = ['AgentOS', 'IDE', 'CLI', 'AutoTrading', 'Game', 'Utility'];
                expect(knownCategories).toContain(parsed.category);
            });

            it('config.json format should be a known value', () => {
                const parsed = JSON.parse(fs.readFileSync(configFile, 'utf-8'));
                const knownFormats = ['json', 'toml', 'yaml'];
                expect(knownFormats).toContain(parsed.format);
            });
        });
    }
});
