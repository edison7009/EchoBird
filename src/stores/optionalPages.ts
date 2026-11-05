import type { AppSettings } from '../api/types';

export const OPTIONAL_PAGE_SETTINGS = {
  aiNews: 'aiNewsEnabled',
  aiCareer: 'aiCareerEnabled',
  localLlm: 'localLlmEnabled',
} as const;

export type OptionalPage = keyof typeof OPTIONAL_PAGE_SETTINGS;
export type OptionalPages = Record<OptionalPage, boolean>;

export const optionalPagesFromSettings = (settings: AppSettings): OptionalPages => ({
  aiNews: settings.aiNewsEnabled !== false,
  aiCareer: settings.aiCareerEnabled !== false,
  localLlm: settings.localLlmEnabled !== false,
});
