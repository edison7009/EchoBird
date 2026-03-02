import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { getNestedValue, setNestedValue, deleteNestedValue } from '../electron/tools/utils';
import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';

/**
 * Test the data-driven config read/write logic
 * by simulating what DataDrivenToolConfigurator does with claudecode's mapping.
 */

// claudecode config.json mapping (data-driven, not custom)
const claudeCodeMapping = {
    read: {
        model: ['env.ANTHROPIC_MODEL'],
        baseUrl: ['env.ANTHROPIC_BASE_URL'],
        apiKey: ['env.ANTHROPIC_API_KEY'],
        proxyUrl: ['env.HTTPS_PROXY', 'env.https_proxy'],
    },
    write: {
        'env.ANTHROPIC_MODEL': 'model',
        'env.ANTHROPIC_BASE_URL': 'baseUrl',
        'env.ANTHROPIC_API_KEY': 'apiKey',
    } as Record<string, string>,
    writeProxy: {
        'env.HTTPS_PROXY': 'proxyUrl',
        'env.HTTP_PROXY': 'proxyUrl',
        'env.https_proxy': 'proxyUrl',
        'env.http_proxy': 'proxyUrl',
    } as Record<string, string>,
};

// Simulated read (mirrors DataDrivenToolConfigurator.getCurrentModelInfo logic)
function simulateRead(config: any) {
    const readMap = claudeCodeMapping.read;
    const readField = (paths: string[]): string => {
        for (const p of paths) {
            const val = getNestedValue(config, p);
            if (val != null && val !== '') return String(val);
        }
        return '';
    };

    const model = readField(readMap.model);
    if (!model) return null;

    return {
        id: 'current',
        name: model,
        model: model,
        baseUrl: readField(readMap.baseUrl),
        apiKey: readField(readMap.apiKey),
        proxyUrl: readField(readMap.proxyUrl) || undefined,
    };
}

// Simulated write (mirrors DataDrivenToolConfigurator.applyConfig logic)
function simulateWrite(config: any, modelInfo: any) {
    const knownModelFields = ['id', 'name', 'baseUrl', 'apiKey', 'model', 'proxyUrl'];
    for (const [configPath, modelField] of Object.entries(claudeCodeMapping.write)) {
        const value = modelInfo[modelField];
        if (value != null) {
            setNestedValue(config, configPath, value);
        } else if (!knownModelFields.includes(modelField)) {
            setNestedValue(config, configPath, modelField);
        }
    }

    // Proxy
    if (modelInfo.proxyUrl) {
        for (const configPath of Object.keys(claudeCodeMapping.writeProxy)) {
            setNestedValue(config, configPath, modelInfo.proxyUrl);
        }
    } else {
        for (const configPath of Object.keys(claudeCodeMapping.writeProxy)) {
            deleteNestedValue(config, configPath);
        }
    }

    return config;
}

describe('Config read/write (claudecode data-driven mapping)', () => {
    let tmpDir: string;

    beforeEach(() => {
        tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'Echobird-test-'));
    });

    afterEach(() => {
        fs.rmSync(tmpDir, { recursive: true, force: true });
    });

    it('should write model config correctly', () => {
        const config = {};
        const modelInfo = {
            id: 'test',
            name: 'Claude 3.5 Sonnet',
            model: 'claude-3-5-sonnet-20241022',
            baseUrl: 'https://api.anthropic.com',
            apiKey: 'sk-ant-test-key',
        };

        const result = simulateWrite(config, modelInfo);
        expect(result).toEqual({
            env: {
                ANTHROPIC_MODEL: 'claude-3-5-sonnet-20241022',
                ANTHROPIC_BASE_URL: 'https://api.anthropic.com',
                ANTHROPIC_API_KEY: 'sk-ant-test-key',
            },
        });
    });

    it('should read back written config', () => {
        const config = {};
        const modelInfo = {
            id: 'test',
            name: 'DeepSeek',
            model: 'deepseek-chat',
            baseUrl: 'https://api.deepseek.com/v1',
            apiKey: 'sk-deepseek-key',
        };

        simulateWrite(config, modelInfo);
        const readBack = simulateRead(config);

        expect(readBack).not.toBeNull();
        expect(readBack!.model).toBe('deepseek-chat');
        expect(readBack!.baseUrl).toBe('https://api.deepseek.com/v1');
        expect(readBack!.apiKey).toBe('sk-deepseek-key');
    });

    it('should handle proxy write and delete', () => {
        const config: any = {};
        const modelInfoWithProxy = {
            id: 'test',
            name: 'Claude',
            model: 'claude-3',
            baseUrl: 'https://api.anthropic.com',
            apiKey: 'sk-key',
            proxyUrl: 'http://127.0.0.1:1080',
        };

        // Write with proxy
        simulateWrite(config, modelInfoWithProxy);
        expect(config.env.HTTPS_PROXY).toBe('http://127.0.0.1:1080');
        expect(config.env.HTTP_PROXY).toBe('http://127.0.0.1:1080');
        expect(config.env.https_proxy).toBe('http://127.0.0.1:1080');
        expect(config.env.http_proxy).toBe('http://127.0.0.1:1080');

        // Read proxy back
        const readBack = simulateRead(config);
        expect(readBack!.proxyUrl).toBe('http://127.0.0.1:1080');

        // Remove proxy
        const modelInfoNoProxy = { ...modelInfoWithProxy, proxyUrl: undefined };
        simulateWrite(config, modelInfoNoProxy);
        expect(config.env.HTTPS_PROXY).toBeUndefined();
        expect(config.env.HTTP_PROXY).toBeUndefined();
    });

    it('should overwrite previous model config', () => {
        const config: any = {};

        // First write
        simulateWrite(config, {
            id: '1', name: 'Model A', model: 'model-a',
            baseUrl: 'https://a.com', apiKey: 'key-a',
        });

        // Second write (overwrite)
        simulateWrite(config, {
            id: '2', name: 'Model B', model: 'model-b',
            baseUrl: 'https://b.com', apiKey: 'key-b',
        });

        const readBack = simulateRead(config);
        expect(readBack!.model).toBe('model-b');
        expect(readBack!.baseUrl).toBe('https://b.com');
        expect(readBack!.apiKey).toBe('key-b');
    });

    it('should read from empty config as null', () => {
        const result = simulateRead({});
        expect(result).toBeNull();
    });

    it('should write and read config file to disk', () => {
        const configPath = path.join(tmpDir, 'settings.json');
        const config: any = {};

        simulateWrite(config, {
            id: 'disk-test',
            name: 'DiskTest',
            model: 'disk-model',
            baseUrl: 'https://disk.api.com',
            apiKey: 'sk-disk-key',
        });

        // Write to disk
        fs.writeFileSync(configPath, JSON.stringify(config, null, 2), 'utf-8');

        // Read back from disk
        const readFromDisk = JSON.parse(fs.readFileSync(configPath, 'utf-8'));
        const readBack = simulateRead(readFromDisk);

        expect(readBack!.model).toBe('disk-model');
        expect(readBack!.apiKey).toBe('sk-disk-key');
    });
});
