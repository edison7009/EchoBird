import React from 'react';
import { act, create } from 'react-test-renderer';
import { describe, expect, it, vi } from 'vitest';
import museDefinition from '../../../tools/muse/paths.json';
import type { ModelConfig } from '../../api/types';
import { useNavigationStore } from '../../stores/navigationStore';
import { AppManagerContext, type AppManagerContextType } from './context';
import { ApplicationDetailsPanel } from './ApplicationDetailsPanel';

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
    internalId: 'smart-router',
    name: 'Auto Router',
    baseUrl: 'http://127.0.0.1:53683/v1',
    apiKey: '',
  },
];

describe('Muse install and launch panel', () => {
  it.each([false, true])(
    'uses the existing action without account or model controls (installed: %s)',
    async (installed) => {
      const { AppManagerPanel, AppManagerBottom } = await import('./AppManagerComponents');
      const { ModelNexusContext } = await import('../ModelNexus/context');
      const { ConfirmDialogProvider } = await import('../../components/ConfirmDialog');
      useNavigationStore.setState({ activePage: 'apps' });
      const handleLaunch = vi.fn();
      const onGoToMother = vi.fn();
      const context = {
        selectedTool: 'muse',
        selectedToolData: {
          ...museDefinition,
          id: 'muse',
          installed,
          detectedPath: '/custom/muse',
          version: '1.0',
        },
        userModels: models,
        toolModelConfig: {},
        viewMode: 'desktop',
        launchAfterApply: false,
        isLaunching: false,
        handleLaunch,
        onGoToMother,
      } as unknown as AppManagerContextType;
      const view = (
        <ConfirmDialogProvider>
          <ModelNexusContext.Provider value={{} as React.ContextType<typeof ModelNexusContext>}>
            <AppManagerContext.Provider value={context}>
              <AppManagerPanel />
              <AppManagerBottom />
            </AppManagerContext.Provider>
          </ModelNexusContext.Provider>
        </ConfirmDialogProvider>
      );
      const label = installed ? 'btn.launchApp' : 'btn.installOneClick';
      let renderer: ReturnType<typeof create>;
      act(() => {
        renderer = create(view);
      });
      try {
        const markup = JSON.stringify(renderer!.toJSON());
        expect(markup).not.toContain('Cloud Model');
        expect(markup).not.toContain('Auto Router');
        expect(markup).not.toContain('agent.addAccount');
        expect(renderer!.root.findAllByType(ApplicationDetailsPanel)).toHaveLength(
          installed ? 1 : 0
        );
        expect(markup.includes('/custom/muse')).toBe(installed);
        expect(renderer!.root.findAll((node) => node.props.role === 'radio')).toHaveLength(0);
        expect(renderer!.root.findAllByType('a').map((link) => link.props.href)).toEqual(
          installed ? [] : ['https://muse.ai/']
        );
        expect(handleLaunch).not.toHaveBeenCalled();
        expect(onGoToMother).not.toHaveBeenCalled();
        const primary = renderer!.root
          .findAllByType('button')
          .find((button) => button.children.includes(label))!;
        expect(primary.props.disabled).toBe(false);
        act(() => primary.props.onClick());
        if (installed) {
          expect(handleLaunch).toHaveBeenCalledOnce();
          expect(onGoToMother).not.toHaveBeenCalled();
        } else {
          expect(onGoToMother).toHaveBeenCalledWith('muse', 'Muse');
          expect(handleLaunch).not.toHaveBeenCalled();
        }
      } finally {
        act(() => renderer!.unmount());
      }
    }
  );
});
