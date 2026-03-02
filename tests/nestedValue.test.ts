import { describe, it, expect } from 'vitest';
import { getNestedValue, setNestedValue, deleteNestedValue } from '../electron/tools/utils';

describe('getNestedValue', () => {
    it('should read top-level value', () => {
        const obj = { name: 'test' };
        expect(getNestedValue(obj, 'name')).toBe('test');
    });

    it('should read nested value', () => {
        const obj = { env: { ANTHROPIC_MODEL: 'claude-3' } };
        expect(getNestedValue(obj, 'env.ANTHROPIC_MODEL')).toBe('claude-3');
    });

    it('should read deeply nested value', () => {
        const obj = { a: { b: { c: { d: 42 } } } };
        expect(getNestedValue(obj, 'a.b.c.d')).toBe(42);
    });

    it('should return undefined for non-existent path', () => {
        const obj = { name: 'test' };
        expect(getNestedValue(obj, 'foo.bar')).toBeUndefined();
    });

    it('should return undefined for null intermediate', () => {
        const obj = { a: null };
        expect(getNestedValue(obj, 'a.b')).toBeUndefined();
    });

    it('should handle empty object', () => {
        expect(getNestedValue({}, 'any.path')).toBeUndefined();
    });
});

describe('setNestedValue', () => {
    it('should set top-level value', () => {
        const obj: any = {};
        setNestedValue(obj, 'name', 'test');
        expect(obj.name).toBe('test');
    });

    it('should set nested value creating intermediates', () => {
        const obj: any = {};
        setNestedValue(obj, 'env.ANTHROPIC_MODEL', 'claude-3');
        expect(obj.env.ANTHROPIC_MODEL).toBe('claude-3');
    });

    it('should set deeply nested value', () => {
        const obj: any = {};
        setNestedValue(obj, 'a.b.c.d', 42);
        expect(obj.a.b.c.d).toBe(42);
    });

    it('should overwrite existing value', () => {
        const obj = { env: { MODEL: 'old' } };
        setNestedValue(obj, 'env.MODEL', 'new');
        expect(obj.env.MODEL).toBe('new');
    });

    it('should not destroy sibling properties', () => {
        const obj = { env: { KEY: 'keep', MODEL: 'old' } };
        setNestedValue(obj, 'env.MODEL', 'new');
        expect(obj.env.KEY).toBe('keep');
        expect(obj.env.MODEL).toBe('new');
    });
});

describe('deleteNestedValue', () => {
    it('should delete a nested value', () => {
        const obj = { env: { PROXY: 'http://proxy', MODEL: 'claude' } };
        deleteNestedValue(obj, 'env.PROXY');
        expect(obj.env.PROXY).toBeUndefined();
        expect(obj.env.MODEL).toBe('claude');
    });

    it('should not throw on non-existent path', () => {
        const obj = { a: 1 };
        expect(() => deleteNestedValue(obj, 'x.y.z')).not.toThrow();
    });

    it('should not throw on null intermediate', () => {
        const obj = { a: null };
        expect(() => deleteNestedValue(obj as any, 'a.b')).not.toThrow();
    });

    it('should delete top-level value', () => {
        const obj: any = { name: 'test', age: 10 };
        deleteNestedValue(obj, 'name');
        expect(obj.name).toBeUndefined();
        expect(obj.age).toBe(10);
    });
});
