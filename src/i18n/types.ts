import type { en } from './en';

export type TKey = keyof typeof en;
export type Translations = Record<TKey, string>;
