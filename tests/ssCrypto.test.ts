import { describe, it, expect } from 'vitest';
import {
    kdf, hkdfSha1, incrementNonce, encryptChunk, decryptChunk,
    CIPHERS, HKDF_INFO
} from '../electron/ssCrypto';
import * as crypto from 'crypto';

describe('SS Proxy - KDF (EVP_BytesToKey)', () => {
    it('should derive 16-byte key for aes-128-gcm', () => {
        const key = kdf('test-password', 16);
        expect(key.length).toBe(16);
        expect(Buffer.isBuffer(key)).toBe(true);
    });

    it('should derive 32-byte key for aes-256-gcm', () => {
        const key = kdf('test-password', 32);
        expect(key.length).toBe(32);
    });

    it('should produce deterministic output', () => {
        const key1 = kdf('same-password', 32);
        const key2 = kdf('same-password', 32);
        expect(key1.equals(key2)).toBe(true);
    });

    it('should produce different keys for different passwords', () => {
        const key1 = kdf('password-1', 32);
        const key2 = kdf('password-2', 32);
        expect(key1.equals(key2)).toBe(false);
    });

    it('should handle empty password', () => {
        const key = kdf('', 16);
        expect(key.length).toBe(16);
    });

    it('should handle unicode password', () => {
        const key = kdf('密码测试🔑', 32);
        expect(key.length).toBe(32);
    });
});

describe('SS Proxy - HKDF-SHA1', () => {
    it('should derive key of specified length', () => {
        const secret = kdf('test', 32);
        const salt = crypto.randomBytes(32);
        const derived = hkdfSha1(secret, salt, HKDF_INFO, 32);
        expect(derived.length).toBe(32);
    });

    it('should produce deterministic output for same inputs', () => {
        const secret = kdf('test', 32);
        const salt = Buffer.from('fixed-salt-for-test-0000000000000');
        const d1 = hkdfSha1(secret, salt, HKDF_INFO, 32);
        const d2 = hkdfSha1(secret, salt, HKDF_INFO, 32);
        expect(d1.equals(d2)).toBe(true);
    });

    it('should produce different output for different salts', () => {
        const secret = kdf('test', 32);
        const salt1 = Buffer.alloc(32, 1);
        const salt2 = Buffer.alloc(32, 2);
        const d1 = hkdfSha1(secret, salt1, HKDF_INFO, 32);
        const d2 = hkdfSha1(secret, salt2, HKDF_INFO, 32);
        expect(d1.equals(d2)).toBe(false);
    });

    it('should derive 16-byte key', () => {
        const secret = kdf('test', 16);
        const salt = crypto.randomBytes(16);
        const derived = hkdfSha1(secret, salt, HKDF_INFO, 16);
        expect(derived.length).toBe(16);
    });
});

describe('SS Proxy - incrementNonce', () => {
    it('should increment first byte', () => {
        const nonce = Buffer.alloc(12, 0);
        incrementNonce(nonce);
        expect(nonce[0]).toBe(1);
        expect(nonce[1]).toBe(0);
    });

    it('should carry on overflow', () => {
        const nonce = Buffer.alloc(12, 0);
        nonce[0] = 255;
        incrementNonce(nonce);
        expect(nonce[0]).toBe(0);
        expect(nonce[1]).toBe(1);
    });

    it('should handle multi-byte carry', () => {
        const nonce = Buffer.alloc(12, 0);
        nonce[0] = 255;
        nonce[1] = 255;
        incrementNonce(nonce);
        expect(nonce[0]).toBe(0);
        expect(nonce[1]).toBe(0);
        expect(nonce[2]).toBe(1);
    });

    it('should increment sequentially', () => {
        const nonce = Buffer.alloc(12, 0);
        for (let i = 0; i < 256; i++) {
            incrementNonce(nonce);
        }
        // After 256 increments: byte[0]=0, byte[1]=1
        expect(nonce[0]).toBe(0);
        expect(nonce[1]).toBe(1);
    });
});

