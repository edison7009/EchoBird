import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { ManusAccountSection } from './ManusAccountSection';
import { AppManagerContext, type AppManagerContextType } from './context';
import { I18nContext } from '../../hooks/i18nContext';
import { en } from '../../i18n/en';

// Both account clients use the shared isolated-browser waiting presentation.
function render(awaitingClientExit: boolean, tool: 'manus' | 'cue' = 'manus') {
  const context = {
    manusAccounts: { accounts: [], busy: true, remainingSeconds: 30, awaitingClientExit },
    cueAccounts: { accounts: [], busy: true, remainingSeconds: 30, awaitingClientExit },
  } as unknown as AppManagerContextType;
  return renderToStaticMarkup(
    <I18nContext.Provider value={{ locale: 'en', setLocale: () => {}, t: (key) => en[key] }}>
      <AppManagerContext.Provider value={context}>
        <ManusAccountSection tool={tool} />
      </AppManagerContext.Provider>
    </I18nContext.Provider>
  );
}

describe('Manus and Cue browser login instructions', () => {
  it('keeps Cue on shared browser waiting throughout authorization', () => {
    expect(render(false, 'cue')).toContain('Waiting for browser (30)');
    const exit = render(true, 'cue');
    expect(exit).toContain('Waiting for browser (30)');
    expect(exit).not.toContain('Please quit');
    expect(exit).toContain('/icons/tools/cue.png');
    expect(exit).toContain('h-12');
    expect(exit).not.toContain('title=');
    expect(exit).not.toContain('cursor-');
  });
  it('keeps Manus on browser waiting without requiring native client exit', () => {
    const waiting = render(false);
    const exit = render(true);
    expect(waiting).toContain('Waiting for browser (30)');
    expect(exit).not.toContain('Please quit Manus');
    expect(exit).toContain('Waiting for browser (30)');
    expect(exit).toContain('h-12');
    expect(exit).toContain(`aria-label="${en['btn.cancel']}"`);
    expect(exit).toContain('/icons/tools/manus.png');
    expect(exit).not.toContain('title=');
    expect(exit).not.toContain('cursor-');
  });
});
