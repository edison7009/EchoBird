import React from 'react';
import { act, create } from 'react-test-renderer';
import { describe, expect, it, vi } from 'vitest';
import museCodeDefinition from '../../../tools/musecode/paths.json';
import type { ModelConfig } from '../../api/types';
import { useNavigationStore } from '../../stores/navigationStore';
import { AppManagerContext, type AppManagerContextType } from './context';
import { AppManagerPanel, AppManagerBottom } from './AppManagerComponents';
import { ModelNexusContext } from '../ModelNexus/context';
import { ConfirmDialogProvider } from '../../components/ConfirmDialog';

vi.hoisted(() => vi.stubGlobal('__APP_EDITION__', 'full'));
vi.mock('../../components', () => ({
  getModelIcon: () => null,
}));
vi.mock('../../utils/platform', () => ({ IS_WINDOWS: false, IS_LINUX: true, IS_MACOS: false }));

const models: ModelConfig[] = [
  {
    internalId: 'cloud-model',
    name: 'Cloud Model',
    baseUrl: 'https://example.test/v1',
    apiKey: '',
  },
  {
    internalId: 'anthropic-model',
    name: 'Anthropic Model',
    baseUrl: '',
    anthropicUrl: 'https://example.test/anthropic',
    apiKey: '',
  },
  {
    internalId: 'smart-router',
    name: 'Auto Router',
    baseUrl: 'http://127.0.0.1:53683/v1',
    apiKey: '',
  },
];

describe('Muse Code install and model panel', () => {
  it.each([false, true])(
    'uses the existing install or model action without account controls (installed: %s)',
    (installed) => {
      useNavigationStore.setState({ activePage: 'apps' });
      const handleLaunch = vi.fn();
      const onGoToMother = vi.fn();
      const handleSelectModel = vi.fn();
      const context = {
        selectedTool: 'musecode',
        selectedToolData: { ...museCodeDefinition, id: 'musecode', installed },
        userModels: models,
        toolModelConfig: {},
        viewMode: 'desktop',
        smartRouterEnabled: true,
        launchAfterApply: true,
        isLaunching: false,
        handleLaunch,
        onGoToMother,
        handleSelectModel,
      } as unknown as AppManagerContextType;
      let renderer: ReturnType<typeof create>;
      act(() => {
        renderer = create(
          <ConfirmDialogProvider>
            <ModelNexusContext.Provider value={{} as React.ContextType<typeof ModelNexusContext>}>
              <AppManagerContext.Provider value={context}>
                <AppManagerPanel />
                <AppManagerBottom />
              </AppManagerContext.Provider>
            </ModelNexusContext.Provider>
          </ConfirmDialogProvider>
        );
      });
      try {
        const markup = JSON.stringify(renderer!.toJSON());
        expect(markup.includes('Cloud Model')).toBe(installed);
        expect(markup.includes('Auto Router')).toBe(installed);
        expect(markup).not.toContain('Anthropic Model');
        expect(markup).not.toContain('agent.addAccount');
        expect(markup).not.toContain('agent.noModelConfig');
        expect(markup).not.toContain('agent.restore');
        expect(renderer!.root.findAllByType('a').map((link) => link.props.href)).toEqual(
          installed ? [] : ['https://dev.meta.ai/']
        );
        expect(handleLaunch).not.toHaveBeenCalled();
        expect(onGoToMother).not.toHaveBeenCalled();
        expect(handleSelectModel).not.toHaveBeenCalled();
        const label = installed ? 'btn.launchApp' : 'btn.installOneClick';
        const primary = renderer!.root
          .findAllByType('button')
          .find((button) => button.children.includes(label))!;
        expect(primary.props.disabled).toBe(false);
        act(() => primary.props.onClick());
        if (installed) {
          expect(handleLaunch).toHaveBeenCalledOnce();
          expect(onGoToMother).not.toHaveBeenCalled();
        } else {
          expect(onGoToMother).toHaveBeenCalledWith('musecode', 'Muse Code');
          expect(handleLaunch).not.toHaveBeenCalled();
        }
      } finally {
        act(() => renderer!.unmount());
      }
    }
  );
});