describe('SS Proxy - AEAD Encrypt/Decrypt', () => {
    for (const [cipherName, cipherInfo] of Object.entries(CIPHERS)) {
        describe(`Cipher: ${cipherName}`, () => {
            const password = 'test-password-123';
            const psk = kdf(password, cipherInfo.keyLen);

            it('should encrypt and decrypt correctly', () => {
                const salt = crypto.randomBytes(cipherInfo.saltLen);
                const key = hkdfSha1(psk, salt, HKDF_INFO, cipherInfo.keyLen);
                const encNonce = Buffer.alloc(cipherInfo.nonceLen);
                const decNonce = Buffer.alloc(cipherInfo.nonceLen);

                const plaintext = Buffer.from('Hello, Shadowsocks!');
                const encrypted = encryptChunk(plaintext, key, encNonce, cipherInfo.algorithm, cipherInfo.tagLen);
                const decrypted = decryptChunk(encrypted, key, decNonce, cipherInfo.algorithm, cipherInfo.tagLen);

                expect(decrypted.toString()).toBe('Hello, Shadowsocks!');
            });

            it('should produce different ciphertext for same plaintext (nonce changes)', () => {
                const salt = crypto.randomBytes(cipherInfo.saltLen);
                const key = hkdfSha1(psk, salt, HKDF_INFO, cipherInfo.keyLen);
                const nonce = Buffer.alloc(cipherInfo.nonceLen);

                const plaintext = Buffer.from('test data');
                const enc1 = encryptChunk(plaintext, key, nonce, cipherInfo.algorithm, cipherInfo.tagLen);
                const enc2 = encryptChunk(plaintext, key, nonce, cipherInfo.algorithm, cipherInfo.tagLen);
                // nonce auto-increments, so enc2 should differ
                expect(enc1.equals(enc2)).toBe(false);
            });

            it('should handle empty plaintext', () => {
                const salt = crypto.randomBytes(cipherInfo.saltLen);
                const key = hkdfSha1(psk, salt, HKDF_INFO, cipherInfo.keyLen);
                const encNonce = Buffer.alloc(cipherInfo.nonceLen);
                const decNonce = Buffer.alloc(cipherInfo.nonceLen);

                const plaintext = Buffer.alloc(0);
                const encrypted = encryptChunk(plaintext, key, encNonce, cipherInfo.algorithm, cipherInfo.tagLen);
                const decrypted = decryptChunk(encrypted, key, decNonce, cipherInfo.algorithm, cipherInfo.tagLen);

                expect(decrypted.length).toBe(0);
            });

            it('should handle large data', () => {
                const salt = crypto.randomBytes(cipherInfo.saltLen);
                const key = hkdfSha1(psk, salt, HKDF_INFO, cipherInfo.keyLen);
                const encNonce = Buffer.alloc(cipherInfo.nonceLen);
                const decNonce = Buffer.alloc(cipherInfo.nonceLen);

                const plaintext = crypto.randomBytes(16383); // max SS payload
                const encrypted = encryptChunk(plaintext, key, encNonce, cipherInfo.algorithm, cipherInfo.tagLen);
                const decrypted = decryptChunk(encrypted, key, decNonce, cipherInfo.algorithm, cipherInfo.tagLen);

                expect(decrypted.equals(plaintext)).toBe(true);
            });

            it('should fail decryption with wrong key', () => {
                const salt = crypto.randomBytes(cipherInfo.saltLen);
                const key = hkdfSha1(psk, salt, HKDF_INFO, cipherInfo.keyLen);
                const wrongKey = hkdfSha1(kdf('wrong-password', cipherInfo.keyLen), salt, HKDF_INFO, cipherInfo.keyLen);
                const encNonce = Buffer.alloc(cipherInfo.nonceLen);
                const decNonce = Buffer.alloc(cipherInfo.nonceLen);

                const plaintext = Buffer.from('secret data');
                const encrypted = encryptChunk(plaintext, key, encNonce, cipherInfo.algorithm, cipherInfo.tagLen);

                expect(() => {
                    decryptChunk(encrypted, wrongKey, decNonce, cipherInfo.algorithm, cipherInfo.tagLen);
                }).toThrow();
            });
        });
    }
});

describe('SS Proxy - Cipher config', () => {
    it('should have aes-128-gcm', () => {
        expect(CIPHERS['aes-128-gcm']).toBeDefined();
        expect(CIPHERS['aes-128-gcm'].keyLen).toBe(16);
    });

    it('should have aes-256-gcm', () => {
        expect(CIPHERS['aes-256-gcm']).toBeDefined();
        expect(CIPHERS['aes-256-gcm'].keyLen).toBe(32);
    });

    it('should have chacha20-ietf-poly1305', () => {
        expect(CIPHERS['chacha20-ietf-poly1305']).toBeDefined();
        expect(CIPHERS['chacha20-ietf-poly1305'].keyLen).toBe(32);
    });

    it('all ciphers should have correct nonce length (12)', () => {
        for (const [, info] of Object.entries(CIPHERS)) {
            expect(info.nonceLen).toBe(12);
        }
    });

    it('all ciphers should have correct tag length (16)', () => {
        for (const [, info] of Object.entries(CIPHERS)) {
            expect(info.tagLen).toBe(16);
        }
    });
});
