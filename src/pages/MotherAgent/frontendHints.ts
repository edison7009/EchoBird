import type { MotherHint } from './types';

// Quick-phrase hints supplied by the frontend, in addition to the bundled
// `hints.json` registry fetched via `getMotherHints()`. Kept in this small
// module — not inline in MotherAgentMain — so new frontend hints stay additive
// here and the component doesn't accumulate. Each hint's localized chip text
// lives in i18n under
// `mother.hint<Action>`, so it follows EchoBird's own UI language.
export const FRONTEND_HINTS: MotherHint[] = [
  // EchoBird's Chinese plugin marketplaces — zh-Hans only (empty en label).
  { action: 'unlockCodexPlugins' },
  { action: 'analyzeResiduals' },
  { action: 'analyzeDiskSpace' },
];
